# AGENTS.md

## Project Overview

This is a macOS menu bar vocabulary app built with Tauri v2, React 19, TypeScript, Vite, Rust, and Bun.

The frontend lives in `src/` and renders a compact vocabulary popup. The Tauri backend lives in `src-tauri/` and owns the native menu bar tray icon, popup window visibility, tray title updates, and auto-rotation behavior.

## Commands

- Install dependencies: `bun install`
- Run web-only Vite dev server: `bun run dev`
- Run the full Tauri desktop app in development: `bun run tauri dev`
- Build the frontend: `bun run build`
- Build the Tauri app bundle: `bun run tauri build`
- Preview the built frontend only: `bun run preview`

Tauri dev expects Vite on port `1420`. This is configured in `vite.config.ts` and `src-tauri/tauri.conf.json`.

## Source Layout

- `src/App.tsx` wires loading, error, and vocabulary card states.
- `src/hooks/useVocab.ts` loads vocabulary from `/vocab.json`, reads and updates the Rust-backed current word index, and listens for `vocab-changed` events.
- `src/components/` contains small presentational React components.
- `src/types.ts` defines the shared frontend `VocabWord` shape.
- `src/App.css` contains the full popup styling, including light/dark mode and transparent/vibrancy-friendly layout.
- `public/vocab.json` is the vocabulary data source used by the frontend at runtime.
- `src-tauri/src/lib.rs` contains app state, Tauri commands, tray setup, popup positioning, macOS vibrancy, and auto-rotation.
- `src-tauri/tauri.conf.json` configures the app window, tray icon, dev URL, build hooks, and bundle metadata.

## Important Implementation Notes

- Keep `public/vocab.json` compatible with both consumers:
  - The frontend fetches it as `/vocab.json`.
  - Rust embeds it with `include_str!("../../public/vocab.json")`.
- The vocabulary entry shape is:
  - `word: string`
  - `meaning: string`
  - `example: string`
  - `category: string`
- The main Tauri window is intentionally hidden on startup and shown from the tray click handler.
- The app uses a frameless, transparent, always-on-top popup window with macOS vibrancy. Be careful when changing window dimensions, positioning, decorations, transparency, or focus behavior.
- The tray title is updated from the current vocabulary word. Frontend navigation calls `update_current_word_index` to keep Rust state and the tray title in sync.
- Auto-rotation runs in Rust every 10 seconds when the popup is not visible and emits `vocab-changed` for the frontend.

## Coding Conventions

- Use Bun for package scripts and dependency management.
- Keep React components typed with TypeScript and preserve strict compiler settings.
- Prefer small, focused React components and hooks; keep native behavior in `src-tauri/src/lib.rs`.
- Preserve the existing CSS class naming and compact popup layout unless intentionally redesigning the UI.
- Avoid introducing new formatting tools or broad refactors without a specific need.
- Do not commit generated build outputs such as `dist/`, `target/`, or temporary app bundles.

## Verification

For frontend-only changes, run:

```bash
bun run build
```

For native tray, popup positioning, vibrancy, or Tauri command changes, run:

```bash
bun run tauri dev
```

Use `bun run tauri build` when changes affect packaging, Tauri configuration, native dependencies, icons, or release behavior.
