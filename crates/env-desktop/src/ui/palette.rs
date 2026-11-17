use super::*;
use crate::i18n::tr;
use gpui_kit::{
    base::FocusTrapElement,
    component::command::{Command, CommandGroup, CommandItem, CommandState},
    component::{Icon, IconName, IndexPath},
};
use std::{cell::Cell, rc::Rc};

pub(super) struct ProjectPalette {
    state: Entity<CommandState>,
    focus: FocusHandle,
    previous_focus: Option<FocusHandle>,
    selection: Rc<Cell<Option<IndexPath>>>,
}

fn palette_foreground(selected: bool) -> u32 {
    if selected {
        design::VALUE_COLOR
    } else {
        design::TAB_TEXT_ACTIVE
    }
}

fn palette_row(selected: bool) -> Div {
    // Command supplies the hit area and keyboard navigation. Cover its default
    // row padding so the entire highlight uses the app's button colors.
    div()
        .flex()
        .flex_1()
        .min_w_0()
        .mx_neg_2()
        .my(rems(-0.375))
        .px_2()
        .py_1p5()
        .rounded(px(8.))
        .bg(rgb(if selected {
            design::SELECTOR_BG
        } else {
            CONTENT_BG
        }))
        .text_color(rgb(palette_foreground(selected)))
}

fn sync_selection(
    state: &WeakEntity<CommandState>,
    selection: &Cell<Option<IndexPath>>,
    cx: &mut App,
) {
    let _ = state.update(cx, |state, cx| {
        let next = state.selected_index();
        if selection.replace(next) != next {
            // Repaint the Command itself, preserving its measured rows and scroll.
            cx.notify();
        }
    });
}

impl AppWindow {
    pub(super) fn toggle_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.is_some() {
            self.close_palette(window, cx);
            return;
        }
        if self.dialog.is_some() || self.opening || self.closing || self.config_closing {
            return;
        }
        let previous_focus = window.focused(cx);
        let state = cx.new(|cx| CommandState::new(window, cx));
        state.update(cx, |state, cx| state.focus(window, cx));
        self.palette = Some(ProjectPalette {
            state,
            focus: cx.focus_handle(),
            previous_focus,
            selection: Rc::new(Cell::new(Some(
                IndexPath::new(0).section(usize::from(self.projects.is_empty())),
            ))),
        });
        cx.notify();
    }

    pub(super) fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(palette) = self.palette.take() {
            palette
                .previous_focus
                .unwrap_or_else(|| self.focus.clone())
                .focus(window, cx);
            cx.notify();
        }
    }

    pub(super) fn palette_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Keep Tab inside the search field; arrows and Enter belong to Command.
        match event.keystroke.key.as_str() {
            "escape" => {
                self.close_palette(window, cx);
                cx.stop_propagation();
            }
            "tab" if !event.keystroke.modifiers.control => {
                if let Some(palette) = &self.palette {
                    palette
                        .state
                        .update(cx, |state, cx| state.focus(window, cx));
                }
                cx.stop_propagation();
            }
            _ => {}
        }
    }

    pub(super) fn palette_view(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let palette = self.palette.as_ref()?;
        let mut projects = CommandGroup::new().label(tr!("palette.projects"));
        let mut ids = Vec::with_capacity(self.projects.len());
        for (index, project) in self.projects.iter().enumerate() {
            let selection = palette.selection.clone();
            let name = project
                .root
                .file_name()
                .unwrap_or(project.root.as_os_str())
                .to_string_lossy()
                .into_owned();
            let path = project.root.display().to_string();
            let active = index == self.active_project;
            ids.push(project.id);
            projects = projects.item(
                CommandItem::new()
                    .label(name.clone())
                    .keywords([path.clone()])
                    .child(move |_, _| {
                        let selected = selection.get() == Some(IndexPath::new(index));
                        palette_row(selected)
                            .items_start()
                            .gap(px(10.))
                            .child(div().h(px(18.)).flex().items_center().child(design::asset(
                                if selected {
                                    Asset::WelcomeFolder
                                } else {
                                    Asset::Folder
                                },
                            )))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_w_0()
                                    .gap_0()
                                    .child(
                                        Label::new(name.clone())
                                            .truncate()
                                            .text_size(px(14.))
                                            .line_height(px(18.))
                                            .text_color(rgb(palette_foreground(selected))),
                                    )
                                    .child(
                                        Label::new(path.clone())
                                            .truncate()
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .text_color(rgb(if selected {
                                                design::TAB_TEXT_ACTIVE
                                            } else {
                                                SECONDARY
                                            })),
                                    ),
                            )
                            .when(active, |row| {
                                row.child(
                                    div()
                                        .h(px(18.))
                                        .flex()
                                        .items_center()
                                        .child(Icon::new(IconName::Check).xsmall()),
                                )
                            })
                    }),
            );
        }
        let has_project = !self.projects.is_empty();
        let weak = cx.weak_entity();
        let actions = [
            tr!("project.add_action"),
            tr!("project.locate_action"),
            tr!("project.remove"),
        ]
        .into_iter()
        .enumerate()
        .map(|(row, label)| {
            let selection = palette.selection.clone();
            let label = SharedString::from(label);
            CommandItem::new()
                .label(label.clone())
                .disabled(row > 0 && !has_project)
                .child(move |_, _| {
                    let selected = selection.get() == Some(IndexPath::new(row).section(1));
                    palette_row(selected).child(
                        Label::new(label.clone())
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .text_color(rgb(if row > 0 && !has_project {
                                SECONDARY
                            } else {
                                palette_foreground(selected)
                            })),
                    )
                })
        });
        let selection = palette.selection.clone();
        let state = palette.state.downgrade();
        let query_selection = selection.clone();
        let query_state = state.clone();
        let command = Command::new(&palette.state)
            .placeholder(tr!("palette.search"))
            .bordered(false)
            .rounded(px(11.))
            .bg(rgb(CONTENT_BG))
            .text_color(rgb(design::VALUE_COLOR))
            .max_h(px(320.))
            .group(projects)
            .separator()
            .group(
                CommandGroup::new()
                    .label(tr!("palette.actions"))
                    .items(actions),
            )
            .on_select(move |_, _, cx| sync_selection(&state, &selection, cx))
            .on_query(move |_, _, cx| sync_selection(&query_state, &query_selection, cx))
            .empty(|_, _, _| {
                div().p_6().text_center().child(
                    Label::new(tr!("palette.no_results"))
                        .text_sm()
                        .text_color(rgb(SECONDARY)),
                )
            })
            .footer(|_, _, _| {
                div()
                    .px_3()
                    .py_2()
                    .border_t_1()
                    .border_color(rgb(design::SELECTOR_BG))
                    .child(
                        Label::new(tr!("palette.shortcuts"))
                            .text_xs()
                            .text_color(rgb(SECONDARY)),
                    )
            })
            .on_confirm(move |index, window, cx| {
                let _ = weak.update(cx, |this, cx| {
                    this.close_palette(window, cx);
                    match index.section {
                        0 => {
                            if let Some(id) = ids.get(index.row)
                                && let Some(index) = this.projects.iter().position(|p| p.id == *id)
                            {
                                this.select_project(index, window, cx);
                            }
                        }
                        1 => match index.row {
                            0 => this.choose(true, window, cx),
                            1 => this.locate_project(window, cx),
                            2 => this.remove_project(window, cx),
                            _ => {}
                        },
                        _ => {}
                    }
                });
            });
        Some(
            div()
                .id("project-palette-overlay")
                .absolute()
                .inset_0()
                .flex()
                .items_start()
                .justify_center()
                .pt(px(110.))
                .bg(rgba(0x00000088))
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        this.close_palette(window, cx);
                    }),
                )
                .child(
                    div()
                        .id("project-palette-dialog")
                        .track_focus(&palette.focus)
                        .w(px(560.))
                        .max_w_full()
                        .overflow_hidden()
                        .rounded(px(12.))
                        .bg(rgb(CONTENT_BG))
                        .border_1()
                        .border_color(rgb(design::SELECTOR_BG))
                        .shadow_lg()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(command)
                        .focus_trap("project-palette-focus", &palette.focus),
                )
                .into_any_element(),
        )
    }
}
