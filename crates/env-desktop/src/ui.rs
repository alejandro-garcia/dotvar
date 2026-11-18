use crate::i18n::{self, tr};
use crate::{
    action_icons::{self, ActionIcon},
    app::*,
    config::ConfigError,
    design::{self, Asset},
    model,
    session::{DocumentSession, SessionError},
    settings::AppSettings,
    storage,
};
use env_core::{EditError, EntryId};
use gpui_kit::base::{Disableable, Tab as BaseTab, Tabs as BaseTabs};
use gpui_kit::component::{
    Sizable, TitleBar,
    button::{Button, ButtonCustomVariant, ButtonVariants},
    label::Label,
    menu::{DropdownMenu, PopupMenuItem},
    tooltip::Tooltip,
};
use gpui_kit::{prelude::FluentBuilder, *};
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashSet, VecDeque},
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex},
    time::Instant,
};

mod commands;
mod document;
mod editing;
mod interaction;
mod new_variable;
mod palette;
mod projects;
mod reorder;
mod tab_rename;
mod welcome;
use editing::{ActiveEditor, EditorTarget, RenameDialog};
use interaction::SearchBox;
use new_variable::NewVariableRow;
use palette::ProjectPalette;
use tab_rename::TabNameEditor;

use design::{CONTENT_BG, KEY_COLOR, SECONDARY, WINDOW_BG};

struct InteractionMetrics {
    path: PathBuf,
    pending: RefCell<Vec<(&'static str, Instant)>>,
}

impl InteractionMetrics {
    fn record(&self, action: &'static str) {
        self.pending.borrow_mut().push((action, Instant::now()));
    }

    fn marker(self: Rc<Self>) -> impl IntoElement {
        canvas(
            |_, _, _| (),
            move |_, (), _, cx| {
                let painted = Instant::now();
                let samples = std::mem::take(&mut *self.pending.borrow_mut());
                if samples.is_empty() {
                    return;
                }
                let path = self.path.clone();
                cx.background_executor()
                    .spawn(async move {
                        use std::io::Write;
                        if let Ok(mut output) = std::fs::OpenOptions::new().append(true).open(path)
                        {
                            for (action, started) in samples {
                                let micros = painted.duration_since(started).as_micros();
                                let _ = writeln!(output, "{action} {micros}");
                            }
                        }
                    })
                    .detach();
            },
        )
        .absolute()
        .size_full()
    }
}

#[cfg(test)]
mod tests {
    use super::{FileTab, ListOffset, PathBuf, px};

    #[test]
    fn changing_editor_registration_preserves_partial_row_scroll() {
        let tab = FileTab::new(1, PathBuf::from("service-icons.env"));
        tab.list.reset(80);
        let offset = ListOffset {
            item_ix: 20,
            offset_in_item: px(17.),
        };
        tab.list.scroll_to(offset);

        // Starting, finishing, and switching editors all invalidate a row.
        // In particular, splicing the first visible row would clear its 17px offset.
        for row in [20, 20, 24, 19, 20] {
            tab.set_row_focus(row, None);
            assert_eq!(tab.list.logical_scroll_top(), offset);
            assert_eq!(tab.list.item_count(), 80);
        }
    }
}

enum UiError {
    Message(&'static str),
    Storage(storage::StorageError),
    Session(SessionError),
}

impl UiError {
    fn message(&self) -> std::borrow::Cow<'static, str> {
        match self {
            Self::Message(key) => i18n::text(key),
            Self::Storage(error) => error.to_string().into(),
            Self::Session(error) => error.to_string().into(),
        }
    }
}

struct FileTab {
    id: u64,
    path: PathBuf,
    content: Option<DocumentSession>,
    error: Option<UiError>,
    storage_error: Option<storage::StorageError>,
    loading: bool,
    rename_destination: Option<PathBuf>,
    read_epoch: u64,
    revealed: bool,
    exceptions: HashSet<EntryId>,
    copied: BTreeMap<Option<EntryId>, std::time::Instant>,
    list: ListState,
    new_variable: Option<NewVariableRow>,
    layout_work: Option<(u64, u64)>,
    reorder: Option<reorder::ReorderMotion>,
    query: String,
    matched: Option<EntryId>,
}

impl FileTab {
    fn sync_document_list(&mut self) {
        let draft_row = self.content.as_ref().and_then(|session| {
            session
                .new_variable()
                .map(|_| session.document.blocks().len())
        });
        let had_draft = self.new_variable.is_some();
        let count = self.content.as_ref().map_or(0, |session| {
            session.document.blocks().len()
                + usize::from(!session.document.is_read_only())
                + usize::from(draft_row.is_some())
        });
        if self.list.item_count() != count {
            // Estimate off-screen row heights for the scrollbar.
            let offset = self.list.logical_scroll_top();
            self.list
                .reset_with_uniform_height(count, px(design::FIELD_HEIGHT + design::BLOCK_GAP));
            if had_draft {
                self.list.scroll_to(offset);
            }
            if let Some(row) = draft_row
                && let Some(view) = &self.new_variable
            {
                self.set_row_focus(row, Some(view.focus.clone()));
            }
            self.layout_work = None;
        } else if draft_row.is_none() && had_draft && count >= 2 {
            // Confirming replaces the draft slot with a real assignment.
            self.set_row_focus(count - 2, None);
        }
        if draft_row.is_none() {
            self.new_variable = None;
        }
    }

    fn set_row_focus(&self, row: usize, focus: Option<FocusHandle>) {
        // Registering an editor invalidates the row's measurement and resets
        // the offset when it is the first visible row. Keep the viewport fixed.
        let offset = self.list.logical_scroll_top();
        self.list.splice_focusable(row..row + 1, [focus]);
        self.list.scroll_to(offset);
    }

    fn new(id: u64, path: PathBuf) -> Self {
        Self {
            id,
            path,
            content: None,
            error: None,
            storage_error: None,
            loading: false,
            rename_destination: None,
            read_epoch: 0,
            revealed: false,
            exceptions: HashSet::new(),
            copied: BTreeMap::new(),
            list: ListState::new(0, ListAlignment::Top, px(200.)),
            new_variable: None,
            layout_work: None,
            reorder: None,
            query: String::new(),
            matched: None,
        }
    }
    fn dirty(&self) -> bool {
        self.content.as_ref().is_some_and(DocumentSession::is_dirty)
    }
    fn saving(&self) -> bool {
        self.content
            .as_ref()
            .is_some_and(DocumentSession::is_saving)
    }
}

#[derive(Clone)]
struct TabReference {
    document: u64,
    path: PathBuf,
}

struct Project {
    id: u64,
    root: PathBuf,
    tabs: Vec<TabReference>,
    active: usize,
    available: Vec<PathBuf>,
    missing: bool,
    scroll: ScrollHandle,
}

#[derive(Clone)]
enum CloseTarget {
    Window,
    Tab {
        project: u64,
        document: u64,
        path: PathBuf,
    },
    Project(u64),
}

pub struct AppWindow {
    projects: Vec<Project>,
    documents: BTreeMap<u64, FileTab>,
    active_project: usize,
    next_tab: u64,
    next_project: u64,
    error: Option<UiError>,
    opening: bool,
    open_request: u64,
    pending_external_files: VecDeque<PathBuf>,
    focus: FocusHandle,
    editor: Option<ActiveEditor>,
    dialog: Option<RenameDialog>,
    tab_name_editor: Option<TabNameEditor>,
    creating_file: Option<PathBuf>,
    search: Option<SearchBox>,
    palette: Option<ProjectPalette>,
    notice: Option<&'static str>,
    needs_root_focus: bool,
    project_switcher_hovered: bool,
    hovered_tab: Option<(u64, usize)>,
    hovered_close_tab: Option<(u64, u64)>,
    tab_scroll_anchor: Option<(u64, usize, Pixels)>,
    closing: bool,
    close_target: Option<CloseTarget>,
    close_queue: Vec<u64>,
    preferences: Entity<AppSettings>,
    config: Option<crate::config::ConfigStore>,
    config_error: Option<ConfigError>,
    config_saving: bool,
    config_pending: Option<crate::config::Catalog>,
    config_closing: bool,
    config_window: Option<AnyWindowHandle>,
    config_close_work: Vec<(u64, Option<(u64, u64)>)>,
    watcher: Option<Arc<Mutex<crate::watch::WatchService>>>,
    watch_roots: Vec<PathBuf>,
    watch_reconciling: bool,
    watch_pending_roots: Option<Vec<PathBuf>>,
    watch_scanning: bool,
    watch_pending: bool,
    startup_metric: Option<(PathBuf, std::time::Instant)>,
    interaction_metrics: Option<Rc<InteractionMetrics>>,
}

impl AppWindow {
    pub(crate) fn refresh_language(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(search) = &self.search {
            search.state.update(cx, |state, cx| {
                state.set_placeholder(tr!("variable.name"), window, cx);
            });
        }
        if let Some(dialog) = &self.dialog {
            dialog.refresh_language(window, cx);
        }
        for row in self
            .documents
            .values()
            .filter_map(|tab| tab.new_variable.as_ref())
        {
            row.name.update(cx, |state, cx| {
                state.set_placeholder(tr!("variable.name_example"), window, cx);
            });
        }
        cx.notify();
    }

    pub(crate) fn new(
        initial: Option<PathBuf>,
        started: std::time::Instant,
        preferences: Entity<AppSettings>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let weak = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            let _ = weak.update(cx, |this, cx| this.request_close(window, cx));
            false
        });
        let mut this = Self {
            projects: Vec::new(),
            documents: BTreeMap::new(),
            active_project: usize::MAX,
            next_tab: 0,
            next_project: 0,
            error: None,
            opening: true,
            open_request: 0,
            pending_external_files: VecDeque::new(),
            focus: cx.focus_handle(),
            editor: None,
            dialog: None,
            tab_name_editor: None,
            creating_file: None,
            search: None,
            palette: None,
            notice: None,
            needs_root_focus: false,
            project_switcher_hovered: false,
            hovered_tab: None,
            hovered_close_tab: None,
            tab_scroll_anchor: None,
            closing: false,
            close_target: None,
            close_queue: Vec::new(),
            preferences,
            config: None,
            config_error: None,
            config_saving: false,
            config_pending: None,
            config_closing: false,
            config_window: None,
            config_close_work: Vec::new(),
            watcher: None,
            watch_roots: Vec::new(),
            watch_reconciling: false,
            watch_pending_roots: None,
            watch_scanning: false,
            watch_pending: false,
            startup_metric: crate::config::app_env("STARTUP_METRIC").map(|p| (p.into(), started)),
            interaction_metrics: crate::config::app_env("QA_INTERACTION_METRICS").map(|path| {
                Rc::new(InteractionMetrics {
                    path: path.into(),
                    pending: RefCell::default(),
                })
            }),
        };
        this.focus.focus(window, cx);
        this.restore(initial, cx);
        this
    }
    fn make_tab(&mut self, path: PathBuf) -> u64 {
        if let Some(tab) = self.documents.values().find(|t| t.path == path) {
            return tab.id;
        }
        self.next_tab += 1;
        let id = self.next_tab;
        self.documents.insert(id, FileTab::new(id, path));
        id
    }
    fn location(&self, id: u64) -> Option<(usize, usize)> {
        let preferred = self
            .projects
            .get(self.active_project)
            .and_then(|p| p.tabs.iter().position(|t| t.document == id))
            .map(|t| (self.active_project, t));
        preferred.or_else(|| {
            self.projects.iter().enumerate().find_map(|(p, project)| {
                project
                    .tabs
                    .iter()
                    .position(|t| t.document == id)
                    .map(|t| (p, t))
            })
        })
    }
    fn tab(&self, id: u64) -> Option<&FileTab> {
        self.documents.get(&id)
    }
    fn tab_mut(&mut self, id: u64) -> Option<&mut FileTab> {
        self.documents.get_mut(&id)
    }
    fn active_tab(&self) -> Option<&FileTab> {
        if self.new_file_draft().is_some() {
            return None;
        }
        let p = self.projects.get(self.active_project)?;
        self.tab(p.tabs.get(p.active)?.document)
    }
    fn active_tab_mut(&mut self) -> Option<&mut FileTab> {
        if self.new_file_draft().is_some() {
            return None;
        }
        let p = self.projects.get(self.active_project)?;
        let id = p.tabs.get(p.active)?.document;
        self.tab_mut(id)
    }
    fn select_tab(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.search = None;
        self.finish_editor(false, window, cx);
        if let Some((p, t)) = self.location(id) {
            self.active_project = p;
            self.projects[p].active = t;
        }
        self.load_active(cx);
        self.focus.focus(window, cx);
        self.persist(cx);
        self.refresh_watches(cx);
        cx.notify();
    }

    fn record_interaction(&self, action: &'static str) {
        if let Some(metrics) = &self.interaction_metrics {
            metrics.record(action);
        }
    }

    fn load_active(&mut self, cx: &mut Context<Self>) {
        let Some(reference) = self
            .projects
            .get(self.active_project)
            .and_then(|p| p.tabs.get(p.active))
        else {
            return;
        };
        let Some(tab) = self.tab_mut(reference.document) else {
            return;
        };
        if tab.content.is_some() || tab.loading {
            return;
        }
        tab.loading = true;
        tab.error = None;
        tab.read_epoch += 1;
        let (id, epoch, path) = (tab.id, tab.read_epoch, tab.path.clone());
        let job = cx
            .background_executor()
            .spawn(async move { storage::open(path) });
        cx.spawn(async move |this, cx| {
            let result = job.await;
            let _ = this.update(cx, |this, cx| {
                if !this.tab(id).is_some_and(|t| t.read_epoch == epoch) {
                    return;
                }
                match result {
                    Ok(baseline) => this.attach_loaded(id, baseline),
                    Err(error) => {
                        if let Some(tab) = this.tab_mut(id) {
                            tab.loading = false;
                            tab.error = Some(UiError::Storage(error));
                        }
                    }
                }
                this.refresh_watches(cx);
                cx.notify();
            });
        })
        .detach();
    }
    fn attach_loaded(&mut self, id: u64, baseline: storage::FileBaseline) {
        let existing = self
            .documents
            .values()
            .find(|t| {
                t.id != id
                    && t.content.as_ref().is_some_and(|s| {
                        s.baseline.identity() == baseline.identity()
                            || s.baseline.resolved_path == baseline.resolved_path
                    })
            })
            .map(|t| t.id);
        if let Some(existing) = existing {
            for p in &mut self.projects {
                for reference in &mut p.tabs {
                    if reference.document == id {
                        reference.document = existing;
                    }
                }
            }
            self.documents.remove(&id);
        } else if let Some(tab) = self.tab_mut(id) {
            tab.loading = false;
            tab.content = Some(DocumentSession::new(baseline));
            tab.storage_error = None;
            tab.error = None;
        }
    }
}

impl Render for AppWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.needs_root_focus && self.palette.is_none() {
            self.focus.focus(window, cx);
            self.needs_root_focus = false;
        }
        if self
            .search
            .as_ref()
            .is_some_and(|s| self.active_tab().map(|t| t.id) != Some(s.document))
        {
            self.search = None;
        }
        if self.tab_name_editor.as_ref().is_some_and(|editor| {
            self.projects
                .get(self.active_project)
                .is_none_or(|project| {
                    let active_file = project
                        .tabs
                        .get(project.active)
                        .is_some_and(|tab| editor.matches(project.id, &tab.path));
                    !active_file && (project.missing || !editor.creates_in(project.id))
                })
        }) {
            self.tab_name_editor = None;
        }
        let creating_tab = self.new_file_draft().is_some();
        let tab_layout = self.projects.get(self.active_project).map(|p| {
            (
                p.id,
                if creating_tab { p.tabs.len() } else { p.active },
                window.viewport_size().width,
            )
        });
        if tab_layout != self.tab_scroll_anchor {
            if let Some((_, index, _)) = tab_layout {
                let p = &self.projects[self.active_project];
                p.scroll.scroll_to_item(index);
                // The scroll handle learns its new viewport in prepaint. Repeat
                // this one reveal after layout, so resize uses the current bounds.
                let scroll = p.scroll.clone();
                window.on_next_frame(move |window, _| {
                    scroll.scroll_to_item(index);
                    window.refresh();
                });
            }
            self.tab_scroll_anchor = tab_layout;
        }
        let ready = !self.opening && self.active_tab().is_none_or(|tab| !tab.loading);
        let project = self.projects.get(self.active_project);
        let welcome = project.is_none();
        let missing_project = project.is_some_and(|p| p.missing);
        let has_document = self.active_tab().and_then(|t| t.content.as_ref()).is_some();
        let show_recovery = missing_project && !has_document;
        // Keep buffered work accessible even when the active tab couldn't be loaded.
        let show_tabs = project.is_some_and(|p| {
            !show_recovery
                || p.tabs.iter().any(|reference| {
                    self.tab(reference.document)
                        .is_some_and(|tab| tab.content.is_some())
                })
        });
        if ready && let Some((path, started)) = self.startup_metric.take() {
            window.on_next_frame(move |_, cx| {
                let micros = started.elapsed().as_micros();
                cx.background_executor()
                    .spawn(async move {
                        let _ = std::fs::write(path, format!("{micros}\n"));
                    })
                    .detach();
            });
        }
        let projects: Vec<_> = self
            .projects
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let name = p
                    .root
                    .file_name()
                    .unwrap_or(p.root.as_os_str())
                    .to_string_lossy();
                let label = if p.missing {
                    tr!("project.unavailable", name = name)
                } else {
                    name
                };
                (i, label.into_owned(), p.root.display().to_string())
            })
            .collect();
        let weak = cx.weak_entity();
        let name = self
            .projects
            .get(self.active_project)
            .and_then(|p| p.root.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| tr!("project.choose").into());
        let title = TitleBar::new()
            .h(px(design::TITLE_HEIGHT))
            .pl_0()
            .bg(rgb(WINDOW_BG))
            .border_0()
            .when(!welcome, |this| {
                this.child(
                    div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .id("project-switcher-group")
                                .flex()
                                .items_center()
                                .gap(px(10.))
                                .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                                    this.project_switcher_hovered = *hovered;
                                    cx.notify();
                                }))
                                .child(
                                    Button::new("project-switcher")
                                        .custom(
                                            ButtonCustomVariant::new(cx)
                                                .hover(rgb(design::SELECTOR_BG).into())
                                                .active(rgb(design::SELECTOR_BG).into()),
                                        )
                                        .small()
                                        .accessibility_label(tr!("project.label", name = name))
                                        .h(px(28.))
                                        .px(px(8.))
                                        .py(px(4.))
                                        .gap(px(10.))
                                        .border_0()
                                        .rounded(px(8.))
                                        .bg(rgb(if self.project_switcher_hovered {
                                            design::SELECTOR_BG
                                        } else {
                                            WINDOW_BG
                                        }))
                                        .child(design::asset(if self.project_switcher_hovered {
                                            Asset::WelcomeFolder
                                        } else {
                                            Asset::Folder
                                        }))
                                        .child(
                                            Label::new(name)
                                                .text_size(px(14.))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(rgb(
                                                    if self.project_switcher_hovered {
                                                        design::TAB_TEXT_ACTIVE
                                                    } else {
                                                        SECONDARY
                                                    },
                                                )),
                                        )
                                        .child(design::asset(if self.project_switcher_hovered {
                                            Asset::ChevronHover
                                        } else {
                                            Asset::Chevron
                                        }))
                                        .dropdown_menu(move |mut menu, _, _| {
                                            for (index, name, path) in &projects {
                                                let index = *index;
                                                let name = name.clone();
                                                let path = path.clone();
                                                let weak = weak.clone();
                                                menu = menu.item(
                                                    PopupMenuItem::element(move |_, _| {
                                                        let path = path.clone();
                                                        div()
                                                            .id(("project-menu-item", index))
                                                            .flex()
                                                            .flex_1()
                                                            .min_w_0()
                                                            .mx_neg_2()
                                                            .px_2()
                                                            .min_h(rems(1.625))
                                                            .items_center()
                                                            .gap(px(8.))
                                                            .child(design::asset(Asset::Folder))
                                                            .child(name.clone())
                                                            .tooltip_show_delay(
                                                                std::time::Duration::from_millis(
                                                                    1500,
                                                                ),
                                                            )
                                                            .tooltip(move |window, cx| {
                                                                Tooltip::new(path.clone())
                                                                    .build(window, cx)
                                                            })
                                                    })
                                                    .on_click(move |_, window, cx| {
                                                        let _ = weak.update(cx, |this, cx| {
                                                            this.select_project(index, window, cx);
                                                        });
                                                    }),
                                                );
                                            }
                                            menu.separator()
                                                .item(
                                                    PopupMenuItem::new(tr!("project.add_action"))
                                                        .action(Box::new(OpenProject)),
                                                )
                                                .item(
                                                    PopupMenuItem::new(tr!(
                                                        "project.locate_action"
                                                    ))
                                                    .action(Box::new(LocateProject)),
                                                )
                                                .item(
                                                    PopupMenuItem::new(tr!("project.remove"))
                                                        .action(Box::new(RemoveProject)),
                                                )
                                        })
                                        .when_some(
                                            self.interaction_metrics.clone(),
                                            |menu, metrics| {
                                                menu.on_open_change(move |open, _, _| {
                                                    if *open {
                                                        metrics.record("project_menu");
                                                    }
                                                })
                                            },
                                        ),
                                )
                                // Keep the hit area, including the gap, stable while hidden.
                                .child(div().w(px(25.0004)).h(px(25.)).flex_shrink_0().when(
                                    self.project_switcher_hovered && !missing_project,
                                    |this| {
                                        this.child(
                                            Button::new("open-project-folder")
                                                .group("open-project-folder")
                                                .custom(
                                                    ButtonCustomVariant::new(cx)
                                                        .hover(rgb(design::SELECTOR_BG).into())
                                                        .active(rgb(design::SELECTOR_BG).into()),
                                                )
                                                .w(px(25.0004))
                                                .h(px(25.))
                                                .p_0()
                                                .border_0()
                                                .rounded(px(8.))
                                                .bg(transparent_black())
                                                .accessibility_label(tr!("project.open_folder"))
                                                .tooltip(tr!("project.open_in_finder"))
                                                .child(
                                                    svg()
                                                        .path(Asset::OpenFolder.path())
                                                        .w(px(25.0004))
                                                        .h(px(25.))
                                                        .flex_shrink_0()
                                                        .text_color(rgb(design::QUOTE_COLOR))
                                                        .group_hover(
                                                            "open-project-folder",
                                                            |this| {
                                                                this.text_color(rgb(
                                                                    design::TAB_TEXT_ACTIVE,
                                                                ))
                                                            },
                                                        ),
                                                )
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    cx.stop_propagation();
                                                    if let Some(project) =
                                                        this.projects.get(this.active_project)
                                                    {
                                                        cx.open_with_system(&project.root);
                                                    }
                                                })),
                                        )
                                    },
                                )),
                        ),
                )
            });
        let tabs = self.projects.get(self.active_project).map(|p| {
            let project = p.id;
            let count = p.tabs.len() + usize::from(creating_tab);
            BaseTabs::new("files")
                .flex()
                .gap(px(5.))
                .h(px(design::TAB_HEIGHT))
                .overflow_x_scroll()
                .track_scroll(&p.scroll)
                .children(
                    p.tabs
                        .iter()
                        .enumerate()
                        .filter_map(|(index, reference)| {
                            self.tab(reference.document)
                                .map(|tab| (index, reference, tab))
                        })
                        .map(|(index, reference, tab)| {
                            let selected = !creating_tab && index == p.active;
                            let hovered = self.hovered_tab == Some((project, index));
                            let show_close = selected || hovered;
                            let id = tab.id;
                            let close_hovered = self.hovered_close_tab == Some((project, id));
                            let close_path = reference.path.clone();
                            let renaming = self
                                .tab_name_editor
                                .as_ref()
                                .filter(|rename| rename.matches(project, &reference.path));
                            BaseTab::new(("file-tab", index))
                                .selected(selected)
                                .set_position(index + 1, count)
                                .accessibility_label(if tab.dirty() {
                                    tr!("tab.unsaved", path = reference.path.display()).into_owned()
                                } else {
                                    reference.path.display().to_string()
                                })
                                .flex_shrink_0()
                                .h(px(design::TAB_HEIGHT))
                                .pl(px(10.))
                                .pr(px(if selected { 10. } else { 15. }))
                                .gap(px(8.))
                                .rounded_tl(px(6.))
                                .rounded_tr(px(6.))
                                .bg(rgb(if selected { CONTENT_BG } else { WINDOW_BG }))
                                .text_color(rgb(if selected {
                                    design::TAB_TEXT_ACTIVE
                                } else {
                                    design::TAB_TEXT_INACTIVE
                                }))
                                .font_family(design::UI_FONT)
                                .text_size(px(16.))
                                .child(design::asset(design::file_icon(&reference.path, selected)))
                                .child(if let Some(rename) = renaming {
                                    rename.input(tab.rename_destination.is_some())
                                } else {
                                    Self::tab_name(project, index, &reference.path, selected, cx)
                                })
                                .child(
                                    div()
                                        .id(("close-tab-slot", index))
                                        .w(px(18.))
                                        .h(px(18.))
                                        .flex_shrink_0()
                                        .on_hover(cx.listener(
                                            move |this, hovered: &bool, _, cx| {
                                                if *hovered {
                                                    this.hovered_close_tab = Some((project, id));
                                                } else if this.hovered_close_tab
                                                    == Some((project, id))
                                                {
                                                    this.hovered_close_tab = None;
                                                }
                                                cx.notify();
                                            },
                                        ))
                                        .when(show_close, |this| {
                                            this.child(
                                                Button::new(("close-tab", index))
                                                    .ghost()
                                                    .w(px(18.))
                                                    .h(px(18.))
                                                    .p_0()
                                                    .border_0()
                                                    .rounded(px(4.))
                                                    .bg(transparent_black())
                                                    .accessibility_label(if tab.dirty() {
                                                        tr!("tab.close_unsaved")
                                                    } else {
                                                        tr!("tab.close")
                                                    })
                                                    .tooltip(tr!("tab.close_shortcut"))
                                                    .child(design::asset(if close_hovered {
                                                        Asset::CloseHover
                                                    } else if tab.dirty() && !hovered {
                                                        Asset::Dirty
                                                    } else {
                                                        Asset::Close
                                                    }))
                                                    .on_click(cx.listener(
                                                        move |this, _, window, cx| {
                                                            cx.stop_propagation();
                                                            this.hovered_close_tab = None;
                                                            this.request_close_target(
                                                                CloseTarget::Tab {
                                                                    project,
                                                                    document: id,
                                                                    path: close_path.clone(),
                                                                },
                                                                window,
                                                                cx,
                                                            );
                                                        },
                                                    )),
                                            )
                                        })
                                        .when(!show_close && tab.dirty(), |this| {
                                            this.child(design::asset(Asset::Dirty))
                                        }),
                                )
                                .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                                    if *hovered {
                                        this.hovered_tab = Some((project, index));
                                    } else if this.hovered_tab == Some((project, index)) {
                                        this.hovered_tab = None;
                                    }
                                    cx.notify();
                                }))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.cancel_tab_name_edit(window, cx);
                                    this.finish_editor(false, window, cx);
                                    this.search = None;
                                    this.notice = None;
                                    this.projects[this.active_project].active = index;
                                    this.load_active(cx);
                                    this.persist(cx);
                                    this.refresh_watches(cx);
                                    cx.notify();
                                }))
                        }),
                )
                .when_some(self.new_file_draft(), |tabs, draft| {
                    tabs.child(
                        BaseTab::new("new-file-tab")
                            .selected(true)
                            .set_position(count, count)
                            .accessibility_label(tr!("file.create_in_project"))
                            .flex_shrink_0()
                            .h(px(design::TAB_HEIGHT))
                            .px(px(10.))
                            .gap(px(8.))
                            .rounded_tl(px(6.))
                            .rounded_tr(px(6.))
                            .bg(rgb(CONTENT_BG))
                            .child(design::asset(Asset::FileNeutral))
                            .child(draft.input(self.creating_file.is_some()))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.focus_new_file_name(window, cx)
                            })),
                    )
                })
                .when(!p.missing, |tabs| tabs.child(self.add_file_button(cx)))
        });
        let recovery_view = project
            .filter(|_| show_recovery)
            .map(|project| self.missing_project_view(project, cx));
        let show_editor = !welcome && !show_recovery;
        let document = (show_editor && !creating_tab).then(|| self.document_view(window, cx));
        let file_menu = self.files_button(cx);
        let revealed = self.active_tab().is_some_and(|t| t.revealed);
        let tab_id = self.active_tab().map_or(0, |t| t.id);
        let copied = self
            .active_tab()
            .is_some_and(|t| t.copied.contains_key(&None));
        let search = self.search_view(cx);
        let toolbar = div()
            .h(px(design::CONTENT_TOP))
            .flex_shrink_0()
            .flex()
            .items_start()
            .justify_end()
            .pt(px(5.))
            .pr(px(10.))
            .gap(px(10.))
            .when_some(self.notice, |this, notice| {
                this.child(
                    div().flex_1().pl(px(15.)).pt_2().child(
                        Label::new(notice)
                            .text_color(rgb(SECONDARY))
                            .text_size(px(12.)),
                    ),
                )
            })
            .child(
                action_icons::button("copy-document", cx)
                    .accessibility_label(tr!("editor.copy_full_document"))
                    .tooltip(tr!("editor.copy_document_shortcut"))
                    .child(action_icons::swap(
                        format!("copy-document-{tab_id}"),
                        ActionIcon::Duplicate,
                        ActionIcon::Check,
                        copied,
                        window,
                        cx,
                    ))
                    .on_click(cx.listener(|this, _, window, cx| this.copy_document(window, cx))),
            )
            .child(
                action_icons::button("reveal-all", cx)
                    .accessibility_label(if revealed {
                        tr!("editor.hide_all_values")
                    } else {
                        tr!("editor.show_all_values")
                    })
                    .tooltip(if revealed {
                        tr!("editor.hide_all_values")
                    } else {
                        tr!("editor.show_all_values")
                    })
                    .child(action_icons::swap(
                        format!("reveal-document-{tab_id}"),
                        ActionIcon::Eye,
                        ActionIcon::EyeSlash,
                        revealed,
                        window,
                        cx,
                    ))
                    .on_click(cx.listener(|this, _, window, cx| this.toggle_all(window, cx))),
            );
        let first_tab_selected = self
            .projects
            .get(self.active_project)
            .is_some_and(|project| {
                if creating_tab {
                    project.tabs.is_empty()
                } else {
                    project.active == 0 && !project.tabs.is_empty()
                }
            });
        let content = div()
            .flex_1()
            .min_h_0()
            .mx(px(design::CONTENT_INSET))
            .mb(px(3.))
            .flex()
            .flex_col()
            .bg(rgb(CONTENT_BG))
            .rounded(px(12.))
            .when(first_tab_selected, |this| this.rounded_tl(px(0.)))
            .when_some(search, |this, search| this.child(search))
            .when(has_document, |this| this.child(toolbar))
            .when_some(document, |this, document| this.child(document));
        let dialog = self.dialog_view(cx);
        let palette = self.palette_view(cx);
        let welcome_view = welcome.then(|| self.welcome_view(cx));
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(WINDOW_BG))
            .text_color(rgb(0xe7e5e4))
            .font_family(design::UI_FONT)
            .track_focus(&self.focus)
            .on_drag_move(|event: &DragMoveEvent<ExternalPaths>, window, cx| {
                // GPUI's file drag events leave keyboard modality enabled,
                // which disables the hover hit test used by on_drop.
                if window.last_input_was_keyboard() {
                    let mut motion = event.event.clone();
                    motion.pressed_button = None;
                    window.defer(cx, move |window, cx| {
                        window.dispatch_event(PlatformInput::MouseMove(motion), cx);
                    });
                }
            })
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                this.open_external_files(paths.paths().iter().cloned(), window, cx);
            }))
            .capture_action(
                cx.listener(|this, _: &gpui_kit::base::actions::Cancel, window, cx| {
                    if this.palette.is_some() {
                        this.close_palette(window, cx);
                    } else {
                        cx.propagate();
                    }
                }),
            )
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" && this.cancel_row_drag(window, cx) {
                    cx.stop_propagation();
                } else if this.palette.is_some() {
                    this.palette_key(event, window, cx);
                } else if !this.tab_name_key(event, window, cx) {
                    this.editor_key(event, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleCommandPalette, window, cx| {
                this.toggle_palette(window, cx)
            }))
            .on_action(cx.listener(|this, _: &OpenFile, window, cx| this.choose(false, window, cx)))
            .on_action(
                cx.listener(|this, _: &OpenProject, window, cx| this.choose(true, window, cx)),
            )
            .on_action(cx.listener(|this, _: &NewFile, window, cx| this.create_file(window, cx)))
            .on_action(
                cx.listener(|this, _: &Save, window, cx| this.save_active(false, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &SaveAs, window, cx| this.save_active(true, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &UndoDocument, window, cx| this.history(false, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &RedoDocument, window, cx| this.history(true, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &AddVariable, window, cx| {
                    this.begin_new_variable(window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &CloseWindow, window, cx| this.request_close(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &CloseTab, window, cx| this.close_active_tab(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &RemoveProject, window, cx| this.remove_project(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &LocateProject, window, cx| this.locate_project(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &MoveTabLeft, window, cx| this.move_tab(false, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &MoveTabRight, window, cx| this.move_tab(true, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ResetCatalog, window, cx| this.reset_catalog(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &CopyDocument, window, cx| this.copy_document(window, cx)),
            )
            .on_action(cx.listener(|this, _: &FindKey, window, cx| this.open_search(window, cx)))
            .on_action(cx.listener(|this, _: &FindNext, _, cx| this.find_match(false, cx)))
            .on_action(cx.listener(|this, _: &FindPrevious, _, cx| this.find_match(true, cx)))
            .on_action(
                cx.listener(|this, _: &NextTab, window, cx| this.cycle_tab(false, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &PreviousTab, window, cx| this.cycle_tab(true, window, cx)),
            )
            .on_action(cx.listener(|_, _: &QaResize, window, cx| {
                if crate::config::app_env("QA_FLOATING").as_deref()
                    == Some(std::ffi::OsStr::new("1"))
                {
                    let next = if window.viewport_size().width > px(1000.) {
                        size(px(900.), px(600.))
                    } else {
                        size(px(1280.), px(832.))
                    };
                    window.resize(next);
                    cx.notify();
                }
            }))
            .on_action(
                cx.listener(|this, _: &NextValue, window, cx| {
                    this.navigate_field(false, window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &PreviousValue, window, cx| {
                this.navigate_field(true, window, cx)
            }))
            .child(title)
            .when(show_tabs, |this| {
                this.child(
                    div()
                        .h(px(design::TAB_HEIGHT))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .px(px(design::CONTENT_INSET))
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .when_some(tabs, |this, tabs| this.child(tabs)),
                        )
                        .when(show_editor, |this| this.child(file_menu)),
                )
            })
            .when_some(
                self.tab_name_editor
                    .as_ref()
                    .and_then(|rename| rename.error())
                    .or(self.error.as_ref())
                    .map(UiError::message),
                |this, error| {
                    this.child(
                        div()
                            .px_6()
                            .py_2()
                            .child(Label::new(error).text_color(rgb(0xfda4af))),
                    )
                },
            )
            .when_some(self.config_error, |this, error| {
                this.child(
                    div()
                        .px_6()
                        .flex()
                        .gap_3()
                        .child(Label::new(error.message()).text_color(rgb(0xfda4af)))
                        .child(
                            Button::new("reset-catalog")
                                .label(tr!("catalog.reset_action"))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.reset_catalog(window, cx)
                                })),
                        ),
                )
            })
            .when(self.opening && show_editor, |this| {
                this.child(div().px_6().child(Label::new(tr!("project.opening"))))
            })
            .when(missing_project && has_document, |this| {
                this.child(self.missing_project_notice(cx))
            })
            .when(show_editor, |this| this.child(content))
            .when_some(recovery_view, |this, recovery| this.child(recovery))
            .when_some(welcome_view, |this, welcome| this.child(welcome))
            .when_some(dialog, |this, dialog| this.child(dialog))
            .when_some(palette, |this, palette| this.child(palette))
            .when_some(self.interaction_metrics.clone(), |this, metrics| {
                this.child(metrics.marker())
            })
    }
}
