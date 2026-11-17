use super::*;
use crate::i18n::tr;

const WELCOME_WIDTH: f32 = 249.;
const FOCUS_GUTTER: f32 = 4.;

fn welcome_layout() -> Div {
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .items_center()
        .justify_center()
        // Compensate for the title bar to center in the entire window.
        .pb(px(design::TITLE_HEIGHT))
}

fn welcome_button(id: impl Into<ElementId>, label: String) -> Button {
    Button::new(id)
        .ghost()
        .h(px(27.))
        .px(px(8.))
        .py(px(4.))
        .border_0()
        .rounded(px(8.))
        .bg(rgb(WINDOW_BG))
        .child(
            div()
                .flex()
                .w_full()
                .min_w_0()
                .items_center()
                .gap(px(10.))
                .child(design::asset(Asset::WelcomeFolder))
                .child(
                    Label::new(label)
                        .truncate()
                        .text_size(px(16.))
                        .line_height(px(19.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(design::TAB_TEXT_ACTIVE)),
                ),
        )
}

fn shortcut_key() -> Div {
    div()
        .flex()
        .items_center()
        .justify_center()
        .size(px(24.))
        .flex_shrink_0()
        .rounded(px(4.))
        .bg(rgb(CONTENT_BG))
}

impl AppWindow {
    pub(super) fn welcome_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let recent = self.projects.iter().enumerate().map(|(index, project)| {
            let name = project
                .root
                .file_name()
                .unwrap_or(project.root.as_os_str())
                .to_string_lossy()
                .into_owned();
            welcome_button(("recent-project", index), name)
                .max_w_full()
                .flex_shrink_0()
                .disabled(self.opening)
                .accessibility_label(tr!("project.open_path", path = project.root.display()))
                .tooltip(project.root.display().to_string())
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.select_project(index, window, cx);
                }))
        });
        welcome_layout()
            .child(
                div()
                    .w(px(WELCOME_WIDTH))
                    .flex()
                    .flex_col()
                    .gap(px(20.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(15.))
                            .child(
                                welcome_button("welcome-open", tr!("file.open_or_folder").into())
                                    .flex_shrink_0()
                                    .disabled(self.opening)
                                    .accessibility_label(tr!("file.open_or_folder"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.choose(false, window, cx)
                                    })),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(shortcut_key().child(design::asset(Asset::Command)))
                                    .child(
                                        shortcut_key().child(
                                            Label::new("O")
                                                .text_size(px(14.))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(rgb(SECONDARY)),
                                        ),
                                    ),
                            ),
                    )
                    .when(!self.projects.is_empty(), |this| {
                        this.child(
                            div()
                                .w_full()
                                .flex()
                                .flex_col()
                                .gap(px(9.))
                                .child(
                                    div().pl(px(7.)).child(
                                        Label::new(tr!("project.recent"))
                                            .text_size(px(14.))
                                            .line_height(px(17.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(rgb(SECONDARY)),
                                    ),
                                )
                                .child(
                                    div()
                                        .id("recent-projects")
                                        // Leave room for the native focus ring outside
                                        // each button without moving its visible content.
                                        .w(px(WELCOME_WIDTH + 2. * FOCUS_GUTTER))
                                        .max_h(px(280. + 2. * FOCUS_GUTTER))
                                        .m(px(-FOCUS_GUTTER))
                                        .p(px(FOCUS_GUTTER))
                                        .overflow_y_scroll()
                                        .flex()
                                        .flex_col()
                                        .items_start()
                                        .gap(px(5.))
                                        .children(recent),
                                ),
                        )
                    }),
            )
            .into_any_element()
    }

    pub(super) fn missing_project_view(&self, project: &Project, cx: &mut Context<Self>) -> Div {
        let name = project
            .root
            .file_name()
            .unwrap_or(project.root.as_os_str())
            .to_string_lossy();
        let path = project.root.display().to_string();
        let message = tr!("project.missing_detail", name = name);
        welcome_layout().child(
            div()
                .w(px(384.))
                .flex()
                .flex_col()
                .items_start()
                .gap(px(20.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(60.))
                        .rounded(px(16.))
                        .bg(rgba(0xffb86a14))
                        .child(
                            svg()
                                .path(Asset::Ufo.path())
                                .size(px(36.))
                                .text_color(rgb(KEY_COLOR)),
                        ),
                )
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(
                            Label::new(tr!("project.missing_title"))
                                .text_size(px(24.))
                                .line_height(px(29.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(design::VALUE_COLOR)),
                        )
                        .child(
                            Label::new(message)
                                .text_size(px(15.))
                                .line_height(px(21.))
                                .text_color(rgb(design::TAB_TEXT_ACTIVE)),
                        )
                        .child(
                            div()
                                .id("missing-project-path")
                                .w_full()
                                .min_w_0()
                                .rounded(px(6.))
                                .bg(rgb(CONTENT_BG))
                                .px(px(8.))
                                .py(px(5.))
                                .tooltip({
                                    let path = path.clone();
                                    move |window, cx| Tooltip::new(path.clone()).build(window, cx)
                                })
                                .child(
                                    Label::new(path)
                                        .truncate()
                                        .font_family(design::MONO_FONT)
                                        .text_size(px(12.))
                                        .text_color(rgb(design::TAB_TEXT_ACTIVE)),
                                ),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_start()
                        .gap(px(5.))
                        .child(self.locate_project_button(cx))
                        .child(
                            Button::new("missing-project-open")
                                .ghost()
                                .h(px(27.))
                                .px(px(8.))
                                .border_0()
                                .rounded(px(8.))
                                .disabled(self.opening)
                                .accessibility_label(tr!("project.open_another"))
                                .child(
                                    Label::new(tr!("project.open_another"))
                                        .text_size(px(14.))
                                        .text_color(rgb(design::TAB_TEXT_ACTIVE)),
                                )
                                .on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.choose(true, window, cx)
                                    }),
                                ),
                        ),
                ),
        )
    }

    pub(super) fn missing_project_notice(&self, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .px_6()
            .py_2()
            .child(
                svg()
                    .path(Asset::Ufo.path())
                    .size(px(18.))
                    .text_color(rgb(KEY_COLOR)),
            )
            .child(
                Label::new(tr!("project.missing_banner"))
                    .text_size(px(14.))
                    .text_color(rgb(design::TAB_TEXT_ACTIVE)),
            )
            .child(self.locate_project_button(cx))
    }

    fn locate_project_button(&self, cx: &mut Context<Self>) -> Button {
        welcome_button(
            "missing-project-locate",
            tr!("project.locate_folder").into(),
        )
        .disabled(self.opening)
        .accessibility_label(tr!("project.locate_folder"))
        .on_click(cx.listener(|this, _, window, cx| this.locate_project(window, cx)))
    }
}
