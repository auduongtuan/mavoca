# Mavoca - macOS Menu Bar App

A beautiful macOS menu bar application for learning vocabulary words, built with Tauri and React.

## Features

- **Menu Bar Integration**: Lives in your macOS menu bar for quick access
- **Random Vocabulary**: Browse through vocabulary words with random selection
- **Rich Content**: Each word includes meaning, example usage, and category
- **Clean UI**: Modern, responsive design with dark mode support
- **Quick Navigation**: Previous/Next buttons and random word selection

## Tech Stack

- **Backend**: Rust with Tauri v2
- **Frontend**: React 19 + TypeScript
- **Build Tool**: Vite
- **Package Manager**: Bun

## Development

### Prerequisites

- Rust (installed via rustup)
- Bun
- macOS (for menu bar functionality)

### Getting Started

1. Clone the repository
2. Install dependencies:
   ```bash
   bun install
   ```

3. Start development server:
   ```bash
   bun run tauri dev
   ```

### Building

```bash
bun run tauri build
```

## Project Structure

```
mavoca/
├── src/                    # React frontend
│   ├── components/         # React components
│   ├── hooks/             # Custom React hooks
│   └── types.ts           # TypeScript definitions
├── src-tauri/             # Rust backend
│   ├── src/               # Rust source code
│   └── tauri.conf.json    # Tauri configuration
└── public/                # Static assets
    └── vocab.json         # Vocabulary data
```

## Vocabulary Data

The app currently uses a JSON file (`public/vocab.json`) with sample vocabulary words. Each entry contains:

- `word`: The vocabulary word
- `meaning`: Definition of the word
- `example`: Example sentence using the word
- `category`: Part of speech (noun, verb, adjective, etc.)

## Future Enhancements

- Notion database integration
- Progress tracking
- Spaced repetition learning
- Custom vocabulary lists
- Export/import functionality

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)

## License

MIT License
