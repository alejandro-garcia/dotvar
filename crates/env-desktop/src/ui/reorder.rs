use super::*;
use env_core::BlockKind;
use gpui_kit::base::motion::{Transition, TransitionState};
use std::time::{Duration, Instant};

const MOVE_DURATION: Duration = Duration::from_millis(180);

#[derive(Clone)]
pub(super) struct RowDrag {
    pub tab: u64,
    pub row: usize,
}

pub(super) struct RowPreview {
    owner: WeakEntity<AppWindow>,
    drag: RowDrag,
    width: Pixels,
    lift: Pixels,
    valid: bool,
}

impl Render for RowPreview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let row = if self.valid {
            self.owner
                .update(cx, |this, cx| {
                    this.block_view(self.drag.tab, self.drag.row, true, window, cx)
                })
                .ok()
        } else {
            None
        };
        div()
            .w(self.width)
            .relative()
            .top(-self.lift)
            .px_1()
            .py_1()
            .bg(rgb(CONTENT_BG))
            .rounded(px(6.))
            .shadow_lg()
            .opacity(0.96)
            .children(row)
    }
}

struct Gesture {
    from: usize,
    to: usize,
    pointer: Point<Pixels>,
    grab_y: f32,
    last_tick: Instant,
}

/// Keep clocks outside virtualized elements so reversing a drag stays continuous.
/// Positions are absolute within the document, then painted relative to each slot.
pub(super) struct ReorderMotion {
    pub work: (u64, u64),
    heights: Vec<f32>,
    bases: Vec<f32>,
    targets: Vec<f32>,
    clocks: Vec<TransitionState<f32>>,
    gesture: Option<Gesture>,
    settles_at: Instant,
}

fn tops(heights: &[f32]) -> Vec<f32> {
    let mut y = 0.;
    heights
        .iter()
        .map(|height| {
            let top = y;
            y += height;
            top
        })
        .collect()
}

fn target_row(from: usize, center: f32, bases: &[f32], heights: &[f32]) -> usize {
    let mut target = from;
    for (row, (base, height)) in bases.iter().zip(heights).enumerate() {
        let middle = base + height / 2.;
        if row < from && center < middle {
            target = target.min(row);
        }
        if row > from && center > middle {
            target = row;
        }
    }
    target
}

fn move_item<T>(items: &mut Vec<T>, from: usize, to: usize) {
    let item = items.remove(from);
    items.insert(to, item);
}

fn targets(heights: &[f32], from: usize, to: usize) -> Vec<f32> {
    let mut order: Vec<_> = (0..heights.len()).collect();
    move_item(&mut order, from, to);
    let mut result = vec![0.; heights.len()];
    let mut y = 0.;
    for index in order {
        result[index] = y;
        y += heights[index];
    }
    result
}

pub(super) fn block_gap(session: &DocumentSession, index: usize) -> f32 {
    if matches!(session.document.blocks()[index].kind, BlockKind::Comment)
        && session
            .document
            .blocks()
            .get(index + 1)
            .is_some_and(|b| matches!(b.kind, BlockKind::Comment))
    {
        design::COMMENT_GAP
    } else {
        design::BLOCK_GAP
    }
}

fn row_heights(tab: &FileTab) -> Vec<f32> {
    let Some(session) = &tab.content else {
        return vec![];
    };
    session
        .document
        .blocks()
        .iter()
        .enumerate()
        .map(|(row, block)| {
            if let Some(bounds) = tab.list.bounds_for_item(row) {
                return f32::from(bounds.size.height);
            }
            let body = match &block.kind {
                BlockKind::Assignment(entry) => {
                    let revealed = tab.revealed ^ tab.exceptions.contains(&entry.id);
                    let lines = if revealed {
                        session
                            .value_content(entry.id)
                            .unwrap_or("")
                            .split('\n')
                            .count()
                    } else {
                        1
                    };
                    design::FIELD_HEIGHT
                        + (lines - 1) as f32 * design::FIELD_LINE_HEIGHT
                        + if entry.inline_comment.is_some() {
                            22.
                        } else {
                            0.
                        }
                }
                BlockKind::Comment => 18.,
                BlockKind::Blank => 6.,
                BlockKind::Opaque(_) => 18.,
            };
            body + block_gap(session, row)
        })
        .collect()
}

impl ReorderMotion {
    pub fn painting(&self, row: usize, window: &mut Window, cx: &mut App) -> (f32, bool) {
        let Some(clock) = self.clocks.get(row) else {
            return (0., false);
        };
        let y = clock
            .sample(
                self.targets[row],
                Transition::new(MOVE_DURATION),
                window,
                cx,
            )
            .value;
        (
            y - self.bases[row],
            self.gesture.as_ref().is_some_and(|g| g.from == row),
        )
    }

    fn cancel(&mut self) {
        self.gesture = None;
        self.targets.clone_from(&self.bases);
        self.settles_at = Instant::now() + MOVE_DURATION;
    }
}

type MenuBuilder = std::rc::Rc<
    dyn Fn(
        gpui_kit::component::menu::PopupMenu,
        &mut Window,
        &mut Context<gpui_kit::component::menu::PopupMenu>,
    ) -> gpui_kit::component::menu::PopupMenu,
>;
#[derive(Default)]
struct RowMenu {
    menu: Option<Entity<gpui_kit::component::menu::PopupMenu>>,
    dismissal: Option<Subscription>,
}

fn open_menu(state: &Entity<RowMenu>, build: &MenuBuilder, window: &mut Window, cx: &mut App) {
    use gpui_kit::component::menu::PopupMenu;
    let build = build.clone();
    let menu = PopupMenu::build(window, cx, move |menu, window, cx| build(menu, window, cx));
    state.update(cx, |state, _| state.menu = Some(menu));
    window.refresh();
}

/// The standard dropdown opens on mouse-down, consuming the drag gesture.
/// Open this menu on a completed click instead, sharing the same icon hit area.
pub(super) fn click_menu(
    id: String,
    button: Button,
    build: impl Fn(
        gpui_kit::component::menu::PopupMenu,
        &mut Window,
        &mut Context<gpui_kit::component::menu::PopupMenu>,
    ) -> gpui_kit::component::menu::PopupMenu
    + 'static,
    window: &mut Window,
    cx: &mut App,
) -> gpui_kit::component::popover::Popover {
    let state = window.use_keyed_state(SharedString::from(id.clone()), cx, |_, _| {
        RowMenu::default()
    });
    let build: MenuBuilder = std::rc::Rc::new(build);
    let clicked_state = state.clone();
    let clicked_build = build.clone();
    let changed_state = state.clone();
    let content_state = state.clone();
    let open = state.read(cx).menu.is_some();
    gpui_kit::component::popover::Popover::new(SharedString::from(id))
        .appearance(false)
        // Left mouse-down must reach the drag handle. Right click still opens the menu.
        .mouse_button(MouseButton::Right)
        .open(open)
        .trigger(button.on_click(move |_, window, cx| {
            if cx.has_active_drag() {
                return;
            }
            if clicked_state.read(cx).menu.is_some() {
                clicked_state.update(cx, |state, _| *state = RowMenu::default());
                window.refresh();
            } else {
                open_menu(&clicked_state, &clicked_build, window, cx);
            }
        }))
        .on_open_change(move |open, window, cx| {
            if *open {
                if changed_state.read(cx).menu.is_none() {
                    open_menu(&changed_state, &build, window, cx);
                }
            } else {
                changed_state.update(cx, |state, _| *state = RowMenu::default());
                window.refresh();
            }
        })
        .content(move |_, window, cx| {
            let popover = cx.weak_entity();
            let menu_state = content_state.downgrade();
            let menu = content_state.update(cx, |state, cx| {
                let menu = state.menu.clone()?;
                if state.dismissal.is_none() {
                    // Keyboard activation opens the popover before this menu
                    // exists. Focus it as soon as its content is mounted.
                    menu.focus_handle(cx).focus(window, cx);
                    state.dismissal = Some(window.subscribe(
                        &menu,
                        cx,
                        move |_, _: &DismissEvent, window, cx| {
                            // Dismiss directly even if the action removed the trigger row.
                            let _ = popover.update(cx, |state, cx| state.dismiss(window, cx));
                            let _ = menu_state.update(cx, |state, _| *state = RowMenu::default());
                            window.refresh();
                        },
                    ));
                }
                Some(menu)
            });
            div().key_context("VariableMenu").children(menu)
        })
}

impl AppWindow {
    pub(super) fn drag_handle(&self, drag: RowDrag, cx: &Context<Self>) -> Stateful<Div> {
        let owner = cx.weak_entity();
        div()
            .id(("reorder-handle", drag.row))
            .flex_shrink_0()
            .cursor(CursorStyle::OpenHand)
            .on_drag(drag, move |drag, _, window, cx| {
                let mut valid = false;
                let mut width = px(400.);
                let mut lift = px(0.);
                let _ = owner.update(cx, |this, cx| {
                    // Finishing a focused field invalidates its list measurement.
                    let row_bounds = this
                        .tab(drag.tab)
                        .and_then(|tab| tab.list.bounds_for_item(drag.row));
                    this.finish_editor(false, window, cx);
                    let Some(tab) = this.tab_mut(drag.tab) else {
                        return;
                    };
                    let Some(session) = tab.content.as_mut() else {
                        return;
                    };
                    if let Err(error) = session.confirm_all() {
                        this.error = Some(UiError::Session(error));
                        cx.notify();
                        return;
                    }
                    if session.document.is_read_only()
                        || drag.row >= session.document.blocks().len()
                    {
                        return;
                    }
                    let work = session.work_revision();
                    let Some(bounds) = row_bounds else {
                        return;
                    };
                    width = bounds.size.width;
                    lift = if matches!(
                        session.document.blocks()[drag.row].kind,
                        BlockKind::Assignment(_)
                    ) {
                        px(8.)
                    } else {
                        px(0.)
                    };
                    let heights = row_heights(tab);
                    let bases = tops(&heights);
                    let clocks: Vec<_> = bases
                        .iter()
                        .map(|top| {
                            let clock = TransitionState::default();
                            clock.sample(*top, Transition::new(MOVE_DURATION), window, cx);
                            clock
                        })
                        .collect();
                    let pointer = window.mouse_position();
                    let row_height = f32::from(bounds.size.height);
                    let start_offset = f32::from(pointer.y - bounds.top());
                    // A fast first move can start recognition past the source row.
                    let grab_y = if (0. ..=row_height).contains(&start_offset) {
                        start_offset
                    } else {
                        row_height / 2.
                    };
                    tab.reorder = Some(ReorderMotion {
                        work,
                        targets: bases.clone(),
                        bases,
                        heights,
                        clocks,
                        settles_at: Instant::now() + MOVE_DURATION,
                        gesture: Some(Gesture {
                            from: drag.row,
                            to: drag.row,
                            pointer,
                            grab_y,
                            last_tick: Instant::now(),
                        }),
                    });
                    valid = true;
                    cx.notify();
                });
                cx.stop_propagation();
                cx.new(|_| RowPreview {
                    owner: owner.clone(),
                    drag: drag.clone(),
                    width,
                    lift,
                    valid,
                })
            })
    }

    pub(super) fn move_drag(&mut self, event: &DragMoveEvent<RowDrag>, cx: &mut Context<Self>) {
        let drag = event.drag(cx);
        if let Some(motion) = self.tab_mut(drag.tab).and_then(|tab| tab.reorder.as_mut())
            && let Some(gesture) = &mut motion.gesture
        {
            gesture.pointer = event.event.position;
            cx.notify();
        }
    }

    pub(super) fn advance_row_drag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab_mut() else {
            return;
        };
        let Some(motion) = tab.reorder.as_mut() else {
            return;
        };
        if tab.content.as_ref().map(DocumentSession::work_revision) != Some(motion.work) {
            tab.reorder = None;
            return;
        }
        let Some(gesture) = motion.gesture.as_mut() else {
            if Instant::now() >= motion.settles_at {
                tab.reorder = None;
            }
            return;
        };
        if !cx.has_active_drag() {
            motion.cancel();
            return;
        }
        let bounds = tab.list.viewport_bounds();
        let pointer = gesture.pointer;
        if pointer.x < bounds.left() || pointer.x > bounds.right() {
            return;
        }
        let now = Instant::now();
        let elapsed = now
            .duration_since(gesture.last_tick)
            .as_secs_f32()
            .min(0.05);
        gesture.last_tick = now;
        // Continue scrolling on animation frames even when the pointer is still.
        let edge = 40.;
        let velocity = if pointer.y < bounds.top() + px(edge) {
            -((f32::from(bounds.top() + px(edge) - pointer.y) / edge).clamp(0., 1.)) * 700.
        } else if pointer.y > bounds.bottom() - px(edge) {
            (f32::from(pointer.y - bounds.bottom() + px(edge)) / edge).clamp(0., 1.) * 700.
        } else {
            0.
        };
        if velocity != 0. {
            tab.list.scroll_by(px(velocity * elapsed));
            window.request_animation_frame();
        }
        let first = tab.list.logical_scroll_top().item_ix;
        let Some(first_bounds) = tab.list.bounds_for_item(first) else {
            return;
        };
        let Some(base) = motion.bases.get(first) else {
            return;
        };
        let center = f32::from(pointer.y - first_bounds.top()) + base - gesture.grab_y
            + motion.heights[gesture.from] / 2.;
        let to = target_row(gesture.from, center, &motion.bases, &motion.heights);
        if to != gesture.to {
            gesture.to = to;
            motion.targets = targets(&motion.heights, gesture.from, to);
            for (clock, target) in motion.clocks.iter().zip(&motion.targets) {
                clock.sample(*target, Transition::new(MOVE_DURATION), window, cx);
            }
        }
    }

    pub(super) fn drop_row(&mut self, drag: &RowDrag, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_tab().map(|tab| tab.id) != Some(drag.tab) {
            return;
        }
        let Some(tab) = self.tab_mut(drag.tab) else {
            return;
        };
        let Some(mut motion) = tab.reorder.take() else {
            return;
        };
        let Some(mut gesture) = motion.gesture.take() else {
            tab.reorder = Some(motion);
            return;
        };
        // The mouse-up can arrive before the frame that consumes the last motion event.
        let first = tab.list.logical_scroll_top().item_ix;
        if let (Some(bounds), Some(base)) =
            (tab.list.bounds_for_item(first), motion.bases.get(first))
        {
            let center = f32::from(window.mouse_position().y - bounds.top()) + base
                - gesture.grab_y
                + motion.heights[gesture.from] / 2.;
            gesture.to = target_row(gesture.from, center, &motion.bases, &motion.heights);
        }
        let Some(session) = tab.content.as_mut() else {
            return;
        };
        let old_gaps: Vec<_> = (0..motion.heights.len())
            .map(|i| block_gap(session, i))
            .collect();
        match session.move_block(gesture.from, gesture.to, motion.work) {
            Ok(_) => {
                for (height, gap) in motion.heights.iter_mut().zip(old_gaps) {
                    *height -= gap;
                }
                move_item(&mut motion.heights, gesture.from, gesture.to);
                move_item(&mut motion.clocks, gesture.from, gesture.to);
                for (index, height) in motion.heights.iter_mut().enumerate() {
                    *height += block_gap(session, index);
                }
                motion.bases = tops(&motion.heights);
                motion.targets.clone_from(&motion.bases);
                motion.work = session.work_revision();
                motion.settles_at = Instant::now() + MOVE_DURATION;
                // The row render samples the new targets and schedules animation frames.
                // Sampling a running transition from this mouse-up handler panics in GPUI.
                tab.list.remeasure();
                tab.reorder = Some(motion);
                self.error = None;
                self.notice = None;
            }
            Err(error) => {
                motion.cancel();
                tab.reorder = Some(motion);
                self.show_session_error(error, window, cx);
            }
        }
        cx.notify();
    }

    pub(super) fn cancel_row_drag(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let mut cancelled = false;
        for tab in self.documents.values_mut() {
            if let Some(motion) = &mut tab.reorder
                && motion.gesture.is_some()
            {
                motion.cancel();
                cancelled = true;
            }
        }
        if cancelled {
            cx.stop_active_drag(window);
            cx.notify();
        }
        cancelled
    }
}

#[cfg(test)]
mod tests {
    use super::{targets, tops};

    #[test]
    fn unequal_height_rows_close_and_open_the_exact_dragged_space() {
        let heights = [30., 76., 18., 44.];
        assert_eq!(targets(&heights, 1, 3), vec![0., 92., 30., 48.]);
        assert_eq!(targets(&heights, 3, 0), vec![44., 74., 150., 0.]);
        assert_eq!(targets(&heights, 2, 2), tops(&heights));
    }
}
