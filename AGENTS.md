# AGENTS.md

## Project Overview

Mavoca is a macOS menu bar vocabulary app built with Tauri v2, React 19,
TypeScript, Vite, Rust, SQLite, and Bun.

The app has four Tauri windows:

- `main`: frameless transparent always-on-top meaning popup.
- `quick-add`: frameless one-field window the add-word shortcut opens.
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
  - `quick-add` renders `QuickAdd`.
- `src/hooks/useVocab.ts` loads the current popup card through Tauri commands
  and listens for Rust events: `vocab-changed`, `vocab-list-changed`, and
  `vocab-empty`. Rotation is driven by commands, not by index math in React.
- `src/components/VocabCard.tsx` renders the compact menu bar popup card, which
  shows one definition at a time.
- `src/components/VocabManager.tsx` renders the dark/light manager UI and calls
  commands for list, add, refresh, create, update, and delete.
- `src/components/VocabTreeEditor.tsx` edits the three-level tree: entries,
  guide-word sense groups, and definitions with their examples.
- `src/components/SettingsPanel.tsx` renders the open-at-login toggle, the
  add-word shortcut, the rotation interval, and the backup path settings UI.
- `src/components/ShortcutRecorder.tsx` records a key combination as an
  accelerator string such as `Cmd+Ctrl+Shift+N` and displays it as `⌃⇧⌘N`.
- `src/components/QuickAdd.tsx` takes a single word and calls `add_vocab_word`,
  which fetches every meaning; on success the meaning popup shows the new word.
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
- `src-tauri/src/macos_dictionary.rs` and `src-tauri/native/` read the
  dictionaries installed with macOS; `build.rs` compiles the C bridge.
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
app data directory as `vocab.sqlite3`. That directory is named after the bundle
`identifier`, which is still `com.vocablearning.app` from before the app was
renamed Mavoca. Changing it would start every install on an empty database.

A word holds multiple meanings in a three-level tree modelled on Cambridge:
part-of-speech entries, guide-word sense groups, and definitions with examples.

Tables:

- `vocabs`
  - `id INTEGER PRIMARY KEY`
  - `word TEXT NOT NULL UNIQUE COLLATE NOCASE`
  - `source TEXT NOT NULL DEFAULT 'manual'` — `macos`, `cambridge`,
    `dictionaryapi`, or `manual`
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

## Open At Login

`tauri-plugin-autostart` registers a LaunchAgent for the running executable.
macOS owns that state, so `get_launch_at_login` reads it back instead of
storing a copy in `settings`, and the toggle applies immediately rather than on
Save. Enabling it from `bun run tauri dev` registers the debug binary under
`target/`; toggle it from the bundled app instead.

The plugin is held at 2.5.x: 2.5.1 requires Tauri 2.10 and pulls the whole
Tauri stack up with it. Upgrade the two together, deliberately.

## Add Word Shortcut

`tauri-plugin-global-shortcut` registers a system-wide shortcut that opens the
`quick-add` window. The accelerator lives in the `add_word_shortcut` setting,
seeded with `DEFAULT_ADD_WORD_SHORTCUT` (`Cmd+Ctrl+Shift+N`); an empty value
turns it off. `SettingsPanel` repeats the default for its Reset button.

- `register_add_word_shortcut` is the only place shortcuts are registered. It
  clears every registration first, so the plugin handler can treat any press as
  the add-word shortcut.
- `update_settings` registers the new shortcut before saving it. If another app
  owns the combination it re-registers the saved one and returns the error, so
  a rejected shortcut never leaves the app with none.
- A failure at launch is only logged; it must not stop the app starting.
- The plugin is pinned at `=2.3.0`: 2.3.1 requires a newer Tauri, the same
  trap as the autostart plugin.
- `add_vocab_word` is `async` and does its work in `spawn_blocking`, so the
  quick-add window keeps painting while the dictionary lookup runs.

## Backup Behavior

Backup is configured through the `settings` window, not hardcoded. The setting
key is `backup_path`.

- If `backup_path` is empty, backup is disabled.
- `~/...` paths are expanded in Rust.
- Folder paths are accepted and normalized to `folder/vocab.sqlite3`.
- On first launch, the default is `~/Dropbox/Mavoca/vocab.sqlite3` only when
  `~/Dropbox` exists.
- Backup is written using `VACUUM main INTO` to a temporary SQLite file, then
  atomically renamed into place.
- Backup sync runs after vocabulary changes and after saving settings.

## Dictionary Lookup

Lookup lives in `src-tauri/src/dictionary.rs`, not in the original Node API.
`fetch_word` tries three sources in order and the word's `source` records
which answered:

1. The New Oxford American Dictionary installed with macOS (`macos`).
2. Cambridge (`cambridge`).
3. `api.dictionaryapi.dev` (`dictionaryapi`).

### macOS dictionary

The public DictionaryServices call only returns plain text from the default
dictionary, so `native/dictionary_services.c` uses the private functions
Dictionary.app uses (`DCSCopyAvailableDictionaries`,
`DCSCopyRecordsForSearchString`, `DCSRecordCopyData`, and friends) to pick the
dictionary by name and get each entry as HTML.

- They are found with `dlsym` at runtime, never linked. A macOS release that
  drops or renames one makes every lookup return `Unavailable`, and
  `fetch_word` falls through to the online sources. Do not link them directly:
  a missing symbol would then stop the app launching.
- The bridge keeps only records whose headword is the word itself; a search
  also returns phrases and derivatives.
- `parse_macos_dictionary` takes the first definition of each numbered sense
  (`.se2`) and drops the "•" subsenses, phrases, and derivatives. Oxford has
  no guide words and no UK pronunciation.
- The transcription belongs to the part of speech, not the word: `record` is
  `/ˈrɛkərd/` as a noun and `/rəˈkɔrd/` as a verb, and each homograph carries
  its own. So it is read from the first `.ph` inside each `.se1`, falling back
  to the entry's own transcription when that part of speech has none (`run`
  keeps one for the whole entry). The first `.ph` on the page is *not* it: the
  inflections in `.infg` come first and each is transcribed (`running`
  `/ˈrənɪŋ/`, `ran` `/ræn/`), as do the phrases and derivatives in
  `.subEntryBlock`, so both are skipped. The popup shows a card's own
  transcription, so it follows the definition and part of speech on screen.
- Which US pronunciation an entry's HTML holds depends on Oxford's setting in
  Dictionary.app: the default is Oxford's own respelling (`/baNGk/`), and "US
  English (IPA)" gives IPA. The setting lives in the `DCSDictionaryPrefs` key of
  the `com.apple.DictionaryServices` domain, keyed by `com.apple.dictionary.NOAD`.
  At launch `prefer_ipa_pronunciation` switches it to IPA, but only when the user
  has never chosen a style. It is system-wide, so Dictionary.app and Look Up
  change with it. A user who picks the respelling keeps it, and new words then
  get respellings again.
- The parser test uses invented text in Oxford's markup. Keep real dictionary
  content out of the repo. To inspect live markup:
  `cd src-tauri && MAVOCA_WORD=run cargo test dump_macos_entries -- --ignored --nocapture`

### Online sources

The online sources use:

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
`parse_cambridge` and `parse_macos_dictionary` are covered by fixture tests;
`#[ignore]`d live tests in `dictionary::live_tests` check whether Cambridge is
still reachable and whether the macOS dictionary is still readable, and that
both still parse:

```bash
cd src-tauri && cargo test -- --ignored --nocapture
```

## Translation

Every popup card shows the word's Vietnamese meaning from Lạc Việt under the
definition, and the VI button adds the definition translated into Vietnamese.
Both come from one `translate_card` call, which `src-tauri/src/vietnamese.rs`
implements, made whenever the popup is open on a card. Examples stay in
English. Nothing is stored.

- The definition goes through Apple's on-device Translation framework, via the
  `translation-rs` crate. It is free, offline, and private, but the English and
  Vietnamese packs must be downloaded in System Settings → General → Language &
  Region → Translation Languages; without them `translator.rs` returns a hint
  saying so. Manual sessions need macOS 26 or later.
- The word comes from Từ điển Lạc Việt, the English–Vietnamese dictionary
  installed with macOS, read through the same bridge as the Oxford lookup. Its
  senses are filtered by part of speech (`PART_OF_SPEECH_LABELS`), and
  `pick_sense` chooses the one sharing the most syllables with the translated
  definition, because Lạc Việt numbers senses differently from Oxford or
  Cambridge. Fewer than `MIN_SENSE_OVERLAP` shared syllables keeps the first,
  most common sense. The sense is returned as `meaning`, split into its
  headline ("bờ; dải") and the explanation in parentheses after it. When Lạc
  Việt has no entry, `meaning` is null and the word is machine translated,
  shown only under VI; when the translator fails but Lạc Việt has the word,
  the card shows its first sense with no translated definition.
- The translator takes about a second per sentence and five or more after its
  model has been unloaded, and picking the sense waits on it, so the meaning
  appears a moment after the card. Results are cached in `vietnamese.rs`, and
  the cache lock is held for a whole translation, so overlapping calls for a
  card wait for the first instead of translating twice. The word is only
  machine translated when Lạc Việt lacks it.
- Lạc Việt is not saved with the word: the lookup is local and instant, its
  senses do not line up with the stored ones, and saving would copy licensed
  dictionary text into the backup.
- `translator.rs` and `vietnamese.rs` have no database or Tauri dependencies,
  like `dictionary.rs`.
- The framework finishes its work on the main queue. `translate_card` is an
  `async` command that moves the work to `spawn_blocking`; calling the
  translator from the main thread, including from a synchronous command,
  stalls until the bridge times out. For the same reason the translator cannot
  run in `cargo test`; the `#[ignore]`d `lac_viet_is_readable_and_still_parses`
  test feeds hand-written Vietnamese to the sense picker instead.
- `build.rs` adds `-rpath /usr/lib/swift`. The crate sets it for itself but it
  does not propagate, and without it the app aborts at launch with
  `Library not loaded: @rpath/libswift_Concurrency.dylib`.
- `App.tsx` keys `VocabCard` by card, so a new card starts untranslated.
  `VocabCard` keeps a session cache keyed by word, part of speech, and
  definition, so stepping through a word's definitions keeps the meaning it
  already fetched instead of blanking until Rust answers again.

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
- The menu bar item stays highlighted while the popup is open, like an item
  showing its own menu or popover. Since macOS 26 the menu bar draws that
  highlight itself, and `NSStatusBarButton.highlight` changes nothing on
  screen. `set_tray_highlighted` makes the private calls `NSPopover` makes,
  `_willPresentContent:cancellationHandler:` and `_didPresentContent:` with
  the same token object, checking `respondsToSelector:` first. The handler
  must be a real block; with nil nothing is drawn. Tauri 2.7 has no handle to
  the `NSStatusItem` (`with_inner_tray_icon` arrives in 2.10), so it is
  reached through the `NSStatusBarButton` in the app's status bar window. Every
  path that shows or hides the popup must call `set_tray_highlighted`.
- The main popup is hidden on startup and hidden again when it loses focus.
- Popup positioning is handled in Rust and uses the click position when opened
  from the tray.
- The popup fits its card. `usePopupAutoResize` watches the card with a
  `ResizeObserver` and calls `resize_popup`, which clamps the height between
  `POPUP_MIN_HEIGHT` and `POPUP_MAX_HEIGHT` and to the room left on screen;
  beyond that `.app` scrolls. `.vocab-card` must keep its natural height — if it
  sizes from the window again (`height: 100%`, `flex: 1`), the measurement feeds
  back into itself and the window can never shrink.
- Auto-rotation steps to the next definition every `rotation_interval_secs`
  seconds (default `DEFAULT_ROTATION_INTERVAL_SECS`, 10; 0 turns it off),
  counting only time the popup is hidden: opening the popup resets the count,
  so the word never changes the moment it closes. The thread checks once per
  `ROTATION_TICK` and reads the interval from `AppState`, so a change in
  Settings applies without a restart.
- The rotation unit is a single definition, not a word. Rust owns the flattened
  card list in `AppState.cards`, and `current_index` indexes into it. Prev/Next
  walk every definition of a word before moving to the next word. The ‹ ›
  buttons beside the popup's `1 of 3` counter wrap within the current word;
  `useVocab` turns them into `step_vocab_card` deltas.
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
- Never use `window.confirm`, `alert`, or `prompt`. The macOS webview does
  not implement them: `confirm` returns false without showing anything, so the
  action silently never happens. Use `ConfirmDialog` in `src/components/ui/`,
  built on Base UI's `AlertDialog`, as Delete does.
- Icons come from `@phosphor-icons/react`, imported by name so only the used
  ones are bundled. Do not hand-draw SVGs or use text glyphs or emoji as icons.
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
the delete cascade, card flattening, and the dictionary parsers. Run it after any change
to the schema, the tree model, or the dictionary parsing.

Use `bun run tauri dev` when behavior must be verified interactively in the
real macOS menu bar app.
