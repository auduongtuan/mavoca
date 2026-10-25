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
- `src/components/VocabCard.tsx` renders the compact menu bar popup card: one
  English definition at a time with the pronunciation of the entry it belongs
  to, and beneath it the Vietnamese section for that entry, which the stepper
  leaves alone. It also holds the session translation cache, so stepping
  through a word's definitions does not re-ask Rust for a translation it
  already has (see Translation).
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
  Typing completes the word from the system word list and previews the meaning of
  the row under the arrow keys beside it (see Add Word Suggestions).
  It also offers completions while the word is typed (see Add Word
  Suggestions).
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
- `src-tauri/src/selection.rs` reads the word highlighted in the application in
  front of Mavoca, through the Accessibility bridge in `native/selection.c`.
- `src-tauri/src/completion.rs` answers the add-word field's completions from
  the word list macOS installs. Like the parsers, it has no database or Tauri
  dependencies.
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
  - `definition_vi TEXT NOT NULL DEFAULT ''` — the machine translation of
    `definition`, empty until it has been translated
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

`definition_vi` is a cache of a translation, not content: it belongs to the
English text beside it, so `replace_word_definitions` copies the stored value
across when the definition text is unchanged and leaves it empty otherwise.
Nothing else in the tree round-trip carries it — the editor neither reads nor
writes it. Lạc Việt's own senses are never stored; see Translation.

`PRAGMA foreign_keys = ON` must be set on the connection or the cascade never
fires and deleting a word leaves orphaned definitions behind.

Databases created before this schema are migrated on launch by
`migrate_legacy_vocabs`: `category` becomes the part of speech, `pronunciation`
the US pronunciation, `meaning` the single definition, and `example` its only
example. The migration is transactional and idempotent.

A column added later is handled separately, because `CREATE TABLE IF NOT EXISTS`
leaves an existing table alone: `ensure_definition_vi_column` checks
`table_columns` and issues an `ALTER TABLE`, once per launch. Add columns that
way rather than editing the `CREATE` alone, which only new databases would get.

The frontend mirrors these shapes in `src/types.ts` using camelCase:
`VocabWord` holds `entries`, `source`, and `definitionCount`; `VocabCardData` is
one flattened definition with `definitionId`, `position`, and `total` for the
popup.

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

### The Selected Word

Pressing the shortcut over a highlighted word opens the field on that word, so
looking one up in another app costs a shortcut rather than a shortcut and a
copy. Double click a word in Safari, Preview, or an editor, press the shortcut,
and the word is there with its suggestions and its meaning already showing.

- The read happens in the shortcut handler, **before** `show_quick_add_window`:
  Mavoca is an agent app, so the other application still has the keyboard at
  that moment, and once the quick-add window takes the focus the frontmost
  application is Mavoca itself and there is nothing to read. `recall_selected_word`
  stores the result in `AppState.pending_selection`; `take_pending_selection`
  hands it over and clears it, because the window is shown again and again and
  only the opening behind a press has a selection behind it.
- `QuickAdd.tsx` asks for it in the focus handler, which is also where the field
  is emptied, so a new opening starts on the selection rather than on the word
  left over from last time. The field takes the word selected, so typing replaces
  it, and anything typed while the answer was on its way is left alone.
- `selection::single_word` decides what counts as a word: a selection is
  trimmed of the quotes, brackets, and punctuation an application wraps it in,
  and is refused outright when it is not exactly one word. A triple click selects
  a paragraph, and a paragraph is not a word to add.
- This is the Accessibility API (`AXFocusedApplicationAttribute` →
  `AXFocusedUIElementAttribute` → `AXSelectedTextAttribute`), so Mavoca needs the
  Accessibility permission. Without it every call fails, the first failure asks
  macOS to show its own grant dialog once per launch, and the shortcut opens an
  empty field exactly as before. A rebuilt bundle is re-signed, so the grant
  belongs to the previous binary and has to be given again after an install.
- The calls are synchronous IPC into the other application, so the system-wide
  element is given a short `AXUIElementSetMessagingTimeout`: a stalled
  application must not hold the key press.

## Add Word Suggestions

The add-word field completes what is being typed, and the row under the arrow
keys shows its meaning beside the list. Candidates come from the saved words and
from `/usr/share/dict/words`, the list macOS uses for Look Up and spell checking.

- `suggest_words` returns saved words first, flagged `saved`, which doubles as a
  duplicate warning; the system list fills the rest. Both are prefix matches,
  and the shortest comes first, since the word closest to what was typed is the
  likeliest one. Everything is capped at `MAX_SUGGESTIONS` (6).
- `completion.rs` reads the whole file once, folds it to lowercase, drops
  entries that are not plain letters (proper nouns, `o'clock`, `e-mail`), sorts
  and dedupes, and binary searches it. `setup` warms it on a background thread
  so no keystroke waits for the read; a machine without the file simply gets no
  completions.
- The lookup is debounced in `QuickAdd.tsx` (`SUGGEST_DEBOUNCE_MS`), and each
  answer is dropped when a newer word has superseded it, so one pause in typing
  costs one query and a slow answer cannot overwrite a newer list.
- The window is measured to its content: `QuickAdd.tsx` reports the height of
  `.quick-add-content` plus the window's own padding to
  `resize_quick_add_window`, so a suggestion takes a row of window and nothing
  is left over when the list closes. `show_quick_add_window` puts it back to
  `QUICK_ADD_HEIGHT` on every opening, because it hides on blur and would
  otherwise remember the height of a list that is gone. `.quick-add` keeps the
  field at the top of the window rather than centring it, so only the list gains
  room and nothing on screen moves.
- Keys: ↓/↑ choose, Enter adds the word in the field, Tab completes the
  highlighted row, Esc closes the list and only closes the window when the list
  is already closed. Clicking a row takes it, and `preventDefault` on `mousedown`
  is what keeps the focus in the field.
- Enter always adds what the field holds, even with a row highlighted, because
  the list now carries a highlight from the moment it opens
  (`autoHighlight="always"`, with `keepHighlight` so a pointer passing over does
  not drop it). The doc recommends `'always'` for a list rendered inline; here it
  is also what gives the pane something to show without pressing ↓ first. Tab is
  therefore the only keyboard way to take a suggestion.
- The list is Base UI's `Autocomplete`, in `inline` mode with `filter={null}`:
  Rust has already filtered against the whole system word list, and the window is
  sized from the content it measures, so a popup that floated above the window
  could never be measured.

### Preview

The highlighted suggestion shows its meaning beside the list, so the field never
has to be given up to answer "what does this mean?".

- `preview_word` answers from the saved vocabulary first, through the database, so
  what it shows is exactly what the popup will show, translation included. Any
  other word goes through `dictionary::fetch_word`, the same order
  `add_vocab_word` uses: macOS (Oxford), then Cambridge, then the dictionary API.
- So the pane can make a request: a word Oxford has never seen — `griff`, most
  names and most new words — is looked up online rather than answered with "not
  in the dictionary". Only a word none of the three sources carries previews as
  `source: "none"`. Nothing is written; a preview is a read.
- Because that can cost a second, the pane keeps the meaning it already has
  while the next one loads, its lookup is debounced (`PREVIEW_DEBOUNCE_MS`), and
  each word's answer is cached for the opening, so stepping down the list does
  not send a request per row. The header names a source the user would not
  expect it from: `Saved`, `Cambridge`, `Dictionary API`.
- The pane shows `PREVIEW_DEFINITIONS` (4) definitions with their guide words and
  part of speech, and `total` behind them for "N more definitions when added".
  `VocabCard` is where the whole tree is read.
- It answers in Vietnamese too, because that is what a quick lookup is for: the
  same `vietnamese::lac_viet_meaning` the popup uses, under a `Lạc Việt · danh từ`
  label, three senses and a `+N more` count (`PREVIEW_SENSES`). The part of speech
  comes from the first previewed definition, and a word the English sources
  missed — `source: "none"` — passes none, so every Lạc Việt group is offered
  rather than none. That is often the only thing the pane can say about such a
  word, and why the section renders outside the English one.
- Nothing is ranked and no sense is marked: the preview has no translation of its
  own definition, so the order is Lạc Việt's own and `ranked` is always false. A
  saved word it does not carry falls back to the stored `definitionVi`, which is
  the one thing there that belongs to the definition above it.
- Lạc Việt is read in the same `spawn_blocking` as the fetch, so the pane still
  arrives in one round trip; it is a local dictionary read and adds nothing
  measurable to the online path. `.quick-add-preview` scrolls past
  `QUICK_ADD_MAX_HEIGHT` instead of letting a long entry lose its last sense off
  the bottom of the window.
- The window widens while a meaning is shown: `resize_quick_add_window` takes a
  width as well as a height and clamps it to
  `QUICK_ADD_WIDTH..QUICK_ADD_MAX_WIDTH` (420..780). `QuickAdd.tsx` measures both
  from `.quick-add-content`, which is `width: max-content` so it reports the room
  the columns need rather than the width of the window, and hides the pane again
  when the list closes, back to 420.

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

A card is two sections, not one translated block: the English definition follows
the definition stepper, and the Vietnamese entry below it does not follow it.
They are separate on purpose. Lạc Việt numbers its senses its own way, merges
homographs under one part of speech, and has no notion of the guide words
Cambridge uses, so no sense it holds can be called the translation of the
definition on screen. Examples stay in English.

- `src-tauri/src/vietnamese.rs` implements both halves, and the popup asks for
  them separately: `lac_viet_meaning(word, part_of_speech, definition_id)` and
  `translate_definition(definition, definition_id)`, plus `translate_word(word)`
  for the case where Lạc Việt has no entry. The first is a local dictionary
  lookup and returns at once; the second takes about a second and is not
  allowed to hold the Vietnamese section back.
- The word comes from Từ điển Lạc Việt, the English–Vietnamese dictionary
  installed with macOS, read through the same bridge as the Oxford lookup. Its
  groups are filtered by part of speech (`PART_OF_SPEECH_LABELS`, longest key
  first, so `pronoun` is not caught by `noun`); two groups carrying the same
  label are Lạc Việt homographs and are shown joined under one label. No group
  matches — the POS is unknown, or the entry has none — and every group is
  offered rather than none.
- `rank_senses` puts the sense sharing the most syllables with the translated
  definition first, and only ever reorders. Lạc Việt's senses do not line up
  with the English ones, so the rest stay visible and the section shows the
  first `SENSES_SHOWN` under a "N more" expand. Below `MIN_SENSE_OVERLAP` shared
  syllables, or with no translation to compare against, the order is Lạc
  Việt's own, most common first, and `ranked` is false so the UI marks nothing:
  the first sense is highlighted only when the definition actually moved it
  there.
- Each sense arrives split into its headline ("bờ; dải") and the explanation in
  parentheses after it (`split_sense`).
- The definition goes through Apple's on-device Translation framework, via the
  `translation-rs` crate. It is free, offline, and private, but the English and
  Vietnamese packs must be downloaded in System Settings → General → Language &
  Region → Translation Languages; without them `translator.rs` returns a hint
  saying so. Manual sessions need macOS 26 or later.
- Phrasal verbs are not special-cased for Lạc Việt: a card whose part of speech
  is `phrasal verb` matches on the `verb` it contains.
- The translated definition is **stored**, in `vocab_definitions.definition_vi`,
  so a card is translated once per definition. Lạc Việt's senses are **not**
  stored: Từ điển Lạc Việt is licensed, its text does not line up with the rows,
  and `VACUUM INTO` copies the whole database into Dropbox. Only machine output
  for our own English text is written.
- A stored translation belongs to the English text, not to the row:
  `replace_word_definitions` carries the translations of unchanged definitions
  over and drops the rest, so a manager save or a re-fetch keeps them and a
  re-worded definition is translated again. Row ids are reused by SQLite after
  a replace, which is why `VocabCard` keys its session cache on
  word + part of speech + definition rather than on the id.
- Results are cached in `vietnamese.rs` too, and the cache lock is held for a
  whole translation, so overlapping calls for one text wait for the first
  instead of translating twice.
- `translator.rs` and `vietnamese.rs` have no database or Tauri dependencies,
  like `dictionary.rs`.
- The framework finishes its work on the main queue. The translation commands
  are `async` and move the work to `spawn_blocking`; calling the translator
  from the main thread, including from a synchronous command, stalls until the
  bridge times out. For the same reason the translator cannot run in
  `cargo test`; the `#[ignore]`d `lac_viet_is_readable_and_still_parses` test
  feeds hand-written Vietnamese to the ranker instead.
- `build.rs` adds `-rpath /usr/lib/swift`. The crate sets it for itself but it
  does not propagate, and without it the app aborts at launch with
  `Library not loaded: @rpath/libswift_Concurrency.dylib`.
- `App.tsx` keys `VocabCard` by card, so a new card starts untranslated.
  `VocabCard` keeps a session cache keyed by word, part of speech, and
  definition, so stepping through a word's definitions keeps the translation it
  already fetched instead of blanking until Rust answers again.
- The translation is fetched as soon as the card is visible, not when VI is
  pressed: it is what orders the Lạc Việt senses, and it is stored once. Cards
  mounted while the popup is hidden are skipped, so the rotation does not
  translate words nobody is looking at.

## Menu Bar Agent

Mavoca is a menu bar extra, not an app in the Dock.

- `src-tauri/Info.plist` sets `LSUIElement`, and Tauri merges it into the
  bundle's `Info.plist`. That key is what Launch Services reads: no Dock icon,
  no Cmd+Tab row, no Force Quit row, whether or not a window is open. Setting
  the policy only at runtime leaves a Dock icon on screen for the first frames
  of the launch, which is why the plist carries it too. `setup` still calls
  `set_activation_policy(Accessory)` so `bun run tauri dev`, which never sees
  the bundle plist, behaves the same.
- An agent app cannot be activated. `NSApp.activate` is refused, and switching
  the policy to `Regular` to get around it does not hold: macOS grants the
  activation for a couple of seconds around the click that asked for it and
  then hands the front back, so a window that started typing stops mid-word and
  the blur handler closes it. Do not reintroduce policy promotion.
- So a window only holds the keyboard around the user's own click, and the tray
  menu therefore has a `Quit Mavoca` item: with no Dock icon and no Cmd+Tab
  row, it is the only way to quit the app.
- `MavocaPanel`, `attach_panel`, `convert_to_panel`, and `make_panel_key` are a
  non-activating panel (`NSWindowStyleMaskNonactivatingPanel`) that would let an
  inactive app type into its windows, the arrangement Spotlight and Alfred use.
  They are **off by default**, behind `MAVOCA_PANEL_MODE=full`. Retyping a
  window with `object_setClass` does give it the keyboard while another app
  stays in front — `quick-add` took typing and wrote the word — but on macOS 27
  every window is an `AppKit._NSGlassEffectWindow`, and replacing that class
  loses the window's mouse hit testing: a click inside the manager lands on
  whatever is behind it, and one attempt aborted the app on an Objective-C
  exception unwinding through Rust. Turning panels on for real means creating
  the `NSPanel` rather than converting an existing window, which needs the
  window to come from `tauri-nspanel` or from our own `NSWindow` rather than
  from `WebviewWindowBuilder`.

## Tray And Window Behavior

- The tray title is the current vocabulary word.
- The configured tray icon is cleared at runtime with `tray.set_icon(None)` so
  the menu bar item shows only the vocab text.
- Left click on the tray toggles the meaning popup.
- Right click opens the tray menu with:
  - `Vocabulary Manager`
  - `Settings`
  - `Quit Mavoca`
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
- Popup positioning is handled in Rust and is anchored to the menu bar item, not
  to the screen or to a click: `position_popup_under_tray` centres the popup
  under the status item's button and puts its top edge just below the menu bar.
  The item's frame is read from AppKit, because Tauri 2.7 exposes none for a tray
  icon, and it is the only anchor that fits the case where no click happened at
  all — quick-add shows the popup from the frontend after a word is added. The
  old corner fallback (`position_popup_window`) only runs when the item cannot be
  found, and a tray click only falls back to its click position for the same
  reason. Note that a position set from the main thread is applied by the event
  loop after the command returns: reading `outer_position` in the same breath
  shows the stale one, which is not a positioning failure.
- The item is exactly as wide as the title, so the anchor moves every time the
  title does: stepping from `amenity` to `mellifluous` walks the item's centre
  left and a popup left where the previous word put it sits off its own item.
  `update_tray_title` therefore ends with `follow_tray_item`, which re-anchors a
  popup that is already open through the same `position_popup_under_tray`. It is
  posted to the main queue rather than run inline — the rotation thread changes
  the title too, and posting lets the item lay itself out at the new width
  first — and it does nothing while the popup is hidden, which is re-anchored
  when it is shown.
- The popup fits its card. `usePopupAutoResize` watches the card with a
  `ResizeObserver` and calls `resize_popup`, which clamps the height between
  `POPUP_MIN_HEIGHT` and `POPUP_MAX_HEIGHT` and to the room left on screen;
  beyond that the popup scrolls. The scroll container is Base UI's `ScrollArea`
  in `App.tsx`, whose viewport keeps the webview's own scrollbar hidden and
  draws a thumb over the card instead; a native bar would take a gutter and
  reflow the text sideways the moment the card outgrew the window. `.vocab-card`
  must keep its natural height — if it sizes from the window again
  (`height: 100%`, `flex: 1`), the measurement feeds back into itself and the
  window can never shrink. The 6px inset that `usePopupAutoResize` adds to the
  card belongs to `.app-body`, the card's parent, which is where the hook reads
  it from.
- Auto-rotation steps to the next definition every `rotation_interval_secs`
  seconds (default `DEFAULT_ROTATION_INTERVAL_SECS`, 10; 0 turns it off),
  counting only time the popup is hidden: opening the popup resets the count,
  so the word never changes the moment it closes. The thread checks once per
  `ROTATION_TICK` and reads the interval from `AppState`, so a change in
  Settings applies without a restart.
- The rotation unit is a single definition, not a word. Rust owns the flattened
  card list in `AppState.cards`, and `current_index` indexes into it. The two
  ways of moving are separate on purpose:
  - The ‹ › buttons beside the popup's `1 of 3` counter walk the definitions of
    the current word and wrap within it; `useVocab` turns them into
    `step_vocab_card` deltas. They sit on the card's word line, above the part
    of speech and the transcription, because a step changes those as well as the
    definition text.
  - The footer's ← → move between words, skipping whatever is left of the
    current word's definitions and landing on the first definition of the next
    or previous one, wrapping at both ends. A word is a run of cards sharing a
    `vocab_id`, so `step_vocab_word` finds the boundary in `word_step_index`
    rather than counting positions — the card it starts from may be any of the
    word's definitions.
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
- The popup window is 320px wide. Judge a card layout at that width, not in a
  wide browser window: a row that fits at 1000px wraps in the real popup.
- The card header changes as one unit. The ‹ `1 of 3` › stepper sits on the
  word's line, and the part-of-speech badge with the pronunciation row sits
  under it, because a step replaces all of them: the badge and the
  transcription belong to the definition on screen, not to the word.
  `--meta-btn-size` lives on `.vocab-card` rather than `.card-meta`, since the
  stepper moved out of that row and still has to match the VI toggle.
- The pronunciation row (`.word-pronunciation`) shows the card's `pronUk` and
  `pronUs` as accent-labelled chips, each with its own speak button so each is
  read in its own accent. Equal values collapse to a single chip, and a word
  with no transcription keeps the speaker on the word line instead: the macOS
  dictionary carries one transcription, Cambridge carries both.
- Keep manager and settings visually aligned with the current restrained
  dark/light UI.
- The card body is two sections with their own `.section-label` heading,
  English and Tiếng Việt, divided by a hairline. A step replaces the English
  section and leaves the Vietnamese one in place: it is the entry's meaning,
  not the definition's, so it must not move with the stepper.
- A Lạc Việt sense is shown as a bulleted `.sense` with its headline in bold and
  the explanation after it, three at a time under a `+N more` toggle
  (`SENSES_SHOWN`). Only the first sense is filled in, and only when the
  translated definition ranked it there — do not present a sense as *the*
  translation, because it is not one.
- Primary buttons should use a solid background.
- Muted text is measured, not guessed. Labels and metadata in the popup and the
  add-word window sit at 4.5:1 or better against the vibrancy backdrop, which is
  `0.7` ink in light mode and `0.6`-`0.66` white in dark; the card's `1 of 3`
  counter, its guide word, its `+N more` toggle, the dictionary source and the
  translation status are all in that group. The part-of-speech badges mix their
  hue toward the ink in light mode (72%) and toward white in dark (85%), because
  as text on their own 16% tint several hues sat at 3.4-4.3:1. Keep the
  hierarchy in size, weight and case instead of dropping below that floor.
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
  scrollbar adaptation. The popup is the exception, because it is sized to its
  content: it scrolls through `ScrollArea` with an overlay thumb
  (`.app-scrollbar`), so a bar never takes width from the card or shifts it.
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
