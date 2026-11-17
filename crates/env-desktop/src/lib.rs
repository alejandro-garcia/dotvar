rust_i18n::i18n!("locales", fallback = "en");

#[cfg(feature = "native")]
mod action_icons;
pub mod config;
#[cfg(feature = "native")]
mod design;
mod i18n;
pub mod model;
pub mod presentation;
pub mod service_logos;
pub mod services;
pub mod session;
pub mod storage;
pub mod watch;

#[cfg(feature = "macos-quit")]
mod platform;

#[cfg(feature = "native")]
mod app;
#[cfg(feature = "native")]
mod settings;
#[cfg(feature = "native")]
mod ui;
#[cfg(feature = "native")]
pub use app::{CloseWindow, OpenFile, OpenProject, Quit, run};
#[cfg(feature = "native")]
pub use i18n::usage;
