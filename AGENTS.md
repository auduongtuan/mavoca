# AGENTS.md

## Project Overview

This is a macOS menu bar vocabulary app built with Tauri v2, React 19,
TypeScript, Vite, Rust, SQLite, and Bun.

The app has three Tauri windows:

- `main`: frameless transparent always-on-top meaning popup.
- `manager`: full vocabulary manager for add/edit/delete/lookup.
- `settings`: backup configuration UI.

The Rust backend owns the SQLite database, tray title, tray click behavior,
Cambridge Dictionary lookup, popup positioning, backup sync, and auto-rotation.
The React frontend renders the popup, manager, and settings screens.

## Commands

- Install dependencies: `bun install`
- Run web-only Vite dev server: `bun run dev`
- Run the full Tauri desktop app in development: `bun run tauri dev`
- Build the frontend: `bun run build`
- Build the Tauri app bundle: `bun run tauri build`
- Preview the built frontend only: `bun run preview`
- Check Rust code: `cd src-tauri && cargo check`
- Format Rust code: `cd src-tauri && cargo fmt`

Tauri dev expects Vite on port `1420`. This is configured in `vite.config.ts`
and `src-tauri/tauri.conf.json`.

## Source Layout

- `src/App.tsx` routes by current Tauri window label:
  - `main` renders the learning popup.
  - `manager` renders `VocabManager`.
  - `settings` renders `SettingsPanel`.
- `src/hooks/useVocab.ts` loads the current popup card through Tauri commands
  and listens for Rust events: `vocab-changed`, `vocab-list-changed`, and
  `vocab-empty`. Rotation is driven by commands, not by index math in React.
- `src/components/VocabCard.tsx` renders the compact menu bar popup card, which
  shows one definition at a time.
- `src/components/VocabManager.tsx` renders the dark/light manager UI and calls
  commands for list, add, refresh, create, update, and delete.
- `src/components/VocabTreeEditor.tsx` edits the three-level tree: entries,
  guide-word sense groups, and definitions with their examples.
- `src/components/SettingsPanel.tsx` renders the backup path settings UI.
- `src/types.ts` defines frontend types shared across React components.
- `src/App.css` contains popup, manager, settings, light mode, and dark mode
  styling. It intentionally uses `color-scheme` rather than custom scrollbar
  styling.
- `public/vocab.json` is seed data only. Rust embeds it on first database
  initialization; runtime reads should go through SQLite.
- `src-tauri/src/lib.rs` contains app state, database setup and migration,
  Tauri commands, tray setup, popup positioning, macOS vibrancy, backup sync,
  and auto-rotation.
- `src-tauri/src/model.rs` defines the vocabulary tree types shared by the
  database and fetch layers, and converts between the tree and the flat
  definition rows the schema stores.
- `src-tauri/src/dictionary.rs` fetches and parses dictionary sources. It has
  no database or Tauri dependencies.
- `src-tauri/tauri.conf.json` configures windows, tray, dev URL, build hooks,
  bundle metadata, and macOS private API access.
- `src-tauri/capabilities/default.json` must include every window label that
  invokes backend commands, and every core command outside the default sets.
  `core:window:default` does not grant `allow-start-dragging` or
  `allow-toggle-maximize`, both of which the manager title bar calls. A denied
  command rejects inside the webview and never reaches Rust, so the only symptom
  is a control that quietly does nothing — always surface the rejection rather
  than discarding the promise.

## Data Model

SQLite is the runtime source of truth. The database file is created in Tauri's
app data directory as `vocab.sqlite3`.

A word holds multiple meanings in a three-level tree modelled on Cambridge:
part-of-speech entries, guide-word sense groups, and definitions with examples.

Tables:

- `vocabs`
  - `id INTEGER PRIMARY KEY`
  - `word TEXT NOT NULL UNIQUE COLLATE NOCASE`
  - `source TEXT NOT NULL DEFAULT 'manual'` — `cambridge`, `dictionaryapi`, or
    `manual`
  - `created_at TEXT NOT NULL DEFAULT (datetime('now'))`
  - `updated_at TEXT NOT NULL DEFAULT (datetime('now'))`
- `vocab_definitions` — one row per definition
  - `id INTEGER PRIMARY KEY`
  - `vocab_id INTEGER NOT NULL REFERENCES vocabs(id) ON DELETE CASCADE`
  - `entry_index INTEGER NOT NULL` — which part-of-speech entry
  - `part_of_speech TEXT NOT NULL`
  - `pron_uk TEXT NOT NULL DEFAULT ''`
  - `pron_us TEXT NOT NULL DEFAULT ''`
  - `guide_word TEXT NOT NULL DEFAULT ''`
  - `definition TEXT NOT NULL`
  - `examples TEXT NOT NULL DEFAULT '[]'` — JSON array of strings
  - `sort_order INTEGER NOT NULL`
- `settings`
  - `key TEXT PRIMARY KEY`
  - `value TEXT NOT NULL`

The tree is stored flat and rebuilt on read by grouping on `entry_index`, then
on consecutive `guide_word`. The schema does not enforce tree consistency;
`replace_word_definitions` is the single write path and enforces it there. Every
write goes through `save_word_tree`, which wraps the word row and its
definitions in one transaction.

`PRAGMA foreign_keys = ON` must be set on the connection or the cascade never
fires and deleting a word leaves orphaned definitions behind.

Databases created before this schema are migrated on launch by
`migrate_legacy_vocabs`: `category` becomes the part of speech, `pronunciation`
the US pronunciation, `meaning` the single definition, and `example` its only
example. The migration is transactional and idempotent.

The frontend mirrors these shapes in `src/types.ts` using camelCase:
`VocabWord` holds `entries`, `source`, and `definitionCount`; `VocabCardData` is
one flattened definition with `position` and `total` for the popup.

## Backup Behavior

Backup is configured through the `settings` window, not hardcoded. The setting
key is `backup_path`.

- If `backup_path` is empty, backup is disabled.
- `~/...` paths are expanded in Rust.
- Folder paths are accepted and normalized to `folder/vocab.sqlite3`.
- On first launch, the default is `~/Dropbox/Vocab Learning/vocab.sqlite3` only
  when `~/Dropbox` exists.
- Backup is written using `VACUUM main INTO` to a temporary SQLite file, then
  atomically renamed into place.
- Backup sync runs after vocabulary changes and after saving settings.

## Dictionary Lookup

Lookup lives in `src-tauri/src/dictionary.rs`, not in the original Node API.
It uses:

- `reqwest` blocking client with `rustls-tls`
- `scraper` for HTML parsing
- `urlencoding` for dictionary URL paths
- `serde_json` for the fallback API — `reqwest` is built with
  `default-features = false`, so `Response::json` is not available

`fetch_word` tries Cambridge first and falls back to `api.dictionaryapi.dev`
when Cambridge fails for any reason. The word's `source` records which answered.

Cambridge sits behind a Cloudflare challenge that rejects unusual clients. The
app's previous `VocabLearning/0.1` user-agent received a blanket HTTP 403; a
browser user-agent passes. Do not change `USER_AGENT` back to a custom string.

The fallback has no guide words and no UK/US split, so words sourced from it
have an empty middle tier.

Both sources return far more definitions than are useful — 57 and 62 for `run` —
so `MAX_DEFINITIONS_PER_WORD` caps what is imported. Both order their most
common senses first, so truncation keeps the useful ones.

There is no result picker. `add_vocab_word` fetches and imports every definition
in one step; `refresh_vocab_word` re-fetches an existing word and replaces its
definitions. Both are network calls made before the database lock is taken.

Scraped markup is not a stable contract. Selectors have looser fallbacks, and an
entry that yields no definitions is skipped rather than failing the whole fetch.
`parse_cambridge` is covered by a fixture test; a `#[ignore]`d live test in
`dictionary::live_tests` checks whether Cambridge is still reachable and still
parses:

```bash
cd src-tauri && cargo test -- --ignored --nocapture
```

## Translation

The popup's Translate button turns the current word and its definition into
Vietnamese with Apple's on-device Translation framework, through the
`translation-rs` crate. Examples stay in English. It is free, offline, and private, but the English and
Vietnamese packs must be downloaded in System Settings → General → Language &
Region → Translation Languages; without them `translator.rs` returns a hint
saying so. Manual sessions need macOS 26 or later.

- `src-tauri/src/translator.rs` has no database or Tauri dependencies, like
  `dictionary.rs`.
- The framework finishes its work on the main queue. `translate_texts` is an
  `async` command that moves the work to `spawn_blocking`; calling the
  translator from the main thread, including from a synchronous command,
  stalls until the bridge times out.
- `build.rs` adds `-rpath /usr/lib/swift`. The crate sets it for itself but it
  does not propagate, and without it the app aborts at launch with
  `Library not loaded: @rpath/libswift_Concurrency.dylib`.
- `App.tsx` keys `VocabCard` by card, so a new card starts untranslated.

## Tray And Window Behavior

- The tray title is the current vocabulary word.
- The configured tray icon is cleared at runtime with `tray.set_icon(None)` so
  the menu bar item shows only the vocab text.
- Left click on the tray toggles the meaning popup.
- Right click opens the tray menu with:
  - `Vocabulary Manager`
  - `Settings`
- The tray menu must not open on left click. Keep both config keys false:
  - `menuOnLeftClick`
  - `showMenuOnLeftClick`
- `setup_system_tray` also calls `set_show_menu_on_left_click(false)` at
  runtime to make this explicit.
- The menu is never attached to the tray item with `set_menu`. On macOS 27 an
  attached `NSStatusItem` menu opens on left click and swallows the event, so
  the popup never toggles, whatever the flags above say. `build_tray_menu`
  builds it and the right-click branch of `on_tray_icon_event` shows it with
  `popup_menu`. Do not reattach it.
- The main popup is hidden on startup and hidden again when it loses focus.
- Popup positioning is handled in Rust and uses the click position when opened
  from the tray.
- Auto-rotation runs every 10 seconds only while the popup is hidden.
- The rotation unit is a single definition, not a word. Rust owns the flattened
  card list in `AppState.cards`, and `current_index` indexes into it. Prev/Next
  walk every definition of a word before moving to the next word.
- `AppState.cards` and `AppState.current_index` are never locked at the same
  time. Keep it that way.

## macOS Notes

- The popup uses macOS vibrancy through `window-vibrancy`.
- The project uses `objc2-app-kit` and `objc2-quartz-core` for typed AppKit and
  Core Animation calls. Do not reintroduce the old `objc` `msg_send!` macro
  path; it triggered `unexpected cfg condition value: cargo-clippy` warnings.
- Be careful when changing window transparency, decorations, focus behavior,
  vibrancy, or the tray title. These are tightly connected to the menu bar
  popup experience.

## UI Conventions

- Keep the popup compact and transparent/vibrancy-friendly.
- Keep manager and settings visually aligned with the current restrained
  dark/light UI.
- Primary buttons should use a solid background.
- Part-of-speech selection in the manager is a `<select>`, not freeform text.
- The manager title bar drags the window through `startDragging()`. Two separate
  things are required: the `mousedown` handler must call `preventDefault()`, or
  the webview opens its own drag session next to the native one and macOS shows
  the copy cursor; and the capability must grant the command, or the window
  never moves at all. See the capabilities note under Source Layout.
- Form labels are uppercase 11px bold. `font-size`, `font-weight`,
  `letter-spacing`, and `text-transform` all inherit into the control a label
  wraps, so the shared input rule sets each one outright. A new control that
  skips that rule renders typed text as uppercase bold 11px.
- Do not customize scrollbars directly; rely on `color-scheme` for light/dark
  scrollbar adaptation.
- Avoid unrelated palette churn or decorative shadows.

## Coding Conventions

- Use Bun for package scripts and dependency management.
- Use TypeScript types for frontend command payloads.
- Keep native state and side effects in `src-tauri/src/lib.rs`.
- Keep React components small and focused.
- Prefer existing CSS classes and patterns before adding new styling.
- Avoid broad refactors unless needed for the specific task.
- Do not commit generated build outputs such as `dist/`, `target/`, or app
  bundles.

## Verification

For frontend-only changes, run:

```bash
bun run build
```

For Rust, Tauri command, tray, popup positioning, database, backup, or native
dependency changes, run:

```bash
cd src-tauri && cargo fmt
cd src-tauri && cargo check
cd src-tauri && cargo test
bun run build
```

`cargo test` covers the legacy migration, the tree round-trip through SQLite,
the delete cascade, card flattening, and both parsers. Run it after any change
to the schema, the tree model, or the dictionary parsing.

Use `bun run tauri dev` when behavior must be verified interactively in the
real macOS menu bar app.
