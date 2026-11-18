use super::*;
use crate::i18n::{diagnostic_message, edit_error_message, tr};
use crate::session::QuoteStyle;
use env_core::{BlockKind, TextField};
use gpui_kit::component::{
    Icon, IconName, InteractiveElementExt, Size, ThemeStyled,
    input::{Escape, Input, Textarea, TextareaState},
    scroll::{ScrollableMask, Scrollbar, ScrollbarMode},
};

fn value_line(text: &str, color: u32, window: &Window) -> ShapedLine {
    window.text_system().shape_line(
        text.to_owned().into(),
        px(12.),
        &[TextRun {
            len: text.len(),
            font: Font {
                family: design::MONO_FONT.into(),
                weight: FontWeight::NORMAL,
                ..Default::default()
            },
            color: rgb(color).into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }],
        None,
    )
}

enum QuoteScroll {
    Editor(Entity<TextareaState>),
    Preview(ScrollHandle),
}

/// Painted decoration has no text selection, input semantics or mouse hit area.
fn quote_decoration(
    style: QuoteStyle,
    content: Option<String>,
    scroll: QuoteScroll,
    color: u32,
) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, cx| {
            let Some(delimiter) = style.delimiter() else {
                return;
            };
            let offset = match &scroll {
                QuoteScroll::Editor(state) => state.read(cx).scroll_offset(),
                QuoteScroll::Preview(handle) => handle.offset(),
            };
            let quote = value_line(delimiter, color, window);
            let inset = quote.width + px(design::QUOTE_GAP);
            let (last_width, last_row) = match &content {
                Some(content) => {
                    let last = content
                        .split('\n')
                        .next_back()
                        .unwrap_or("")
                        .trim_end_matches('\r');
                    (
                        value_line(last, design::VALUE_COLOR, window).width,
                        content.split('\n').count() - 1,
                    )
                }
                // A hidden value keeps its fixed mask width, never its source length.
                None => (
                    bounds.size.width - px(design::FIELD_PADDING_X * 2.) - inset * 2.,
                    0,
                ),
            };
            let start = bounds.origin
                + point(
                    px(design::FIELD_PADDING_X) + offset.x,
                    px(design::FIELD_PADDING_Y) + offset.y,
                );
            let end = bounds.origin
                + point(
                    px(design::FIELD_PADDING_X)
                        + inset
                        + last_width
                        + px(design::QUOTE_GAP)
                        + offset.x,
                    px(design::FIELD_PADDING_Y + last_row as f32 * design::FIELD_LINE_HEIGHT)
                        + offset.y,
                );
            window.with_content_mask(Some(ContentMask { bounds }), |window| {
                let _ = quote.paint(
                    start,
                    px(design::FIELD_LINE_HEIGHT),
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                );
                let _ = quote.paint(
                    end,
                    px(design::FIELD_LINE_HEIGHT),
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                );
            });
        },
    )
    .absolute()
    .inset_0()
}

fn hidden_pattern() -> impl IntoElement {
    canvas(
        |_, _, _| (),
        |bounds, _, window, _| {
            // The native pattern is clipped to the quad's rounded corners.
            let scale = window.scale_factor();
            let stroke = std::f32::consts::SQRT_2 * scale;
            window.paint_quad(
                fill(
                    bounds,
                    pattern_slash(rgb(design::QUOTE_COLOR), stroke, 5. * scale - stroke),
                )
                .corner_radii(px(4.)),
            );
        },
    )
    .w_full()
    .h(px(16.))
}

fn key_column_width(window: &Window) -> Pixels {
    px(if window.viewport_size().width >= px(1100.) {
        design::KEY_WIDTH
    } else {
        220.
    })
}

fn quote_button(id: impl Into<ElementId>, quote_style: QuoteStyle, commented: bool) -> Button {
    let icon = design::quote_icon(quote_style);
    let (width, height) = icon.dimensions();
    Button::new(id)
        .ghost()
        .size(px(design::QUOTE_BUTTON_SIZE))
        .p_0()
        .border_0()
        .rounded(px(6.))
        .bg(rgb(design::SELECTOR_BG))
        .accessibility_label(tr!(
            "editor.quote_change_label",
            current = quote_style.label(),
            next = quote_style.next().label(),
        ))
        .tooltip(tr!(
            "editor.quote_change_hint",
            current = quote_style.label(),
            next = quote_style.next().label(),
        ))
        .child(if commented {
            svg()
                .path(icon.path())
                .w(px(width))
                .h(px(height))
                .text_color(rgb(SECONDARY))
                .into_any_element()
        } else {
            design::asset(icon).into_any_element()
        })
}

fn variable_menu_icon(svg: &[u8]) -> Icon {
    // PopupMenu applies its extra-small size. Match the project menu's 16px
    // icon and 8px gap while keeping the standard menu item's accessible label.
    Icon::empty()
        .data(svg)
        .text_color(rgb(SECONDARY))
        .min_w(px(16.))
        .min_h(px(16.))
        .mr(px(4.))
}

pub(super) fn delete_menu_item(label: impl Into<SharedString>) -> PopupMenuItem {
    PopupMenuItem::new(label.into()).icon(variable_menu_icon(include_bytes!(
        "../../assets/actions/variable-menu/trash-2.svg"
    )))
}

fn value_editor_frame(
    state: &Entity<TextareaState>,
    textarea: Textarea,
    quote_style: QuoteStyle,
    quote_button: Option<Button>,
    commented: bool,
    window: &Window,
    cx: &App,
) -> Div {
    let size = Size::Small;
    let value = state.read(cx).value();
    let lines = value.split('\n').count();
    let color = if commented {
        SECONDARY
    } else {
        design::VALUE_COLOR
    };
    let quote_color = if commented {
        SECONDARY
    } else {
        design::QUOTE_COLOR
    };
    let quote_inset = quote_style.delimiter().map_or(px(0.), |quote| {
        value_line(quote, quote_color, window).width + px(design::QUOTE_GAP)
    });
    design::variable_field(commented)
        .pl(px(design::FIELD_PADDING_X) - size.input_px() + quote_inset)
        .pr(px(design::FIELD_PADDING_X + design::QUOTE_BUTTON_SIZE) - size.input_px() + quote_inset)
        .py(px(design::FIELD_PADDING_Y) - size.input_py())
        .when(state.read(cx).focus_handle(cx).is_focused(window), |this| {
            this.focus_ring_style(window, cx)
        })
        .child(
            textarea
                .appearance(false)
                .bordered(false)
                .with_size(size)
                .h(px(lines as f32 * design::FIELD_LINE_HEIGHT) + size.input_py() * 2.)
                .text_size(px(12.))
                .text_color(rgb(color))
                .font_weight(FontWeight::NORMAL)
                .line_height(px(design::FIELD_LINE_HEIGHT)),
        )
        // The document controls vertical scroll when an input receives focus.
        .child(
            canvas(
                |_, window, _| {
                    window.take_autoscroll();
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_0(),
        )
        .child(quote_decoration(
            quote_style,
            Some(value.to_string()),
            QuoteScroll::Editor(state.clone()),
            quote_color,
        ))
        .when_some(quote_button, |this, button| {
            this.child(div().absolute().top(px(2.)).right(px(2.)).child(button))
        })
}

impl AppWindow {
    fn add_row(&self, tab_id: u64, cx: &mut Context<Self>) -> AnyElement {
        const DISPLAY_FONT: &str = "SF Pro Display";
        static HAS_DISPLAY_FONT: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        let has_display_font = *HAS_DISPLAY_FONT.get_or_init(|| {
            cx.text_system()
                .all_font_names()
                .iter()
                .any(|name| name == DISPLAY_FONT)
        });
        let label = tr!("variable.add");
        let options_label = tr!("editor.add_options");
        let comment_label = tr!("editor.add_comment");
        let owner = cx.weak_entity();
        let segment_height = px(design::FIELD_HEIGHT - 2.);
        let variable = Button::new("add-variable")
            .ghost()
            .h(segment_height)
            .border_0()
            .rounded_none()
            .rounded_l(px(5.))
            .text_color(rgb(design::TAB_TEXT_ACTIVE))
            .accessibility_label(label.clone())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(design::asset(Asset::AddVariable))
                    .child(
                        div()
                            .when(has_display_font, |this| this.font_family(DISPLAY_FONT))
                            .text_size(px(14.))
                            .line_height(px(17.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(label),
                    ),
            )
            .on_click(cx.listener(|this, _, window, cx| this.begin_new_variable(window, cx)));
        let options = Button::new("add-row-menu")
            .ghost()
            .w(segment_height)
            .h(segment_height)
            .p_0()
            .border_0()
            .rounded_none()
            .rounded_r(px(5.))
            .accessibility_label(options_label.clone())
            .tooltip(options_label)
            .child(
                Icon::new(IconName::ChevronDown)
                    .with_size(Size::Small)
                    .text_color(rgb(design::TAB_TEXT_ACTIVE)),
            )
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                let owner = owner.clone();
                menu.item(
                    PopupMenuItem::new(comment_label.clone())
                        .icon(variable_menu_icon(include_bytes!(
                            "../../assets/design/comment.svg"
                        )))
                        .on_click(move |_, window, cx| {
                            let _ = owner.update(cx, |this, cx| {
                                this.add_comment(tab_id, window, cx);
                            });
                        }),
                )
            });
        div()
            .flex()
            .items_start()
            .child(
                div()
                    .id("add-row")
                    .role(Role::Group)
                    .flex()
                    .items_center()
                    .bg(rgb(WINDOW_BG))
                    .border_1()
                    .border_color(rgb(design::SELECTOR_BG))
                    .rounded(px(6.))
                    .child(variable)
                    .child(
                        div()
                            .w(px(1.))
                            .h(segment_height)
                            .bg(rgb(design::SELECTOR_BG)),
                    )
                    .child(options),
            )
            .into_any_element()
    }

    fn new_variable_view(
        &self,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(tab) = self.tab(id) else {
            return div().into_any_element();
        };
        let Some(row) = &tab.new_variable else {
            return div().into_any_element();
        };
        let Some(draft) = tab.content.as_ref().and_then(DocumentSession::new_variable) else {
            return div().into_any_element();
        };
        let owner = cx.weak_entity();
        let discard = owner.clone();
        let actions = reorder::click_menu(
            format!("new-variable-menu-{id}"),
            Button::new(("new-variable-actions", id))
                .ghost()
                .w(px(16.))
                .h(px(16.))
                .p_0()
                .border_0()
                .accessibility_label(tr!("variable.new_actions"))
                .tooltip(tr!("variable.new_actions"))
                .child(design::asset(Asset::GenericNeutral)),
            move |menu, _, _| {
                let discard = discard.clone();
                menu.item(delete_menu_item(tr!("variable.delete")).on_click(
                    move |_, window, cx| {
                        let _ =
                            discard.update(cx, |this, cx| this.cancel_new_variable(id, window, cx));
                    },
                ))
            },
            window,
            cx,
        );
        let paste_duplicate = row
            .paste_issue
            .as_ref()
            .is_some_and(|issue| issue.fragment.is_some());
        let error = row
            .paste_issue
            .as_ref()
            .map(|issue| &issue.error)
            .or(draft.error.as_ref());
        let style = draft.quote_style;
        let value = if row.revealed {
            value_editor_frame(
                &row.value,
                Textarea::new(&row.value).aria_label(tr!("variable.new_literal")),
                style,
                Some(
                    quote_button("new-variable-quotes", style, false).on_click(cx.listener(
                        move |this, event: &ClickEvent, window, cx| {
                            this.update_new_variable(id, Some(style.next()), cx);
                            if event.mouse_position().is_some() {
                                this.focus_new_variable(id, true, window, cx);
                            }
                        },
                    )),
                ),
                false,
                window,
                cx,
            )
            .into_any_element()
        } else {
            design::variable_field(false)
                .id("new-variable-hidden")
                .tab_index(0)
                .focus_visible(|this| this.border_color(rgb(design::TAB_TEXT_ACTIVE)))
                .role(Role::Button)
                .aria_label(tr!("variable.show_value"))
                .cursor_text()
                .p(px(design::FIELD_PADDING_Y))
                .when(!draft.value.is_empty(), |this| this.child(hidden_pattern()))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.reveal_new_variable(id, window, cx);
                }))
                .into_any_element()
        };
        let visibility = if row.revealed {
            tr!("variable.hide_value")
        } else {
            tr!("variable.show_value")
        };
        let eye = if row.revealed {
            design::asset(Asset::VariableEye).into_any_element()
        } else {
            action_icons::swap(
                "new-variable-hidden-icon".into(),
                ActionIcon::EyeSlash,
                ActionIcon::EyeSlash,
                false,
                window,
                cx,
            )
            .into_any_element()
        };
        div()
            .id("new-variable-row")
            .track_focus(&row.focus)
            .capture_action(cx.listener(move |this, _: &Escape, window, cx| {
                this.cancel_new_variable(id, window, cx)
            }))
            .font_family(design::MONO_FONT)
            .w_full()
            .flex()
            .flex_col()
            .pb(px(design::BLOCK_GAP))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(design::KEY_GAP))
                    .child(
                        div()
                            .w(key_column_width(window))
                            .h(px(design::FIELD_HEIGHT))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(actions)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .mx(px(-4.))
                                    .rounded_sm()
                                    .when(
                                        window.last_input_was_keyboard()
                                            && row
                                                .name
                                                .read(cx)
                                                .focus_handle(cx)
                                                .is_focused(window),
                                        |this| this.focus_ring_style(window, cx),
                                    )
                                    .child(
                                        Input::new(&row.name)
                                            .appearance(false)
                                            .bordered(false)
                                            .with_size(Size::XSmall)
                                            .h(px(18.))
                                            .text_size(px(14.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .line_height(px(18.))
                                            .text_color(rgb(KEY_COLOR))
                                            .aria_label(tr!("variable.name"))
                                            .on_paste(move |item, window, cx| {
                                                let Some(text) =
                                                    item.text().filter(|text| text.contains('='))
                                                else {
                                                    return false;
                                                };
                                                // The paste action still holds the input entity while this hook runs.
                                                let owner = owner.clone();
                                                window.defer(cx, move |window, cx| {
                                                    let _ = owner.update(cx, |this, cx| {
                                                        this.paste_new_variables(
                                                            id, text, window, cx,
                                                        )
                                                    });
                                                });
                                                true
                                            }),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_start()
                            .gap(px(10.))
                            .child(div().flex_1().min_w_0().child(value))
                            .child(
                                action_icons::button("new-variable-copy", cx)
                                    .w(px(Asset::VariableCopy.dimensions().0))
                                    .accessibility_label(tr!("variable.copy_value"))
                                    .tooltip(tr!("variable.copy_literal"))
                                    .child(design::asset(Asset::VariableCopy))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(draft) = this
                                            .tab(id)
                                            .and_then(|tab| tab.content.as_ref())
                                            .and_then(DocumentSession::new_variable)
                                        {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                draft.token(),
                                            ));
                                        }
                                    })),
                            )
                            .child(
                                action_icons::button("new-variable-reveal", cx)
                                    .accessibility_label(visibility.clone())
                                    .tooltip(visibility)
                                    .child(eye)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.toggle_new_variable(id, window, cx)
                                    })),
                            ),
                    ),
            )
            .when_some(error, |this, error| {
                this.child(
                    div()
                        .pt_1()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            Label::new(edit_error_message(error))
                                .text_color(rgb(0xfda4af))
                                .text_size(px(12.)),
                        )
                        .when(paste_duplicate, |this| {
                            this.child(
                                div().flex().child(
                                    Button::new("new-variable-paste-duplicates")
                                        .ghost()
                                        .small()
                                        .label(tr!("variable.paste_with_duplicates"))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.confirm_pasted_duplicates(id, window, cx)
                                        })),
                                ),
                            )
                        })
                        .when_some(
                            match error {
                                EditError::DuplicateName { existing } if !paste_duplicate => {
                                    Some(*existing)
                                }
                                _ => None,
                            },
                            |this, existing| {
                                this.child(
                                    div()
                                        .flex()
                                        .gap_2()
                                        .child(
                                            Button::new("new-variable-existing")
                                                .ghost()
                                                .small()
                                                .label(tr!("variable.go_to_existing"))
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        this.begin_editor(id, existing, window, cx)
                                                    },
                                                )),
                                        )
                                        .child(
                                            Button::new("new-variable-duplicate")
                                                .ghost()
                                                .small()
                                                .label(tr!("variable.create_duplicate"))
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        this.apply_new_variable(
                                                            id, true, window, cx,
                                                        )
                                                    },
                                                )),
                                        ),
                                )
                            },
                        ),
                )
            })
            .into_any_element()
    }

    fn inline_text_view(
        &self,
        tab_id: u64,
        field: TextField,
        prefix: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(session) = self.tab(tab_id).and_then(|tab| tab.content.as_ref()) else {
            return div().into_any_element();
        };
        let (color, font_size, weight) = if let TextField::Name(id) = field {
            let commented = session
                .document
                .entry(id)
                .is_some_and(|entry| entry.commented);
            (
                if commented { SECONDARY } else { KEY_COLOR },
                14.,
                FontWeight::MEDIUM,
            )
        } else {
            (SECONDARY, 12., FontWeight::NORMAL)
        };
        let text = session.field_text(field).unwrap_or("");
        let text = if matches!(field, TextField::Comment(_)) {
            editing::comment_content(text).1
        } else {
            text
        }
        .to_owned();
        let error = session
            .text_draft(field)
            .and_then(|d| d.error.as_ref())
            .map(edit_error_message);
        let editing = self
            .editor
            .as_ref()
            .filter(|e| e.tab == tab_id && e.target == EditorTarget::Text(field));
        let target = EditorTarget::Text(field);
        let editable = !session.document.is_read_only();
        let focus = self.field_preview(tab_id, target, editable, window, cx);
        let content = if let Some(editor) = editing {
            // Compensate Kit's 4px horizontal inset without moving the text.
            div()
                .flex_1()
                .min_w_0()
                .mx(px(-4.))
                .rounded_sm()
                .when(
                    window.last_input_was_keyboard()
                        && editor.state.read(cx).focus_handle(cx).is_focused(window),
                    |this| this.focus_ring_style(window, cx),
                )
                .child(
                    Textarea::new(&editor.state)
                        .appearance(false)
                        .bordered(false)
                        .with_size(Size::XSmall)
                        .h(px(18.))
                        .text_size(px(font_size))
                        .line_height(px(18.))
                        .font_weight(weight)
                        .text_color(rgb(color))
                        .aria_label(match field {
                            TextField::Name(_) => tr!("variable.name"),
                            _ => tr!("editor.comment"),
                        }),
                )
                .into_any_element()
        } else {
            let text: SharedString = text.into();
            let preview = StyledText::new(text.clone());
            let layout = preview.layout().clone();
            let tooltip = text.clone();
            let accessible_text = text.clone();
            div()
                .id(SharedString::from(format!("inline-{field:?}")))
                .when(editable, |this| this.track_focus(&focus.focus))
                .flex_1()
                .min_w_0()
                .h(px(18.))
                .rounded_sm()
                .when(
                    window.last_input_was_keyboard() && focus.focus.is_focused(window),
                    |this| this.focus_ring_style(window, cx),
                )
                .when(matches!(field, TextField::Name(id) if self.tab(tab_id).is_some_and(|tab| tab.matched == Some(id))), |this| {
                    let highlight = if color == SECONDARY { design::SELECTOR_BG } else { 0x4a3822 };
                    this.bg(rgb(highlight)).rounded_sm()
                })
                .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                .cursor_text()
                .role(Role::Button)
                .aria_label(match field {
                    TextField::Name(_) => tr!("editor.edit_name"),
                    _ => tr!("editor.edit_comment"),
                })
                .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                    let caret = event.mouse_position().and_then(|position| {
                        if text.is_empty() {
                            return Some(0);
                        }
                        let line = layout.line_layout_for_index(0)?;
                        let offset = point(position.x - layout.bounds().left(), px(0.));
                        let index = line.closest_index_for_position(offset, layout.line_height())
                            .unwrap_or_else(|index| index);
                        // The ellipsis is decoration and has no offset in the editor.
                        let displayed = layout.text();
                        let visible_end = if displayed != text.as_ref() {
                            displayed.strip_suffix('…').unwrap_or(&displayed).len()
                        } else {
                            text.len()
                        };
                        Some(index.min(visible_end))
                    });
                    this.edit_preview(tab_id, target, focus.clone(), caret, window, cx)
                }))
                .child(
                    div()
                        .id("inline-preview")
                        .when(!accessible_text.is_empty(), |this| {
                            this.role(Role::Label).aria_value(accessible_text)
                        })
                        .truncate()
                        .text_size(px(font_size))
                        .font_weight(weight)
                        .text_color(rgb(color))
                        .line_height(px(18.))
                        .child(preview),
                )
                .into_any_element()
        };
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_start()
                    .w_full()
                    .min_w_0()
                    .when(!prefix.is_empty(), |this| {
                        this.child(
                            Label::new(prefix.to_owned())
                                .text_size(px(font_size))
                                .font_weight(weight)
                                .text_color(rgb(color))
                                .line_height(px(18.)),
                        )
                    })
                    .child(content)
                    .when(editing.is_some(), |this| {
                        this.child(
                            canvas(
                                |_, window, _| {
                                    window.take_autoscroll();
                                },
                                |_, _, _, _| {},
                            )
                            .absolute()
                            .size_0(),
                        )
                    }),
            )
            .when_some(error, |this, error| {
                this.child(
                    Label::new(error)
                        .text_size(px(12.))
                        .text_color(rgb(0xfda4af)),
                )
            })
            .into_any_element()
    }

    pub(super) fn document_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.advance_row_drag(window, cx);
        if self
            .active_tab()
            .is_some_and(|tab| self.moving_to_trash(tab.id))
        {
            return div()
                .p_8()
                .child(Label::new(tr!("file.trash_pending")))
                .into_any_element();
        }
        let root_focus = self.focus.clone();
        let Some(tab) = self.active_tab_mut() else {
            return div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_4()
                .child(Label::new(if self.projects.is_empty() {
                    tr!("project.open_to_view")
                } else {
                    tr!("tab.empty")
                }))
                .child(
                    Button::new("empty-open").label(tr!("file.open")).on_click(
                        cx.listener(|this, _, window, cx| this.choose(false, window, cx)),
                    ),
                )
                .child(
                    Button::new("empty-new")
                        .ghost()
                        .label(tr!("file.create"))
                        .on_click(cx.listener(|this, _, window, cx| this.create_file(window, cx))),
                )
                .into_any_element();
        };
        let id = tab.id;
        let draft_focused = tab
            .new_variable
            .as_ref()
            .is_some_and(|row| row.focus.contains_focused(window, cx));
        tab.sync_document_list();
        if draft_focused && tab.new_variable.is_none() {
            root_focus.focus(window, cx);
        }
        let Some(session) = tab.content.as_ref() else {
            return div()
                .p_8()
                .flex()
                .flex_col()
                .gap_3()
                .child(Label::new(
                    tab.error
                        .as_ref()
                        .map(UiError::message)
                        .unwrap_or_else(|| tr!("file.opening")),
                ))
                .when(tab.error.is_some(), |this| {
                    this.child(Button::new("retry").label(tr!("action.retry")).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.load_active(cx);
                            cx.notify();
                        }),
                    ))
                })
                .into_any_element();
        };
        let count = session.document.blocks().len();
        let work = session.work_revision();
        if tab.layout_work != Some(work) {
            tab.list.remeasure();
            tab.layout_work = Some(work);
        }
        let list_state = tab.list.clone();
        let measured_list = list_state.clone();
        let has_draft = session.new_variable().is_some();
        let error = tab.error.as_ref().map(UiError::message);
        let storage_error = tab.storage_error.as_ref().map(ToString::to_string);
        let access =
            session.baseline.is_hardlinked() || session.baseline.has_unsupported_access_metadata();
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .when_some(error, |this, error| {
                this.child(
                    div()
                        .px_4()
                        .py_2()
                        .child(Label::new(error).text_color(rgb(0xfda4af))),
                )
            })
            .when_some(storage_error, |this, error| {
                this.child(
                    div()
                        .px_4()
                        .py_2()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(Label::new(error).text_color(rgb(0xfda4af)))
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .child(
                                    Button::new("recover-save-as")
                                        .ghost()
                                        .small()
                                        .label(tr!("file.save_as"))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.save_active(true, window, cx)
                                        })),
                                )
                                .child(
                                    Button::new("reload-disk")
                                        .ghost()
                                        .small()
                                        .label(tr!("file.reload_from_disk"))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.reload(id, window, cx)
                                        })),
                                ),
                        ),
                )
            })
            .when(access, |this| {
                this.child(
                    div()
                        .px_4()
                        .py_2()
                        .child(Label::new(tr!("file.save_as_required")).text_color(rgb(SECONDARY))),
                )
            })
            .when(list_state.item_count() > 0, |this| {
                this.child(
                    div()
                        .id("reorder-surface")
                        .relative()
                        .flex_1()
                        .min_h_0()
                        .overflow_hidden()
                        .on_drag_move(cx.listener(
                            |this, event: &DragMoveEvent<reorder::RowDrag>, _, cx| {
                                this.move_drag(event, cx)
                            },
                        ))
                        .on_drop(cx.listener(|this, drag: &reorder::RowDrag, window, cx| {
                            this.drop_row(drag, window, cx)
                        }))
                        .child(
                            list(
                                list_state.clone(),
                                cx.processor(move |this, index, window, cx| {
                                    let row = if index < count {
                                        this.block_view(id, index, false, window, cx)
                                    } else if has_draft && index == count {
                                        this.new_variable_view(id, window, cx)
                                    } else {
                                        this.add_row(id, cx)
                                    };
                                    this.keyboard_document_row(id, index, row, window, cx)
                                }),
                            )
                            .size_full()
                            .px(px(design::CONTENT_LEFT))
                            .pr(px(design::CONTENT_RIGHT))
                            .pb(px(15.)),
                        )
                        .child(
                            canvas(
                                move |_, window, cx| {
                                    // GPUI discards height hints on the first layout and on resize.
                                    // Restore them after list prepaint, before painting the scrollbar.
                                    let width = measured_list.viewport_bounds().size.width;
                                    window
                                        .use_keyed_state(
                                            format!("document-list-width-{id}"),
                                            cx,
                                            |_, _| None::<Pixels>,
                                        )
                                        .update(cx, |previous, _| {
                                            if *previous != Some(width) {
                                                *previous = Some(width);
                                                let _ = measured_list
                                                    .clone()
                                                    .with_uniform_item_height(px(
                                                        design::FIELD_HEIGHT + design::BLOCK_GAP,
                                                    ));
                                            }
                                        });
                                },
                                |_, _, _, _| {},
                            )
                            .absolute()
                            .inset_0(),
                        )
                        // Overlay the existing list so the scrollbar never shifts the fields.
                        // The scrollbar itself hides when the document fits the viewport.
                        .child(
                            Scrollbar::vertical(&list_state)
                                .id(("document-scrollbar", id))
                                .mode(ScrollbarMode::Always)
                                .styles(|styles| {
                                    styles
                                        .track(|track| {
                                            track
                                                .width(px(12.))
                                                .bg(transparent_black())
                                                .border_color(transparent_black())
                                        })
                                        .track_hover(|track| {
                                            track
                                                .bg(transparent_black())
                                                .border_color(transparent_black())
                                        })
                                        .track_active(|track| {
                                            track
                                                .bg(transparent_black())
                                                .border_color(transparent_black())
                                        })
                                        .thumb(|thumb| {
                                            thumb
                                                .width(px(6.))
                                                .inset(px(3.))
                                                .radius(px(4.))
                                                .bg(rgb(0x79716b).opacity(0.55))
                                        })
                                        .thumb_hover(|thumb| {
                                            thumb
                                                .width(px(8.))
                                                .inset(px(2.))
                                                .bg(rgb(0xa6a09b).opacity(0.7))
                                        })
                                        .thumb_active(|thumb| {
                                            thumb.width(px(8.)).inset(px(2.)).bg(rgb(0xa6a09b))
                                        })
                                }),
                        ),
                )
            })
            .into_any_element()
    }

    pub(super) fn block_view(
        &mut self,
        tab_id: u64,
        index: usize,
        preview: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(tab) = self.tab(tab_id) else {
            return div().into_any_element();
        };
        let Some(session) = tab.content.as_ref() else {
            return div().into_any_element();
        };
        let document = &session.document;
        let Some(block) = document.blocks().get(index) else {
            return div().into_any_element();
        };
        let env = design::Environment::from_path(
            self.projects
                .get(self.active_project)
                .and_then(|p| p.tabs.get(p.active))
                .map(|r| r.path.as_path())
                .unwrap_or(&tab.path),
        );
        let element = match &block.kind {
            BlockKind::Assignment(entry) => {
                let id = entry.id;
                let commented = entry.commented;
                let color = if commented {
                    SECONDARY
                } else {
                    design::VALUE_COLOR
                };
                let quote_color = if commented {
                    SECONDARY
                } else {
                    design::QUOTE_COLOR
                };
                let name = session
                    .field_text(TextField::Name(id))
                    .unwrap_or("")
                    .to_owned();
                let revealed = tab.revealed ^ tab.exceptions.contains(&id);
                let value = session.value_content(id).unwrap_or("");
                let quote_style = session.quote_style(id).unwrap_or(QuoteStyle::Unquoted);
                let quote_inset = quote_style.delimiter().map_or(px(0.), |quote| {
                    value_line(quote, quote_color, window).width + px(design::QUOTE_GAP)
                });
                let editing = self
                    .editor
                    .as_ref()
                    .filter(|e| e.tab == tab_id && e.target == EditorTarget::Value(id) && revealed);
                let target = EditorTarget::Value(id);
                let editable = !document.is_read_only();
                // Tabbing to a hidden value keeps it masked until activated.
                let focus = self.field_preview(tab_id, target, editable && revealed, window, cx);
                let value_element = if let Some(editor) = editing {
                    let button = quote_button(("cycle-quotes", id.0), quote_style, commented)
                        .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                            this.cycle_quotes(
                                tab_id,
                                id,
                                event.mouse_position().is_some(),
                                window,
                                cx,
                            );
                        }));
                    value_editor_frame(
                        &editor.state,
                        Textarea::new(&editor.state)
                            .aria_label(tr!("variable.value", name = name))
                            .accessibility_id(format!("value-editor-{}", id.0)),
                        quote_style,
                        Some(button),
                        commented,
                        window,
                        cx,
                    )
                    .id(("value-editor-context", id.0))
                    .into_any_element()
                } else {
                    let preview_scroll = window
                        .use_keyed_state(format!("value-preview-{tab_id}-{}", id.0), cx, |_, _| {
                            ScrollHandle::default()
                        })
                        .read(cx)
                        .clone();
                    let display = if revealed || value.is_empty() {
                        Some(value.to_owned())
                    } else {
                        None
                    };
                    let multiline = revealed && value.contains('\n');
                    let content_width = if revealed {
                        value
                            .split('\n')
                            .map(|line| {
                                let line = line.trim_end_matches('\r');
                                window
                                    .text_system()
                                    .shape_line(
                                        line.to_owned().into(),
                                        px(12.),
                                        &[TextRun {
                                            len: line.len(),
                                            font: Font {
                                                family: design::MONO_FONT.into(),
                                                weight: FontWeight::NORMAL,
                                                ..Default::default()
                                            },
                                            color: rgb(color).into(),
                                            background_color: None,
                                            underline: None,
                                            strikethrough: None,
                                        }],
                                        None,
                                    )
                                    .width
                                    + px(1.)
                            })
                            .fold(px(1.), Pixels::max)
                    } else {
                        px(1.)
                    };
                    design::variable_field(commented)
                        .id(("edit-value", id.0))
                        .when(editable, |this| this.track_focus(&focus.focus))
                        .when(
                            window.last_input_was_keyboard() && focus.focus.is_focused(window),
                            |this| this.focus_ring_style(window, cx),
                        )
                        .flex_1()
                        .pl(px(design::FIELD_PADDING_X) + quote_inset)
                        .pr(px(design::FIELD_PADDING_X) + quote_inset)
                        .py(px(design::FIELD_PADDING_Y))
                        .role(Role::Button)
                        .aria_label(tr!("variable.edit_named_value", name = name))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.edit_preview(tab_id, target, focus.clone(), None, window, cx)
                        }))
                        .child(
                            div()
                                .relative()
                                .w_full()
                                .child(
                                    div()
                                        .id(("value-scroll", id.0))
                                        .flex()
                                        .w_full()
                                        .h(px(if multiline {
                                            value.split('\n').count() as f32
                                                * design::FIELD_LINE_HEIGHT
                                        } else {
                                            design::FIELD_LINE_HEIGHT
                                        }))
                                        .when_some(display, |this, value| {
                                            this.child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .w(content_width)
                                                    .min_w_full()
                                                    .min_h(px(design::FIELD_LINE_HEIGHT))
                                                    .flex_shrink_0()
                                                    .whitespace_nowrap()
                                                    .when(!multiline, |this| {
                                                        this.h(px(design::FIELD_LINE_HEIGHT))
                                                    })
                                                    .children(value.split('\n').enumerate().map(
                                                        |(line_index, line)| {
                                                            div()
                                                                .id(("value-line", line_index))
                                                                .h(px(design::FIELD_LINE_HEIGHT))
                                                                .child(
                                                                    Label::new(
                                                                        line.trim_end_matches('\r')
                                                                            .to_owned(),
                                                                    )
                                                                    .text_size(px(12.))
                                                                    .font_weight(FontWeight::NORMAL)
                                                                    .line_height(px(
                                                                        design::FIELD_LINE_HEIGHT,
                                                                    ))
                                                                    .text_color(rgb(color)),
                                                                )
                                                        },
                                                    )),
                                            )
                                        })
                                        .when(!revealed && !value.is_empty(), |this| {
                                            this.child(hidden_pattern())
                                        })
                                        .overflow_x_scroll()
                                        .lock_scroll_axis()
                                        .track_scroll(&preview_scroll),
                                )
                                .child(ScrollableMask::new(Axis::Horizontal, &preview_scroll))
                                .child(
                                    div()
                                        .absolute()
                                        .top_0()
                                        .left_0()
                                        .right_0()
                                        .bottom(px(-design::FIELD_PADDING_Y))
                                        .child(
                                            Scrollbar::horizontal(&preview_scroll)
                                                .mode(ScrollbarMode::Always)
                                                .viewport_from_layout(),
                                        ),
                                ),
                        )
                        .child(quote_decoration(
                            quote_style,
                            (revealed || value.is_empty()).then(|| value.to_owned()),
                            QuoteScroll::Preview(preview_scroll.clone()),
                            quote_color,
                        ))
                        .into_any_element()
                };
                let weak = cx.weak_entity();
                let actions = reorder::click_menu(
                    format!("row-menu-{tab_id}-{}", id.0),
                    Button::new(("row-actions", id.0))
                        .ghost()
                        .cursor(CursorStyle::OpenHand)
                        .w(px(16.))
                        .h(px(16.))
                        .p_0()
                        .border_0()
                        .accessibility_label(tr!("variable.actions", name = name))
                        .tooltip(tr!("variable.actions", name = name))
                        .child(if commented {
                            design::asset(Asset::Comment).into_any_element()
                        } else {
                            design::variable_asset(&name, session.value(id).unwrap_or(""), env)
                                .into_any_element()
                        }),
                    move |menu, _, _| {
                        let edit = weak.clone();
                        let copy = weak.clone();
                        let rename = weak.clone();
                        let toggle_comment = weak.clone();
                        let remove = weak.clone();
                        menu.item(
                            PopupMenuItem::new(tr!("variable.edit_value"))
                                .icon(variable_menu_icon(include_bytes!(
                                    "../../assets/actions/variable-menu/input-password-edit.svg"
                                )))
                                .on_click(move |_, window, cx| {
                                    let _ = edit.update(cx, |this, cx| {
                                        this.begin_editor(tab_id, id, window, cx)
                                    });
                                }),
                        )
                        .item(
                            PopupMenuItem::new(tr!("variable.copy_value"))
                                .icon(variable_menu_icon(include_bytes!(
                                    "../../assets/actions/variable-menu/duplicate.svg"
                                )))
                                .on_click(move |_, window, cx| {
                                    let _ = copy.update(cx, |this, cx| {
                                        this.copy_entry(tab_id, id, window, cx)
                                    });
                                }),
                        )
                        .item(
                            PopupMenuItem::new(tr!("variable.rename_action"))
                                .icon(variable_menu_icon(include_bytes!(
                                    "../../assets/actions/variable-menu/file-pen.svg"
                                )))
                                .on_click(move |_, window, cx| {
                                    let _ = rename.update(cx, |this, cx| {
                                        this.open_rename_dialog(id, window, cx)
                                    });
                                }),
                        )
                        .item(
                            PopupMenuItem::new(if commented {
                                tr!("variable.uncomment")
                            } else {
                                tr!("variable.comment")
                            })
                            .icon(variable_menu_icon(include_bytes!(
                                "../../assets/actions/variable-menu/circle-hashtag.svg"
                            )))
                            .on_click(move |_, window, cx| {
                                let _ = toggle_comment.update(cx, |this, cx| {
                                    this.set_variable_commented(tab_id, id, !commented, window, cx);
                                });
                            }),
                        )
                        .separator()
                        .item(
                            delete_menu_item(tr!("variable.delete")).on_click(
                                move |_, window, cx| {
                                    let _ = remove.update(cx, |this, cx| {
                                        this.remove_variable(tab_id, id, window, cx)
                                    });
                                },
                            ),
                        )
                    },
                    window,
                    cx,
                );
                div()
                    .flex()
                    .items_start()
                    .gap(px(design::KEY_GAP))
                    .w_full()
                    .child(
                        div()
                            .w(key_column_width(window))
                            .flex_shrink_0()
                            .min_w_0()
                            .min_h(px(32.))
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                self.drag_handle(
                                    reorder::RowDrag {
                                        tab: tab_id,
                                        row: index,
                                    },
                                    cx,
                                )
                                .child(actions),
                            )
                            .child(self.inline_text_view(
                                tab_id,
                                TextField::Name(id),
                                "",
                                window,
                                cx,
                            )),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .flex()
                                    .items_start()
                                    .gap(px(10.))
                                    .w_full()
                                    .child(div().flex_1().min_w_0().child(value_element))
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .flex()
                                            .gap(px(10.))
                                            .child(
                                                action_icons::button(("copy", id.0), cx)
                                                    .accessibility_label(tr!(
                                                        "variable.copy_named_value",
                                                        name = name,
                                                    ))
                                                    .tooltip(tr!("variable.copy_literal"))
                                                    .child(action_icons::swap(
                                                        format!("copy-{tab_id}-{}", id.0),
                                                        ActionIcon::Duplicate,
                                                        ActionIcon::Check,
                                                        tab.copied.contains_key(&Some(id)),
                                                        window,
                                                        cx,
                                                    ))
                                                    .on_click(cx.listener(
                                                        move |this, _, window, cx| {
                                                            this.copy_entry(tab_id, id, window, cx)
                                                        },
                                                    )),
                                            )
                                            .child(
                                                action_icons::button(("reveal", id.0), cx)
                                                    .accessibility_label(if revealed {
                                                        tr!(
                                                            "variable.hide_named_value",
                                                            name = name
                                                        )
                                                    } else {
                                                        tr!(
                                                            "variable.show_named_value",
                                                            name = name
                                                        )
                                                    })
                                                    .tooltip(if revealed {
                                                        tr!("variable.hide_value")
                                                    } else {
                                                        tr!("variable.show_value")
                                                    })
                                                    .child(action_icons::swap(
                                                        format!("reveal-{tab_id}-{}", id.0),
                                                        ActionIcon::Eye,
                                                        ActionIcon::EyeSlash,
                                                        revealed,
                                                        window,
                                                        cx,
                                                    ))
                                                    .on_click(cx.listener(
                                                        move |this, _, window, cx| {
                                                            this.toggle_row(tab_id, id, window, cx)
                                                        },
                                                    )),
                                            ),
                                    ),
                            )
                            .when(entry.inline_comment.is_some(), |this| {
                                this.child(self.inline_text_view(
                                    tab_id,
                                    TextField::InlineComment(id),
                                    "#",
                                    window,
                                    cx,
                                ))
                            })
                            .when_some(
                                session
                                    .draft(id)
                                    .and_then(|d| d.error.as_ref())
                                    .map(edit_error_message),
                                |this, error| {
                                    this.child(
                                        Label::new(error)
                                            .text_color(rgb(0xfda4af))
                                            .text_size(px(12.))
                                            .font_weight(FontWeight::NORMAL)
                                            .line_height(px(18.)),
                                    )
                                },
                            ),
                    )
                    .into_any_element()
            }
            BlockKind::Comment => {
                let icon = design::asset(Asset::Comment);
                let actions = if preview || document.is_read_only() {
                    icon.into_any_element()
                } else {
                    let owner = cx.weak_entity();
                    let label = tr!("editor.comment_actions");
                    let menu = reorder::click_menu(
                        format!("comment-menu-{tab_id}-{index}"),
                        Button::new(("comment-actions", index))
                            .ghost()
                            .cursor(CursorStyle::OpenHand)
                            .w(px(16.))
                            .h(px(14.))
                            .p_0()
                            .border_0()
                            .accessibility_label(label.clone())
                            .tooltip(label)
                            .child(icon),
                        move |menu, window, cx| {
                            let target = owner
                                .update(cx, |this, cx| {
                                    // Opening the popup blurs the text field. Commit its
                                    // edit before capturing the row's revision.
                                    this.finish_editor(false, window, cx);
                                    this.active_tab()
                                        .filter(|tab| tab.id == tab_id)
                                        .and_then(|tab| tab.content.as_ref())
                                        .map(|session| session.capture_comment_target(index))
                                })
                                .ok()
                                .flatten();
                            let Some(target) = target else {
                                return menu;
                            };
                            let owner = owner.clone();
                            menu.item(delete_menu_item(tr!("editor.delete_comment")).on_click(
                                move |_, window, cx| {
                                    let _ = owner.update(cx, |this, cx| {
                                        this.remove_comment(tab_id, &target, window, cx);
                                    });
                                },
                            ))
                        },
                        window,
                        cx,
                    );
                    self.drag_handle(
                        reorder::RowDrag {
                            tab: tab_id,
                            row: index,
                        },
                        cx,
                    )
                    .child(menu)
                    .into_any_element()
                };
                div()
                    .flex()
                    .items_start()
                    .gap(px(10.))
                    .w_full()
                    .child(actions)
                    .child(self.inline_text_view(tab_id, TextField::Comment(index), "", window, cx))
                    .into_any_element()
            }
            BlockKind::Blank => div().h(px(6.)).into_any_element(),
            BlockKind::Opaque(diagnostic) => div()
                .pl(px(27.))
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    Label::new(diagnostic_message(*diagnostic))
                        .text_color(rgb(SECONDARY))
                        .text_size(px(12.))
                        .font_weight(FontWeight::NORMAL)
                        .line_height(px(18.)),
                )
                .when(tab.revealed && !document.is_read_only(), |this| {
                    this.child(
                        Label::new(
                            document
                                .text(block.range.clone())
                                .unwrap_or("")
                                .trim_end_matches(['\r', '\n'])
                                .to_owned(),
                        )
                        .text_color(rgb(SECONDARY))
                        .text_size(px(12.))
                        .font_weight(FontWeight::NORMAL)
                        .line_height(px(18.)),
                    )
                })
                .into_any_element(),
        };
        let gap = reorder::block_gap(session, index);
        let (offset, dragged) = if preview {
            (0., false)
        } else {
            tab.reorder
                .as_ref()
                .map(|motion| motion.painting(index, window, cx))
                .unwrap_or((0., false))
        };
        div()
            .id(("block", index))
            .relative()
            .top(px(offset))
            .opacity(if dragged { 0.15 } else { 1. })
            .font_family(design::MONO_FONT)
            .text_size(px(12.))
            .font_weight(FontWeight::NORMAL)
            .line_height(px(18.))
            .line_height(px(18.))
            .pb(px(gap))
            .child(element)
            .into_any_element()
    }
}
