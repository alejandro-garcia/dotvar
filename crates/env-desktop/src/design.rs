//! Figma-derived presentation tokens and original, embedded SVGs.
//!
//! Sources and SHA-256 hashes live beside the unchanged SVG files. Variable icons
//! use GPUI's monochrome mask; other design assets keep their original colors.
//! macOS window controls remain native; the exported reference is not rendered.

use crate::{
    service_logos::{self, Service},
    services,
    session::QuoteStyle,
};
use gpui_kit::{
    AssetSource, BoxShadow, Div, Img, ObjectFit, Result, SharedString, Styled, StyledImage, Svg,
    div, img, px, rgb, svg,
};
use std::{borrow::Cow, path::Path};

pub const WINDOW_BG: u32 = 0x1c1917;
pub const CONTENT_BG: u32 = 0x292524;
pub const SELECTOR_BG: u32 = 0x44403b;
pub const FIELD_BORDER: u32 = 0x0c0a09;
pub const SECONDARY: u32 = 0x79716b;
pub const KEY_COLOR: u32 = 0xffb86a;
pub const VALUE_COLOR: u32 = 0xe7e5e4;
pub const QUOTE_COLOR: u32 = 0x57534d;
pub const QUOTE_GAP: f32 = 3.;
pub const QUOTE_BUTTON_SIZE: f32 = 26.;
pub const TAB_TEXT_ACTIVE: u32 = 0xa6a09b;
pub const TAB_TEXT_INACTIVE: u32 = 0x57534d;
pub const TITLE_HEIGHT: f32 = 43.;
pub const TAB_HEIGHT: f32 = 31.;
pub const FIELD_HEIGHT: f32 = 32.;
pub const FIELD_LINE_HEIGHT: f32 = 18.;
pub const FIELD_PADDING_X: f32 = 10.;
pub const FIELD_PADDING_Y: f32 = 6.;
pub const KEY_WIDTH: f32 = 295.;
pub const KEY_GAP: f32 = 30.;
pub const BLOCK_GAP: f32 = 12.;
pub const COMMENT_GAP: f32 = 5.;
pub const CONTENT_INSET: f32 = 5.;
pub const CONTENT_LEFT: f32 = 15.;
pub const CONTENT_RIGHT: f32 = 10.;
pub const CONTENT_TOP: f32 = 40.;
pub const MONO_FONT: &str = "SF Mono";
pub const UI_FONT: &str = ".AppleSystemUIFont";

/// The same frame surrounds a value in both preview and edit mode.
pub fn variable_field() -> Div {
    div()
        .relative()
        .w_full()
        .min_w_0()
        .min_h(px(FIELD_HEIGHT))
        .bg(rgb(WINDOW_BG))
        .border_1()
        .border_color(rgb(FIELD_BORDER))
        .rounded(px(6.))
        .shadow(vec![
            BoxShadow::new(px(0.), px(1.), gpui_kit::white().opacity(0.06)),
            BoxShadow::new(px(0.), px(2.), gpui_kit::black().opacity(0.25))
                .blur_radius(px(2.))
                .inset(),
        ])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Environment {
    Neutral,
    Local,
    Staging,
    Production,
}

impl Environment {
    pub const fn icon_color(self) -> u32 {
        match self {
            Self::Neutral => 0x79716b,
            Self::Local => 0x008236,
            Self::Staging => 0xbb4d00,
            Self::Production => 0xe7000b,
        }
    }

    /// Presentation by exact filename segments, never by inspecting file values.
    pub fn from_path(path: &Path) -> Self {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let parts: Vec<_> = name.split('.').collect();
        if parts.iter().any(|p| matches!(*p, "production" | "prod")) {
            Self::Production
        } else if parts.iter().any(|p| matches!(*p, "staging" | "stage")) {
            Self::Staging
        } else if parts.contains(&"local") {
            Self::Local
        } else {
            Self::Neutral
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asset {
    AddFile,
    AddFileHover,
    AddVariable,
    Comment,
    Folder,
    WelcomeFolder,
    Ufo,
    OpenFolder,
    Command,
    Chevron,
    ChevronHover,
    Close,
    CloseHover,
    Dirty,
    Dots,
    QuoteUnquoted,
    QuoteDouble,
    QuoteSingle,
    VariableCopy,
    VariableEye,
    FileNeutral,
    FileNeutralInactive,
    FileLocal,
    FileLocalInactive,
    FileStaging,
    FileProduction,
    FileProductionInactive,
    Service(Service),
    GenericNeutral,
    GenericLocal,
    GenericProduction,
}

impl Asset {
    pub const fn path(self) -> &'static str {
        match self {
            Self::AddFile => "design/add-file.svg",
            Self::AddFileHover => "design/add-file-hover.svg",
            Self::AddVariable => "design/add-variable.svg",
            Self::Comment => "design/comment.svg",
            Self::Folder => "design/a9b45.svg",
            Self::WelcomeFolder => "design/17ad8.svg",
            Self::Ufo => "design/ufo.svg",
            Self::OpenFolder => "design/ba30e.svg",
            Self::Command => "design/24e04.svg",
            Self::Chevron => "design/91bbf.svg",
            Self::ChevronHover => "design/e78ce.svg",
            Self::Close => "design/b089c.svg",
            Self::CloseHover => "design/037c3.svg",
            Self::Dirty => "design/96321.svg",
            Self::Dots => "design/dots.svg",
            Self::QuoteUnquoted => "design/quote-unquoted.svg",
            Self::QuoteDouble => "design/quote-double.svg",
            Self::QuoteSingle => "design/quote-single.svg",
            Self::VariableCopy => "design/1619b.svg",
            Self::VariableEye => "design/2e0b8.svg",
            Self::FileNeutral => "design/cce02.svg",
            Self::FileNeutralInactive => "design/6f03e.svg",
            Self::FileLocal => "design/a80f5.svg",
            Self::FileLocalInactive => "design/07adb.svg",
            Self::FileStaging => "design/1d52b.svg",
            Self::FileProduction => "design/c3937.svg",
            Self::FileProductionInactive => "design/4ebda.svg",
            Self::Service(service) => service.path(),
            Self::GenericNeutral => "design/e76a5.svg",
            Self::GenericLocal => "design/26b2c.svg",
            Self::GenericProduction => "design/5f0d3.svg",
        }
    }
    pub const fn dimensions(self) -> (f32, f32) {
        match self {
            Self::AddFile | Self::AddFileHover => (13., 13.),
            Self::Comment => (15.9993, 14.0006),
            Self::OpenFolder => (25.0004, 25.),
            Self::Command => (14., 14.),
            Self::Close | Self::CloseHover | Self::Dirty | Self::Dots | Self::Ufo => (18., 18.),
            Self::Chevron | Self::ChevronHover => (7.99989, 4.42848),
            Self::QuoteUnquoted => (12., 12.),
            Self::QuoteDouble => (12.0001, 9.00048),
            Self::QuoteSingle => (5.25, 9.00048),
            Self::VariableCopy => (32.0005, 32.),
            Self::VariableEye => (32., 32.),
            _ => (16., 16.),
        }
    }
}

pub fn quote_icon(style: QuoteStyle) -> Asset {
    match style {
        QuoteStyle::Unquoted => Asset::QuoteUnquoted,
        QuoteStyle::Double => Asset::QuoteDouble,
        QuoteStyle::Single => Asset::QuoteSingle,
    }
}

pub fn file_icon(path: &Path, selected: bool) -> Asset {
    match (Environment::from_path(path), selected) {
        (Environment::Neutral, true) => Asset::FileNeutral,
        (Environment::Neutral, false) => Asset::FileNeutralInactive,
        (Environment::Local, true) => Asset::FileLocal,
        (Environment::Local, false) => Asset::FileLocalInactive,
        (Environment::Staging, _) => Asset::FileStaging,
        (Environment::Production, true) => Asset::FileProduction,
        (Environment::Production, false) => Asset::FileProductionInactive,
    }
}

pub fn variable_icon(name: &str, value: &str, environment: Environment) -> Asset {
    if let Some(service) = services::detect(name, value) {
        return Asset::Service(service);
    }
    match environment {
        Environment::Local => Asset::GenericLocal,
        Environment::Production => Asset::GenericProduction,
        _ => Asset::GenericNeutral,
    }
}

/// Apply the file palette to both detected logos and the generic fallback.
pub fn variable_asset(name: &str, value: &str, environment: Environment) -> Svg {
    let icon = variable_icon(name, value, environment);
    let path = match icon {
        Asset::Service(service) => service.monochrome_path(),
        _ => icon.path(),
    };
    svg()
        .path(path)
        .w(px(16.))
        .h(px(16.))
        .flex_shrink_0()
        .text_color(rgb(environment.icon_color()))
}

/// Preserve the SVG's root geometry and colors; callers size the surrounding hit area.
pub fn asset(asset: Asset) -> Img {
    let (width, height) = asset.dimensions();
    img(asset.path())
        .w(px(width))
        .h(px(height))
        .flex_shrink_0()
        .object_fit(ObjectFit::Contain)
}

/// Compose application SVGs with GPUI Kit's own embedded controls/resources.
pub struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = service_logos::load(path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        let bytes: Option<&'static [u8]> = match path {
            "design/dotvar.png" => Some(include_bytes!("../../../assets/icon.png")),
            "design/add-file.svg" => Some(include_bytes!("../assets/design/add-file.svg")),
            "design/add-file-hover.svg" => {
                Some(include_bytes!("../assets/design/add-file-hover.svg"))
            }
            "design/add-variable.svg" => Some(include_bytes!("../assets/design/add-variable.svg")),
            "design/comment.svg" => Some(include_bytes!("../assets/design/comment.svg")),
            "design/ba30e.svg" => Some(include_bytes!("../assets/design/ba30e.svg")),
            "design/dots.svg" => Some(include_bytes!("../assets/design/dots.svg")),
            "design/quote-unquoted.svg" => {
                Some(include_bytes!("../assets/design/quote-unquoted.svg"))
            }
            "design/quote-double.svg" => Some(include_bytes!("../assets/design/quote-double.svg")),
            "design/quote-single.svg" => Some(include_bytes!("../assets/design/quote-single.svg")),
            "design/17ad8.svg" => Some(include_bytes!("../assets/design/17ad8.svg")),
            "design/ufo.svg" => Some(include_bytes!("../assets/design/ufo.svg")),
            "design/24e04.svg" => Some(include_bytes!("../assets/design/24e04.svg")),
            "design/a9b45.svg" => Some(include_bytes!("../assets/design/a9b45.svg")),
            "design/91bbf.svg" => Some(include_bytes!("../assets/design/91bbf.svg")),
            "design/e78ce.svg" => Some(include_bytes!("../assets/design/e78ce.svg")),
            "design/1619b.svg" => Some(include_bytes!("../assets/design/1619b.svg")),
            "design/2e0b8.svg" => Some(include_bytes!("../assets/design/2e0b8.svg")),
            "design/9f0a1.svg" => Some(include_bytes!("../assets/design/9f0a1.svg")),
            "design/a5845.svg" => Some(include_bytes!("../assets/design/a5845.svg")),
            "design/b089c.svg" => Some(include_bytes!("../assets/design/b089c.svg")),
            "design/037c3.svg" => Some(include_bytes!("../assets/design/037c3.svg")),
            "design/96321.svg" => Some(include_bytes!("../assets/design/96321.svg")),
            "design/cce02.svg" => Some(include_bytes!("../assets/design/cce02.svg")),
            "design/6f03e.svg" => Some(include_bytes!("../assets/design/6f03e.svg")),
            "design/a80f5.svg" => Some(include_bytes!("../assets/design/a80f5.svg")),
            "design/07adb.svg" => Some(include_bytes!("../assets/design/07adb.svg")),
            "design/1d52b.svg" => Some(include_bytes!("../assets/design/1d52b.svg")),
            "design/c3937.svg" => Some(include_bytes!("../assets/design/c3937.svg")),
            "design/4ebda.svg" => Some(include_bytes!("../assets/design/4ebda.svg")),
            "design/d897f.svg" => Some(include_bytes!("../assets/design/d897f.svg")),
            "design/168cd.svg" => Some(include_bytes!("../assets/design/168cd.svg")),
            "design/07564.svg" => Some(include_bytes!("../assets/design/07564.svg")),
            "design/24921.svg" => Some(include_bytes!("../assets/design/24921.svg")),
            "design/65933.svg" => Some(include_bytes!("../assets/design/65933.svg")),
            "design/3988a.svg" => Some(include_bytes!("../assets/design/3988a.svg")),
            "design/e97f0.svg" => Some(include_bytes!("../assets/design/e97f0.svg")),
            "design/02660.svg" => Some(include_bytes!("../assets/design/02660.svg")),
            "design/9e80b.svg" => Some(include_bytes!("../assets/design/9e80b.svg")),
            "design/e76a5.svg" => Some(include_bytes!("../assets/design/e76a5.svg")),
            "design/26b2c.svg" => Some(include_bytes!("../assets/design/26b2c.svg")),
            "design/5f0d3.svg" => Some(include_bytes!("../assets/design/5f0d3.svg")),
            _ => None,
        };
        if let Some(bytes) = bytes {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        gpui_kit::assets::Assets.load(path)
    }
    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::Assets.list(path)?;
        paths.extend(
            DESIGN_PATHS
                .iter()
                .filter(|name| name.starts_with(path))
                .map(|name| SharedString::from(*name)),
        );
        paths.extend(
            service_logos::paths()
                .filter(|name| name.starts_with(path))
                .map(SharedString::from),
        );
        Ok(paths)
    }
}
const DESIGN_PATHS: &[&str] = &[
    "design/add-file.svg",
    "design/add-file-hover.svg",
    "design/add-variable.svg",
    "design/comment.svg",
    "design/ba30e.svg",
    "design/dots.svg",
    "design/quote-unquoted.svg",
    "design/quote-double.svg",
    "design/quote-single.svg",
    "design/17ad8.svg",
    "design/ufo.svg",
    "design/24e04.svg",
    "design/a9b45.svg",
    "design/91bbf.svg",
    "design/e78ce.svg",
    "design/1619b.svg",
    "design/2e0b8.svg",
    "design/9f0a1.svg",
    "design/a5845.svg",
    "design/b089c.svg",
    "design/037c3.svg",
    "design/96321.svg",
    "design/cce02.svg",
    "design/6f03e.svg",
    "design/a80f5.svg",
    "design/07adb.svg",
    "design/1d52b.svg",
    "design/c3937.svg",
    "design/4ebda.svg",
    "design/d897f.svg",
    "design/168cd.svg",
    "design/07564.svg",
    "design/24921.svg",
    "design/65933.svg",
    "design/3988a.svg",
    "design/e97f0.svg",
    "design/02660.svg",
    "design/9e80b.svg",
    "design/e76a5.svg",
    "design/26b2c.svg",
    "design/5f0d3.svg",
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filename_palette_does_not_confuse_unrelated_names_or_override_production_local() {
        assert_eq!(
            Environment::from_path(Path::new(".env")),
            Environment::Neutral
        );
        assert_eq!(
            Environment::from_path(Path::new(".env.local")),
            Environment::Local
        );
        assert_eq!(
            Environment::from_path(Path::new("app.staging.env")),
            Environment::Staging
        );
        assert_eq!(
            Environment::from_path(Path::new(".env.production.local")),
            Environment::Production
        );
        assert_eq!(
            Environment::from_path(Path::new(".env.productionish")),
            Environment::Neutral
        );
    }
    #[test]
    fn variable_icons_use_detected_service_or_environment_generic_fallback() {
        assert_eq!(
            variable_icon("DATABASE_URL", "", Environment::Local),
            Asset::GenericLocal
        );
        assert_eq!(
            variable_icon("BETTER_AUTH_SECRET", "", Environment::Production),
            Asset::Service(Service::BetterAuth)
        );
        assert_eq!(
            variable_icon("RESEND_API_KEY", "", Environment::Neutral),
            Asset::Service(Service::Resend)
        );
        assert_eq!(
            variable_icon("EMAIL_FROM", "", Environment::Production),
            Asset::GenericProduction
        );
        assert_eq!(
            variable_icon("UNKNOWN", "", Environment::Local),
            Asset::GenericLocal
        );
        assert_eq!(
            file_icon(Path::new(".env.local"), false),
            Asset::FileLocalInactive
        );
        assert_eq!(file_icon(Path::new(".env.local"), true), Asset::FileLocal);
        for service in Service::ALL {
            let asset = Asset::Service(*service);
            assert_eq!(asset.dimensions(), (16., 16.));
            assert!(Assets.load(asset.path()).unwrap().is_some());
        }
        assert_eq!(
            Assets.list("services/").unwrap().len(),
            service_logos::paths().count()
        );
    }
    #[test]
    fn quote_icons_keep_figma_root_geometry_and_render_visible_pixels() {
        for (style, expected_size) in [
            (QuoteStyle::Unquoted, (12., 12.)),
            (QuoteStyle::Double, (12.0001, 9.00048)),
            (QuoteStyle::Single, (5.25, 9.00048)),
        ] {
            let icon = quote_icon(style);
            assert_eq!(icon.dimensions(), expected_size);
            let bytes = Assets.load(icon.path()).unwrap().unwrap();
            let tree =
                resvg::usvg::Tree::from_data(&bytes, &resvg::usvg::Options::default()).unwrap();
            assert_eq!((tree.size().width(), tree.size().height()), expected_size);
            let mut pixels = resvg::tiny_skia::Pixmap::new(26, 26).unwrap();
            resvg::render(
                &tree,
                resvg::tiny_skia::Transform::identity(),
                &mut pixels.as_mut(),
            );
            assert!(pixels.pixels().iter().any(|pixel| pixel.alpha() > 0));
        }
    }

    #[test]
    fn original_design_assets_are_embedded_nonempty_and_exclude_native_window_controls() {
        let source = Assets;
        let listed = source.list("design/").unwrap();
        assert_eq!(listed.len(), 41);
        for path in DESIGN_PATHS {
            let bytes = source.load(path).unwrap().unwrap();
            assert!(bytes.len() > 100);
            assert!(
                std::str::from_utf8(&bytes)
                    .unwrap()
                    .trim_start()
                    .starts_with("<svg")
            );
        }
        assert!(!listed.iter().any(|name| name.contains("bbb88")));
        assert_eq!(Asset::Chevron.dimensions(), (7.99989, 4.42848));
    }
}
