# AGENTS.md

Guidance for coding agents working on Dotvar, a native `.env` editor for macOS, Windows, and Linux written in Rust (GPUI via GPUI Kit). Human-facing docs: [README.md](README.md), [CONTRIBUTING.md](CONTRIBUTING.md), [crates/env-core/README.md](crates/env-core/README.md).

## Workspace

| Crate | Role |
| --- | --- |
| `crates/env-core` | Dependency-free, byte-preserving dotenv parser and editing model with undo/redo. No I/O, no OS code. |
| `crates/env-desktop` | GPUI app (`dotvar` binary), document sessions, storage, file watching, catalog, i18n. |

Key modules:

- `env-core/src/document.rs` – public types (`Document`, `Block`, `Assignment`, `EntryId`, `Diagnostic`).
- `env-core/src/parser.rs` – line parser; anything ambiguous becomes `Opaque`.
- `env-core/src/edit.rs` – all mutations, revision checks, undo history, `EditError`.
- `env-desktop/src/session.rs` – in-memory session, drafts, undo, save flow.
- `env-desktop/src/storage.rs` – open/save/rename/trash with conflict detection and atomic replace; Windows FFI lives in `storage/windows.rs`.
- `env-desktop/src/config.rs` – versioned `catalog.json` (paths, order, selection).
- `env-desktop/src/watch.rs` – non-recursive directory watching.
- `env-desktop/src/i18n.rs` – `tr!` macro and locale selection.
- `env-desktop/src/platform.rs` – objc2 bridge. It and `storage/windows.rs` are the only modules allowed to use `unsafe`.
- `env-desktop/src/{app,ui,ui/*,settings,design,action_icons,service_logos}.rs` – UI, gated behind the `native` feature.

## Toolchain and commands

Rust is pinned to 1.100.0 (edition 2024) in `rust-toolchain.toml`. Always pass `--locked`; do not change pinned (`=x.y.z`) dependency versions unless asked.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Faster loops:

```sh
cargo test -p env-core --locked                                   # parser/editing only
cargo test -p env-desktop --lib --no-default-features --locked    # desktop logic without the native UI
```

CI (`.github/workflows/ci.yml`) runs the three commands above on macOS, Ubuntu, and Windows with default features. Release packaging targets Apple Silicon macOS 15+ only. Linux needs GPUI system libraries (see CONTRIBUTING). Storage tests are platform-specific (macOS xattrs, Linux POSIX ACLs, Windows DACLs/attributes/streams), so report any platform whose checks you could not run instead of claiming it passed.

Manual runs use synthetic data and an isolated catalog:

```sh
qa_dir=$(mktemp -d)
python3 scripts/create-fixtures.py "$qa_dir/fixtures"
DOTVAR_CONFIG_DIR="$qa_dir/config" cargo run -p env-desktop --locked -- "$qa_dir/fixtures/.env"
```

## Invariants that are easy to break

**Byte preservation (`env-core`).** Editing one value must leave every other byte identical.

- Mutate only through the range patches in `edit.rs` and their neighbor checks (`check_unchanged_blocks`, `check_appended_blocks`, `remove_block_candidate`). Never rebuild file text from names and values.
- Keep ambiguous syntax `Opaque`. Loosening the opaque heuristics in `parser.rs` makes uncertain text editable.
- `#| ` (`COMMENT_CONTINUATION`) marks commented-out multiline values; changing it breaks round-trips.
- Every mutator takes an `expected_revision`; stale edits fail with `StaleRevision`.
- History is capped (100 operations / 8 MiB); over the cap, `HistoryLimit` leaves the document unchanged.
- Add round-trip cases to `crates/env-core/tests/preservation.rs` (fixtures in `tests/fixtures/*.fixture`) and API cases to `tests/editing.rs`.

**Secrets.** Errors, diagnostics, and logs must never include variable names or values; tests assert this. Use synthetic values (for example `SYNTHETIC=...`) in fixtures, docs, and reports.

**Saving (`storage.rs`).** Windows uses an owner-only DACL temp file and `ReplaceFileW`; Linux rejects files with POSIX ACLs. `save_with_hook` follows a fixed order: revalidate the snapshot, reject hard links, check access metadata and permissions, write a private (0600) temp file beside the target, sync, preserve metadata, revalidate, persist, sync the directory, read back. A failure after replacement is reported as `Uncertain`, never as a rollback. **Save As** uses no-clobber persistence and rename uses `RENAME_NOREPLACE`. Do not reorder or skip steps.

**Catalog (`config.rs`).** Writes are atomic, and an invalid catalog is preserved untouched. The location is `~/Library/Application Support/Dotvar` (macOS), `%APPDATA%\Dotvar` (Windows), or `$XDG_CONFIG_HOME/dotvar` (Linux); `DOTVAR_CONFIG_DIR` overrides it for tests.

**Shortcuts.** Bind with `secondary-` (⌘ on macOS, Ctrl elsewhere) and show labels through `i18n::shortcut_label`; never hardcode ⌘ glyphs in locales.

## Conventions

- `unsafe_code = "deny"` workspace-wide; only `platform.rs` opts out.
- Errors are hand-written enums with `message()`, `Display`, and `std::error::Error`; no `thiserror` or `anyhow`.
- UI strings go through `tr!` with keys in `crates/env-desktop/locales/en.json` and `es.json`. Keep keys and `%{}` placeholders aligned; the `dictionaries_cover_the_same_messages_and_placeholders` test enforces it. Note: `env-core` messages (`EditError::message`, diagnostics) are currently hardcoded Spanish and do not use `tr!`.
- Desktop tests live inline in `#[cfg(test)]` modules and use `tempfile::tempdir()`; save tests are macOS-only.
- Use English for code, comments, commits, issues, PRs, and docs.
- Record user-visible changes under `## [Unreleased]` in `CHANGELOG.md` (Keep a Changelog). Update README/CONTRIBUTING when you change a shortcut, command, or behavior they describe.

## Out of scope without explicit request

Releases and signing (`scripts/release.py`, `scripts/package-macos.py --distribution`), version bumps, dependency upgrades, CI changes, and pushing to `master`.
