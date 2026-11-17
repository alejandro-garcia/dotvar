use crate::{
    app::{BUNDLE_ID, CloseWindow, OpenSettings, set_menus},
    config::ConfigStore,
    design::{CONTENT_BG, SELECTOR_BG, TAB_TEXT_ACTIVE, UI_FONT, VALUE_COLOR, WINDOW_BG},
    i18n::{self, LanguagePreference, tr},
    session::QuoteStyle,
    ui::AppWindow,
};
use gpui_kit::{
    base::{Disableable, Selectable},
    component::{
        button::{Button, ButtonVariants},
        label::Label,
        menu::{DropdownMenu, PopupMenuItem},
    },
    prelude::FluentBuilder,
    *,
};
use std::{cell::Cell, rc::Rc};

const SIDEBAR_BG: u32 = 0x24201e;
const DIVIDER: u32 = 0x393432;

pub(crate) struct AppSettings {
    store: Option<ConfigStore>,
    language: LanguagePreference,
    saving_language: bool,
    language_error: Option<&'static str>,
    quote_style: QuoteStyle,
    saving_quote_style: bool,
    quote_style_error: Option<&'static str>,
}

struct LanguageChanged;
impl EventEmitter<LanguageChanged> for AppSettings {}

impl AppSettings {
    pub(crate) fn load() -> Self {
        let store = ConfigStore::platform_default();
        let store_ref = store.as_ref().map_err(|error| *error);
        let language = store_ref.and_then(ConfigStore::load_language);
        let quote_style = store_ref.and_then(ConfigStore::load_quote_style);
        i18n::set_language(language.unwrap_or_default());
        Self {
            store: store.ok(),
            language: language.unwrap_or_default(),
            saving_language: false,
            language_error: language.err().map(|_| "settings.language_load_failed"),
            quote_style: quote_style.unwrap_or_default(),
            saving_quote_style: false,
            quote_style_error: quote_style
                .err()
                .map(|_| "settings.quote_style_load_failed"),
        }
    }

    pub(crate) fn default_quote_style(&self) -> QuoteStyle {
        self.quote_style
    }

    fn select_language(&mut self, language: LanguagePreference, cx: &mut Context<Self>) {
        if self.saving_language || (self.language == language && self.language_error.is_none()) {
            return;
        }
        let Some(store) = self.store.clone() else {
            self.language_error = Some("settings.language_save_failed");
            cx.notify();
            return;
        };
        self.saving_language = true;
        self.language_error = None;
        cx.notify();
        let save = cx
            .background_executor()
            .spawn(async move { store.save_language(language) });
        cx.spawn(async move |settings, cx| {
            let result = save.await;
            let _ = settings.update(cx, |settings, cx| {
                settings.saving_language = false;
                if result.is_ok() {
                    settings.language = language;
                    i18n::set_language(language);
                    set_menus(cx);
                    cx.refresh_windows();
                    cx.emit(LanguageChanged);
                } else {
                    settings.language_error = Some("settings.language_save_failed");
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn select_quote_style(&mut self, style: QuoteStyle, cx: &mut Context<Self>) {
        if self.saving_quote_style
            || (self.quote_style == style && self.quote_style_error.is_none())
        {
            return;
        }
        let Some(store) = self.store.clone() else {
            self.quote_style_error = Some("settings.quote_style_save_failed");
            cx.notify();
            return;
        };
        self.saving_quote_style = true;
        self.quote_style_error = None;
        cx.notify();
        let save = cx
            .background_executor()
            .spawn(async move { store.save_quote_style(style) });
        cx.spawn(async move |settings, cx| {
            let result = save.await;
            let _ = settings.update(cx, |settings, cx| {
                settings.saving_quote_style = false;
                if result.is_ok() {
                    settings.quote_style = style;
                } else {
                    settings.quote_style_error = Some("settings.quote_style_save_failed");
                }
                cx.notify();
            });
        })
        .detach();
    }
}

pub(crate) fn register(
    editor: AnyWindowHandle,
    view: &Entity<AppWindow>,
    preferences: Entity<AppSettings>,
    cx: &mut App,
) {
    let _ = editor.update(cx, |_, window, cx| {
        view.update(cx, |_, cx| {
            cx.subscribe_in(
                &preferences,
                window,
                |view, _, _: &LanguageChanged, window, cx| {
                    view.refresh_language(window, cx);
                },
            )
            .detach();
        });
    });
    let settings = Rc::new(Cell::new(None::<AnyWindowHandle>));
    let open_settings = settings.clone();
    cx.on_action(move |_: &OpenSettings, cx| {
        if let Some(handle) = open_settings.get()
            && (cx.active_window() == Some(handle)
                || handle
                    .update(cx, |_, window, _| window.activate_window())
                    .is_ok())
        {
            return;
        }
        open_settings.set(open(&preferences, cx));
    });
    cx.on_window_closed(move |cx, closed| {
        if closed == editor.window_id()
            && let Some(handle) = settings.take()
        {
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, _| window.remove_window());
            });
        }
    })
    .detach();
}

fn open(preferences: &Entity<AppSettings>, cx: &mut App) -> Option<AnyWindowHandle> {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(780.), px(532.)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some(tr!("settings.title").into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(9.), px(9.))),
        }),
        app_id: Some(BUNDLE_ID.into()),
        is_minimizable: false,
        is_resizable: false,
        ..WindowOptions::default()
    };
    match open_window(options, cx, |window, cx| {
        cx.new(|cx| {
            let focus = cx.focus_handle();
            focus.focus(window, cx);
            cx.observe(preferences, |_, _, cx| cx.notify()).detach();
            cx.subscribe_in(
                preferences,
                window,
                |_, _, _: &LanguageChanged, window, _| {
                    window.set_window_title(&tr!("settings.title"));
                },
            )
            .detach();
            SettingsWindow {
                page: Page::General,
                focus,
                preferences: preferences.clone(),
            }
        })
    }) {
        Ok((handle, _)) => {
            let _ = handle.update(cx, |_, window, _| window.activate_window());
            Some(handle)
        }
        Err(error) => {
            eprintln!("{}", tr!("settings.window_failed", error = error));
            None
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    General,
    About,
}

impl Page {
    fn title(self) -> SharedString {
        match self {
            Self::General => tr!("settings.general").into(),
            Self::About => tr!("settings.about").into(),
        }
    }

    fn icon(self) -> Div {
        let (path, color) = match self {
            Self::General => ("icons/settings.svg", 0x69625e),
            Self::About => ("icons/info.svg", 0x0969c7),
        };
        div()
            .size(px(24.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.))
            .bg(rgb(color))
            .child(svg().path(path).size(px(15.)).text_color(white()))
    }
}

struct SettingsWindow {
    page: Page,
    focus: FocusHandle,
    preferences: Entity<AppSettings>,
}

impl SettingsWindow {
    fn sidebar(&self, cx: &mut Context<Self>) -> Div {
        div()
            .w(px(200.))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .bg(rgb(SIDEBAR_BG))
            .border_r_1()
            .border_color(rgb(DIVIDER))
            .pt(px(54.))
            .px(px(10.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(8.))
                    .mb(px(24.))
                    .child(app_icon(32., 8.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                Label::new("Dotvar")
                                    .line_height(px(16.))
                                    .font_weight(FontWeight::SEMIBOLD),
                            )
                            .child(
                                Label::new(tr!("settings.label"))
                                    .text_size(px(11.))
                                    .line_height(px(13.))
                                    .text_color(rgb(TAB_TEXT_ACTIVE)),
                            ),
                    ),
            )
            .child(self.page_button(Page::General, cx))
            .child(div().mx(px(8.)).my(px(12.)).h(px(1.)).bg(rgb(DIVIDER)))
            .child(self.page_button(Page::About, cx))
    }

    fn page_button(&self, page: Page, cx: &mut Context<Self>) -> Button {
        let selected = self.page == page;
        Button::new(("settings-page", page as usize))
            .ghost()
            .selected(selected)
            .when(selected, |button| button.bg(rgb(SELECTOR_BG)))
            .border_0()
            .rounded(px(8.))
            .h(px(38.))
            .w_full()
            .px(px(10.))
            .accessibility_label(page.title())
            .child(
                div()
                    .flex()
                    .w_full()
                    .items_center()
                    .gap(px(10.))
                    .child(page.icon())
                    .child(
                        Label::new(page.title())
                            .text_size(px(13.))
                            .text_color(rgb(VALUE_COLOR))
                            .when(selected, |label| label.font_weight(FontWeight::MEDIUM)),
                    ),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.page = page;
                cx.notify();
            }))
    }

    fn general(&self, cx: &Context<Self>) -> Div {
        let settings = self.preferences.read(cx);
        let language = settings.language;
        let selected_language = language_label(language);
        let preferences = self.preferences.clone();
        let language_selector = settings_selector(
            "settings-language",
            selected_language.clone(),
            tr!(
                "settings.language_accessibility",
                language = selected_language
            )
            .into(),
            settings.saving_language,
        )
        .dropdown_menu_with_anchor(Anchor::TopRight, move |mut menu, _, _| {
            for option in [
                LanguagePreference::System,
                LanguagePreference::English,
                LanguagePreference::Spanish,
            ] {
                let preferences = preferences.clone();
                menu = menu.item(
                    PopupMenuItem::new(language_label(option))
                        .checked(option == language)
                        .on_click(move |_, _, cx| {
                            preferences.update(cx, |settings, cx| {
                                settings.select_language(option, cx);
                            });
                        }),
                );
            }
            menu
        });

        let quote_style = settings.default_quote_style();
        let quote_label = quote_style.label();
        let preferences = self.preferences.clone();
        let quote_selector = settings_selector(
            "settings-quote-style",
            quote_label.clone().into(),
            tr!("settings.quote_style_accessibility", style = quote_label).into(),
            settings.saving_quote_style,
        )
        .dropdown_menu_with_anchor(Anchor::TopRight, move |mut menu, _, _| {
            for option in [QuoteStyle::Double, QuoteStyle::Single, QuoteStyle::Unquoted] {
                let preferences = preferences.clone();
                menu = menu.item(
                    PopupMenuItem::new(option.label())
                        .checked(option == quote_style)
                        .on_click(move |_, _, cx| {
                            preferences.update(cx, |settings, cx| {
                                settings.select_quote_style(option, cx);
                            });
                        }),
                );
            }
            menu
        });

        div()
            .flex()
            .flex_col()
            .gap(px(22.))
            .child(settings_section(
                "settings-language-heading",
                tr!("settings.language_section").into(),
                tr!("settings.language").into(),
                language_selector,
                settings.language_error,
            ))
            .child(settings_section(
                "settings-editor-heading",
                tr!("settings.editor_section").into(),
                tr!("settings.quote_style").into(),
                quote_selector,
                settings.quote_style_error,
            ))
    }

    fn about(&self) -> Div {
        div()
            .flex()
            .items_center()
            .gap(px(16.))
            .child(app_icon(64., 14.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        Label::new("Dotvar")
                            .text_size(px(18.))
                            .font_weight(FontWeight::SEMIBOLD),
                    )
                    .child(
                        Label::new(tr!("settings.version", version = env!("CARGO_PKG_VERSION")))
                            .text_color(rgb(TAB_TEXT_ACTIVE)),
                    )
                    .child(
                        Label::new(tr!("settings.description")).text_color(rgb(TAB_TEXT_ACTIVE)),
                    ),
            )
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("settings")
            .size_full()
            .flex()
            .bg(rgb(WINDOW_BG))
            .font_family(UI_FONT)
            .text_size(px(13.))
            .text_color(rgb(VALUE_COLOR))
            .track_focus(&self.focus)
            .key_context("Settings")
            .on_action(|_: &CloseWindow, window, _| window.remove_window())
            .child(self.sidebar(cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .pt(px(26.))
                    .child(
                        div()
                            .id("settings-heading")
                            .px(px(28.))
                            .pb(px(22.))
                            .text_size(px(20.))
                            .line_height(px(24.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .role(Role::Heading)
                            .aria_level(1)
                            .aria_label(self.page.title())
                            .child(self.page.title()),
                    )
                    .child(
                        div()
                            .id("settings-content")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .px(px(28.))
                            .pb(px(28.))
                            .child(div().max_w(px(580.)).child(match self.page {
                                Page::General => self.general(cx),
                                Page::About => self.about(),
                            })),
                    ),
            )
    }
}

fn settings_selector(
    id: &'static str,
    selected: SharedString,
    accessibility_label: SharedString,
    saving: bool,
) -> Button {
    Button::new(id)
        .ghost()
        .border_0()
        .rounded(px(6.))
        .h(px(28.))
        .w(px(146.))
        .px(px(9.))
        .bg(rgb(SELECTOR_BG))
        .disabled(saving)
        .accessibility_label(accessibility_label)
        .child(
            div()
                .flex()
                .w_full()
                .items_center()
                .justify_between()
                .child(Label::new(selected).text_size(px(13.)))
                .child(
                    svg()
                        .path("icons/chevron-down.svg")
                        .size(px(13.))
                        .text_color(rgb(TAB_TEXT_ACTIVE)),
                ),
        )
}

fn settings_section(
    id: &'static str,
    heading: SharedString,
    label: SharedString,
    selector: impl IntoElement,
    error: Option<&'static str>,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(10.))
        .child(
            div()
                .id(id)
                .role(Role::Heading)
                .aria_level(2)
                .font_weight(FontWeight::MEDIUM)
                .child(Label::new(heading)),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .rounded(px(12.))
                .bg(rgb(CONTENT_BG))
                .px(px(14.))
                .py(px(10.))
                .child(Label::new(label))
                .child(selector),
        )
        .when_some(error, |this, key| {
            this.child(Label::new(i18n::text(key)).text_color(rgb(0xfda4af)))
        })
}

fn language_label(language: LanguagePreference) -> SharedString {
    match language {
        LanguagePreference::System => tr!("settings.language_system"),
        LanguagePreference::English => tr!("settings.language_english"),
        LanguagePreference::Spanish => tr!("settings.language_spanish"),
    }
    .into()
}

fn app_icon(size: f32, radius: f32) -> Img {
    img("design/dotvar.png")
        .size(px(size))
        .rounded(px(radius))
        .flex_shrink_0()
        .object_fit(ObjectFit::Contain)
}
