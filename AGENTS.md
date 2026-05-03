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
- `src/hooks/useVocab.ts` loads vocab words through Tauri commands and listens
  for Rust events: `vocab-changed`, `vocab-list-changed`, and `vocab-empty`.
- `src/components/VocabCard.tsx` renders the compact menu bar popup card.
- `src/components/VocabManager.tsx` renders the dark/light manager UI and calls
  commands for list, lookup, create, update, and delete.
- `src/components/SettingsPanel.tsx` renders the backup path settings UI.
- `src/types.ts` defines frontend types shared across React components.
- `src/App.css` contains popup, manager, settings, light mode, and dark mode
  styling. It intentionally uses `color-scheme` rather than custom scrollbar
  styling.
- `public/vocab.json` is seed data only. Rust embeds it on first database
  initialization; runtime reads should go through SQLite.
- `src-tauri/src/lib.rs` contains app state, database setup, Tauri commands,
  Cambridge scraping, tray setup, popup positioning, macOS vibrancy, backup
  sync, and auto-rotation.
- `src-tauri/tauri.conf.json` configures windows, tray, dev URL, build hooks,
  bundle metadata, and macOS private API access.
- `src-tauri/capabilities/default.json` must include every window label that
  invokes backend commands.

## Data Model

SQLite is the runtime source of truth. The database file is created in Tauri's
app data directory as `vocab.sqlite3`.

Tables:

- `vocabs`
  - `id INTEGER PRIMARY KEY`
  - `word TEXT NOT NULL UNIQUE COLLATE NOCASE`
  - `meaning TEXT NOT NULL`
  - `example TEXT NOT NULL`
  - `category TEXT NOT NULL`
  - `created_at TEXT NOT NULL DEFAULT (datetime('now'))`
  - `updated_at TEXT NOT NULL DEFAULT (datetime('now'))`
- `settings`
  - `key TEXT PRIMARY KEY`
  - `value TEXT NOT NULL`

The frontend `VocabWord` shape uses camelCase for timestamps:

- `id: number`
- `word: string`
- `meaning: string`
- `example: string`
- `category: string`
- `createdAt: string`
- `updatedAt: string`

The input shape for create/update/lookup is:

- `word: string`
- `meaning: string`
- `example: string`
- `category: string`

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

## Cambridge Lookup

Dictionary lookup is implemented in Rust, not by running the original Node API.
It uses:

- `reqwest` blocking client with `rustls-tls`
- `scraper` for HTML parsing
- `urlencoding` for dictionary URL paths

The lookup command is `lookup_vocab_word`. It fetches Cambridge Dictionary and
returns a `VocabInput`. The manager UI puts lookup results into a new-entry
form so it does not overwrite the selected row by accident.

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
- The main popup is hidden on startup and hidden again when it loses focus.
- Popup positioning is handled in Rust and uses the click position when opened
  from the tray.
- Auto-rotation runs every 10 seconds only while the popup is hidden.

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
- Category selection in the manager is a `<select>`, not freeform text.
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
bun run build
```

Use `bun run tauri dev` when behavior must be verified interactively in the
real macOS menu bar app.
