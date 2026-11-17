<p align="center">
  <img src="assets/icon.png" width="96" height="96" alt="Dotvar app icon">
</p>

# Dotvar

Dotvar is a native macOS editor for `.env` files. Open a project, switch between its environment files, and edit variables and comments while preserving the rest of each file byte for byte.

## What you can do

- Open a file or a folder. Dotvar finds `.env`, `.env.*`, and `*.env` files at the folder's root and remembers projects and tabs locally.
- Edit names, values, and comments in place. Add assignments by pasting dotenv text, reorder rows, and undo or redo edits.
- Search by variable name. Reveal individual values when needed, or copy a value or the entire working document, including pending valid edits.
- Recognize common services from key names and values using [bundled icons](docs/service-icons.md). Environment colors help distinguish local, staging, and production files.
- See external changes as they happen. If you have unsaved edits, Dotvar keeps them and lets you resolve the conflict or save a separate copy.

## Get started on macOS

The supported target is Apple Silicon. The app bundle declares macOS 15 as its minimum version.

To build and run from source, install [Rust with rustup](https://rustup.rs/) and [Xcode](https://developer.apple.com/xcode/). From the repository root, run:

```sh
rustup toolchain install --no-self-update
cargo run -p env-desktop --locked
```

Press **⌘O** to open a file or folder, or pass a file path after `--`. You can try Dotvar with synthetic data and an isolated catalog:

```sh
demo_dir=$(mktemp -d)
cp examples/service-icons.env "$demo_dir/.env"
DOTVAR_CONFIG_DIR="$demo_dir/config" cargo run -p env-desktop --locked -- "$demo_dir/.env"
```

To create a local `Dotvar.app`, run `python3 scripts/package-macos.py` and open `dist/Dotvar.app`. See the [development guide](CONTRIBUTING.md) for build requirements, tests, and packaging.

## Working with `.env` files

| Action | Shortcut |
| --- | --- |
| Open a file or folder | **⌘O** |
| Add a project | **⇧⌘O** |
| Switch projects | **⌘K** |
| Find a variable | **⌘F** |
| Save / Save As | **⌘S** / **⇧⌘S** |
| Copy the working document | **⇧⌘C** |

Values start hidden. Copy actions write literal values to the macOS clipboard, including hidden values. Dotvar reads and edits files locally, and service detection runs offline. The local catalog stores file and project paths, tab order, and selection. Preferences are stored alongside it in `~/Library/Application Support/Dotvar`.

Dotvar preserves comments, untouched whitespace, line endings, and UTF-8 BOMs. It treats variables, escapes, and shell expressions as literal text. Ambiguous syntax is kept as opaque text, and files with invalid UTF-8 open read-only. See the [dotenv grammar and editing guarantees](crates/env-core/README.md).

English is the default on first launch. **Settings → General → Language** also offers Spanish and System.

## Project layout

| Directory | Purpose |
| --- | --- |
| [`crates/env-core`](crates/env-core) | Dependency-free parser, patch-based editing, and undo history. |
| [`crates/env-desktop`](crates/env-desktop) | GPUI interface, document sessions, file watching, and local storage. |
| [`scripts`](scripts) | macOS packaging and synthetic QA fixtures. |

See [Contributing](CONTRIBUTING.md) for local verification and [third-party notices](THIRD_PARTY_NOTICES.md) for dependencies and assets. The project is licensed under [MIT](LICENSE).
