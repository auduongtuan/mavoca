# Tauri to GPUI Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace Mavoca's Tauri shell and React webviews with native GPUI windows, without losing any behaviour recorded in `AGENTS.md` or any user data.

**Architecture:** Extract everything that is not UI into a Tauri-free `mavoca-core` crate, keep the Tauri app working on it, and build a GPUI app beside it. Tauri is removed only after the GPUI app reaches parity, so the app stays shippable throughout.

**Tech Stack:** `gpui-pre` =0.3.11, `gpui-component` (through `gpui-kit` =0.9.0), `tray-icon` 0.26, `global-hotkey` 0.9, `objc2`, `rusqlite`, `translation-rs`.

**Spec:** `AGENTS.md` is the behaviour spec. The spike is in `.plans/_prototypes/gpui-spike/`, with its findings in `REPORT.md`; the Corrections section below overrides that report where they differ.

## Verdict

**Plausible, but not proven, and not yet recommended.** The stack builds and launches on macOS 27.2, but the behaviours that justify the migration have not been observed. Task 0 is a go/no-go gate: do not start Task 1 until it has passed and the person has decided to continue.

| Area | Status | Basis |
|---|---|---|
| Build and launch, no Dock icon | Works | Run by the spike; `lsappinfo` reports `UIElement` |
| Translation off the main thread | Works | 1.1 to 2.5 s, correct Vietnamese, needs the Swift rpath |
| Status item alongside GPUI's run loop | Item builds, clicks untested | `tray-icon` attaches its menu only during a right click |
| Typing while another app stays in front | **Untested, decides the gate** | Real `NSPanel` with `NonactivatingPanel`, but see the warning below |
| Global shortcut | Registers, firing untested | The spike ran alongside Mavoca, which owns the same shortcut |
| Selected word, tray highlight, blur look, Vietnamese and IPA glyphs | Untested | All need a person |
| Inline autocomplete, shortcut recorder, speech | Must be built | Not in `gpui-component` |

**Warning on the keyboard result.** The spike's logs show `NSApp.isActive=true` beside `frontmost=<other app>` after showing the panel, in several runs. The spike never calls `activate`, so this is unexplained. A non-activating panel should not be activating the app. Test C must settle it.

### What the migration buys

- **Possible gain:** typing into quick-add without a click, if Test C passes.
- **Certain gain:** no webview and no JS/CSS toolchain.
- **Not shown:** smaller or lighter. Tauri's bundle is already 18 MB. The installed app's main process holds about 70 MB, not counting its WebKit processes; the spike, a debug build with almost no UI, holds 75 to 85 MB. These are not a fair comparison, so Task 0 measures one.

### The cheaper alternative

`AGENTS.md` (Menu Bar Agent) says the keyboard problem could be solved in Tauri by creating a real `NSPanel` through `tauri-nspanel`. **If the keyboard is the main motive, try that first.** It is a contained change, while this plan rewrites about 4,400 lines of React and CSS.

### Corrections to the spike report

- **Hide on focus loss needs no native observer.** The report says GPUI has no public hook. `Context::observe_window_activation` is public (`gpui-pre/src/app/context.rs:410`), and macOS wires it to `windowDidResignKey:` (`gpui-pre-macos/src/window.rs:462`). The spike did not try it, so Task 0 does.
- **The `isActive` contradiction is not from "one run".** It repeats across runs.
- **`translation-rs` differs:** the spike resolved 0.5.2; Mavoca's lockfile has 0.5.0.
- **The report's line counts for shims are estimates**, not measurements.
- **Test results taken while Mavoca was running are suspect.** It owns the same shortcut, and five spike processes were left running at once. Only one copy of either app may run during Task 0.

## Global Constraints

- Bundle identifier stays `com.vocablearning.app`; the data directory is named after it, and changing it starts every install on an empty database.
- Signing identity stays `Mavoca Local Signing`, with hardened runtime off. A new identity voids the Accessibility grant.
- Pin `gpui-kit = "=0.9.0"` exactly. It pins the `gpui-pre` family at `=0.3.11`. `cargo add gpui` gets the stale 0.2.2.
- `gpui-pre` has one crates.io owner (`huacnlee`) and republishes Zed snapshots about weekly, with breaking changes expected. Upgrade deliberately, in its own commit, and only when needed.
- Every final binary's `build.rs` emits `cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift`, because the flag does not propagate from a dependency.
- Do not lock `cards` and `current_index` at the same time.
- Translation runs on a worker thread, never the main thread.
- Lạc Việt's text is never stored. Only machine output for our own English text is written.
- Popup is 320 px wide. Muted text stays at 4.5:1 or better, measured on the real surface.
- Icons are Phosphor. Private AppKit calls check `respondsToSelector:` or the class name first, and fail soft.
- macOS only.

## Review Focus

1. **An existing database with real data:** opens from the same path, with translations and points intact. Pinned in Task 1.
2. **An existing Accessibility grant:** survives the move to a differently built bundle, or the person is told it must be re-granted. Pinned in Task 2.
3. **The word changes while the popup is open:** the item's width changes, and the popup follows it. Pinned in Task 3.
4. **A shortcut another app owns:** rejected, and the saved one stays registered. Pinned in Task 4.
5. **Long content on a short screen:** a long word at 320 px and a long entry with little room both scroll or wrap, and nothing is cut off. Pinned in Tasks 3 and 5.

---

### Task 0: Go/no-go gate

**Files:**
- Modify: `.plans/_prototypes/gpui-spike/src/main.rs`
- Create: `.plans/_prototypes/gpui-spike/GATE.md`

**Interfaces:**
- Produces: a recorded decision. Nothing later may start without it.

- [ ] **Step 1: Add hide on focus loss to the spike**

Use `cx.observe_window_activation` to call `toggle_window(window, false)` when `window.is_window_active()` becomes false. Add no notification observer.

- [ ] **Step 2: Build a release binary**

Run: `cd .plans/_prototypes/gpui-spike && cargo build --release`
Expected: finishes with no errors. Keep the build and test output out of the repo.

- [ ] **Step 3: Quit Mavoca and confirm a clean machine**

Run: `pgrep -lf 'Mavoca.app|gpui-spike'`
Expected: no output.

- [ ] **Step 4: The person runs Tests A to E from `REPORT.md`, and this one**

Test G: with the popup open, click another app. Expected: the popup hides.

- [ ] **Step 5: Measure memory fairly**

With the popup open on each app, record `footprint` for the GPUI process, and for the Tauri process plus its `com.apple.WebKit.WebContent` and `.Networking` processes. Record the GPUI figure as a lower bound, because the spike has no real UI.

- [ ] **Step 6: Try the alternative for comparison**

On a throwaway branch, test whether a `tauri-nspanel` quick-add takes typing with another app in front, using Test C. Record the result and the size of the change.

- [ ] **Step 7: Record the decision in `GATE.md`**

Continue only if **Test C passes** with the frontmost app unchanged, **and** Tests A and D pass, **and** the person prefers this over the `tauri-nspanel` result. A failed Test C ends this plan. A failed highlight (Test B) or blur look (Test E) does not by itself, but the person must accept the loss.

---

### Task 1: Extract `mavoca-core`

No behaviour changes. Valuable on its own, even if the migration later stops.

**Files:**
- Create: `Cargo.toml` (workspace with `src-tauri`, `crates/mavoca-core`)
- Create: `crates/mavoca-core/{Cargo.toml,build.rs,src/lib.rs,src/db.rs,src/rotation.rs,src/navigation.rs}`
- Move to `crates/mavoca-core/`: `src-tauri/src/{model,dictionary,macos_dictionary,vietnamese,translator,completion,selection}.rs`, `src-tauri/native/`
- Modify: `src-tauri/src/lib.rs`, `src-tauri/build.rs`, `src-tauri/Cargo.toml`

**Interfaces:**
- Produces:
  - `Core::open(data_dir: &Path, events: Arc<dyn CoreEvents>) -> Result<Core, String>`
  - `trait CoreEvents: Send + Sync { fn card_changed(&self, card: &VocabCard); fn list_changed(&self); fn empty(&self); fn title_changed(&self, word: &str); }`
  - On `Core`, one method per non-window Tauri command, same name and same `Result<_, String>`, without `tauri::State` or `AppHandle`. Slow ones stay synchronous; callers move them off the main thread.

- [ ] **Step 1: Write the failing test `opens_existing_database_with_translations_and_points`**

Create a database at the old schema with a stored `definition_vi`, `examples_vi`, and `points = 3`. Assert `Core::open` then `list_vocab_words` returns all three unchanged. Assert a second `open` changes nothing.

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p mavoca-core opens_existing_database`
Expected: FAIL, `Core` not defined.

- [ ] **Step 3: Move the Tauri-free modules, then split `lib.rs`**

Move schema and SQL functions to `db.rs`, `weighted_pick` and friends to `rotation.rs`, `NavigationHistory` and word stepping to `navigation.rs`, along with their tests. Replace `init_database(app)` with a `data_dir` parameter. `src-tauri/src/lib.rs` keeps windows, tray, plugins, and thin command wrappers, and implements `CoreEvents` with `app.emit`. Keep the rpath in `src-tauri/build.rs`; move the `cc` bridge to the core's `build.rs`.

- [ ] **Step 4: Run all checks**

Run: `cargo fmt --all && cargo test --workspace && cargo check --workspace && bun run build`
Expected: at least 47 tests pass (the existing 46 plus the new one), 0 failed. `grep -r tauri crates/mavoca-core/src` prints nothing.

- [ ] **Step 5: Confirm the Tauri app is unchanged**

Run: `bun run tauri build`, install it, and exercise the popup, quick-add, and manager with the existing database. Expected: no visible change.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock crates src-tauri
git commit -m "refactor(core): move the non-UI code into a Tauri-free crate"
```

---

### Task 2: GPUI shell and signed bundle

**Files:**
- Create: `crates/mavoca-app/{Cargo.toml,build.rs,Info.plist,src/main.rs,src/tray.rs,src/windows.rs,src/events.rs}`
- Create: `scripts/bundle-macos.sh`
- Modify: `Cargo.toml`

**Interfaces:**
- Consumes: `Core::open`, `CoreEvents`.
- Produces:
  - `tray::Tray::new(cx) -> Tray`, with `set_title(&str)`, `set_highlighted(bool)`, `item_frame() -> Option<Bounds<Pixels>>`
  - `windows::Popup` with `show(cx)`, `hide(cx)`, `toggle(cx)`, `anchor_under_tray(window)`, `resize_to_content(window, height: f64)`
  - `events::UiEvents`, implementing `CoreEvents` by forwarding to the main thread

- [ ] **Step 1: Write the failing test `clamps_popup_height`**

`clamp_popup_height(content: f64, room: f64) -> f64` returns `POPUP_MIN_HEIGHT` for 0, `POPUP_MAX_HEIGHT` for 10000 with ample room, and `room` when room is smaller.

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p mavoca-app clamps_popup_height`
Expected: FAIL, not defined.

- [ ] **Step 3: Implement the shell, reusing the spike's working code**

Accessory policy; text-only tray title; left click toggles; right click shows `Vocabulary Manager`, `Settings`, `Quit Mavoca`. Popup is `WindowKind::PopUp`, hidden at start, hidden through `observe_window_activation`. Anchor from the item's frame read at show time, never at setup, where the spike read a height of 0. Start auto-rotation on a thread, keeping its rule that only hidden time counts. Share `Core` as a GPUI global.

- [ ] **Step 4: Write `scripts/bundle-macos.sh`**

Builds release, assembles `Mavoca.app` with `Info.plist` (`CFBundleIdentifier` `com.vocablearning.app`, `LSUIElement` true), and runs `codesign --force -s "Mavoca Local Signing" --identifier com.vocablearning.app`. Fails if the identity is missing.

- [ ] **Step 5: Verify the signature matches the Tauri bundle's**

Run: `codesign -dr - Mavoca.app` on this bundle and on the installed Tauri one.
Expected: the same designated requirement. If they differ, tell the person that Accessibility must be re-granted, using the `tccutil` steps in `AGENTS.md`.

- [ ] **Step 6: Run the tests, then check by hand in the bundle**

Run: `cargo test -p mavoca-app`
Expected: PASS. By hand: no Dock icon, left and right click as above, the item stays highlighted while open, and `Quit Mavoca` quits.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock crates/mavoca-app scripts/bundle-macos.sh
git commit -m "feat(gpui): add the menu bar shell and signed bundle"
```

---

### Task 3: Popup card

**Files:**
- Create: `crates/mavoca-app/src/theme.rs`, `src/card.rs`, `src/speech.rs`
- Reference: `src/components/VocabCard.tsx`, `src/components/ui/IconButton.tsx`, the UI Conventions in `AGENTS.md`

**Interfaces:**
- Consumes: `Popup`, `Core::{get_current_vocab_card, step_vocab_card, step_vocab_word, random_vocab_card, award_vocab_point, lac_viet_meaning, translate_definition, translate_examples}`.
- Produces:
  - `theme::Tokens` for light and dark
  - `card::VocabCardView`
  - `speech::speak(word: &str, lang: &str)`, using `AVSpeechSynthesizer`

- [ ] **Step 1: Write the failing tests in `card.rs`**

- `identical_pronunciations_collapse_to_one_chip`: equal `pronUk` and `pronUs` give one chip; different give two; empty gives none, and the speaker stays on the word line.
- `senses_show_three_then_more`: five senses give three visible and a `+2 more` label.
- `only_a_ranked_first_sense_is_filled`: `ranked == false` fills none.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p mavoca-app card::`
Expected: FAIL, not defined.

- [ ] **Step 3: Implement the card at 320 px**

Follow the card rules in `AGENTS.md`: header changes as one unit, a step replaces the English section and leaves Vietnamese alone, examples nest under the definition, `Got it` is the only labelled footer button. Use `gpui-component`'s `Scrollbar` for the overlay thumb. Size the window with `on_children_prepainted` plus `clamp_popup_height`, and keep the card at natural height so the measurement cannot feed back. Keep the session translation caches.

- [ ] **Step 4: Follow the title**

After `set_title`, post `anchor_under_tray` to the next main-loop turn if the popup is open. Test by hand: step from `amenity` to `mellifluous` and let rotation change the word; the popup stays centred under the item.

- [ ] **Step 5: Measure contrast**

Screenshot the card in both themes over a bright and a dark background. Sample each muted token and compute its ratio. Expected: 4.5:1 or better. The surface has changed, so do not copy CSS numbers without measuring.

- [ ] **Step 6: Check rendering by hand**

Check Vietnamese marks, IPA glyphs, a long word that wraps past the badge, `Got it` drawing the next word, the arrows' history, the VI toggle, and both speakers in their own accents.

- [ ] **Step 7: Commit**

```bash
git add crates/mavoca-app
git commit -m "feat(gpui): render the popup card"
```

---

### Task 4: Settings, shortcut, and open at login

**Files:**
- Create: `crates/mavoca-app/src/settings.rs`, `src/shortcut.rs`, `src/login_item.rs`
- Reference: `src/components/SettingsPanel.tsx`, `src/components/ShortcutRecorder.tsx`

**Interfaces:**
- Consumes: `Core::{get_settings, update_settings}`.
- Produces:
  - `shortcut::register(value: &str) -> Result<(), String>`, the only registration path, clearing earlier ones first
  - `shortcut::format_accelerator(keystroke) -> String` (`Cmd+Ctrl+Shift+N`) and `shortcut::display(&str) -> String` (`⌃⇧⌘N`)
  - `login_item::{get() -> Result<bool, String>, set(bool) -> Result<bool, String>}`

- [ ] **Step 1: Write the failing tests**

- `formats_and_displays_accelerators`: round-trips `Cmd+Ctrl+Shift+N` and displays `⌃⇧⌘N`.
- `rejected_shortcut_keeps_the_saved_one`: with a fake registrar that rejects the new value, the setting stays the old value, the old one is re-registered, and the error is returned.
- `empty_shortcut_turns_it_off`: nothing is registered, and no error.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p mavoca-app shortcut::`
Expected: FAIL, not defined.

- [ ] **Step 3: Implement**

`global-hotkey` replaces the plugin. Build the recorder from `on_key_down` and `Kbd`. Read login state back from macOS rather than storing a copy, and apply the toggle at once. Replace the LaunchAgent that `tauri-plugin-autostart` wrote, so a user with it enabled does not end up launching a stale path or two copies. A failed registration at launch only logs.

- [ ] **Step 4: Run the tests, then check by hand in the bundle**

Run: `cargo test -p mavoca-app`
Expected: PASS. By hand: record a shortcut one other app owns and see the error with the old one still working; the rotation interval applies without a restart; the backup path saves; and login works after a real logout.

- [ ] **Step 5: Commit**

```bash
git add crates/mavoca-app
git commit -m "feat(gpui): add settings, shortcut, and open at login"
```

---

### Task 5: Quick-add

**Files:**
- Create: `crates/mavoca-app/src/quick_add.rs`, `src/suggestions.rs`, `src/preview.rs`
- Reference: `src/components/QuickAdd.tsx`, Add Word Shortcut and Add Word Suggestions in `AGENTS.md`

**Interfaces:**
- Consumes: `Core::{suggest_words, preview_word, add_vocab_word, take_pending_selection}`, `selection`, `shortcut::register`.
- Produces: `quick_add::open(cx)`, and `suggestions::SuggestionList` with `apply_key(key) -> KeyOutcome`.

- [ ] **Step 1: Write the failing tests for the keyboard contract**

- `enter_adds_the_field_even_with_a_row_highlighted`
- `tab_takes_the_highlighted_row`
- `esc_closes_the_list_then_the_window`: first press closes the list; second closes the window.
- `stale_answers_are_dropped`: an answer for a superseded word does not replace the list.
- `resize_clamps`: width is clamped to 420..780; height is at most 640 and the room left on screen.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p mavoca-app suggestions::`
Expected: FAIL, not defined.

- [ ] **Step 3: Implement**

The list is an inline `div`, not `Combobox`, so it can be measured. Read the selection in the hotkey handler **before** the window takes focus, and keep the Accessibility-missing message. Keep the constants: `MAX_SUGGESTIONS` 6, `PREVIEW_DEFINITIONS` 4, `PREVIEW_SENSES` 3, and fixed pixel caps only. Keep the debounces, and the per-opening preview cache. Fetch on a worker thread.

- [ ] **Step 4: Run the tests, then check by hand in the signed bundle**

Run: `cargo test -p mavoca-app`
Expected: PASS. By hand: with Safari in front and a word selected, press the shortcut and the field starts on it, selected. Type without clicking, and Safari stays frontmost. Try `griff`, which Oxford lacks, for an online preview; a word nobody has; and a short screen, where the entry scrolls rather than cuts off.

- [ ] **Step 5: Commit**

```bash
git add crates/mavoca-app
git commit -m "feat(gpui): add the quick-add window with suggestions and preview"
```

---

### Task 6: Vocabulary manager

**Files:**
- Create: `crates/mavoca-app/src/manager.rs`, `src/tree_editor.rs`
- Reference: `src/components/VocabManager.tsx`, `src/components/VocabTreeEditor.tsx`

**Interfaces:**
- Consumes: `Core::{list_vocab_words, add_vocab_word, refresh_vocab_word, create_vocab, update_vocab, delete_vocab}`.
- Produces: `manager::open(cx)`, `tree_editor::TreeEditor`, whose `to_input() -> VocabTreeInput` feeds `update_vocab`.

- [ ] **Step 1: Write the failing test `editor_round_trip_keeps_the_tree`**

Load a word with two entries, a guide-word group, and examples; `to_input` with no edits equals the input the word came from. Saving through `Core` keeps its `points` and `definition_vi`.

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p mavoca-app editor_round_trip`
Expected: FAIL, not defined.

- [ ] **Step 3: Implement**

Use `Input`, `Select` for part of speech, and `AlertDialog` for delete. Show points in the list. Keep the traffic lights and title bar drag. Non-floating window.

- [ ] **Step 4: Run the tests, then check by hand**

Run: `cargo test -p mavoca-app`
Expected: PASS. By hand: add, refresh, edit, and delete a word; the popup updates live, and cancelling a delete deletes nothing.

- [ ] **Step 5: Commit**

```bash
git add crates/mavoca-app
git commit -m "feat(gpui): add the vocabulary manager"
```

---

### Task 7: Cutover

**Files:**
- Delete: `src/`, `src-tauri/`, `public/` (after moving `vocab.json` into `crates/mavoca-core`), `index.html`, `vite.config.ts`, `package.json`, `bun.lock`, `tsconfig*.json`
- Modify: `AGENTS.md`, `Cargo.toml`

**Interfaces:**
- Consumes: everything above.

- [ ] **Step 1: Walk `AGENTS.md` line by line against the bundle**

Record each behaviour as pass, fail, or deliberately dropped in the PR description. Do not continue with an unexplained fail.

- [ ] **Step 2: Upgrade test on a copy of real data**

Run the GPUI bundle against a copy of the real database: the word count, points, and stored translations match, and a backup writes through `VACUUM INTO`. Test with open at login enabled beforehand.

- [ ] **Step 3: Confirm the person accepts removing Tauri**

This deletes the fallback. Tag the last Tauri commit first: `git tag tauri-final`.

- [ ] **Step 4: Remove Tauri and the frontend, and rewrite `AGENTS.md`**

Update commands, layout, and verification sections. Keep the rules that still apply, and remove those about webviews, capabilities, CSS, and Tauri plugins.

- [ ] **Step 5: Run all checks**

Run: `cargo fmt --all --check && cargo test --workspace && ./scripts/bundle-macos.sh`
Expected: all pass. `grep -ri tauri crates scripts Cargo.toml` prints nothing.

- [ ] **Step 6: Commit**

```bash
git add -u && git add crates scripts Cargo.toml Cargo.lock
git commit -m "feat!: replace Tauri with GPUI"
```
