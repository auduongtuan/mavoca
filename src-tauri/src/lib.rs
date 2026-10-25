mod completion;
mod dictionary;
#[cfg(target_os = "macos")]
mod macos_dictionary;
mod model;
mod translator;
mod vietnamese;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::{env, fs};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    Emitter, Manager, Runtime,
};

use model::{
    build_cards, count_definitions, entries_to_rows, rows_to_entries, DefinitionRow, VocabCard,
    VocabEntry, VocabSenseGroup, VocabTreeInput, VocabWord, WordSource,
};

const BACKUP_PATH_SETTING: &str = "backup_path";
const ADD_WORD_SHORTCUT_SETTING: &str = "add_word_shortcut";
/// Global shortcut that opens the quick-add window. An empty setting disables it.
const DEFAULT_ADD_WORD_SHORTCUT: &str = "Cmd+Ctrl+Shift+N";
const QUICK_ADD_WINDOW_LABEL: &str = "quick-add";
/// The quick-add window's own size, from `tauri.conf.json`: the field, the
/// status line and the window's padding. It grows downward while its suggestion
/// list is open and is put back to this height after.
const QUICK_ADD_WIDTH: f64 = 420.0;
const QUICK_ADD_HEIGHT: f64 = 94.0;
/// The quick-add window widens by this much while it shows a meaning beside the
/// list, and no further than that.
const QUICK_ADD_MAX_WIDTH: f64 = 780.0;
const QUICK_ADD_MAX_HEIGHT: f64 = 420.0;
/// Definitions the add-word preview shows before it counts the rest.
const PREVIEW_DEFINITIONS: usize = 4;

const TRAY_OPEN_MANAGER_ID: &str = "open-manager";
const TRAY_OPEN_SETTINGS_ID: &str = "open-settings";
const TRAY_QUIT_ID: &str = "quit";
const ROTATION_INTERVAL_SETTING: &str = "rotation_interval_secs";
/// Seconds between words while the popup is hidden; 0 turns rotation off.
const DEFAULT_ROTATION_INTERVAL_SECS: u64 = 10;
/// Upper bound accepted from the settings window, one hour.
const MAX_ROTATION_INTERVAL_SECS: u64 = 3600;
/// Suggestions the add-word field shows at once. Each costs a row, and the list
/// is capped by the window height, not by this.
const MAX_SUGGESTIONS: usize = 6;
/// How often the rotation thread checks the popup and the interval, so a new
/// interval or a closed popup takes effect within a second.
const ROTATION_TICK: Duration = Duration::from_secs(1);
/// Logical popup width; matches `width` for the `main` window in tauri.conf.json.
const POPUP_WIDTH: f64 = 320.0;
/// The popup grows to fit its card between these heights, then scrolls.
const POPUP_MIN_HEIGHT: f64 = 240.0;
const POPUP_MAX_HEIGHT: f64 = 720.0;
/// Space kept between the popup's bottom edge and the bottom of the screen.
const POPUP_SCREEN_MARGIN: f64 = 12.0;

#[derive(Debug, Clone, Deserialize)]
struct SeedVocabWord {
    word: String,
    meaning: String,
    example: String,
    category: String,
    pronunciation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppSettings {
    backup_path: String,
    add_word_shortcut: String,
    /// 0 turns rotation off.
    rotation_interval_secs: u64,
}

#[derive(Clone)]
struct AppState {
    db: Arc<Mutex<Connection>>,
    vocab_list: Arc<Mutex<Vec<VocabWord>>>,
    /// Flattened rotation list: one element per definition, rebuilt whenever
    /// the vocabulary changes. `current_index` indexes into this, not into
    /// `vocab_list`, which is what makes prev/next step definition by
    /// definition rather than word by word.
    cards: Arc<Mutex<Vec<VocabCard>>>,
    current_index: Arc<Mutex<usize>>,
    is_window_visible: Arc<Mutex<bool>>,
    /// Mirrors the `rotation_interval_secs` setting for the rotation thread.
    rotation_interval_secs: Arc<AtomicU64>,
}

fn load_seed_vocab() -> Vec<SeedVocabWord> {
    let vocab_json = include_str!("../../public/vocab.json");
    serde_json::from_str(vocab_json).unwrap_or_else(|_| vec![])
}

fn normalize_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Cleans a hand-edited tree and rejects the states the schema cannot catch:
/// an empty word, an entry with no part of speech, and a tree with no
/// definitions at all. Empty definitions and blank examples are dropped rather
/// than refused, so a half-filled row in the editor is not an error.
fn validate_tree_input(input: &VocabTreeInput) -> Result<(String, Vec<VocabEntry>), String> {
    let word = normalize_text(&input.word);
    if word.is_empty() {
        return Err("Word is required".into());
    }

    let mut entries = Vec::new();
    for entry in &input.entries {
        let part_of_speech = normalize_text(&entry.part_of_speech);

        let mut sense_groups = Vec::new();
        for group in &entry.sense_groups {
            let definitions: Vec<_> = group
                .definitions
                .iter()
                .filter_map(|definition| {
                    let text = normalize_text(&definition.definition);
                    if text.is_empty() {
                        return None;
                    }

                    Some(model::VocabDefinition {
                        id: definition.id,
                        definition: text,
                        examples: definition
                            .examples
                            .iter()
                            .map(|example| normalize_text(example))
                            .filter(|example| !example.is_empty())
                            .collect(),
                    })
                })
                .collect();

            if !definitions.is_empty() {
                sense_groups.push(VocabSenseGroup {
                    guide_word: normalize_text(&group.guide_word),
                    definitions,
                });
            }
        }

        if sense_groups.is_empty() {
            continue;
        }

        if part_of_speech.is_empty() {
            return Err("Every entry needs a part of speech".into());
        }

        entries.push(VocabEntry {
            part_of_speech,
            pron_uk: normalize_text(&entry.pron_uk),
            pron_us: normalize_text(&entry.pron_us),
            sense_groups,
        });
    }

    if entries.is_empty() {
        return Err("At least one definition is required".into());
    }

    Ok((word, entries))
}

fn init_database(app: &tauri::AppHandle) -> Result<Connection, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|err| format!("Failed to locate app data directory: {err}"))?;
    fs::create_dir_all(&app_data_dir)
        .map_err(|err| format!("Failed to create app data directory: {err}"))?;

    let db_path = app_data_dir.join("vocab.sqlite3");
    let conn =
        Connection::open(db_path).map_err(|err| format!("Failed to open database: {err}"))?;

    // Without this the ON DELETE CASCADE below never fires and deleting a word
    // silently leaves its definitions behind.
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(|err| format!("Failed to enable foreign keys: {err}"))?;

    create_schema(&conn)?;
    migrate_legacy_vocabs(&conn)?;
    ensure_default_settings(&conn)?;
    seed_if_empty(&conn)?;

    Ok(conn)
}

/// On a fresh database this creates the current shape. On a legacy one the
/// `vocabs` statement is a no-op and migrate_legacy_vocabs reshapes it.
fn create_schema(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS vocabs (
            id INTEGER PRIMARY KEY,
            word TEXT NOT NULL UNIQUE COLLATE NOCASE,
            source TEXT NOT NULL DEFAULT 'manual',
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS vocab_definitions (
            id INTEGER PRIMARY KEY,
            vocab_id INTEGER NOT NULL REFERENCES vocabs(id) ON DELETE CASCADE,
            entry_index INTEGER NOT NULL,
            part_of_speech TEXT NOT NULL,
            pron_uk TEXT NOT NULL DEFAULT '',
            pron_us TEXT NOT NULL DEFAULT '',
            guide_word TEXT NOT NULL DEFAULT '',
            definition TEXT NOT NULL,
            definition_vi TEXT NOT NULL DEFAULT '',
            examples TEXT NOT NULL DEFAULT '[]',
            sort_order INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_vocab_definitions_vocab
            ON vocab_definitions (vocab_id, sort_order);

        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        "#,
    )
    .map_err(|err| format!("Failed to initialize database: {err}"))?;

    ensure_definition_vi_column(conn)
}

/// `definition_vi` arrived after the first databases were created, and SQLite
/// has no `ADD COLUMN IF NOT EXISTS`. It holds the definition translated into
/// Vietnamese, so a card is translated once instead of on every view; it is a
/// cache, so an empty value only costs another translation.
fn ensure_definition_vi_column(conn: &Connection) -> Result<(), String> {
    if table_columns(conn, "vocab_definitions")?
        .iter()
        .any(|column| column == "definition_vi")
    {
        return Ok(());
    }

    conn.execute(
        "ALTER TABLE vocab_definitions \
         ADD COLUMN definition_vi TEXT NOT NULL DEFAULT ''",
        [],
    )
    .map_err(|err| format!("Failed to add the Vietnamese definition column: {err}"))?;

    Ok(())
}

fn table_columns(conn: &Connection, table: &str) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|err| format!("Failed to inspect {table} schema: {err}"))?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|err| format!("Failed to read {table} schema: {err}"))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| format!("Failed to decode {table} schema: {err}"))?;

    Ok(columns)
}

/// Converts the old one-row-per-word shape into the tree, losslessly: the old
/// `category` becomes the part of speech, `pronunciation` the US pronunciation,
/// `meaning` the single definition, and `example` its only example.
///
/// The whole conversion is one transaction, so a failure leaves the original
/// data untouched rather than half-migrated.
fn migrate_legacy_vocabs(conn: &Connection) -> Result<(), String> {
    let columns = table_columns(conn, "vocabs")?;
    if !columns.iter().any(|column| column == "meaning") {
        return Ok(());
    }

    struct LegacyRow {
        id: i64,
        meaning: String,
        example: String,
        category: String,
        pronunciation: String,
    }

    let legacy_rows = {
        let mut stmt = conn
            .prepare(
                r#"
                SELECT id, meaning, example, category, pronunciation
                FROM vocabs
                ORDER BY id ASC
                "#,
            )
            .map_err(|err| format!("Failed to prepare legacy vocabulary read: {err}"))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(LegacyRow {
                    id: row.get(0)?,
                    meaning: row.get(1)?,
                    example: row.get(2)?,
                    category: row.get(3)?,
                    pronunciation: row.get(4)?,
                })
            })
            .map_err(|err| format!("Failed to read legacy vocabulary: {err}"))?;

        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|err| format!("Failed to decode legacy vocabulary: {err}"))?
    };

    // SQLite cannot drop the parent of a foreign key while enforcement is on,
    // and the pragma is a no-op inside a transaction, so it is toggled around
    // the whole rebuild.
    conn.execute_batch("PRAGMA foreign_keys = OFF;")
        .map_err(|err| format!("Failed to relax foreign keys for migration: {err}"))?;

    let result = (|| -> Result<(), String> {
        conn.execute_batch("BEGIN")
            .map_err(|err| format!("Failed to begin migration: {err}"))?;

        for row in &legacy_rows {
            let category = normalize_text(&row.category);
            let examples = {
                let example = normalize_text(&row.example);
                if example.is_empty() {
                    Vec::new()
                } else {
                    vec![example]
                }
            };

            let entries = vec![VocabEntry {
                part_of_speech: if category.is_empty() {
                    "unknown".into()
                } else {
                    category
                },
                pron_uk: String::new(),
                pron_us: normalize_text(&row.pronunciation),
                sense_groups: vec![VocabSenseGroup {
                    guide_word: String::new(),
                    definitions: vec![model::VocabDefinition {
                        id: 0,
                        definition: normalize_text(&row.meaning),
                        examples,
                    }],
                }],
            }];

            insert_definition_rows(conn, row.id, &entries, &HashMap::new())?;
        }

        conn.execute_batch(
            r#"
            CREATE TABLE vocabs_migrated (
                id INTEGER PRIMARY KEY,
                word TEXT NOT NULL UNIQUE COLLATE NOCASE,
                source TEXT NOT NULL DEFAULT 'manual',
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            INSERT INTO vocabs_migrated (id, word, source, created_at, updated_at)
            SELECT id, word, 'manual', created_at, updated_at FROM vocabs;

            DROP TABLE vocabs;
            ALTER TABLE vocabs_migrated RENAME TO vocabs;
            "#,
        )
        .map_err(|err| format!("Failed to rebuild vocabulary table: {err}"))?;

        conn.execute_batch("COMMIT")
            .map_err(|err| format!("Failed to commit migration: {err}"))?;

        Ok(())
    })();

    if result.is_err() {
        let _ = conn.execute_batch("ROLLBACK");
    }

    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(|err| format!("Failed to restore foreign keys: {err}"))?;

    result
}

fn seed_if_empty(conn: &Connection) -> Result<(), String> {
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM vocabs", [], |row| row.get(0))
        .map_err(|err| format!("Failed to inspect database: {err}"))?;

    if count > 0 {
        return Ok(());
    }

    for seed in load_seed_vocab() {
        let category = normalize_text(&seed.category);
        let example = normalize_text(&seed.example);
        let entries = vec![VocabEntry {
            part_of_speech: if category.is_empty() {
                "unknown".into()
            } else {
                category
            },
            pron_uk: String::new(),
            pron_us: normalize_text(&seed.pronunciation.unwrap_or_default()),
            sense_groups: vec![VocabSenseGroup {
                guide_word: String::new(),
                definitions: vec![model::VocabDefinition {
                    id: 0,
                    definition: normalize_text(&seed.meaning),
                    examples: if example.is_empty() {
                        Vec::new()
                    } else {
                        vec![example]
                    },
                }],
            }],
        }];

        let inserted = conn
            .execute(
                "INSERT OR IGNORE INTO vocabs (word, source) VALUES (?1, ?2)",
                params![normalize_text(&seed.word), WordSource::Manual.as_str()],
            )
            .map_err(|err| format!("Failed to seed vocabulary: {err}"))?;

        if inserted > 0 {
            insert_definition_rows(conn, conn.last_insert_rowid(), &entries, &HashMap::new())?;
        }
    }

    Ok(())
}

fn default_backup_path() -> Option<PathBuf> {
    let home = env::var_os("HOME")?;
    let dropbox_dir = Path::new(&home).join("Dropbox");
    if !dropbox_dir.exists() {
        return None;
    }

    Some(dropbox_dir.join("Mavoca").join("vocab.sqlite3"))
}

fn expand_backup_path(value: &str) -> Option<PathBuf> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    let expanded = if trimmed == "~" {
        env::var_os("HOME").map(PathBuf::from)?
    } else if let Some(rest) = trimmed.strip_prefix("~/") {
        env::var_os("HOME").map(PathBuf::from)?.join(rest)
    } else {
        PathBuf::from(trimmed)
    };

    if expanded.is_dir() || expanded.extension().is_none() {
        Some(expanded.join("vocab.sqlite3"))
    } else {
        Some(expanded)
    }
}

fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        params![key],
        |row| row.get(0),
    )
    .optional()
    .map_err(|err| format!("Failed to read setting '{key}': {err}"))
}

fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute(
        r#"
        INSERT INTO settings (key, value)
        VALUES (?1, ?2)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        "#,
        params![key, value],
    )
    .map_err(|err| format!("Failed to save setting '{key}': {err}"))?;

    Ok(())
}

fn ensure_default_settings(conn: &Connection) -> Result<(), String> {
    if get_setting(conn, BACKUP_PATH_SETTING)?.is_none() {
        let backup_path = default_backup_path()
            .and_then(|path| path.to_str().map(|value| value.to_string()))
            .unwrap_or_default();
        set_setting(conn, BACKUP_PATH_SETTING, &backup_path)?;
    }

    if get_setting(conn, ROTATION_INTERVAL_SETTING)?.is_none() {
        set_setting(
            conn,
            ROTATION_INTERVAL_SETTING,
            &DEFAULT_ROTATION_INTERVAL_SECS.to_string(),
        )?;
    }

    if get_setting(conn, ADD_WORD_SHORTCUT_SETTING)?.is_none() {
        set_setting(conn, ADD_WORD_SHORTCUT_SETTING, DEFAULT_ADD_WORD_SHORTCUT)?;
    }

    Ok(())
}

fn load_settings_from_db(conn: &Connection) -> Result<AppSettings, String> {
    Ok(AppSettings {
        backup_path: get_setting(conn, BACKUP_PATH_SETTING)?.unwrap_or_default(),
        add_word_shortcut: get_setting(conn, ADD_WORD_SHORTCUT_SETTING)?.unwrap_or_default(),
        rotation_interval_secs: get_setting(conn, ROTATION_INTERVAL_SETTING)?
            .and_then(|value| value.parse().ok())
            .unwrap_or(DEFAULT_ROTATION_INTERVAL_SECS),
    })
}

fn configured_backup_path(conn: &Connection) -> Result<Option<PathBuf>, String> {
    let settings = load_settings_from_db(conn)?;
    Ok(expand_backup_path(&settings.backup_path))
}

fn sync_database_backup(conn: &Connection) -> Result<(), String> {
    let Some(backup_path) = configured_backup_path(conn)? else {
        return Ok(());
    };

    let backup_dir = backup_path
        .parent()
        .ok_or_else(|| "Invalid backup path".to_string())?;
    fs::create_dir_all(backup_dir)
        .map_err(|err| format!("Failed to create backup directory: {err}"))?;

    let temp_path = backup_path.with_extension("sqlite3.tmp");
    if temp_path.exists() {
        fs::remove_file(&temp_path)
            .map_err(|err| format!("Failed to remove stale backup: {err}"))?;
    }

    let temp_path_str = temp_path
        .to_str()
        .ok_or_else(|| "Backup path is not valid UTF-8".to_string())?;
    conn.execute("VACUUM main INTO ?1", params![temp_path_str])
        .map_err(|err| format!("Failed to create SQLite backup: {err}"))?;

    fs::rename(&temp_path, &backup_path)
        .map_err(|err| format!("Failed to replace backup: {err}"))?;

    Ok(())
}

/// Writes a tree as ordered definition rows. This is the only place
/// `entry_index` and `sort_order` are assigned, which is what keeps the tree
/// well-formed given the schema does not enforce it.
fn insert_definition_rows(
    conn: &Connection,
    vocab_id: i64,
    entries: &[VocabEntry],
    carried_translations: &HashMap<String, String>,
) -> Result<(), String> {
    let mut stmt = conn
        .prepare(
            r#"
            INSERT INTO vocab_definitions (
                vocab_id, entry_index, part_of_speech, pron_uk, pron_us,
                guide_word, definition, definition_vi, examples, sort_order
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            "#,
        )
        .map_err(|err| format!("Failed to prepare definition insert: {err}"))?;

    for (sort_order, row) in entries_to_rows(entries).into_iter().enumerate() {
        let examples = serde_json::to_string(&row.examples)
            .map_err(|err| format!("Failed to encode examples: {err}"))?;
        // A translation belongs to the English text, not to the row: carrying
        // it over by text keeps it through a manager save or a re-fetch, and
        // drops it when the definition itself changed.
        let definition_vi = carried_translations
            .get(&row.definition)
            .cloned()
            .unwrap_or_default();

        stmt.execute(params![
            vocab_id,
            row.entry_index,
            row.part_of_speech,
            row.pron_uk,
            row.pron_us,
            row.guide_word,
            row.definition,
            definition_vi,
            examples,
            sort_order as i64
        ])
        .map_err(|err| format!("Failed to save definition: {err}"))?;
    }

    Ok(())
}

/// Replaces a word's definitions wholesale. Both importing and hand-editing go
/// through here, so a word's tree is never partially updated.
fn replace_word_definitions(
    conn: &Connection,
    vocab_id: i64,
    entries: &[VocabEntry],
) -> Result<(), String> {
    // Read before the delete: the rows are about to be replaced, and their
    // translations only have to survive when their English text did.
    let carried_translations = definition_translations_for(conn, vocab_id)?;

    conn.execute(
        "DELETE FROM vocab_definitions WHERE vocab_id = ?1",
        params![vocab_id],
    )
    .map_err(|err| format!("Failed to clear existing definitions: {err}"))?;

    insert_definition_rows(conn, vocab_id, entries, &carried_translations)
}

/// Every stored Vietnamese definition of a word, keyed by its English text.
fn definition_translations_for(
    conn: &Connection,
    vocab_id: i64,
) -> Result<HashMap<String, String>, String> {
    let mut stmt = conn
        .prepare(
            r#"
            SELECT definition, definition_vi
            FROM vocab_definitions
            WHERE vocab_id = ?1 AND definition_vi <> ''
            "#,
        )
        .map_err(|err| format!("Failed to prepare translation query: {err}"))?;

    let rows = stmt
        .query_map(params![vocab_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|err| format!("Failed to read translations: {err}"))?;

    rows.collect::<rusqlite::Result<HashMap<_, _>>>()
        .map_err(|err| format!("Failed to decode translations: {err}"))
}

/// The Vietnamese definition already stored for a row, if any.
fn stored_definition_vi(conn: &Connection, definition_id: i64) -> Result<Option<String>, String> {
    let stored = conn.query_row(
        "SELECT definition_vi FROM vocab_definitions WHERE id = ?1",
        params![definition_id],
        |row| row.get::<_, String>(0),
    );

    match stored {
        Ok(definition_vi) => Ok(Some(definition_vi).filter(|value| !value.is_empty())),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(err) => Err(format!("Failed to read the Vietnamese definition: {err}")),
    }
}

fn store_definition_vi(
    conn: &Connection,
    definition_id: i64,
    definition_vi: &str,
) -> Result<(), String> {
    conn.execute(
        "UPDATE vocab_definitions SET definition_vi = ?1 WHERE id = ?2",
        params![definition_vi, definition_id],
    )
    .map_err(|err| format!("Failed to store the Vietnamese definition: {err}"))?;

    Ok(())
}

fn definition_row_from_sql(
    row: &rusqlite::Row<'_>,
    offset: usize,
) -> rusqlite::Result<DefinitionRow> {
    let examples: String = row.get(offset + 7)?;

    Ok(DefinitionRow {
        id: row.get(offset)?,
        entry_index: row.get(offset + 1)?,
        part_of_speech: row.get(offset + 2)?,
        pron_uk: row.get(offset + 3)?,
        pron_us: row.get(offset + 4)?,
        guide_word: row.get(offset + 5)?,
        definition: row.get(offset + 6)?,
        examples: serde_json::from_str(&examples).unwrap_or_default(),
    })
}

fn definition_rows_for(conn: &Connection, vocab_id: i64) -> Result<Vec<DefinitionRow>, String> {
    let mut stmt = conn
        .prepare(
            r#"
            SELECT id, entry_index, part_of_speech, pron_uk, pron_us,
                   guide_word, definition, examples
            FROM vocab_definitions
            WHERE vocab_id = ?1
            ORDER BY sort_order ASC
            "#,
        )
        .map_err(|err| format!("Failed to prepare definition query: {err}"))?;

    let rows = stmt
        .query_map(params![vocab_id], |row| definition_row_from_sql(row, 0))
        .map_err(|err| format!("Failed to read definitions: {err}"))?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| format!("Failed to decode definitions: {err}"))
}

fn build_word(
    id: i64,
    word: String,
    source: String,
    created_at: String,
    updated_at: String,
    rows: Vec<DefinitionRow>,
) -> VocabWord {
    let entries = rows_to_entries(rows);

    VocabWord {
        id,
        word,
        source: WordSource::from_str(&source),
        definition_count: count_definitions(&entries),
        entries,
        created_at,
        updated_at,
    }
}

fn list_vocab_from_db(conn: &Connection) -> Result<Vec<VocabWord>, String> {
    let mut definitions_by_word: HashMap<i64, Vec<DefinitionRow>> = HashMap::new();
    {
        let mut stmt = conn
            .prepare(
                r#"
                SELECT vocab_id, id, entry_index, part_of_speech, pron_uk, pron_us,
                       guide_word, definition, examples
                FROM vocab_definitions
                ORDER BY vocab_id ASC, sort_order ASC
                "#,
            )
            .map_err(|err| format!("Failed to prepare definition query: {err}"))?;

        let rows = stmt
            .query_map([], |row| {
                let vocab_id: i64 = row.get(0)?;
                Ok((vocab_id, definition_row_from_sql(row, 1)?))
            })
            .map_err(|err| format!("Failed to read definitions: {err}"))?;

        for row in rows {
            let (vocab_id, definition) =
                row.map_err(|err| format!("Failed to decode definitions: {err}"))?;
            definitions_by_word
                .entry(vocab_id)
                .or_default()
                .push(definition);
        }
    }

    let mut stmt = conn
        .prepare(
            r#"
            SELECT id, word, source, created_at, updated_at
            FROM vocabs
            ORDER BY lower(word) ASC
            "#,
        )
        .map_err(|err| format!("Failed to prepare vocabulary query: {err}"))?;

    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(|err| format!("Failed to read vocabulary: {err}"))?;

    let mut words = Vec::new();
    for row in rows {
        let (id, word, source, created_at, updated_at) =
            row.map_err(|err| format!("Failed to decode vocabulary: {err}"))?;
        let definitions = definitions_by_word.remove(&id).unwrap_or_default();
        words.push(build_word(
            id,
            word,
            source,
            created_at,
            updated_at,
            definitions,
        ));
    }

    Ok(words)
}

fn get_vocab_by_id(conn: &Connection, id: i64) -> Result<Option<VocabWord>, String> {
    let header = conn
        .query_row(
            "SELECT id, word, source, created_at, updated_at FROM vocabs WHERE id = ?1",
            params![id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .optional()
        .map_err(|err| format!("Failed to read vocabulary word: {err}"))?;

    let Some((id, word, source, created_at, updated_at)) = header else {
        return Ok(None);
    };

    let rows = definition_rows_for(conn, id)?;
    Ok(Some(build_word(
        id, word, source, created_at, updated_at, rows,
    )))
}

fn get_vocab_id_by_word(conn: &Connection, word: &str) -> Result<Option<i64>, String> {
    conn.query_row(
        "SELECT id FROM vocabs WHERE word = ?1 COLLATE NOCASE",
        params![word],
        |row| row.get(0),
    )
    .optional()
    .map_err(|err| format!("Failed to read vocabulary word: {err}"))
}

/// Rebuilds the word list and the flattened rotation list, keeping the popup on
/// the same definition where possible. When that definition is gone, it falls
/// back to the same word, then to the start.
fn refresh_vocab_cache(state: &AppState) -> Result<Vec<VocabWord>, String> {
    let list = {
        let conn = state.db.lock().unwrap();
        list_vocab_from_db(&conn)?
    };
    let cards = build_cards(&list);

    let previous = {
        let existing = state.cards.lock().unwrap();
        let current_index = *state.current_index.lock().unwrap();
        existing
            .get(current_index)
            .map(|card| (card.vocab_id, card.position))
    };

    let next_index = previous
        .and_then(|(vocab_id, position)| {
            cards
                .iter()
                .position(|card| card.vocab_id == vocab_id && card.position == position)
                .or_else(|| cards.iter().position(|card| card.vocab_id == vocab_id))
        })
        .unwrap_or(0);

    *state.vocab_list.lock().unwrap() = list.clone();
    *state.cards.lock().unwrap() = cards;
    *state.current_index.lock().unwrap() = next_index;

    Ok(list)
}

fn sync_backup_from_state(state: &AppState) -> Result<(), String> {
    let conn = state.db.lock().unwrap();
    sync_database_backup(&conn)
}

fn current_card(state: &AppState) -> Option<VocabCard> {
    let cards = state.cards.lock().unwrap();
    let index = *state.current_index.lock().unwrap();
    cards.get(index).cloned()
}

/// Moves the rotation to `index` and tells the popup and tray about it.
fn apply_card_index(app: &tauri::AppHandle, state: &AppState, index: usize) -> Option<VocabCard> {
    let card = {
        let cards = state.cards.lock().unwrap();
        cards.get(index).cloned()
    }?;

    *state.current_index.lock().unwrap() = index;
    update_tray_title(app, &card.word);
    let _ = app.emit("vocab-changed", card.clone());

    Some(card)
}

fn sync_after_vocab_change(
    app: &tauri::AppHandle,
    state: &AppState,
) -> Result<Vec<VocabWord>, String> {
    let list = refresh_vocab_cache(state)?;

    if let Some(card) = current_card(state) {
        update_tray_title(app, &card.word);
        let _ = app.emit("vocab-changed", card);
    } else {
        update_tray_title(app, "");
        let _ = app.emit("vocab-empty", ());
    }

    let _ = app.emit("vocab-list-changed", list.clone());
    sync_backup_from_state(state)?;
    Ok(list)
}

fn update_tray_title(app: &tauri::AppHandle, word: &str) {
    if let Some(tray) = app.tray_by_id("main") {
        let title = if word.is_empty() { None } else { Some(word) };
        let _ = tray.set_title(title);
    }
}

#[cfg(target_os = "macos")]
use objc2::ClassType;

#[cfg(target_os = "macos")]
objc2::define_class!(
    /// A panel that can take the keyboard while the app stays in the background.
    ///
    /// `NSPanel` only becomes key when it has a title bar or a resize bar, and a
    /// frameless window has neither, so the answer is written out here. The
    /// class carries no instance variables of its own: `convert_to_panel`
    /// retypes a window in place, and that only survives if the new class has
    /// exactly the layout of the old one.
    #[unsafe(super(objc2_app_kit::NSPanel))]
    #[thread_kind = objc2::MainThreadOnly]
    #[name = "MavocaPanel"]
    #[ivars = ()]
    struct MavocaPanel;

    impl MavocaPanel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn __can_become_key_window(&self) -> bool {
            true
        }
    }
);

/// Turns a window into a non-activating panel, so it takes the keyboard without
/// activating the app.
///
/// macOS does not activate an agent app (`LSUIElement`) that asks for it, and a
/// plain `NSWindow` only holds the keyboard while its app is active. A panel
/// carrying the `NSWindowStyleMaskNonactivatingPanel` mask is key *without* the
/// app being activated — the arrangement Spotlight, Alfred, and every other
/// agent app relies on.
///
/// **Off by default.** Retyping a window in place works for the keyboard — the
/// quick-add window took typing while another app stayed in front — but on
/// macOS 27 every window is an `AppKit._NSGlassEffectWindow`, and replacing that
/// class takes its mouse hit testing with it: a click inside the manager lands
/// on whatever is behind it, and one attempt aborted the app on a foreign
/// exception thrown out of AppKit. The panel is kept behind `MAVOCA_PANEL_MODE`
/// (`full` turns it on) until it can be built the way AppKit expects, by
/// creating the panel rather than converting one.
///
/// Called on every show: the manager and settings windows are destroyed when
/// they close, so the window that comes back is a fresh one.
#[cfg(target_os = "macos")]
fn attach_panel(window: &tauri::WebviewWindow, floating: bool) {
    if objc2::MainThreadMarker::new().is_some() {
        convert_to_panel(window, floating);
        return;
    }

    // AppKit only lets the main thread near this, and the class is registered
    // on the way in.
    let window = window.clone();
    let _ = window
        .clone()
        .run_on_main_thread(move || convert_to_panel(&window, floating));
}

#[cfg(target_os = "macos")]
fn panel_mode() -> String {
    std::env::var("MAVOCA_PANEL_MODE").unwrap_or_else(|_| "off".to_string())
}

#[cfg(target_os = "macos")]
fn convert_to_panel(window: &tauri::WebviewWindow, floating: bool) {
    unsafe extern "C" {
        fn object_setClass(
            object: *mut objc2::runtime::AnyObject,
            class: *const objc2::runtime::AnyClass,
        ) -> *const objc2::runtime::AnyClass;
    }

    let mode = panel_mode();
    if mode == "off" || objc2::MainThreadMarker::new().is_none() {
        return;
    }
    let Ok(ns_window) = window.ns_window() else {
        return;
    };
    unsafe {
        let object = ns_window.cast::<objc2::runtime::AnyObject>();
        object_setClass(object, MavocaPanel::class() as *const _);
        if mode == "class" {
            return;
        }

        // The window is an `NSPanel` now; only the mask is left to change.
        let panel = &*object.cast::<objc2_app_kit::NSPanel>();
        let mask = panel.styleMask() | objc2_app_kit::NSWindowStyleMask::NonactivatingPanel;
        panel.setStyleMask(mask);
        // Retyping skips `NSPanel`'s own initialization, and `setStyleMask:` does
        // not update the tag the window server uses to decide whether opening
        // the window activates the app.
        let selector = objc2::sel!(_setPreventsActivation:);
        let responds: bool = objc2::msg_send![panel, respondsToSelector: selector];
        if responds {
            let nonactivating = panel
                .styleMask()
                .contains(objc2_app_kit::NSWindowStyleMask::NonactivatingPanel);
            let _: () = objc2::msg_send![panel, _setPreventsActivation: nonactivating];
        }
        if mode != "full" {
            return;
        }
        panel.setFloatingPanel(floating);
        panel.setHidesOnDeactivate(false);
        panel.setBecomesKeyOnlyIfNeeded(false);
        panel.setReleasedWhenClosed(false);
        // A panel is allowed over full screen apps and follows the user to
        // whichever space is in front, like the menu bar item it belongs to.
        panel.setCollectionBehavior(
            panel.collectionBehavior()
                | objc2_app_kit::NSWindowCollectionBehavior::CanJoinAllSpaces
                | objc2_app_kit::NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
    }
}

/// Gives a window the keyboard after it is shown.
///
/// With panels off this is `Window::set_focus`, which is what an agent app can
/// ask for: tao orders the window front and asks macOS to activate the app,
/// and the system grants it only around a real click. With panels on the panel
/// is keyed directly, which is all a window needs to take typing.
#[cfg(target_os = "macos")]
fn make_panel_key(window: &tauri::WebviewWindow) {
    if panel_mode() == "off" {
        let _ = window.set_focus();
        return;
    }
    let window = window.clone();
    let _ = window.clone().run_on_main_thread(move || {
        if objc2::MainThreadMarker::new().is_none() {
            return;
        }
        let Ok(ns_window) = window.ns_window() else {
            return;
        };
        unsafe {
            let panel = &*(ns_window.cast::<objc2_app_kit::NSPanel>());
            panel.makeKeyAndOrderFront(None);
            panel.makeKeyWindow();
        }
    });
}

/// Keeps the menu bar item highlighted while the popup is open, the way macOS
/// highlights an item while its own menu or popover is open.
///
/// Since macOS 26 the menu bar draws that highlight itself, and only for a
/// status item that is presenting content: `NSStatusBarButton.highlight` sets
/// the button's state but nothing is drawn. `NSPopover` marks the presentation
/// with the private `_willPresentContent:cancellationHandler:` and ends it with
/// `_didPresentContent:`; the popup is a plain window, so it makes the same two
/// calls. Both are checked with `respondsToSelector:` first, so a macOS that
/// drops them only loses the highlight.
fn set_tray_highlighted(app: &tauri::AppHandle, highlighted: bool) {
    #[cfg(target_os = "macos")]
    {
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            let Some(mtm) = objc2::MainThreadMarker::new() else {
                return;
            };
            let Some(status_item) = find_status_item(mtm) else {
                return;
            };
            if highlighted {
                begin_tray_presentation(&handle, &status_item);
            } else {
                end_tray_presentation(&status_item);
            }
        });
    }

    #[cfg(not(target_os = "macos"))]
    let _ = (app, highlighted);
}

#[cfg(target_os = "macos")]
thread_local! {
    /// The object the status item is presenting while the popup is open. The
    /// same object has to be handed back to end the presentation. Only touched
    /// on the main thread.
    static TRAY_PRESENTATION: std::cell::RefCell<Option<objc2::rc::Retained<objc2::runtime::NSObject>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(target_os = "macos")]
fn responds_to(object: &objc2::runtime::AnyObject, selector: objc2::runtime::Sel) -> bool {
    unsafe { objc2::msg_send![object, respondsToSelector: selector] }
}

/// Tauri 2.7 has no handle to the `NSStatusItem` (`with_inner_tray_icon`
/// arrives in 2.10), so it is reached through its button, which sits in one of
/// the app's status bar windows. The app has a single tray item.
#[cfg(target_os = "macos")]
fn find_status_item(
    mtm: objc2::MainThreadMarker,
) -> Option<objc2::rc::Retained<objc2::runtime::AnyObject>> {
    fn find_button(
        view: &objc2_app_kit::NSView,
    ) -> Option<objc2::rc::Retained<objc2_app_kit::NSView>> {
        if view
            .downcast_ref::<objc2_app_kit::NSStatusBarButton>()
            .is_some()
        {
            return Some(objc2::Message::retain(view));
        }
        // objc2 has not audited this binding, so it is marked unsafe; reading
        // a view's subviews on the main thread has no other requirement.
        unsafe { view.subviews() }
            .iter()
            .find_map(|subview| find_button(&subview))
    }

    let button = objc2_app_kit::NSApplication::sharedApplication(mtm)
        .windows()
        .iter()
        .filter_map(|window| window.contentView())
        .find_map(|view| find_button(&view))?;

    let selector = objc2::sel!(_statusItem);
    if !responds_to(&button, selector) {
        return None;
    }
    unsafe { objc2::msg_send![&*button, _statusItem] }
}

#[cfg(target_os = "macos")]
fn begin_tray_presentation(app: &tauri::AppHandle, status_item: &objc2::runtime::AnyObject) {
    let will_present = objc2::sel!(_willPresentContent:cancellationHandler:);
    let did_present = objc2::sel!(_didPresentContent:);
    if TRAY_PRESENTATION.with_borrow(Option::is_some)
        || !responds_to(status_item, will_present)
        || !responds_to(status_item, did_present)
    {
        return;
    }

    let token = objc2::runtime::NSObject::new();
    // The menu bar calls this when it takes the highlight away, for example
    // when another menu opens; the popup closes with it, as a popover would.
    let app = app.clone();
    let on_cancel = block2::RcBlock::new(move || hide_popup(&app));
    unsafe {
        let _: () = objc2::msg_send![
            status_item,
            _willPresentContent: &*token,
            cancellationHandler: &*on_cancel
        ];
    }
    TRAY_PRESENTATION.set(Some(token));
}

#[cfg(target_os = "macos")]
fn end_tray_presentation(status_item: &objc2::runtime::AnyObject) {
    let Some(token) = TRAY_PRESENTATION.take() else {
        return;
    };
    unsafe {
        let _: () = objc2::msg_send![status_item, _didPresentContent: &*token];
    }
}

/// Hides the popup and clears the menu bar highlight.
fn hide_popup(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        *state.is_window_visible.lock().unwrap() = false;
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    set_tray_highlighted(app, false);
}

#[tauri::command]
fn list_vocab_words(state: tauri::State<AppState>) -> Result<Vec<VocabWord>, String> {
    refresh_vocab_cache(&state)
}

#[tauri::command]
fn get_current_vocab_card(state: tauri::State<AppState>) -> Option<VocabCard> {
    current_card(&state)
}

/// Steps the rotation by `delta` definitions, wrapping at both ends. This walks
/// every definition of the current word before crossing into the next word.
#[tauri::command]
fn step_vocab_card(
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
    delta: i32,
) -> Option<VocabCard> {
    // Locks are taken one at a time: nothing in this file ever holds `cards`
    // and `current_index` together.
    let len = state.cards.lock().unwrap().len();
    if len == 0 {
        return None;
    }

    let current = *state.current_index.lock().unwrap();

    let len_i64 = len as i64;
    let next = (current as i64 + delta as i64).rem_euclid(len_i64) as usize;
    apply_card_index(&app, &state, next)
}

/// Steps the rotation by one word, landing on the first definition of the word
/// before or after the current one.
///
/// The popup's footer arrows move between words; the ‹ › stepper on the card's
/// word line is what walks the definitions of one word (`step_vocab_card`).
#[tauri::command]
fn step_vocab_word(
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
    direction: i32,
) -> Option<VocabCard> {
    // Locks are taken one at a time: nothing in this file ever holds `cards`
    // and `current_index` together.
    let current = *state.current_index.lock().unwrap();

    let next = {
        let cards = state.cards.lock().unwrap();
        word_step_index(&cards, current, direction)
    }?;

    apply_card_index(&app, &state, next)
}

/// The card a word step lands on: the first definition of the word before or
/// after the one at `current`, wrapping at both ends.
///
/// `build_cards` puts a word's definitions next to each other, so a word is a run
/// of cards sharing a `vocab_id`.
fn word_step_index(cards: &[VocabCard], current: usize, direction: i32) -> Option<usize> {
    if cards.is_empty() {
        return None;
    }
    let current = current.min(cards.len() - 1);
    let word = cards[current].vocab_id;

    if direction >= 0 {
        let mut end = current;
        while end < cards.len() && cards[end].vocab_id == word {
            end += 1;
        }
        // Wrapping past the last word means the first card of the rotation.
        return Some(end % cards.len());
    }

    let mut start = current;
    while start > 0 && cards[start - 1].vocab_id == word {
        start -= 1;
    }
    if start == 0 {
        // Back from the first word lands on the start of the last one.
        let last_word = cards[cards.len() - 1].vocab_id;
        let mut last = cards.len() - 1;
        while last > 0 && cards[last - 1].vocab_id == last_word {
            last -= 1;
        }
        return Some(last);
    }

    let previous_word = cards[start - 1].vocab_id;
    let mut previous = start - 1;
    while previous > 0 && cards[previous - 1].vocab_id == previous_word {
        previous -= 1;
    }
    Some(previous)
}

#[tauri::command]
fn random_vocab_card(state: tauri::State<AppState>, app: tauri::AppHandle) -> Option<VocabCard> {
    let len = state.cards.lock().unwrap().len();
    if len == 0 {
        return None;
    }

    // Cheap, dependency-free jitter: the clock is plenty for shuffling a popup.
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.subsec_nanos() as usize)
        .unwrap_or(0);

    apply_card_index(&app, &state, seed % len)
}

/// Jumps the rotation to the first definition of `id`.
#[tauri::command]
fn set_current_vocab_id(
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
    id: i64,
) -> Result<(), String> {
    let index = {
        let cards = state.cards.lock().unwrap();
        cards.iter().position(|card| card.vocab_id == id)
    };

    let Some(index) = index else {
        return Err("Vocabulary word was not found".into());
    };

    apply_card_index(&app, &state, index);
    Ok(())
}

#[tauri::command]
fn get_settings(state: tauri::State<AppState>) -> Result<AppSettings, String> {
    let conn = state.db.lock().unwrap();
    load_settings_from_db(&conn)
}

/// Replaces the registered add-word shortcut. An empty value leaves none
/// registered. Fails without changing anything when the value does not parse.
fn register_add_word_shortcut(app: &tauri::AppHandle, value: &str) -> Result<(), String> {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

    let value = value.trim();
    let shortcut = if value.is_empty() {
        None
    } else {
        Some(
            value
                .parse::<Shortcut>()
                .map_err(|err| format!("'{value}' is not a valid shortcut: {err}"))?,
        )
    };

    let manager = app.global_shortcut();
    manager
        .unregister_all()
        .map_err(|err| format!("Failed to clear the shortcut: {err}"))?;
    if let Some(shortcut) = shortcut {
        manager.register(shortcut).map_err(|err| {
            format!("Couldn't use {value}; another app may already own it. ({err})")
        })?;
    }
    Ok(())
}

#[tauri::command]
fn update_settings(
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
    settings: AppSettings,
) -> Result<AppSettings, String> {
    if settings.rotation_interval_secs > MAX_ROTATION_INTERVAL_SECS {
        return Err(format!(
            "Rotation can wait at most {MAX_ROTATION_INTERVAL_SECS} seconds"
        ));
    }

    let conn = state.db.lock().unwrap();
    let shortcut = settings.add_word_shortcut.trim();
    if let Err(err) = register_add_word_shortcut(&app, shortcut) {
        // Put the saved shortcut back so a rejected one never leaves it unset.
        let saved = get_setting(&conn, ADD_WORD_SHORTCUT_SETTING)?.unwrap_or_default();
        let _ = register_add_word_shortcut(&app, &saved);
        return Err(err);
    }
    set_setting(&conn, ADD_WORD_SHORTCUT_SETTING, shortcut)?;
    set_setting(
        &conn,
        ROTATION_INTERVAL_SETTING,
        &settings.rotation_interval_secs.to_string(),
    )?;
    state
        .rotation_interval_secs
        .store(settings.rotation_interval_secs, Ordering::Relaxed);
    set_setting(&conn, BACKUP_PATH_SETTING, settings.backup_path.trim())?;
    sync_database_backup(&conn)?;
    load_settings_from_db(&conn)
}

/// The login item lives in a LaunchAgent the OS owns, so it is read back
/// rather than mirrored in the settings table where it could drift.
#[tauri::command]
fn get_launch_at_login(app: tauri::AppHandle) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;

    app.autolaunch()
        .is_enabled()
        .map_err(|err| format!("Failed to read the login item: {err}"))
}

#[tauri::command]
fn set_launch_at_login(app: tauri::AppHandle, enabled: bool) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;

    let autolaunch = app.autolaunch();
    if enabled {
        autolaunch.enable()
    } else {
        autolaunch.disable()
    }
    .map_err(|err| format!("Failed to update the login item: {err}"))?;

    autolaunch
        .is_enabled()
        .map_err(|err| format!("Failed to read the login item: {err}"))
}

/// Writes a word row plus its whole tree in one transaction, so a word is never
/// left with a half-written set of definitions.
fn save_word_tree(
    conn: &Connection,
    id: Option<i64>,
    word: &str,
    source: WordSource,
    entries: &[VocabEntry],
) -> Result<i64, String> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|err| format!("Failed to start save: {err}"))?;

    let vocab_id = match id {
        Some(id) => {
            let updated = tx
                .execute(
                    r#"
                    UPDATE vocabs
                    SET word = ?1, source = ?2, updated_at = datetime('now')
                    WHERE id = ?3
                    "#,
                    params![word, source.as_str(), id],
                )
                .map_err(|err| format!("Failed to update vocabulary: {err}"))?;

            if updated == 0 {
                return Err("Vocabulary word was not found".into());
            }

            id
        }
        None => {
            tx.execute(
                "INSERT INTO vocabs (word, source) VALUES (?1, ?2)",
                params![word, source.as_str()],
            )
            .map_err(|err| format!("Failed to create vocabulary: {err}"))?;

            tx.last_insert_rowid()
        }
    };

    replace_word_definitions(&tx, vocab_id, entries)?;

    tx.commit()
        .map_err(|err| format!("Failed to commit save: {err}"))?;

    Ok(vocab_id)
}

/// A word offered under the add-word field.
#[derive(Serialize)]
struct WordSuggestion {
    word: String,
    /// Already in the vocabulary: the list doubles as a duplicate warning, and
    /// those words are listed first.
    saved: bool,
}

/// Words to offer while typing in the add-word window.
///
/// Saved words first, matching what `add_vocab_word` would refuse as a
/// duplicate; the system word list then fills the list up. The frontend debounces
/// this, so one call answers one pause in typing rather than every keystroke.
#[tauri::command]
fn suggest_words(
    state: tauri::State<AppState>,
    prefix: String,
) -> Result<Vec<WordSuggestion>, String> {
    let prefix = normalize_text(&prefix).to_lowercase();
    if prefix.chars().count() < 2 {
        return Ok(Vec::new());
    }

    let saved = {
        let conn = state.db.lock().unwrap();
        saved_words_starting_with(&conn, &prefix, MAX_SUGGESTIONS)?
    };

    let taken: Vec<String> = saved.iter().map(|word| word.to_lowercase()).collect();
    let mut suggestions: Vec<WordSuggestion> = saved
        .into_iter()
        .map(|word| WordSuggestion { word, saved: true })
        .collect();

    // The system list knows the saved words too, so they are skipped rather than
    // offered twice.
    for word in completion::suggest(&prefix, MAX_SUGGESTIONS * 2) {
        if suggestions.len() == MAX_SUGGESTIONS {
            break;
        }
        if taken.contains(&word) {
            continue;
        }
        suggestions.push(WordSuggestion { word, saved: false });
    }

    Ok(suggestions)
}

fn saved_words_starting_with(
    conn: &Connection,
    prefix: &str,
    limit: usize,
) -> Result<Vec<String>, String> {
    // `%` and `_` are LIKE wildcards, and the prefix is text the user typed.
    let escaped = prefix
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let mut stmt = conn
        .prepare(
            "SELECT word FROM vocabs WHERE word LIKE ?1 || '%' ESCAPE '\\' \
             ORDER BY length(word), word LIMIT ?2",
        )
        .map_err(|err| format!("Failed to prepare suggestion query: {err}"))?;
    let words = stmt
        .query_map(params![escaped, limit as i64], |row| row.get(0))
        .map_err(|err| format!("Failed to read suggestions: {err}"))?;

    words
        .collect::<Result<Vec<String>, _>>()
        .map_err(|err| format!("Failed to read suggestions: {err}"))
}

/// One definition shown beside the add-word list.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PreviewDefinition {
    part_of_speech: String,
    pron_uk: String,
    pron_us: String,
    guide_word: String,
    definition: String,
}

/// The meaning of the suggestion the add-word list has highlighted.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WordPreview {
    word: String,
    /// Already in the vocabulary, so adding it would be refused as a duplicate.
    saved: bool,
    /// `saved` read the vocabulary, `macos` the installed dictionary,
    /// `cambridge` and `dictionaryapi` the online sources, and `none` found no
    /// entry at all.
    source: String,
    definitions: Vec<PreviewDefinition>,
    /// Definitions the entry has, which can be more than are shown.
    total: usize,
    /// The stored Vietnamese translation of the first definition, when there is
    /// one; only a saved word has any.
    definition_vi: Option<String>,
}

/// The meaning of a suggestion, for the pane beside the add-word list.
///
/// A saved word is read from the database, so the pane shows exactly what the
/// user already has and the list doubles as a duplicate warning. Anything else
/// is fetched the way `add_vocab_word` fetches it, the installed dictionary
/// first and Cambridge after, so the pane answers for the words Oxford lacks
/// too. Nothing is written: a preview must not change the vocabulary.
///
/// Async and moved to a worker for the same reason as `lac_viet_meaning`: the
/// dictionary bridge finishes its work on the main queue.
#[tauri::command]
async fn preview_word(
    state: tauri::State<'_, AppState>,
    word: String,
) -> Result<WordPreview, String> {
    let word = normalize_text(&word);
    if word.is_empty() {
        return Err("Enter a word to look up".into());
    }

    let saved = {
        let conn = state.db.lock().unwrap();
        saved_word_preview(&conn, &word)?
    };
    if let Some(preview) = saved {
        return Ok(preview);
    }

    let lookup = word.clone();
    let fetched = tauri::async_runtime::spawn_blocking(move || dictionary::fetch_word(&lookup))
        .await
        .map_err(|err| format!("Dictionary task failed: {err}"))?;

    // A word none of the sources carries is not an error here either: the pane
    // says so, and adding it reports the failure in the field's own status line.
    Ok(match fetched {
        Ok(fetched) => {
            preview_from_entries(word, fetched.entries, false, None, fetched.source.as_str())
        }
        Err(_) => WordPreview {
            word,
            saved: false,
            source: "none".into(),
            definitions: Vec::new(),
            total: 0,
            definition_vi: None,
        },
    })
}

fn saved_word_preview(conn: &Connection, word: &str) -> Result<Option<WordPreview>, String> {
    let vocab_id: Option<i64> = conn
        .query_row(
            "SELECT id FROM vocabs WHERE word = ?1",
            params![word],
            |row| row.get(0),
        )
        .optional()
        .map_err(|err| format!("Failed to read vocabulary: {err}"))?;
    let Some(vocab_id) = vocab_id else {
        return Ok(None);
    };

    let rows = definition_rows_for(conn, vocab_id)?;
    // The pane stands in for the card, so it carries the translation the card
    // would show for the definition it puts first.
    let definition_vi = match rows.first() {
        Some(row) => stored_definition_vi(conn, row.id)?,
        None => None,
    };
    let entries = rows_to_entries(rows);

    Ok(Some(preview_from_entries(
        word.to_string(),
        entries,
        true,
        definition_vi,
        "saved",
    )))
}

/// Flattens the tree into the first few definitions, which is all the pane has
/// room for; the popup shows the whole card once the word is added.
fn preview_from_entries(
    word: String,
    entries: Vec<VocabEntry>,
    saved: bool,
    definition_vi: Option<String>,
    source: &str,
) -> WordPreview {
    let mut definitions = Vec::new();
    for entry in &entries {
        for group in &entry.sense_groups {
            for definition in &group.definitions {
                if definitions.len() == PREVIEW_DEFINITIONS {
                    break;
                }
                definitions.push(PreviewDefinition {
                    part_of_speech: entry.part_of_speech.clone(),
                    pron_uk: entry.pron_uk.clone(),
                    pron_us: entry.pron_us.clone(),
                    guide_word: group.guide_word.clone(),
                    definition: definition.definition.clone(),
                });
            }
        }
    }

    WordPreview {
        word,
        saved,
        source: source.to_string(),
        definitions,
        total: count_definitions(&entries),
        definition_vi,
    }
}

/// Adds a word by fetching it and importing every definition, no picker.
/// Cambridge is tried first; the fetch layer falls back on its own.
///
/// Runs off the main thread: the fetch is a network call, and the quick-add
/// window has to keep painting while it waits.
#[tauri::command]
async fn add_vocab_word(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    word: String,
) -> Result<VocabWord, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || add_vocab_word_blocking(&state, &app, &word))
        .await
        .map_err(|err| format!("Add task failed: {err}"))?
}

fn add_vocab_word_blocking(
    state: &AppState,
    app: &tauri::AppHandle,
    word: &str,
) -> Result<VocabWord, String> {
    let clean_word = normalize_text(word);
    if clean_word.is_empty() {
        return Err("Enter a word to add".into());
    }

    {
        let conn = state.db.lock().unwrap();
        if get_vocab_id_by_word(&conn, &clean_word)?.is_some() {
            return Err(format!("'{clean_word}' is already in your vocabulary"));
        }
    }

    // Fetched before taking the database lock: this is a network call and must
    // not block the popup or the tray.
    let fetched = dictionary::fetch_word(&clean_word)?;

    let saved = {
        let conn = state.db.lock().unwrap();
        let id = save_word_tree(&conn, None, &clean_word, fetched.source, &fetched.entries)?;
        get_vocab_by_id(&conn, id)?.ok_or_else(|| "Created word was not found".to_string())?
    };

    sync_after_vocab_change(app, state)?;
    Ok(saved)
}

/// Re-fetches a word and replaces its definitions, used by the manager's
/// Refresh button to enrich entries that were migrated or hand-written.
#[tauri::command]
fn refresh_vocab_word(
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
    id: i64,
) -> Result<VocabWord, String> {
    let existing_word = {
        let conn = state.db.lock().unwrap();
        get_vocab_by_id(&conn, id)?.ok_or_else(|| "Vocabulary word was not found".to_string())?
    };

    let fetched = dictionary::fetch_word(&existing_word.word)?;

    let saved = {
        let conn = state.db.lock().unwrap();
        save_word_tree(
            &conn,
            Some(id),
            &existing_word.word,
            fetched.source,
            &fetched.entries,
        )?;
        get_vocab_by_id(&conn, id)?.ok_or_else(|| "Refreshed word was not found".to_string())?
    };

    sync_after_vocab_change(&app, &state)?;
    Ok(saved)
}

#[tauri::command]
fn create_vocab(
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
    input: VocabTreeInput,
) -> Result<VocabWord, String> {
    let (word, entries) = validate_tree_input(&input)?;

    let saved = {
        let conn = state.db.lock().unwrap();
        let existing_id = get_vocab_id_by_word(&conn, &word)?;
        let id = save_word_tree(&conn, existing_id, &word, WordSource::Manual, &entries)?;
        get_vocab_by_id(&conn, id)?.ok_or_else(|| "Created word was not found".to_string())?
    };

    sync_after_vocab_change(&app, &state)?;
    Ok(saved)
}

#[tauri::command]
fn update_vocab(
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
    id: i64,
    input: VocabTreeInput,
) -> Result<VocabWord, String> {
    let (word, entries) = validate_tree_input(&input)?;

    let saved = {
        let conn = state.db.lock().unwrap();
        save_word_tree(&conn, Some(id), &word, WordSource::Manual, &entries)?;
        get_vocab_by_id(&conn, id)?.ok_or_else(|| "Vocabulary word was not found".to_string())?
    };

    sync_after_vocab_change(&app, &state)?;
    Ok(saved)
}

#[tauri::command]
fn delete_vocab(
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
    id: i64,
) -> Result<(), String> {
    {
        let conn = state.db.lock().unwrap();
        conn.execute("DELETE FROM vocabs WHERE id = ?1", params![id])
            .map_err(|err| format!("Failed to delete vocabulary: {err}"))?;
    }

    sync_after_vocab_change(&app, &state)?;
    Ok(())
}

/// Lạc Việt's entry for a card, or `None` when it does not have the word.
///
/// The lookup is local and instant, so the popup shows the Vietnamese section
/// as soon as the card appears. When the definition has already been translated
/// and stored, that translation orders the senses; otherwise they stay in Lạc
/// Việt's own order rather than waiting for the translator.
///
/// Blocking for the same reason as `translate_definition`: the dictionary
/// bridge talks to DictionaryServices, which finishes on the main queue.
#[tauri::command]
async fn lac_viet_meaning(
    state: tauri::State<'_, AppState>,
    word: String,
    part_of_speech: String,
    definition_id: Option<i64>,
) -> Result<Option<vietnamese::LacVietMeaning>, String> {
    let translated_definition = match definition_id {
        Some(id) => {
            let conn = state.db.lock().unwrap();
            stored_definition_vi(&conn, id)?
        }
        None => None,
    };

    tauri::async_runtime::spawn_blocking(move || {
        vietnamese::lac_viet_meaning(&word, &part_of_speech, translated_definition.as_deref())
    })
    .await
    .map_err(|err| format!("Dictionary task failed: {err}"))
}

/// The definition in Vietnamese, stored with the row when it has an id so the
/// card is never translated twice.
///
/// Async so Tauri runs it off the main thread, and the blocking work moves to
/// a worker: Translation.framework finishes on the main queue, so holding the
/// main thread here would stall until the bridge times out.
#[tauri::command]
async fn translate_definition(
    state: tauri::State<'_, AppState>,
    definition: String,
    definition_id: Option<i64>,
) -> Result<String, String> {
    if let Some(id) = definition_id {
        let stored = {
            let conn = state.db.lock().unwrap();
            stored_definition_vi(&conn, id)?
        };
        if let Some(stored) = stored {
            return Ok(stored);
        }
    }

    let state = state.inner().clone();
    let translated = {
        let definition = definition.clone();
        tauri::async_runtime::spawn_blocking(move || vietnamese::translate_text(&definition))
            .await
            .map_err(|err| format!("Translation task failed: {err}"))??
    };

    if let (Some(id), false) = (definition_id, translated.is_empty()) {
        let conn = state.db.lock().unwrap();
        store_definition_vi(&conn, id, &translated)?;
    }

    Ok(translated)
}

/// The word itself, for the popup to show under VI when Lạc Việt has no entry
/// for it. Kept separate from `translate_definition` so a card whose meaning
/// Lạc Việt does have never pays for a second translation.
#[tauri::command]
async fn translate_word(word: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || vietnamese::translate_text(&word))
        .await
        .map_err(|err| format!("Translation task failed: {err}"))?
}

#[tauri::command]
fn show_main_window(app: tauri::AppHandle, state: tauri::State<AppState>) {
    *state.is_window_visible.lock().unwrap() = true;
    if let Some(window) = app.get_webview_window("main") {
        // Shown from the frontend after a word was added, so there is no click
        // on the menu bar item to line the popup up with: the item's own frame
        // is the anchor instead, and the corner is only a fallback.
        #[cfg(target_os = "macos")]
        let placed = position_popup_under_tray(&window);
        #[cfg(not(target_os = "macos"))]
        let placed = false;
        if !placed {
            let _ = position_popup_window(&window);
        }
        show_window(&window, true);
        set_tray_highlighted(&app, true);
    }
}

#[tauri::command]
fn show_manager_window(app: tauri::AppHandle) -> Result<(), String> {
    let Some(window) = app.get_webview_window("manager") else {
        return Err("Manager window was not found".into());
    };

    show_window(&window, false);
    Ok(())
}

#[tauri::command]
fn show_settings_window(app: tauri::AppHandle) -> Result<(), String> {
    let Some(window) = app.get_webview_window("settings") else {
        return Err("Settings window was not found".into());
    };

    show_window(&window, false);
    Ok(())
}

/// Opens the quick-add window; the add-word shortcut lands here.
fn show_quick_add_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(QUICK_ADD_WINDOW_LABEL) {
        // The window hides on blur, list open or not, so each opening starts at
        // its own height instead of remembering the list's.
        let _ = window.set_size(tauri::LogicalSize::new(QUICK_ADD_WIDTH, QUICK_ADD_HEIGHT));
        let _ = window.center();
        show_window(&window, true);
    }
}

/// Shows a window and gives it the keyboard.
///
/// The app is a menu bar agent: it has no Dock icon and no Cmd+Tab row, and
/// `LSUIElement` is what keeps it that way, whether or not a window is open.
fn show_window(window: &tauri::WebviewWindow, floating: bool) {
    #[cfg(target_os = "macos")]
    attach_panel(window, floating);
    #[cfg(not(target_os = "macos"))]
    let _ = floating;

    let _ = window.show();
    #[cfg(target_os = "macos")]
    make_panel_key(window);
}

/// Shows a window opened from the tray menu.
fn show_tray_window(app: &tauri::AppHandle, label: &str) {
    if let Some(window) = app.get_webview_window(label) {
        show_window(&window, true);
    }
}

#[tauri::command]
fn hide_quick_add_window(app: tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(QUICK_ADD_WINDOW_LABEL) {
        let _ = window.hide();
    }
}

/// Grows the quick-add window while it shows a suggestion list or a meaning, and
/// shrinks it back afterwards. The field sits at a fixed offset from the top left
/// of the window, so only the list and the pane gain room and nothing on screen
/// moves.
#[tauri::command]
fn resize_quick_add_window(app: tauri::AppHandle, width: f64, height: f64) -> Result<(), String> {
    let Some(window) = app.get_webview_window(QUICK_ADD_WINDOW_LABEL) else {
        return Err("Add word window was not found".into());
    };

    // The frontend measures its own content, which can be the meaning pane as
    // well as the list; the clamps only stop a bad measurement from escaping the
    // screen.
    let width = width.clamp(QUICK_ADD_WIDTH, QUICK_ADD_MAX_WIDTH);
    let height = height.clamp(QUICK_ADD_HEIGHT, QUICK_ADD_MAX_HEIGHT);
    window
        .set_size(tauri::LogicalSize::new(width, height))
        .map_err(|err| format!("Failed to resize the add word window: {err}"))
}

#[tauri::command]
fn hide_main_window(app: tauri::AppHandle) {
    hide_popup(&app);
}

/// Fits the popup to its card. The frontend reports the card's natural height
/// whenever it changes; the result is clamped so a long card never runs off
/// the bottom of the screen, and the card scrolls past that.
#[tauri::command]
fn resize_popup(app: tauri::AppHandle, height: f64) -> Result<(), String> {
    let Some(window) = app.get_webview_window("main") else {
        return Err("Popup window was not found".into());
    };

    let mut max_height = POPUP_MAX_HEIGHT;
    if let (Ok(Some(monitor)), Ok(position)) = (window.current_monitor(), window.outer_position()) {
        let scale_factor = monitor.scale_factor();
        let screen_bottom =
            (monitor.position().y as f64 + monitor.size().height as f64) / scale_factor;
        let room = screen_bottom - position.y as f64 / scale_factor - POPUP_SCREEN_MARGIN;
        max_height = max_height.min(room);
    }

    let height = height
        .max(POPUP_MIN_HEIGHT)
        .min(max_height.max(POPUP_MIN_HEIGHT));
    window
        .set_size(tauri::LogicalSize::new(POPUP_WIDTH, height))
        .map_err(|err| format!("Failed to resize popup: {err}"))
}

fn position_popup_window(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    let monitor = window.primary_monitor()?.unwrap_or_else(|| {
        window
            .available_monitors()
            .unwrap_or_default()
            .into_iter()
            .next()
            .unwrap()
    });

    let screen_size = monitor.size();
    let scale_factor = monitor.scale_factor();
    let window_width = POPUP_WIDTH * scale_factor;

    let x = screen_size.width as f64 - window_width - (20.0 * scale_factor);
    let y = 30.0 * scale_factor;

    window.set_position(tauri::Position::Physical(tauri::PhysicalPosition {
        x: x as i32,
        y: y as i32,
    }))?;

    Ok(())
}

/// Where the menu bar item sits, as the centre of its button and the distance
/// from the top of the screen down to that button's lower edge, in points.
///
/// The item is reached through its button, like the highlight is: Tauri 2.7
/// exposes no frame for a tray icon. AppKit measures screen coordinates from
/// the bottom left, so the window hosting the button is what turns them round:
/// it is the menu bar's own strip, flush with the top of the screen whether it
/// spans the whole bar or, as on macOS 27, only the item.
#[cfg(target_os = "macos")]
fn tray_item_position() -> Option<(f64, f64)> {
    let mtm = objc2::MainThreadMarker::new()?;
    let status_item = find_status_item(mtm)?;

    unsafe {
        let button: *mut objc2::runtime::AnyObject = objc2::msg_send![&*status_item, button];
        if button.is_null() {
            return None;
        }
        let ns_window: *mut objc2::runtime::AnyObject = objc2::msg_send![button, window];
        if ns_window.is_null() {
            return None;
        }

        let bounds: objc2_foundation::NSRect = objc2::msg_send![button, bounds];
        let in_window: objc2_foundation::NSRect = objc2::msg_send![
            button,
            convertRect: bounds,
            toView: std::ptr::null::<objc2::runtime::AnyObject>()
        ];
        let item: objc2_foundation::NSRect =
            objc2::msg_send![ns_window, convertRectToScreen: in_window];
        let host_window: objc2_foundation::NSRect = objc2::msg_send![ns_window, frame];

        let screen_top = host_window.origin.y + host_window.size.height;
        Some((
            item.origin.x + item.size.width / 2.0,
            screen_top - item.origin.y,
        ))
    }
}

/// Puts the popup under the menu bar item, centred on it, and reports whether
/// the item could be found.
#[cfg(target_os = "macos")]
fn position_popup_under_tray(window: &tauri::WebviewWindow) -> bool {
    let Some((center_x, top)) = tray_item_position() else {
        return false;
    };
    let Ok(Some(monitor)) = window.primary_monitor() else {
        return false;
    };

    // The item lives in points; window positions are physical pixels.
    let scale_factor = monitor.scale_factor();
    position_popup_at_click(window, center_x * scale_factor, top * scale_factor).is_ok()
}

fn position_popup_at_click(
    window: &tauri::WebviewWindow,
    click_x: f64,
    click_y: f64,
) -> tauri::Result<()> {
    let monitors = window.available_monitors()?;
    let click_monitor = monitors
        .iter()
        .find(|monitor| {
            let pos = monitor.position();
            let size = monitor.size();
            let x = pos.x as f64;
            let y = pos.y as f64;
            let width = size.width as f64;
            let height = size.height as f64;

            click_x >= x && click_x < x + width && click_y >= y && click_y < y + height
        })
        .unwrap_or_else(|| monitors.first().unwrap());

    let scale_factor = click_monitor.scale_factor();
    let screen_pos = click_monitor.position();
    let screen_size = click_monitor.size();
    let window_width = POPUP_WIDTH * scale_factor;

    let min_x = screen_pos.x as f64 + 10.0;
    let max_x = screen_pos.x as f64 + screen_size.width as f64 - window_width - 10.0;
    let x = (click_x - window_width / 2.0).max(min_x).min(max_x);
    let y = click_y + 5.0;

    window.set_position(tauri::Position::Physical(tauri::PhysicalPosition {
        x: x as i32,
        y: y as i32,
    }))?;

    Ok(())
}

/// Advances one definition at a time while the popup is hidden.
///
/// The interval counts hidden time only: opening the popup resets it, so the
/// word never changes the moment the popup closes.
fn start_auto_rotation(app: tauri::AppHandle, state: AppState) {
    std::thread::spawn(move || {
        let mut hidden_for = Duration::ZERO;
        loop {
            std::thread::sleep(ROTATION_TICK);

            let interval = state.rotation_interval_secs.load(Ordering::Relaxed);
            if interval == 0 || *state.is_window_visible.lock().unwrap() {
                hidden_for = Duration::ZERO;
                continue;
            }

            hidden_for += ROTATION_TICK;
            if hidden_for < Duration::from_secs(interval) {
                continue;
            }
            hidden_for = Duration::ZERO;

            let len = state.cards.lock().unwrap().len();
            if len == 0 {
                continue;
            }

            let index = *state.current_index.lock().unwrap();
            apply_card_index(&app, &state, (index + 1) % len);
        }
    });
}

fn build_tray_menu<R: Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<Menu<R>> {
    let manager_item = MenuItem::with_id(
        app,
        TRAY_OPEN_MANAGER_ID,
        "Vocabulary Manager",
        true,
        None::<&str>,
    )?;
    let settings_item =
        MenuItem::with_id(app, TRAY_OPEN_SETTINGS_ID, "Settings", true, None::<&str>)?;
    // The app is an agent (`LSUIElement`), so it has no Dock icon, no app menu
    // and no row in Force Quit: without this item there is no way to quit it.
    let quit_item = MenuItem::with_id(app, TRAY_QUIT_ID, "Quit Mavoca", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    Menu::with_items(
        app,
        &[&manager_item, &settings_item, &separator, &quit_item],
    )
}

/// The menu is deliberately not attached to the tray item. On macOS 27 an
/// attached `NSStatusItem` menu opens on left click and swallows the event,
/// regardless of `set_show_menu_on_left_click(false)`, so the popup never
/// toggles. Right click pops the menu up from `on_tray_icon_event` instead.
fn setup_system_tray<R: Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<()> {
    let tray = app.tray_by_id("main").unwrap();
    tray.set_menu(None::<Menu<R>>)?;
    tray.set_icon(None)?;
    tray.set_show_menu_on_left_click(false)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    // Only one shortcut is ever registered, so any press is it.
                    if event.state() == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        show_quick_add_window(app);
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            show_main_window,
            hide_quick_add_window,
            hide_main_window,
            resize_popup,
            show_manager_window,
            show_settings_window,
            list_vocab_words,
            get_current_vocab_card,
            step_vocab_card,
            step_vocab_word,
            random_vocab_card,
            set_current_vocab_id,
            get_settings,
            update_settings,
            get_launch_at_login,
            set_launch_at_login,
            suggest_words,
            preview_word,
            add_vocab_word,
            refresh_vocab_word,
            resize_quick_add_window,
            lac_viet_meaning,
            translate_definition,
            translate_word,
            create_vocab,
            update_vocab,
            delete_vocab
        ])
        .setup(move |app| {
            // `Info.plist` already asks Launch Services for an agent app, so the
            // Dock icon never appears in the bundle. Setting the policy here as
            // well keeps `tauri dev` (which only injects that file) consistent.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            setup_system_tray(app.handle())?;

            #[cfg(target_os = "macos")]
            if dictionary::prefer_ipa_pronunciation() {
                eprintln!("Switched the Oxford dictionary to IPA pronunciations");
            }

            let conn = init_database(app.handle())?;
            let initial_vocab = list_vocab_from_db(&conn)?;
            let initial_cards = build_cards(&initial_vocab);
            let rotation_interval_secs = load_settings_from_db(&conn)?.rotation_interval_secs;
            let app_state = AppState {
                db: Arc::new(Mutex::new(conn)),
                vocab_list: Arc::new(Mutex::new(initial_vocab)),
                cards: Arc::new(Mutex::new(initial_cards)),
                current_index: Arc::new(Mutex::new(0)),
                is_window_visible: Arc::new(Mutex::new(false)),
                rotation_interval_secs: Arc::new(AtomicU64::new(rotation_interval_secs)),
            };

            if let Some(card) = current_card(&app_state) {
                update_tray_title(app.handle(), &card.word);
            }

            sync_backup_from_state(&app_state)?;

            let saved_shortcut = {
                let conn = app_state.db.lock().unwrap();
                load_settings_from_db(&conn)?.add_word_shortcut
            };
            // A shortcut another app already owns must not stop the app launching.
            if let Err(err) = register_add_word_shortcut(app.handle(), &saved_shortcut) {
                eprintln!("{err}");
            }

            app.manage(app_state.clone());
            start_auto_rotation(app.handle().clone(), app_state.clone());

            // Reading the system word list takes a moment; doing it here keeps
            // the first typed letter in the add-word window from waiting on it.
            std::thread::spawn(completion::warm);

            if let Some(window) = app.get_webview_window("main") {
                let _ = window.hide();

                #[cfg(target_os = "macos")]
                {
                    use objc2_app_kit::NSWindow;
                    use window_vibrancy::{
                        apply_vibrancy, NSVisualEffectMaterial, NSVisualEffectState,
                    };

                    let _ = apply_vibrancy(
                        &window,
                        NSVisualEffectMaterial::Popover,
                        Some(NSVisualEffectState::Active),
                        None,
                    );

                    unsafe {
                        let ns_window = &*(window.ns_window().unwrap() as *mut NSWindow);
                        if let Some(content_view) = ns_window.contentView() {
                            content_view.setWantsLayer(true);
                            if let Some(layer) = content_view.layer() {
                                layer.setCornerRadius(12.0);
                                layer.setMasksToBounds(true);
                            }
                        }
                    }
                }

                let window_clone = window.clone();
                let state_clone = app_state.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::Focused(false) = event {
                        *state_clone.is_window_visible.lock().unwrap() = false;
                        let _ = window_clone.hide();
                        set_tray_highlighted(window_clone.app_handle(), false);
                    }
                });
            }

            if let Some(window) = app.get_webview_window(QUICK_ADD_WINDOW_LABEL) {
                #[cfg(target_os = "macos")]
                {
                    use objc2_app_kit::NSWindow;
                    use window_vibrancy::{
                        apply_vibrancy, NSVisualEffectMaterial, NSVisualEffectState,
                    };

                    let _ = apply_vibrancy(
                        &window,
                        NSVisualEffectMaterial::Popover,
                        Some(NSVisualEffectState::Active),
                        None,
                    );

                    unsafe {
                        let ns_window = &*(window.ns_window().unwrap() as *mut NSWindow);
                        if let Some(content_view) = ns_window.contentView() {
                            content_view.setWantsLayer(true);
                            if let Some(layer) = content_view.layer() {
                                layer.setCornerRadius(12.0);
                                layer.setMasksToBounds(true);
                            }
                        }
                    }
                }

                let window_clone = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::Focused(false) = event {
                        let _ = window_clone.hide();
                    }
                });
            }

            if let Some(window) = app.get_webview_window("manager") {
                #[cfg(target_os = "macos")]
                {
                    use window_vibrancy::{
                        apply_vibrancy, NSVisualEffectMaterial, NSVisualEffectState,
                    };

                    let _ = apply_vibrancy(
                        &window,
                        NSVisualEffectMaterial::Sidebar,
                        Some(NSVisualEffectState::Active),
                        None,
                    );
                }
            }

            Ok(())
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            TRAY_QUIT_ID => app.exit(0),
            TRAY_OPEN_MANAGER_ID => show_tray_window(app, "manager"),
            TRAY_OPEN_SETTINGS_ID => show_tray_window(app, "settings"),
            _ => {}
        })
        .on_tray_icon_event(move |tray, event| {
            let tauri::tray::TrayIconEvent::Click {
                button,
                button_state: tauri::tray::MouseButtonState::Up,
                position,
                ..
            } = event
            else {
                return;
            };

            let app = tray.app_handle();
            let Some(window) = app.get_webview_window("main") else {
                return;
            };

            match button {
                tauri::tray::MouseButton::Left => {
                    if let Some(state) = app.try_state::<AppState>() {
                        let is_visible = window.is_visible().unwrap_or(false);
                        if is_visible {
                            *state.is_window_visible.lock().unwrap() = false;
                            let _ = window.hide();
                        } else {
                            *state.is_window_visible.lock().unwrap() = true;
                            #[cfg(target_os = "macos")]
                            let placed = position_popup_under_tray(&window);
                            #[cfg(not(target_os = "macos"))]
                            let placed = false;
                            if !placed {
                                let _ = position_popup_at_click(&window, position.x, position.y);
                            }
                            show_window(&window, true);
                        }
                        set_tray_highlighted(app, !is_visible);
                    }
                }
                tauri::tray::MouseButton::Right => {
                    // With no position the menu opens at the cursor in screen
                    // coordinates, so the hidden popup window is a valid host.
                    if let Ok(menu) = build_tray_menu(app) {
                        let _ = window.popup_menu(&menu);
                    }
                }
                _ => {}
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a database in the pre-multi-meaning shape, the one an existing
    /// install actually has on disk.
    fn legacy_database() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory database");
        conn.execute_batch(
            r#"
            CREATE TABLE vocabs (
                id INTEGER PRIMARY KEY,
                word TEXT NOT NULL UNIQUE COLLATE NOCASE,
                meaning TEXT NOT NULL,
                example TEXT NOT NULL,
                category TEXT NOT NULL,
                pronunciation TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            INSERT INTO vocabs (word, meaning, example, category, pronunciation)
            VALUES
                ('serendipity', 'finding good things by chance',
                 'A serendipity of timing.', 'noun', '/ser.an.dip.a.ti/'),
                ('ephemeral', 'lasting a very short time',
                 'Fame is ephemeral.', 'adjective', '');
            "#,
        )
        .expect("legacy schema");
        conn
    }

    #[test]
    fn default_add_word_shortcut_parses() {
        use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};

        let shortcut: Shortcut = DEFAULT_ADD_WORD_SHORTCUT.parse().unwrap();
        assert_eq!(shortcut.key, Code::KeyN);
        assert_eq!(
            shortcut.mods,
            Modifiers::SUPER | Modifiers::CONTROL | Modifiers::SHIFT
        );
    }

    #[test]
    fn migration_preserves_existing_words_losslessly() {
        let conn = legacy_database();
        create_schema(&conn).expect("schema");
        migrate_legacy_vocabs(&conn).expect("migration");

        let words = list_vocab_from_db(&conn).expect("read");
        assert_eq!(words.len(), 2);

        let serendipity = words
            .iter()
            .find(|word| word.word == "serendipity")
            .expect("migrated word");
        assert_eq!(serendipity.source, WordSource::Manual);
        assert_eq!(serendipity.definition_count, 1);
        assert_eq!(serendipity.entries.len(), 1);

        let entry = &serendipity.entries[0];
        assert_eq!(entry.part_of_speech, "noun");
        assert_eq!(entry.pron_us, "/ser.an.dip.a.ti/");
        assert_eq!(entry.sense_groups.len(), 1);
        assert_eq!(entry.sense_groups[0].guide_word, "");

        let definition = &entry.sense_groups[0].definitions[0];
        assert_eq!(definition.definition, "finding good things by chance");
        assert_eq!(definition.examples, vec!["A serendipity of timing."]);
    }

    #[test]
    fn migration_drops_legacy_columns_and_is_idempotent() {
        let conn = legacy_database();
        create_schema(&conn).expect("schema");
        migrate_legacy_vocabs(&conn).expect("first migration");

        let columns = table_columns(&conn, "vocabs").expect("columns");
        assert!(!columns.iter().any(|column| column == "meaning"));
        assert!(columns.iter().any(|column| column == "source"));

        // A second launch must not duplicate definitions.
        migrate_legacy_vocabs(&conn).expect("second migration");
        let words = list_vocab_from_db(&conn).expect("read");
        assert_eq!(words.len(), 2);
        assert!(words.iter().all(|word| word.definition_count == 1));
    }

    #[test]
    fn fresh_database_needs_no_migration() {
        let conn = Connection::open_in_memory().expect("in-memory database");
        create_schema(&conn).expect("schema");
        migrate_legacy_vocabs(&conn).expect("migration");

        assert!(list_vocab_from_db(&conn).expect("read").is_empty());
    }

    /// The Vietnamese definition is a cache of a translation, so it has to
    /// survive a save (a manager edit, a re-fetch) as long as its English text
    /// does, and disappear when the definition itself changed.
    #[test]
    fn keeps_stored_translations_through_a_resave() {
        let conn = Connection::open_in_memory().expect("in-memory database");
        create_schema(&conn).expect("schema");

        let id = save_word_tree(&conn, None, "run", WordSource::Cambridge, &sample_entries())
            .expect("save");
        let definition_id = definition_id_for(&conn, id, "to move using your legs");
        store_definition_vi(&conn, definition_id, "di chuyển bằng chân").expect("store");
        assert_eq!(
            stored_definition_vi(&conn, definition_id)
                .expect("read")
                .as_deref(),
            Some("di chuyển bằng chân")
        );

        // Saving the same tree again replaces every row; the translation of an
        // unchanged definition comes with it.
        save_word_tree(
            &conn,
            Some(id),
            "run",
            WordSource::Cambridge,
            &sample_entries(),
        )
        .expect("resave");
        let resaved = definition_id_for(&conn, id, "to move using your legs");
        assert_eq!(
            stored_definition_vi(&conn, resaved)
                .expect("read")
                .as_deref(),
            Some("di chuyển bằng chân")
        );

        // Re-wording one definition drops only that one's translation. Which
        // comes back empty even though SQLite reuses the row ids of the rows it
        // just deleted, so a stale id can point at a different definition: the
        // popup therefore keys its own cache on the English text, not the id.
        let mut reworded = sample_entries();
        reworded[0].sense_groups[0].definitions[0].definition = "to move quickly on foot".into();
        save_word_tree(&conn, Some(id), "run", WordSource::Cambridge, &reworded).expect("reword");
        assert_eq!(
            stored_definition_vi(
                &conn,
                definition_id_for(&conn, id, "to move quickly on foot")
            )
            .expect("read"),
            None
        );
        assert_eq!(
            stored_definition_vi(&conn, definition_id_for(&conn, id, "an act of running"))
                .expect("read")
                .as_deref(),
            None,
            "the other definitions were never translated"
        );
        assert_eq!(
            stored_definition_vi(
                &conn,
                definition_id_for(&conn, id, "to organize or control")
            )
            .expect("read")
            .as_deref(),
            None
        );
    }

    /// The pane beside the add-word list stands in for the popup card, so a
    /// saved word previews what is stored, translation included.
    #[test]
    fn a_saved_word_previews_from_the_database() {
        let conn = Connection::open_in_memory().expect("in-memory database");
        create_schema(&conn).expect("schema");

        let id = save_word_tree(&conn, None, "run", WordSource::Cambridge, &sample_entries())
            .expect("save");
        let definition_id = definition_id_for(&conn, id, "to move using your legs");
        store_definition_vi(&conn, definition_id, "di chuyển bằng chân").expect("store");

        // The word is matched the way `vocabs.word` is, without regard to case.
        let preview = saved_word_preview(&conn, "Run")
            .expect("preview")
            .expect("a saved word");
        assert!(preview.saved);
        assert_eq!(preview.source, "saved");
        assert_eq!(preview.word, "Run");
        assert_eq!(preview.definitions[0].definition, "to move using your legs");
        assert_eq!(preview.definitions[0].part_of_speech, "verb");
        assert_eq!(preview.definitions[0].guide_word, "MOVE QUICKLY");
        assert_eq!(
            preview.definition_vi.as_deref(),
            Some("di chuyển bằng chân")
        );
        assert_eq!(preview.total, 3);

        // A word that is not in the vocabulary is looked up instead.
        assert!(saved_word_preview(&conn, "sprint")
            .expect("preview")
            .is_none());
    }

    /// The pane shows the first few definitions, and counts against the whole
    /// entry so it can say how much is left for the popup.
    #[test]
    fn the_preview_flattens_the_tree_it_is_given() {
        let preview = preview_from_entries("run".into(), sample_entries(), false, None, "macos");

        assert!(!preview.saved);
        assert_eq!(preview.source, "macos");
        assert_eq!(preview.definitions.len(), 3);
        assert_eq!(preview.total, 3);
        assert_eq!(preview.definitions[2].part_of_speech, "noun");
        assert_eq!(preview.definitions[2].definition, "an act of running");
        assert_eq!(preview.definition_vi, None);
    }

    /// The column arrived after the first databases were created, so opening
    /// one of those has to add it rather than fail on every insert.
    #[test]
    fn adds_the_vietnamese_column_to_an_older_database() {
        let conn = Connection::open_in_memory().expect("in-memory database");
        conn.execute_batch(
            r#"
            CREATE TABLE vocab_definitions (
                id INTEGER PRIMARY KEY,
                vocab_id INTEGER NOT NULL,
                entry_index INTEGER NOT NULL,
                part_of_speech TEXT NOT NULL,
                pron_uk TEXT NOT NULL DEFAULT '',
                pron_us TEXT NOT NULL DEFAULT '',
                guide_word TEXT NOT NULL DEFAULT '',
                definition TEXT NOT NULL,
                examples TEXT NOT NULL DEFAULT '[]',
                sort_order INTEGER NOT NULL
            );
            "#,
        )
        .expect("old schema");
        assert!(!table_columns(&conn, "vocab_definitions")
            .expect("columns")
            .iter()
            .any(|column| column == "definition_vi"));

        create_schema(&conn).expect("schema");
        assert!(table_columns(&conn, "vocab_definitions")
            .expect("columns")
            .iter()
            .any(|column| column == "definition_vi"));

        // Idempotent: the app opens the same database on every launch.
        create_schema(&conn).expect("schema again");
    }

    /// Each card carries the row id the popup keys its translation on.
    #[test]
    fn cards_carry_their_definition_row() {
        let conn = Connection::open_in_memory().expect("in-memory database");
        create_schema(&conn).expect("schema");
        let id = save_word_tree(&conn, None, "run", WordSource::Cambridge, &sample_entries())
            .expect("save");

        let words = list_vocab_from_db(&conn).expect("read");
        let cards = build_cards(&words);
        assert_eq!(cards.len(), 3);

        let rows: Vec<i64> = {
            let mut stmt = conn
                .prepare("SELECT id FROM vocab_definitions WHERE vocab_id = ?1 ORDER BY sort_order")
                .expect("prepare");
            stmt.query_map(params![id], |row| row.get(0))
                .expect("query")
                .collect::<rusqlite::Result<Vec<_>>>()
                .expect("rows")
        };
        let card_ids: Vec<i64> = cards.iter().map(|card| card.definition_id).collect();
        assert_eq!(card_ids, rows);
        assert!(cards.iter().all(|card| card.definition_id > 0));
    }

    fn definition_id_for(conn: &Connection, vocab_id: i64, definition: &str) -> i64 {
        conn.query_row(
            "SELECT id FROM vocab_definitions WHERE vocab_id = ?1 AND definition = ?2",
            params![vocab_id, definition],
            |row| row.get(0),
        )
        .expect("definition row")
    }

    fn sample_entries() -> Vec<VocabEntry> {
        vec![
            VocabEntry {
                part_of_speech: "verb".into(),
                pron_uk: "/run/".into(),
                pron_us: "/run/".into(),
                sense_groups: vec![
                    VocabSenseGroup {
                        guide_word: "MOVE QUICKLY".into(),
                        definitions: vec![model::VocabDefinition {
                            id: 0,
                            definition: "to move using your legs".into(),
                            examples: vec!["I run every morning.".into()],
                        }],
                    },
                    VocabSenseGroup {
                        guide_word: "MANAGE".into(),
                        definitions: vec![model::VocabDefinition {
                            id: 0,
                            definition: "to organize or control".into(),
                            examples: vec![],
                        }],
                    },
                ],
            },
            VocabEntry {
                part_of_speech: "noun".into(),
                pron_uk: String::new(),
                pron_us: "/run/".into(),
                sense_groups: vec![VocabSenseGroup {
                    guide_word: String::new(),
                    definitions: vec![model::VocabDefinition {
                        id: 0,
                        definition: "an act of running".into(),
                        examples: vec!["He went for a run.".into()],
                    }],
                }],
            },
        ]
    }

    /// The three-level shape has to survive a write and read unchanged: the
    /// schema stores it flat and rebuilds it by grouping.
    #[test]
    fn tree_survives_a_save_and_load_round_trip() {
        let conn = Connection::open_in_memory().expect("in-memory database");
        create_schema(&conn).expect("schema");

        let entries = sample_entries();
        let id = save_word_tree(&conn, None, "run", WordSource::Cambridge, &entries).expect("save");

        let saved = get_vocab_by_id(&conn, id).expect("read").expect("word");
        assert_eq!(saved.source, WordSource::Cambridge);
        assert_eq!(saved.definition_count, 3);
        assert_eq!(saved.entries.len(), 2);

        assert_eq!(saved.entries[0].part_of_speech, "verb");
        assert_eq!(saved.entries[0].sense_groups.len(), 2);
        assert_eq!(saved.entries[0].sense_groups[0].guide_word, "MOVE QUICKLY");
        assert_eq!(saved.entries[0].sense_groups[1].guide_word, "MANAGE");

        assert_eq!(saved.entries[1].part_of_speech, "noun");
        assert_eq!(saved.entries[1].sense_groups[0].guide_word, "");
        assert_eq!(
            saved.entries[1].sense_groups[0].definitions[0].examples,
            vec!["He went for a run."]
        );
    }

    /// Two entries can share a guide word without being merged: grouping keys
    /// off entry_index first.
    #[test]
    fn entries_with_matching_guide_words_stay_separate() {
        let conn = Connection::open_in_memory().expect("in-memory database");
        create_schema(&conn).expect("schema");

        let entries = vec![
            VocabEntry {
                part_of_speech: "verb".into(),
                pron_uk: String::new(),
                pron_us: String::new(),
                sense_groups: vec![VocabSenseGroup {
                    guide_word: String::new(),
                    definitions: vec![model::VocabDefinition {
                        id: 0,
                        definition: "first".into(),
                        examples: vec![],
                    }],
                }],
            },
            VocabEntry {
                part_of_speech: "noun".into(),
                pron_uk: String::new(),
                pron_us: String::new(),
                sense_groups: vec![VocabSenseGroup {
                    guide_word: String::new(),
                    definitions: vec![model::VocabDefinition {
                        id: 0,
                        definition: "second".into(),
                        examples: vec![],
                    }],
                }],
            },
        ];

        let id = save_word_tree(&conn, None, "split", WordSource::Manual, &entries).expect("save");
        let saved = get_vocab_by_id(&conn, id).expect("read").expect("word");

        assert_eq!(saved.entries.len(), 2);
        assert_eq!(saved.entries[0].part_of_speech, "verb");
        assert_eq!(saved.entries[1].part_of_speech, "noun");
    }

    #[test]
    fn saving_replaces_definitions_instead_of_appending() {
        let conn = Connection::open_in_memory().expect("in-memory database");
        create_schema(&conn).expect("schema");

        let id = save_word_tree(&conn, None, "run", WordSource::Cambridge, &sample_entries())
            .expect("save");

        let trimmed = vec![VocabEntry {
            part_of_speech: "noun".into(),
            pron_uk: String::new(),
            pron_us: String::new(),
            sense_groups: vec![VocabSenseGroup {
                guide_word: String::new(),
                definitions: vec![model::VocabDefinition {
                    id: 0,
                    definition: "only one left".into(),
                    examples: vec![],
                }],
            }],
        }];
        save_word_tree(&conn, Some(id), "run", WordSource::Manual, &trimmed).expect("resave");

        let saved = get_vocab_by_id(&conn, id).expect("read").expect("word");
        assert_eq!(saved.definition_count, 1);
        assert_eq!(saved.source, WordSource::Manual);
    }

    /// Deleting a word must take its definitions with it, which only happens
    /// when the foreign_keys pragma is on.
    #[test]
    fn deleting_a_word_cascades_to_its_definitions() {
        let conn = Connection::open_in_memory().expect("in-memory database");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("pragma");
        create_schema(&conn).expect("schema");

        let id = save_word_tree(&conn, None, "run", WordSource::Cambridge, &sample_entries())
            .expect("save");
        conn.execute("DELETE FROM vocabs WHERE id = ?1", params![id])
            .expect("delete");

        let orphans: i64 = conn
            .query_row("SELECT COUNT(*) FROM vocab_definitions", [], |row| {
                row.get(0)
            })
            .expect("count");
        assert_eq!(orphans, 0);
    }

    /// The footer's arrows move between words: a word step skips the rest of the
    /// current word's definitions instead of walking into the next one.
    #[test]
    fn a_word_step_lands_on_the_next_word() {
        let cards = vec![
            card(1, "apple", 1, 1),
            card(2, "run", 1, 3),
            card(2, "run", 2, 3),
            card(2, "run", 3, 3),
            card(3, "walk", 1, 2),
            card(3, "walk", 2, 2),
        ];

        // Forward from anywhere in a word lands on the first card of the next.
        assert_eq!(word_step_index(&cards, 0, 1), Some(1));
        assert_eq!(word_step_index(&cards, 1, 1), Some(4));
        assert_eq!(word_step_index(&cards, 2, 1), Some(4));
        assert_eq!(word_step_index(&cards, 3, 1), Some(4));
        // Past the last word it wraps to the first card of the rotation.
        assert_eq!(word_step_index(&cards, 4, 1), Some(0));
        assert_eq!(word_step_index(&cards, 5, 1), Some(0));

        // Backwards lands on the first card of the previous word, not on the
        // first definition of this one: that is the card's own stepper.
        assert_eq!(word_step_index(&cards, 5, -1), Some(1));
        assert_eq!(word_step_index(&cards, 4, -1), Some(1));
        assert_eq!(word_step_index(&cards, 3, -1), Some(0));
        assert_eq!(word_step_index(&cards, 1, -1), Some(0));
        // Back from the first word wraps to the start of the last one.
        assert_eq!(word_step_index(&cards, 0, -1), Some(4));

        assert_eq!(word_step_index(&[], 0, 1), None);
    }

    fn card(vocab_id: i64, word: &str, position: usize, total: usize) -> VocabCard {
        VocabCard {
            vocab_id,
            definition_id: 0,
            word: word.into(),
            source: WordSource::Manual,
            part_of_speech: "noun".into(),
            pron_uk: String::new(),
            pron_us: String::new(),
            guide_word: String::new(),
            definition: format!("sense {position} of {word}"),
            examples: vec![],
            position,
            total,
        }
    }

    /// Rotation walks every definition of a word before moving to the next.
    #[test]
    fn cards_flatten_every_definition_in_order() {
        let conn = Connection::open_in_memory().expect("in-memory database");
        create_schema(&conn).expect("schema");

        save_word_tree(&conn, None, "run", WordSource::Cambridge, &sample_entries()).expect("save");
        save_word_tree(
            &conn,
            None,
            "apple",
            WordSource::Manual,
            &[VocabEntry {
                part_of_speech: "noun".into(),
                pron_uk: String::new(),
                pron_us: String::new(),
                sense_groups: vec![VocabSenseGroup {
                    guide_word: String::new(),
                    definitions: vec![model::VocabDefinition {
                        id: 0,
                        definition: "a round fruit".into(),
                        examples: vec![],
                    }],
                }],
            }],
        )
        .expect("save");

        let words = list_vocab_from_db(&conn).expect("read");
        let cards = build_cards(&words);

        // Words are listed alphabetically, so "apple" comes first.
        assert_eq!(cards.len(), 4);
        assert_eq!(cards[0].word, "apple");
        assert_eq!((cards[0].position, cards[0].total), (1, 1));

        assert_eq!(cards[1].word, "run");
        assert_eq!((cards[1].position, cards[1].total), (1, 3));
        assert_eq!(cards[1].guide_word, "MOVE QUICKLY");
        assert_eq!(cards[2].guide_word, "MANAGE");
        assert_eq!(cards[3].part_of_speech, "noun");
        assert_eq!((cards[3].position, cards[3].total), (3, 3));
    }

    #[test]
    fn validation_rejects_empty_trees_and_keeps_partial_rows() {
        let empty = VocabTreeInput {
            word: "run".into(),
            entries: vec![],
        };
        assert!(validate_tree_input(&empty).is_err());

        let blank_definitions = VocabTreeInput {
            word: "run".into(),
            entries: vec![VocabEntry {
                part_of_speech: "verb".into(),
                pron_uk: String::new(),
                pron_us: String::new(),
                sense_groups: vec![VocabSenseGroup {
                    guide_word: String::new(),
                    definitions: vec![model::VocabDefinition {
                        id: 0,
                        definition: "   ".into(),
                        examples: vec![],
                    }],
                }],
            }],
        };
        assert!(validate_tree_input(&blank_definitions).is_err());

        // A blank example line is dropped rather than treated as an error.
        let with_blank_example = VocabTreeInput {
            word: "  run  ".into(),
            entries: vec![VocabEntry {
                part_of_speech: "verb".into(),
                pron_uk: String::new(),
                pron_us: String::new(),
                sense_groups: vec![VocabSenseGroup {
                    guide_word: String::new(),
                    definitions: vec![model::VocabDefinition {
                        id: 0,
                        definition: "to move quickly".into(),
                        examples: vec!["".into(), "I run.".into()],
                    }],
                }],
            }],
        };
        let (word, entries) = validate_tree_input(&with_blank_example).expect("valid");
        assert_eq!(word, "run");
        assert_eq!(
            entries[0].sense_groups[0].definitions[0].examples,
            vec!["I run."]
        );
    }

    /// The manager title bar moves the window with `startDragging` and
    /// `toggleMaximize`. Neither ships in `core:window:default`, and a denied
    /// command rejects inside the webview without reaching Rust, so the only
    /// symptom is a title bar that does nothing. Pin the grant here.
    #[test]
    fn capabilities_grant_the_window_commands_the_title_bar_uses() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("capabilities")
            .join("default.json");
        let raw = std::fs::read_to_string(&path).expect("capability file");
        let parsed: serde_json::Value = serde_json::from_str(&raw).expect("valid json");

        let granted: Vec<&str> = parsed["permissions"]
            .as_array()
            .expect("permissions array")
            .iter()
            .filter_map(|value| value.as_str())
            .collect();
        for required in [
            "core:window:allow-start-dragging",
            "core:window:allow-toggle-maximize",
        ] {
            assert!(
                granted.contains(&required),
                "{required} is missing from capabilities/default.json, \
                 so the manager title bar cannot move the window",
            );
        }

        let windows: Vec<&str> = parsed["windows"]
            .as_array()
            .expect("windows array")
            .iter()
            .filter_map(|value| value.as_str())
            .collect();
        assert!(
            windows.contains(&"manager"),
            "the manager window must be covered by the capability",
        );
    }
}
