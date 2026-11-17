# Third-party notices

Dotvar's source is available under the [MIT License](LICENSE). Third-party dependencies and assets retain their own licenses.

- **GPUI and GPUI Kit.** Dotvar uses [GPUI](https://docs.rs/crate/gpui-pre/0.3.12) and [GPUI Kit](https://docs.rs/crate/gpui-kit/0.10.0), including Kit's component, base, and assets crates, under Apache-2.0. [GPUI Kit Assets](https://docs.rs/crate/gpui-kit-assets/0.10.0) includes Lucide icons under ISC and Feather-derived icons under MIT. See its [icon license text](https://docs.rs/crate/gpui-kit-assets/0.10.0/source/LICENSE-LUCIDE).
- **Service logos.** Dotvar includes SVGs from [SVGL](https://svgl.app/) and local monochrome derivatives. The [asset catalog](crates/env-desktop/assets/services/README.md) records each source, and [SVGL's MIT license](crates/env-desktop/assets/services/LICENSE.svgl) is included. Service names and logos are trademarks of their respective owners and identify the corresponding services.
- **System fonts.** Dotvar uses fonts provided by macOS.
- **Copy and visibility buttons.** The native animation was inspired by the [Icon Swap example](https://transitions.dev/detail.html?t=icon-swap).

[`Cargo.lock`](Cargo.lock) records the exact Rust dependency versions. Consult the upstream license files for the full distribution terms of each dependency.
