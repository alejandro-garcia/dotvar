# Changelog

This file tracks notable changes to Dotvar. It follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Build and run Dotvar from source on Windows and Linux. Shortcuts use Ctrl, settings live in `%APPDATA%\Dotvar` or `$XDG_CONFIG_HOME/dotvar`, saving keeps file permissions, and Move to Trash uses the Recycle Bin or the freedesktop Trash.

### Changed

- CI checks formatting, lints, and tests on macOS, Ubuntu, and Windows.

## [0.3.2] - 2026-11-19

### Fixed

- Copy individual variable values without surrounding quotes while preserving quotes within the value.

## [0.3.1] - 2026-11-19

### Fixed

- Keep the focus ring fully visible when editing the first variable in the document.

## [0.3.0] - 2026-11-18

### Added

- Add and delete standalone comments in `.env` files, with undo and redo.
- Move an `.env` file to the macOS Trash from its tab menu, with confirmation and a warning for unsaved changes.

## [0.2.0] - 2026-11-18

### Added

- Drag tabs to reorder them within a project and use `⌘1`–`⌘9` to switch tabs by position.
- Edit commented-out variables, including multiline values, and comment or uncomment assignments from the variable menu.
- Open `.env` files from Finder or drag them onto Dotvar.
- Recognize `NEXT_PUBLIC_APP_` variables with the Next.js service icon.

### Changed

- The macOS disk image opens with a Dotvar-themed drag-to-install window.
- Use `Tab` and `Shift+Tab` to navigate document fields and variable actions with the keyboard.

### Fixed

- Preserve open drafts when creating an `.env` file at a path already open in Dotvar.
- Keep a pending variable's value masked when pasting into its name field and include it in Show All and Hide All.
- Reject a pending variable rename if an external reload has replaced the document.
- Preserve ambiguous continuations after quoted values together as opaque text.

## [0.1.0] - 2026-11-17

### Added

- Native macOS editor for `.env` files with remembered projects and tabs.
- In-place editing of variable names, values, and comments, with paste, row reordering, and undo and redo. Saving preserves untouched file content.
- Search by variable name, with values hidden by default and actions to reveal or copy individual values and copy the working document.
- Offline service detection with bundled icons and colors to distinguish environments.
- File watching with conflict handling that preserves unsaved edits and lets you save a separate copy.
- English and Spanish interface options, with a setting to follow the system language.
- Signed and notarized macOS installer for Apple Silicon, with a SHA-256 checksum.

[Unreleased]: https://github.com/nicoandrade/dotvar/compare/v0.3.2...master
[0.3.2]: https://github.com/nicoandrade/dotvar/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/nicoandrade/dotvar/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/nicoandrade/dotvar/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/nicoandrade/dotvar/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/nicoandrade/dotvar/releases/tag/v0.1.0
