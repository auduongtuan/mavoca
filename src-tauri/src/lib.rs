use reqwest::blocking::Client;
use rusqlite::{params, Connection, OptionalExtension};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::{env, fs};
use tauri::{
    menu::{Menu, MenuItem},
    Emitter, Manager, Runtime,
};

const BACKUP_PATH_SETTING: &str = "backup_path";
const TRAY_OPEN_MANAGER_ID: &str = "open-manager";
const TRAY_OPEN_SETTINGS_ID: &str = "open-settings";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VocabWord {
    id: i64,
    word: String,
    meaning: String,
    example: String,
    category: String,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
struct SeedVocabWord {
    word: String,
    meaning: String,
    example: String,
    category: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VocabInput {
    word: String,
    meaning: String,
    example: String,
    category: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppSettings {
    backup_path: String,
}

#[derive(Clone)]
struct AppState {
    db: Arc<Mutex<Connection>>,
    vocab_list: Arc<Mutex<Vec<VocabWord>>>,
    current_index: Arc<Mutex<usize>>,
    is_window_visible: Arc<Mutex<bool>>,
}

fn load_seed_vocab() -> Vec<SeedVocabWord> {
    let vocab_json = include_str!("../../public/vocab.json");
    serde_json::from_str(vocab_json).unwrap_or_else(|_| vec![])
}

fn normalize_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn validate_input(input: &VocabInput) -> Result<VocabInput, String> {
    let cleaned = VocabInput {
        word: normalize_text(&input.word),
        meaning: normalize_text(&input.meaning),
        example: normalize_text(&input.example),
        category: normalize_text(&input.category),
    };

    if cleaned.word.is_empty() {
        return Err("Word is required".into());
    }
    if cleaned.meaning.is_empty() {
        return Err("Meaning is required".into());
    }
    if cleaned.example.is_empty() {
        return Err("Example is required".into());
    }
    if cleaned.category.is_empty() {
        return Err("Category is required".into());
    }

    Ok(cleaned)
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

    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS vocabs (
            id INTEGER PRIMARY KEY,
            word TEXT NOT NULL UNIQUE COLLATE NOCASE,
            meaning TEXT NOT NULL,
            example TEXT NOT NULL,
            category TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        "#,
    )
    .map_err(|err| format!("Failed to initialize database: {err}"))?;

    ensure_default_settings(&conn)?;

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM vocabs", [], |row| row.get(0))
        .map_err(|err| format!("Failed to inspect database: {err}"))?;

    if count == 0 {
        for seed in load_seed_vocab() {
            conn.execute(
                r#"
                INSERT OR IGNORE INTO vocabs (word, meaning, example, category)
                VALUES (?1, ?2, ?3, ?4)
                "#,
                params![seed.word, seed.meaning, seed.example, seed.category],
            )
            .map_err(|err| format!("Failed to seed vocabulary: {err}"))?;
        }
    }

    Ok(conn)
}

fn default_backup_path() -> Option<PathBuf> {
    let home = env::var_os("HOME")?;
    let dropbox_dir = Path::new(&home).join("Dropbox");
    if !dropbox_dir.exists() {
        return None;
    }

    Some(dropbox_dir.join("Vocab Learning").join("vocab.sqlite3"))
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

    Ok(())
}

fn load_settings_from_db(conn: &Connection) -> Result<AppSettings, String> {
    Ok(AppSettings {
        backup_path: get_setting(conn, BACKUP_PATH_SETTING)?.unwrap_or_default(),
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

fn row_to_vocab(row: &rusqlite::Row<'_>) -> rusqlite::Result<VocabWord> {
    Ok(VocabWord {
        id: row.get(0)?,
        word: row.get(1)?,
        meaning: row.get(2)?,
        example: row.get(3)?,
        category: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

fn list_vocab_from_db(conn: &Connection) -> Result<Vec<VocabWord>, String> {
    let mut stmt = conn
        .prepare(
            r#"
            SELECT id, word, meaning, example, category, created_at, updated_at
            FROM vocabs
            ORDER BY lower(word) ASC
            "#,
        )
        .map_err(|err| format!("Failed to prepare vocabulary query: {err}"))?;

    let rows = stmt
        .query_map([], row_to_vocab)
        .map_err(|err| format!("Failed to read vocabulary: {err}"))?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| format!("Failed to decode vocabulary: {err}"))
}

fn get_vocab_by_id(conn: &Connection, id: i64) -> Result<Option<VocabWord>, String> {
    conn.query_row(
        r#"
        SELECT id, word, meaning, example, category, created_at, updated_at
        FROM vocabs
        WHERE id = ?1
        "#,
        params![id],
        row_to_vocab,
    )
    .optional()
    .map_err(|err| format!("Failed to read vocabulary word: {err}"))
}

fn get_vocab_by_word(conn: &Connection, word: &str) -> Result<Option<VocabWord>, String> {
    conn.query_row(
        r#"
        SELECT id, word, meaning, example, category, created_at, updated_at
        FROM vocabs
        WHERE word = ?1 COLLATE NOCASE
        "#,
        params![word],
        row_to_vocab,
    )
    .optional()
    .map_err(|err| format!("Failed to read vocabulary word: {err}"))
}

fn refresh_vocab_cache(state: &AppState) -> Result<Vec<VocabWord>, String> {
    let list = {
        let conn = state.db.lock().unwrap();
        list_vocab_from_db(&conn)?
    };

    let previous_current_id = {
        let vocab_list = state.vocab_list.lock().unwrap();
        let current_index = *state.current_index.lock().unwrap();
        vocab_list.get(current_index).map(|word| word.id)
    };

    let next_index = previous_current_id
        .and_then(|id| list.iter().position(|word| word.id == id))
        .unwrap_or(0);

    *state.vocab_list.lock().unwrap() = list.clone();
    *state.current_index.lock().unwrap() = next_index;

    Ok(list)
}

fn sync_backup_from_state(state: &AppState) -> Result<(), String> {
    let conn = state.db.lock().unwrap();
    sync_database_backup(&conn)
}

fn current_word(state: &AppState) -> Option<VocabWord> {
    let vocab_list = state.vocab_list.lock().unwrap();
    let index = *state.current_index.lock().unwrap();
    vocab_list.get(index).cloned()
}

fn sync_after_vocab_change(
    app: &tauri::AppHandle,
    state: &AppState,
) -> Result<Vec<VocabWord>, String> {
    let list = refresh_vocab_cache(state)?;

    if let Some(word) = current_word(state) {
        update_tray_title(app, &word.word);
        let _ = app.emit("vocab-changed", word);
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

fn selector(query: &str) -> Result<Selector, String> {
    Selector::parse(query).map_err(|_| format!("Invalid selector: {query}"))
}

fn first_text_in(
    element: scraper::ElementRef<'_>,
    selector_query: &str,
) -> Result<Option<String>, String> {
    let selector = selector(selector_query)?;
    Ok(element
        .select(&selector)
        .next()
        .map(|node| normalize_text(&node.text().collect::<Vec<_>>().join(" ")))
        .filter(|text| !text.is_empty()))
}

fn lookup_cambridge_word(word: &str) -> Result<VocabInput, String> {
    let clean_word = normalize_text(word);
    if clean_word.is_empty() {
        return Err("Enter a word to look up".into());
    }

    let lookup_path = clean_word.to_lowercase().replace(' ', "-");
    let encoded_word = urlencoding::encode(&lookup_path);
    let url = format!("https://dictionary.cambridge.org/us/dictionary/english/{encoded_word}");
    let client = Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) VocabLearning/0.1")
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|err| format!("Failed to create dictionary client: {err}"))?;

    let response = client
        .get(url)
        .send()
        .map_err(|err| format!("Failed to reach Cambridge Dictionary: {err}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "Cambridge Dictionary returned HTTP {}",
            response.status()
        ));
    }

    let html = response
        .text()
        .map_err(|err| format!("Failed to read Cambridge response: {err}"))?;
    let document = Html::parse_document(&html);
    let entry_selector = selector(".entry-body__el, .pr.entry-body__el")?;
    let headword_selector = selector(".hw.dhw")?;
    let requested = clean_word.to_lowercase();

    for entry in document.select(&entry_selector) {
        let headword = entry
            .select(&headword_selector)
            .next()
            .map(|node| normalize_text(&node.text().collect::<Vec<_>>().join(" ")))
            .unwrap_or_default();

        if headword.to_lowercase() != requested {
            continue;
        }

        let definition = first_text_in(entry, ".def.ddef_d.db")?
            .or_else(|| first_text_in(entry, ".def").ok().flatten());
        let Some(meaning) = definition else {
            continue;
        };

        let category = first_text_in(entry, ".pos.dpos")?
            .or_else(|| first_text_in(entry, ".pos").ok().flatten())
            .unwrap_or_else(|| "unknown".into());
        let example = first_text_in(entry, ".examp.dexamp")?
            .or_else(|| first_text_in(entry, ".eg.deg").ok().flatten())
            .or_else(|| first_text_in(entry, ".examp").ok().flatten())
            .unwrap_or_else(|| format!("No example found for {clean_word}."));

        return Ok(VocabInput {
            word: clean_word,
            meaning,
            example,
            category,
        });
    }

    Err(format!("No Cambridge definition found for '{clean_word}'"))
}

#[tauri::command]
fn list_vocab_words(state: tauri::State<AppState>) -> Result<Vec<VocabWord>, String> {
    refresh_vocab_cache(&state)
}

#[tauri::command]
fn get_current_vocab_word(state: tauri::State<AppState>) -> Option<VocabWord> {
    current_word(&state)
}

#[tauri::command]
fn update_current_word_index(
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
    index: usize,
) -> Result<(), String> {
    let vocab_list = state.vocab_list.lock().unwrap();
    if index >= vocab_list.len() {
        return Err("Vocabulary index is out of range".into());
    }

    *state.current_index.lock().unwrap() = index;
    if let Some(word) = vocab_list.get(index) {
        update_tray_title(&app, &word.word);
        let _ = app.emit("vocab-changed", word.clone());
    }

    Ok(())
}

#[tauri::command]
fn set_current_vocab_id(
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
    id: i64,
) -> Result<(), String> {
    let vocab_list = state.vocab_list.lock().unwrap();
    let Some(index) = vocab_list.iter().position(|word| word.id == id) else {
        return Err("Vocabulary word was not found".into());
    };

    *state.current_index.lock().unwrap() = index;
    if let Some(word) = vocab_list.get(index) {
        update_tray_title(&app, &word.word);
        let _ = app.emit("vocab-changed", word.clone());
    }

    Ok(())
}

#[tauri::command]
fn lookup_vocab_word(word: String) -> Result<VocabInput, String> {
    lookup_cambridge_word(&word)
}

#[tauri::command]
fn get_settings(state: tauri::State<AppState>) -> Result<AppSettings, String> {
    let conn = state.db.lock().unwrap();
    load_settings_from_db(&conn)
}

#[tauri::command]
fn update_settings(
    state: tauri::State<AppState>,
    settings: AppSettings,
) -> Result<AppSettings, String> {
    let conn = state.db.lock().unwrap();
    set_setting(&conn, BACKUP_PATH_SETTING, settings.backup_path.trim())?;
    sync_database_backup(&conn)?;
    load_settings_from_db(&conn)
}

#[tauri::command]
fn create_vocab(
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
    input: VocabInput,
) -> Result<VocabWord, String> {
    let cleaned = validate_input(&input)?;
    let word = {
        let conn = state.db.lock().unwrap();
        if let Some(existing) = get_vocab_by_word(&conn, &cleaned.word)? {
            conn.execute(
                r#"
                UPDATE vocabs
                SET meaning = ?1, example = ?2, category = ?3, updated_at = datetime('now')
                WHERE id = ?4
                "#,
                params![
                    cleaned.meaning,
                    cleaned.example,
                    cleaned.category,
                    existing.id
                ],
            )
            .map_err(|err| format!("Failed to update existing vocabulary: {err}"))?;
            get_vocab_by_id(&conn, existing.id)?
                .ok_or_else(|| "Updated word was not found".to_string())?
        } else {
            conn.execute(
                r#"
                INSERT INTO vocabs (word, meaning, example, category)
                VALUES (?1, ?2, ?3, ?4)
                "#,
                params![
                    cleaned.word,
                    cleaned.meaning,
                    cleaned.example,
                    cleaned.category
                ],
            )
            .map_err(|err| format!("Failed to create vocabulary: {err}"))?;
            let id = conn.last_insert_rowid();
            get_vocab_by_id(&conn, id)?.ok_or_else(|| "Created word was not found".to_string())?
        }
    };

    sync_after_vocab_change(&app, &state)?;
    Ok(word)
}

#[tauri::command]
fn add_vocab_from_cambridge(
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
    word: String,
) -> Result<VocabWord, String> {
    let input = lookup_cambridge_word(&word)?;
    create_vocab(state, app, input)
}

#[tauri::command]
fn update_vocab(
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
    id: i64,
    input: VocabInput,
) -> Result<VocabWord, String> {
    let cleaned = validate_input(&input)?;
    let word = {
        let conn = state.db.lock().unwrap();
        conn.execute(
            r#"
            UPDATE vocabs
            SET word = ?1, meaning = ?2, example = ?3, category = ?4, updated_at = datetime('now')
            WHERE id = ?5
            "#,
            params![
                cleaned.word,
                cleaned.meaning,
                cleaned.example,
                cleaned.category,
                id
            ],
        )
        .map_err(|err| format!("Failed to update vocabulary: {err}"))?;

        get_vocab_by_id(&conn, id)?.ok_or_else(|| "Vocabulary word was not found".to_string())?
    };

    sync_after_vocab_change(&app, &state)?;
    Ok(word)
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

#[tauri::command]
fn show_main_window(app: tauri::AppHandle, state: tauri::State<AppState>) {
    *state.is_window_visible.lock().unwrap() = true;
    if let Some(window) = app.get_webview_window("main") {
        let _ = position_popup_window(&window);
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[tauri::command]
fn show_manager_window(app: tauri::AppHandle) -> Result<(), String> {
    let Some(window) = app.get_webview_window("manager") else {
        return Err("Manager window was not found".into());
    };

    window
        .show()
        .map_err(|err| format!("Failed to show manager: {err}"))?;
    window
        .set_focus()
        .map_err(|err| format!("Failed to focus manager: {err}"))?;
    Ok(())
}

#[tauri::command]
fn show_settings_window(app: tauri::AppHandle) -> Result<(), String> {
    let Some(window) = app.get_webview_window("settings") else {
        return Err("Settings window was not found".into());
    };

    window
        .show()
        .map_err(|err| format!("Failed to show settings: {err}"))?;
    window
        .set_focus()
        .map_err(|err| format!("Failed to focus settings: {err}"))?;
    Ok(())
}

#[tauri::command]
fn hide_main_window(app: tauri::AppHandle, state: tauri::State<AppState>) {
    *state.is_window_visible.lock().unwrap() = false;
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
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
    let window_width = 320.0 * scale_factor;

    let x = screen_size.width as f64 - window_width - (20.0 * scale_factor);
    let y = 30.0 * scale_factor;

    window.set_position(tauri::Position::Physical(tauri::PhysicalPosition {
        x: x as i32,
        y: y as i32,
    }))?;

    Ok(())
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
    let window_width = 320.0 * scale_factor;

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

fn start_auto_rotation(app: tauri::AppHandle, state: AppState) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(10));

        let is_visible = *state.is_window_visible.lock().unwrap();
        if !is_visible {
            let mut index = state.current_index.lock().unwrap();
            let vocab_list = state.vocab_list.lock().unwrap();

            if !vocab_list.is_empty() {
                *index = (*index + 1) % vocab_list.len();
                if let Some(word) = vocab_list.get(*index) {
                    update_tray_title(&app, &word.word);
                    let _ = app.emit("vocab-changed", word.clone());
                }
            }
        }
    });
}

fn setup_system_tray<R: Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<()> {
    let manager_item = MenuItem::with_id(
        app,
        TRAY_OPEN_MANAGER_ID,
        "Vocabulary Manager",
        true,
        None::<&str>,
    )?;
    let settings_item =
        MenuItem::with_id(app, TRAY_OPEN_SETTINGS_ID, "Settings", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&manager_item, &settings_item])?;
    let tray = app.tray_by_id("main").unwrap();
    tray.set_menu(Some(menu))?;
    tray.set_icon(None)?;
    tray.set_show_menu_on_left_click(false)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            show_main_window,
            hide_main_window,
            show_manager_window,
            show_settings_window,
            list_vocab_words,
            get_current_vocab_word,
            update_current_word_index,
            set_current_vocab_id,
            lookup_vocab_word,
            get_settings,
            update_settings,
            create_vocab,
            add_vocab_from_cambridge,
            update_vocab,
            delete_vocab
        ])
        .setup(move |app| {
            setup_system_tray(app.handle())?;

            let conn = init_database(app.handle())?;
            let initial_vocab = list_vocab_from_db(&conn)?;
            let app_state = AppState {
                db: Arc::new(Mutex::new(conn)),
                vocab_list: Arc::new(Mutex::new(initial_vocab)),
                current_index: Arc::new(Mutex::new(0)),
                is_window_visible: Arc::new(Mutex::new(false)),
            };

            if let Some(word) = current_word(&app_state) {
                update_tray_title(app.handle(), &word.word);
            }

            sync_backup_from_state(&app_state)?;

            app.manage(app_state.clone());
            start_auto_rotation(app.handle().clone(), app_state.clone());

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
                    }
                });
            }

            Ok(())
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            TRAY_OPEN_MANAGER_ID => {
                if let Some(window) = app.get_webview_window("manager") {
                    let _ = window.show().and_then(|_| window.set_focus());
                }
            }
            TRAY_OPEN_SETTINGS_ID => {
                if let Some(window) = app.get_webview_window("settings") {
                    let _ = window.show().and_then(|_| window.set_focus());
                }
            }
            _ => {}
        })
        .on_tray_icon_event(move |tray, event| {
            if let tauri::tray::TrayIconEvent::Click {
                button: tauri::tray::MouseButton::Left,
                button_state: tauri::tray::MouseButtonState::Up,
                position,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(state) = app.try_state::<AppState>() {
                    if let Some(window) = app.get_webview_window("main") {
                        let is_visible = window.is_visible().unwrap_or(false);
                        if is_visible {
                            *state.is_window_visible.lock().unwrap() = false;
                            let _ = window.hide();
                        } else {
                            *state.is_window_visible.lock().unwrap() = true;
                            let _ = position_popup_at_click(&window, position.x, position.y);
                            let _ = window.show().and_then(|_| window.set_focus());
                        }
                    }
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
