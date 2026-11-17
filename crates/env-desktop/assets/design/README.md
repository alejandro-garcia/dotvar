# Desktop design assets

This directory contains the SVGs used for Dotvar's tabs, project picker, editor tools, and welcome screen. [`manifest.json`](manifest.json) records each asset's design reference, dimensions, and SHA-256. [`design.rs`](../../src/design.rs) maps runtime identifiers to embedded SVGs and defines presentation colors and dimensions.

`bbb88.svg` is retained for design reference. Dotvar uses the macOS window buttons at runtime. The app uses macOS system fonts.

The [`actions`](../actions) directory contains the copy and visibility graphics. See the [service logo catalog](../services/README.md) for provider assets.
