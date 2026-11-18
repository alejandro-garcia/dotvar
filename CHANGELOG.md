# Changelog

This file tracks notable changes to Dotvar. It follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- The macOS disk image opens with a Dotvar-themed drag-to-install window.

## [0.1.0] - 2026-11-17

### Added

- Native macOS editor for `.env` files with remembered projects and tabs.
- In-place editing of variable names, values, and comments, with paste, row reordering, and undo and redo. Saving preserves untouched file content.
- Search by variable name, with values hidden by default and actions to reveal or copy individual values and copy the working document.
- Offline service detection with bundled icons and colors to distinguish environments.
- File watching with conflict handling that preserves unsaved edits and lets you save a separate copy.
- English and Spanish interface options, with a setting to follow the system language.
- Signed and notarized macOS installer for Apple Silicon, with a SHA-256 checksum.

[Unreleased]: https://github.com/nicoandrade/dotvar/compare/v0.1.0...master
[0.1.0]: https://github.com/nicoandrade/dotvar/releases/tag/v0.1.0
