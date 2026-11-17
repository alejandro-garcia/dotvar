# Monochrome service icons

Each icon is a local derivative of a [source SVG](../README.md). The [manifest](manifest.json) records source URLs and SHA-256 hashes. The root viewport is 16 × 16, with a visible mark of roughly 14.5 px centered inside it. Transparent cutouts and differences in alpha preserve the shapes when GPUI applies an environment color.

The Stripe variant uses the S from [SVGL's wordmark](https://svgl.app/library/stripe_wordmark.svg). The other sources are recorded in the manifest.

Render a comparison atlas from the workspace root:

```sh
cargo run -p env-desktop --example service_icon_atlas --no-default-features --locked -- /tmp/dotvar-icons.png
```

Each cell shows the original SVG, its flattened alpha, and the derivative at enlarged and actual sizes. Add Rust variant names after the output path to render a subset.
