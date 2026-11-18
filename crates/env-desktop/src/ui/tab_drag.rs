use super::*;

pub(super) struct TabDrag {
    project: u64,
    path: PathBuf,
}

struct TabPreview {
    name: SharedString,
    scroll: ScrollHandle,
}

impl Render for TabPreview {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        if edge_scroll_offset(&self.scroll, window).is_some() {
            let scroll = self.scroll.clone();
            window.on_next_frame(move |window, cx| {
                if cx.has_active_drag()
                    && let Some(offset) = edge_scroll_offset(&scroll, window)
                {
                    scroll.set_offset(offset);
                    window.refresh();
                }
            });
        }

        div()
            .flex()
            .items_center()
            .h(px(design::TAB_HEIGHT))
            .max_w(px(250.))
            .px(px(10.))
            .rounded(px(6.))
            .bg(rgb(design::SELECTOR_BG))
            .text_color(rgb(design::TAB_TEXT_ACTIVE))
            .font_family(design::UI_FONT)
            .text_size(px(16.))
            .shadow_lg()
            .child(Label::new(self.name.clone()).truncate())
    }
}

fn edge_scroll_offset(scroll: &ScrollHandle, window: &Window) -> Option<Point<Pixels>> {
    let pointer = window.mouse_position();
    let bounds = scroll.bounds();
    if !bounds.contains(&pointer) {
        return None;
    }
    let offset = scroll.offset();
    let x = if pointer.x < bounds.left() + px(24.) {
        (offset.x + px(8.)).min(px(0.))
    } else if pointer.x > bounds.right() - px(24.) {
        (offset.x - px(8.)).max(-scroll.max_offset().x)
    } else {
        return None;
    };
    (x != offset.x).then_some(point(x, offset.y))
}

pub(super) fn draggable_tab(tab: BaseTab, project: &Project, index: usize) -> BaseTab {
    let path = &project.tabs[index].path;
    let name = SharedString::from(path.file_name().unwrap_or_default().to_string_lossy());
    let scroll = project.scroll.clone();
    let project_id = project.id;
    let target = path.clone();
    tab.on_drag(
        TabDrag {
            project: project_id,
            path: path.clone(),
        },
        move |_, _, _, cx| {
            cx.new(|_| TabPreview {
                name: name.clone(),
                scroll: scroll.clone(),
            })
        },
    )
    .drag_over::<TabDrag>(move |style, drag, _, _| {
        if drag.project == project_id && drag.path != target {
            style.bg(rgb(design::SELECTOR_BG))
        } else {
            style
        }
    })
}

impl Project {
    pub(super) fn reorder_tab(&mut self, from: usize, to: usize) {
        let active = self.tabs[self.active].path.clone();
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        self.active = self.tabs.iter().position(|tab| tab.path == active).unwrap();
    }
}

impl AppWindow {
    pub(super) fn drop_tab(&mut self, drag: &TabDrag, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self
            .projects
            .get_mut(self.active_project)
            .filter(|p| p.id == drag.project)
        else {
            return;
        };
        let Some(from) = project.tabs.iter().position(|tab| tab.path == drag.path) else {
            return;
        };
        let x = window.mouse_position().x - project.scroll.offset().x;
        let to = (0..project.tabs.len())
            .find(|&index| {
                project
                    .scroll
                    .bounds_for_item(index)
                    .is_some_and(|bounds| x <= bounds.right() + px(2.5))
            })
            .unwrap_or(project.tabs.len() - 1);
        if from == to {
            return;
        }
        project.reorder_tab(from, to);
        project.scroll.scroll_to_item(to);
        self.tab_scroll_anchor = Some((project.id, project.active, window.viewport_size().width));
        self.hovered_tab = None;
        self.hovered_close_tab = None;
        self.persist(cx);
        cx.notify();
    }
}
