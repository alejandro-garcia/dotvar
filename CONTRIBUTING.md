# Contributing to Dotvar

Use English for issues, pull requests, and documentation. For a bug report, include your operating system and version, the Dotvar version or commit, and steps to reproduce it. Use a synthetic fixture for dotenv issues and remove credentials and personal paths from reports, logs, and screenshots.

## Development setup

- macOS: Apple Silicon Mac with Xcode selected as the active developer directory.
- Windows: Windows 10 or later with the Visual Studio C++ build tools.
- Linux: a C toolchain and the GPUI system libraries. On Ubuntu or Debian: `sudo apt-get install pkg-config libwayland-dev libxkbcommon-dev libxkbcommon-x11-dev libx11-xcb-dev libfontconfig-dev libfreetype-dev libvulkan1`.
- [Rustup](https://rustup.rs/). The repository pins Rust 1.100.0 with `rustfmt` and `clippy` in [`rust-toolchain.toml`](rust-toolchain.toml). Cargo uses the checked-in lockfile.
- Python 3 for packaging and fixture scripts.

```sh
rustup toolchain install --no-self-update
cargo run -p env-desktop --locked
cargo run -p env-desktop --locked -- /path/to/project/.env
```

The first build fetches and compiles dependencies. The desktop crate uses [GPUI Kit](https://gpui-kit.com/) for rendering and native controls.

## How the editor works

- [`env-core`](crates/env-core/README.md) splits the source into assignments, comments, blank lines, and opaque blocks. Edits apply range patches to the original bytes and update the undo history.
- The desktop session tracks saved bytes, working edits, and drafts. When saving, it checks the file against its saved snapshot, writes a private temporary file beside the original, syncs it, rechecks the target, and verifies the replacement. Hard links and access metadata that cannot be preserved block replacement: macOS extended access metadata, Linux POSIX ACLs, and read-only files. On Windows the temporary file gets an owner-only DACL and `ReplaceFileW` keeps the original's ACL, attributes, and alternate data streams. **Save As** creates a new file with private permissions and rejects existing paths.
- Keyboard shortcuts use the `secondary-` modifier, which is ⌘ on macOS and Ctrl on Windows and Linux.
- The file watcher monitors project directories without recursion. External edits reload clean documents and mark dirty documents as conflicted. A conflict preserves unsaved work until the user reloads or saves a copy.
- The catalog persists project and file paths, order, and selection. Drafts and undo history are kept in memory and are lost if the app is forcibly closed. `DOTVAR_CONFIG_DIR` selects an isolated catalog for testing.
- UI strings live in [`locales/en.json`](crates/env-desktop/locales/en.json) and [`locales/es.json`](crates/env-desktop/locales/es.json). Keep message keys and interpolation placeholders aligned.

## Verify changes

Run these commands from the repository root. CI runs them on macOS, Ubuntu, and Windows:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Some storage tests are platform-specific: macOS covers extended attributes, Linux covers POSIX ACLs, and Windows covers DACLs, file attributes, and alternate data streams. The trash tests move a synthetic file to the system Trash or Recycle Bin. To work on the parser independently of the UI, run `cargo test -p env-core --locked`. To exercise the desktop library without the native UI, run `cargo test -p env-desktop --lib --no-default-features --locked`.

For manual checks, generate fixtures in a fresh temporary directory:

```sh
qa_dir=$(mktemp -d)
python3 scripts/create-fixtures.py "$qa_dir/fixtures"
DOTVAR_CONFIG_DIR="$qa_dir/config" cargo run -p env-desktop --locked -- "$qa_dir/fixtures/.env"
```

The generator includes a small project and a benchmark project. Pass `--stress` to add a larger project catalog. For service recognition and SVG checks, see the [service icon guide](docs/service-icons.md).

## Build a local app bundle

```sh
python3 scripts/package-macos.py
```

The script builds the arm64 release with the macOS 15 deployment target, remaps local source paths, and creates and verifies an ad hoc signed `dist/Dotvar.app`. Open it from Finder or run `open dist/Dotvar.app`.

For maintainers preparing a distribution build, `python3 scripts/package-macos.py --distribution` uses a Developer ID Application certificate and a `notarytool` profile in the local Keychain. Set `NOTARY_PROFILE` or store `notary_profile` in the ignored `local/release.json`. If multiple certificates are available, `SIGN_IDENTITY` selects the certificate by fingerprint. `NOTARY_KEYCHAIN` selects an alternate Keychain. The script signs, notarizes, and validates the DMG, then writes it and a SHA-256 checksum under `dist/`.

Distribution builds use AppKit to render the Retina installer background and pinned Python tools to write and validate the Finder layout. The tools install in a temporary virtual environment, which requires access to PyPI.

Distribution builds put license texts and a dependency index under `Dotvar.app/Contents/Resources/Licenses`. Review `docs/dependency-license-supplements.txt` when upgrading dependencies and check packaged artifacts for local metadata before publishing. See [third-party notices](THIRD_PARTY_NOTICES.md).

## Publish a GitHub release

On the signing Mac, install [GitHub CLI](https://cli.github.com/) and use `gh auth login` with write access to `nicoandrade/dotvar`. The release script uses the same local Keychain setup as the distribution build.

1. Set the release version in `[workspace.package]` in `Cargo.toml` and run `cargo update --workspace` to update `Cargo.lock`. To provide release notes, move the relevant changelog entries under a version heading such as `## [0.1.0] - YYYY-MM-DD`.
2. Commit and push the reviewed release source to the default branch (`master`).
3. Run `python3 scripts/release.py --publish` from that branch on the signing Mac.

The script waits for CI on the release commit, packages that commit in a clean directory, signs and notarizes the DMG, then uploads `Dotvar.dmg` and `Dotvar.dmg.sha256` to a draft. It compares GitHub's asset digests with the local files before publishing as Latest and checks both downloads without authentication. It uses the version's changelog entry when available and otherwise asks GitHub to generate release notes. Use `python3 scripts/release.py --check` to check access and signing, or run `python3 scripts/release.py` for a signed rehearsal on your current checkout.

If an upload is interrupted, rerun the publish command from the same commit. It reuses the verified package under `dist/releases/` and resumes a matching draft. If the assets disagree, the script stops so the draft can be inspected. Published downloads use a [stable Latest URL](https://github.com/nicoandrade/dotvar/releases/latest/download/Dotvar.dmg) across versions.
