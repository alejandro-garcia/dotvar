#[cfg(any(feature = "native", test))]
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(any(feature = "native", test))]
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum LanguagePreference {
    System,
    #[default]
    #[serde(rename = "en")]
    English,
    #[serde(rename = "es")]
    Spanish,
}

macro_rules! tr {
    ($key:literal $(, $name:ident = $value:expr)* $(,)?) => {
        rust_i18n::t!($key, locale = $crate::i18n::locale() $(, $name = $value)*)
    };
}
pub(crate) use tr;

/// How keyboard shortcuts are written for display.
#[cfg(any(feature = "native", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShortcutStyle {
    /// Modifier glyphs, as in `⌘⇧C`.
    Mac,
    /// Written-out modifiers, as in `Ctrl+Shift+C`.
    Other,
}

#[cfg(any(feature = "native", test))]
impl ShortcutStyle {
    #[cfg(feature = "native")]
    pub(crate) const CURRENT: Self = if cfg!(target_os = "macos") {
        Self::Mac
    } else {
        Self::Other
    };
}

/// Text of the primary-modifier shortcut for `key`, e.g. `⌘W` or `Ctrl+W`.
#[cfg(any(feature = "native", test))]
pub(crate) fn shortcut_label(style: ShortcutStyle, shift: bool, key: &str) -> String {
    match (style, shift) {
        (ShortcutStyle::Mac, false) => format!("⌘{key}"),
        (ShortcutStyle::Mac, true) => format!("⌘⇧{key}"),
        (ShortcutStyle::Other, false) => format!("Ctrl+{key}"),
        (ShortcutStyle::Other, true) => format!("Ctrl+Shift+{key}"),
    }
}

/// The shortcut text for the platform this build runs on.
#[cfg(feature = "native")]
pub(crate) fn shortcut(shift: bool, key: &str) -> String {
    shortcut_label(ShortcutStyle::CURRENT, shift, key)
}

#[cfg(feature = "native")]
pub(crate) fn text(key: &'static str) -> std::borrow::Cow<'static, str> {
    rust_i18n::t!(key, locale = locale())
}

#[cfg(any(feature = "native", test))]
fn select_locale(preferred: Option<&str>) -> &'static str {
    let language = preferred
        .unwrap_or_default()
        .split(['-', '_', '.', '@'])
        .next()
        .unwrap_or_default();
    if language.eq_ignore_ascii_case("es") {
        "es"
    } else {
        "en"
    }
}

#[cfg(feature = "native")]
impl LanguagePreference {
    fn locale(self) -> &'static str {
        match self {
            Self::System => select_locale(sys_locale::get_locale().as_deref()),
            Self::English => "en",
            Self::Spanish => "es",
        }
    }
}

static SPANISH: AtomicBool = AtomicBool::new(false);

pub(crate) fn locale() -> &'static str {
    if SPANISH.load(Ordering::Relaxed) {
        "es"
    } else {
        "en"
    }
}

#[cfg(feature = "native")]
pub(crate) fn set_language(preference: LanguagePreference) {
    SPANISH.store(preference.locale() == "es", Ordering::Relaxed);
    init_component_locale();
}

#[cfg(feature = "native")]
fn init_component_locale() {
    use gpui_kit::component;
    use std::sync::Once;

    static EXTEND: Once = Once::new();
    EXTEND.call_once(|| rust_i18n::extend!(component));
    component::set_locale(locale());
}

#[cfg(feature = "native")]
pub fn usage() -> std::borrow::Cow<'static, str> {
    let language = crate::config::ConfigStore::platform_default()
        .and_then(|store| store.load_language())
        .unwrap_or_default();
    rust_i18n::t!("app.usage", locale = language.locale())
}

#[cfg(any(feature = "native", test))]
pub(crate) fn diagnostic_message(
    diagnostic: env_core::Diagnostic,
) -> std::borrow::Cow<'static, str> {
    use env_core::Diagnostic::*;
    match diagnostic {
        UnsupportedEncoding => tr!("diagnostic.unsupported_encoding"),
        UnsupportedSyntax => tr!("diagnostic.unsupported_syntax"),
        AmbiguousComment => tr!("diagnostic.ambiguous_comment"),
        UnclosedQuote => tr!("diagnostic.unclosed_quote"),
    }
}

pub(crate) fn edit_error_message(error: &env_core::EditError) -> std::borrow::Cow<'static, str> {
    use env_core::EditError::*;
    match error {
        ReadOnly => tr!("edit_error.read_only"),
        StaleRevision { .. } => tr!("edit_error.stale_revision"),
        MissingEntry(_) => tr!("edit_error.missing_entry"),
        InvalidName => tr!("edit_error.invalid_name"),
        InvalidValue => tr!("edit_error.invalid_value"),
        InvalidComment => tr!("edit_error.invalid_comment"),
        InvalidFragment => tr!("edit_error.invalid_fragment"),
        UnsafeBoundary => tr!("edit_error.unsafe_boundary"),
        DuplicateName { .. } => tr!("edit_error.duplicate_name"),
        DuplicateFragmentName => tr!("edit_error.duplicate_fragment_name"),
        HistoryLimit => tr!("edit_error.history_limit"),
        RevisionExhausted => tr!("edit_error.revision_exhausted"),
        IdentityExhausted => tr!("edit_error.identity_exhausted"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::collections::BTreeMap;

    #[test]
    fn system_language_uses_spanish_variants_and_defaults_to_english() {
        for tag in ["es", "es-AR", "es-419", "ES_es", "es_ES.UTF-8"] {
            assert_eq!(select_locale(Some(tag)), "es");
        }
        for tag in ["en", "en-GB", "fr-CA", "C.UTF-8", "est", ""] {
            assert_eq!(select_locale(Some(tag)), "en");
        }
        assert_eq!(select_locale(None), "en");
    }

    fn messages(prefix: &str, value: &Value, entries: &mut BTreeMap<String, Vec<String>>) {
        if let Some(group) = value.as_object() {
            for (key, value) in group {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                messages(&path, value, entries);
            }
        } else {
            let text = value.as_str().expect("catalog entries must be strings");
            assert!(!text.trim().is_empty(), "empty translation for {prefix}");
            let mut placeholders: Vec<_> = text
                .split("%{")
                .skip(1)
                .map(|part| {
                    part.split_once('}')
                        .expect("unclosed placeholder")
                        .0
                        .to_owned()
                })
                .collect();
            placeholders.sort_unstable();
            entries.insert(prefix.to_owned(), placeholders);
        }
    }

    #[test]
    fn dictionaries_cover_the_same_messages_and_placeholders() {
        let load = |json| {
            let mut entries = BTreeMap::new();
            messages("", &serde_json::from_str(json).unwrap(), &mut entries);
            entries
        };
        assert_eq!(
            load(include_str!("../locales/en.json")),
            load(include_str!("../locales/es.json"))
        );
    }

    #[test]
    fn translations_render_parameters_and_document_diagnostics() {
        let name = "MY_%{variable}";
        assert_eq!(
            rust_i18n::t!("variable.value", locale = "en", name = name),
            "Value of MY_%{variable}"
        );
        assert_eq!(
            rust_i18n::t!("variable.value", locale = "es", name = name),
            "Valor de MY_%{variable}"
        );
        assert_eq!(
            diagnostic_message(env_core::Diagnostic::UnclosedQuote),
            rust_i18n::t!("diagnostic.unclosed_quote", locale = locale())
        );
    }

    #[cfg(feature = "native")]
    #[test]
    fn gpui_component_receives_the_translated_context_menu_entries() {
        init_component_locale();
        assert_eq!(
            gpui_kit::component::_rust_i18n_translate("es", "Input.Cut"),
            "Cortar"
        );
        assert_eq!(
            gpui_kit::component::_rust_i18n_translate("es", "Input.Select All"),
            "Seleccionar todo"
        );
    }
}

#[cfg(test)]
mod shortcut_tests {
    use super::*;

    #[test]
    fn shortcuts_render_glyphs_on_macos_and_words_elsewhere() {
        assert_eq!(shortcut_label(ShortcutStyle::Mac, false, "W"), "⌘W");
        assert_eq!(shortcut_label(ShortcutStyle::Mac, true, "C"), "⌘⇧C");
        assert_eq!(shortcut_label(ShortcutStyle::Other, false, "W"), "Ctrl+W");
        assert_eq!(
            shortcut_label(ShortcutStyle::Other, true, "C"),
            "Ctrl+Shift+C"
        );
    }

    #[test]
    fn tooltips_take_their_shortcut_from_a_placeholder_instead_of_a_hardcoded_glyph() {
        for json in [
            include_str!("../locales/en.json"),
            include_str!("../locales/es.json"),
        ] {
            assert!(!json.contains(['⌘', '⇧', '⌥', '⌃']));
        }
        let rendered = |key: &str, locale: &str| {
            rust_i18n::t!(key, locale = locale, shortcut = "Ctrl+Shift+C").into_owned()
        };
        assert_eq!(
            rendered("tab.close_shortcut", "en"),
            "Close Tab · Ctrl+Shift+C"
        );
        assert_eq!(
            rendered("tab.close_shortcut", "es"),
            "Cerrar pestaña · Ctrl+Shift+C"
        );
        assert_eq!(
            rendered("editor.copy_document_shortcut", "en"),
            "Copy Document · Ctrl+Shift+C"
        );
        assert_eq!(
            rendered("editor.copy_document_shortcut", "es"),
            "Copiar documento · Ctrl+Shift+C"
        );
    }
}
