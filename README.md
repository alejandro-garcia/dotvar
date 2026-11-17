<p align="center">
  <img src="assets/icon.png" width="96" height="96" alt="Dotvar app icon">
</p>

# Dotvar

Dotvar is a blazing-fast, privacy-first `.env` editor for macOS. Keep your project's environment files in one window, find the variable you need, and make precise edits that preserve the file's original formatting.

## Why Dotvar?

- ⚡ **Find any key fast.** Search by variable name and scroll smoothly through long files.
- 🔒 **Keep secrets local.** Dotvar edits files on your Mac, hides values by default, and recognizes providers offline.
- 📂 **Switch between environments.** Open a folder and Dotvar finds `.env`, `.env.*`, and `*.env` files at its root. Pick up where you left off with remembered projects and tabs.
- ✏️ **Edit in place.** Change names, values, and comments, paste new assignments, reorder rows, and undo or redo edits.
- 🎨 **Spot services at a glance.** Provider icons identify common services. Environment colors distinguish local, staging, and production files. [Service icon guide](docs/service-icons.md)
- 📋 **Copy just what you need.** Copy one value or the entire working file, including valid unsaved edits.
- 🔄 **Catch changes from other tools.** Dotvar reloads files as they change on disk and flags conflicts when you have unsaved edits, so you can resolve the conflict or save a separate copy.

## Built with Rust and GPUI

- **Rust** gives the file editor a memory-safe foundation. Each edit updates only the relevant bytes, keeping the rest of the file intact.
- **GPUI**, through **GPUI Kit**, powers the native macOS interface with GPU-accelerated rendering.

## Get started on macOS

The supported target is Apple Silicon. The app bundle declares macOS 15 as its minimum version.

See [GitHub Releases](https://github.com/nicoandrade/dotvar/releases) for downloadable builds. To install from a DMG, open it and drag Dotvar to Applications.

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

Copy actions write literal values to the macOS clipboard, including hidden values. The local catalog stores file and project paths, tab order, and selection. Preferences are stored alongside it in `~/Library/Application Support/Dotvar`.

Dotvar preserves comments, untouched whitespace, line endings, and UTF-8 BOMs. It treats variables, escapes, and shell expressions as literal text. Ambiguous syntax is kept as opaque text, and files with invalid UTF-8 open read-only. See the [dotenv grammar and editing guarantees](crates/env-core/README.md).

English is the default on first launch. **Settings → General → Language** also offers Spanish and System.

## Project layout

| Directory | Purpose |
| --- | --- |
| [`crates/env-core`](crates/env-core) | Dependency-free parser, patch-based editing, and undo history. |
| [`crates/env-desktop`](crates/env-desktop) | GPUI interface, document sessions, file watching, and local storage. |
| [`scripts`](scripts) | macOS packaging and synthetic QA fixtures. |

See [Contributing](CONTRIBUTING.md) for local verification and [third-party notices](THIRD_PARTY_NOTICES.md) for dependencies and assets. The project is licensed under [MIT](LICENSE).
