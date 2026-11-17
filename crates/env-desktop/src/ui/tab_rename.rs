use super::*;
use gpui_kit::component::input::{Input, InputState};
use std::path::Path;

const DEFAULT_FILENAME: &str = ".env";

pub(super) struct TabNameEditor {
    project: u64,
    path: Option<PathBuf>,
    name: Entity<InputState>,
    error: Option<UiError>,
    _blur: Subscription,
}

impl TabNameEditor {
    pub(super) fn matches(&self, project: u64, path: &Path) -> bool {
        self.project == project && self.path.as_deref() == Some(path)
    }

    pub(super) fn creates_in(&self, project: u64) -> bool {
        self.project == project && self.path.is_none()
    }

    pub(super) fn input(&self, pending: bool) -> AnyElement {
        div()
            .id("tab-rename")
            .w(px(185.))
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                Input::new(&self.name)
                    .small()
                    .aria_label(if self.path.is_some() {
                        tr!("tab.rename")
                    } else {
                        tr!("tab.new_file_name")
                    })
                    .readonly(pending)
                    .font_family(design::UI_FONT)
                    .text_size(px(16.))
                    .font_weight(FontWeight::MEDIUM),
            )
            .into_any_element()
    }

    pub(super) fn error(&self) -> Option<&UiError> {
        self.error.as_ref()
    }
}

impl AppWindow {
    pub(super) fn is_pending_file_destination(&self, path: &Path) -> bool {
        self.creating_file.as_deref() == Some(path)
            || self
                .documents
                .values()
                .any(|tab| tab.rename_destination.as_deref() == Some(path))
    }

    pub(super) fn new_file_draft(&self) -> Option<&TabNameEditor> {
        let project = self.projects.get(self.active_project)?;
        self.tab_name_editor
            .as_ref()
            .filter(|editor| editor.creates_in(project.id))
    }

    pub(super) fn add_file_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let label = tr!("file.create_in_project");
        Button::new("add-file")
            .custom(
                ButtonCustomVariant::new(cx)
                    .hover(rgb(CONTENT_BG).into())
                    .active(rgb(CONTENT_BG).into()),
            )
            .group("add-file")
            .w(px(30.))
            .h(px(30.))
            .p_0()
            .border_0()
            .rounded_none()
            .rounded_tl(px(6.))
            .rounded_tr(px(6.))
            .flex_shrink_0()
            .accessibility_label(label.clone())
            .tooltip(label)
            .disabled(self.creating_file.is_some())
            .child(
                div()
                    .relative()
                    .size(px(18.))
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .group_hover("add-file", |style| style.invisible())
                            .child(design::asset(Asset::AddFile)),
                    )
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .invisible()
                            .group_hover("add-file", |style| style.visible())
                            .child(design::asset(Asset::AddFileHover)),
                    ),
            )
            .on_click(cx.listener(|this, _, window, cx| this.begin_file_creation(window, cx)))
    }

    fn begin_file_creation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.opening
            || self.closing
            || self.config_closing
            || self.dialog.is_some()
            || self.creating_file.is_some()
        {
            return;
        }
        let Some(project) = self
            .projects
            .get(self.active_project)
            .filter(|p| !p.missing)
        else {
            return;
        };
        let project = project.id;
        if self.new_file_draft().is_some() {
            self.focus_new_file_name(window, cx);
            return;
        }
        self.finish_editor(false, window, cx);
        self.search = None;
        self.notice = None;
        self.error = None;
        self.begin_tab_name_edit(project, None, DEFAULT_FILENAME, window, cx);
    }

    pub(super) fn focus_new_file_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(draft) = self.new_file_draft() {
            draft.name.update(cx, |state, cx| state.focus(window, cx));
        }
    }

    pub(super) fn tab_name(
        project: u64,
        index: usize,
        path: &Path,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        div()
            .id(("file-tab-name", index))
            .max_w(px(230.))
            .child(
                Label::new(name.into_owned())
                    .truncate()
                    .text_size(px(16.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(if selected {
                        design::TAB_TEXT_ACTIVE
                    } else {
                        design::TAB_TEXT_INACTIVE
                    })),
            )
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                if event.click_count() == 2 {
                    cx.stop_propagation();
                    this.begin_tab_rename(project, index, window, cx);
                }
            }))
            .into_any_element()
    }

    fn begin_tab_rename(
        &mut self,
        project: u64,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.dialog.is_some() || self.closing || self.config_closing || self.opening {
            return;
        }
        let Some(p) = self
            .projects
            .get(self.active_project)
            .filter(|p| p.id == project)
        else {
            return;
        };
        let Some(path) = p.tabs.get(index).map(|tab| tab.path.clone()) else {
            return;
        };
        let Some(filename) = path.file_name().and_then(|name| name.to_str()) else {
            self.error = Some(UiError::Message("file.invalid_filename"));
            cx.notify();
            return;
        };
        self.finish_editor(false, window, cx);
        self.search = None;
        self.notice = None;
        self.error = None;
        if self.projects[self.active_project].active != index {
            self.projects[self.active_project].active = index;
            self.load_active(cx);
            self.persist(cx);
            self.refresh_watches(cx);
        }
        let filename = filename.to_owned();
        self.begin_tab_name_edit(project, Some(path), &filename, window, cx);
    }

    fn begin_tab_name_edit(
        &mut self,
        project: u64,
        path: Option<PathBuf>,
        filename: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = cx.new(|cx| InputState::new(window, cx).default_value(filename.to_owned()));
        let focus = name.read(cx).focus_handle(cx);
        let editor = name.downgrade();
        let blur = cx.on_blur(&focus, window, move |this, _, cx| {
            if this
                .tab_name_editor
                .as_ref()
                .is_some_and(|rename| rename.name.downgrade() == editor)
            {
                this.tab_name_editor = None;
                this.tab_scroll_anchor = None;
                cx.notify();
            }
        });
        name.update(cx, |state, cx| {
            state.focus(window, cx);
            state.select_all(window, cx);
        });
        self.tab_name_editor = Some(TabNameEditor {
            project,
            path,
            name,
            error: None,
            _blur: blur,
        });
        self.tab_scroll_anchor = None;
        cx.notify();
    }

    pub(super) fn tab_name_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(rename) = &self.tab_name_editor else {
            return false;
        };
        if !matches!(event.keystroke.key.as_str(), "enter" | "escape")
            || !rename.name.read(cx).focus_handle(cx).is_focused(window)
            || rename.name.update(cx, |state, cx| {
                state.marked_text_range(window, cx).is_some()
            })
        {
            return false;
        }
        let creating = rename.path.is_none();
        if creating && self.creating_file.is_some() {
            cx.stop_propagation();
            return true;
        }
        if event.keystroke.key == "enter" {
            let result = if creating {
                self.submit_file_creation(window, cx)
            } else {
                self.submit_tab_rename(window, cx)
            };
            if let Err(error) = result
                && let Some(rename) = &mut self.tab_name_editor
            {
                rename.error = Some(error);
                cx.notify();
            }
        } else {
            self.cancel_tab_name_edit(window, cx);
        }
        cx.stop_propagation();
        true
    }

    pub(super) fn cancel_tab_name_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(rename) = self.tab_name_editor.take()
            && rename.name.read(cx).focus_handle(cx).is_focused(window)
        {
            self.focus.focus(window, cx);
        }
        self.tab_scroll_anchor = None;
        cx.notify();
    }

    fn submit_file_creation(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), UiError> {
        let Some(project) = self.projects.get(self.active_project) else {
            return Ok(());
        };
        let Some(draft) = self.new_file_draft() else {
            return Ok(());
        };
        if self.opening || self.closing || self.config_closing {
            return Err(UiError::Message("tab.rename_busy"));
        }
        let name = draft.name.read(cx).value();
        let destination = storage::file_destination(&project.root, &name)
            .ok_or(UiError::Message("file.invalid_name"))?;
        if self.is_file_destination_taken(&destination) {
            return Err(UiError::Storage(storage::StorageError::AlreadyExists));
        }

        let project_id = project.id;
        let editor = draft.name.downgrade();
        self.creating_file = Some(destination.clone());
        if let Some(draft) = &mut self.tab_name_editor {
            draft.error = None;
        }
        let job = cx
            .background_executor()
            .spawn(async move { storage::create_empty(destination) });
        let handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let result = job.await;
            let _ = handle.update(cx, |_, window, cx| {
                let _ = this.update(cx, |this, cx| {
                    this.creating_file = None;
                    let current_editor = this
                        .tab_name_editor
                        .as_mut()
                        .filter(|draft| draft.name.downgrade() == editor);
                    match result {
                        Ok(baseline) => {
                            let activate = current_editor.is_some();
                            if activate {
                                this.cancel_tab_name_edit(window, cx);
                            }
                            this.attach_created_file(project_id, baseline, activate, cx);
                        }
                        Err(error) => {
                            let error = UiError::Storage(error);
                            if let Some(draft) = current_editor {
                                draft.error = Some(error);
                            } else {
                                this.error = Some(error);
                            }
                        }
                    }
                    this.scan_external(cx);
                    cx.notify();
                });
            });
        })
        .detach();
        cx.notify();
        Ok(())
    }

    fn attach_created_file(
        &mut self,
        project: u64,
        baseline: storage::FileBaseline,
        activate: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.projects.iter().position(|p| p.id == project) else {
            return;
        };
        let previous = self.projects[index].active;
        let path = baseline.chosen_path.clone();
        self.open_project_file(index, path.clone());
        let project = &mut self.projects[index];
        let document = project.tabs[project.active].document;
        if !project.available.contains(&path) {
            project.available.push(path);
        }
        if !activate && project.tabs.len() > 1 {
            project.active = previous;
        }
        if let Some(tab) = self.tab_mut(document) {
            tab.read_epoch += 1;
        }
        self.attach_loaded(document, baseline);
        self.persist(cx);
        self.refresh_watches(cx);
    }

    fn is_file_destination_taken(&self, destination: &Path) -> bool {
        self.projects
            .iter()
            .any(|p| p.tabs.iter().any(|r| r.path == destination))
            || self.is_pending_file_destination(destination)
    }

    fn submit_tab_rename(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), UiError> {
        let Some(rename) = &self.tab_name_editor else {
            return Ok(());
        };
        let Some(source) = rename.path.clone() else {
            return Ok(());
        };
        let name = rename.name.read(cx).value().to_string();
        let Some(id) = self
            .projects
            .get(self.active_project)
            .and_then(|p| {
                p.tabs
                    .get(p.active)
                    .filter(|tab| rename.matches(p.id, &tab.path))
            })
            .map(|tab| tab.document)
        else {
            return Ok(());
        };
        let tab = &self.documents[&id];
        if tab.rename_destination.is_some() {
            return Ok(());
        }
        let destination = source
            .parent()
            .and_then(|directory| storage::file_destination(directory, &name))
            .ok_or(UiError::Message("file.invalid_name"))?;
        if source == destination {
            self.cancel_tab_name_edit(window, cx);
            return Ok(());
        }
        let session = tab
            .content
            .as_ref()
            .ok_or(UiError::Message(if tab.loading {
                "file.opening"
            } else {
                "tab.rename_unavailable"
            }))?;
        if session.is_saving() || self.closing || self.config_closing || self.opening {
            return Err(UiError::Message("tab.rename_busy"));
        }
        if self.is_file_destination_taken(&destination) {
            return Err(UiError::Storage(storage::StorageError::AlreadyExists));
        }
        let baseline = session.baseline.clone();
        self.documents.get_mut(&id).unwrap().rename_destination = Some(destination);
        if let Some(rename) = &mut self.tab_name_editor {
            rename.error = None;
        }
        let job = cx.background_executor().spawn(async move {
            let result = storage::rename_file(&baseline, &source, &name);
            (source, result)
        });
        let handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let (source, result) = job.await;
            let _ = handle.update(cx, |_, window, cx| {
                let _ = this.update(cx, |this, cx| {
                    match result {
                        Ok((baseline, warning)) => {
                            this.apply_tab_rename(id, &source, baseline, warning);
                            if this
                                .tab_name_editor
                                .as_ref()
                                .is_some_and(|rename| rename.path.as_ref() == Some(&source))
                            {
                                this.cancel_tab_name_edit(window, cx);
                            }
                            this.persist(cx);
                            this.refresh_watches(cx);
                        }
                        Err(error) => {
                            if let Some(tab) = this.tab_mut(id) {
                                tab.rename_destination = None;
                            }
                            let error = UiError::Storage(error);
                            if let Some(rename) = this
                                .tab_name_editor
                                .as_mut()
                                .filter(|rename| rename.path.as_ref() == Some(&source))
                            {
                                rename.error = Some(error);
                            } else {
                                this.error = Some(error);
                            }
                        }
                    }
                    this.scan_external(cx);
                    cx.notify();
                });
            });
        })
        .detach();
        cx.notify();
        Ok(())
    }

    fn apply_tab_rename(
        &mut self,
        id: u64,
        source: &Path,
        baseline: storage::FileBaseline,
        warning: Option<storage::StorageError>,
    ) {
        let Some(tab) = self.tab_mut(id) else { return };
        let destination = baseline.chosen_path.clone();
        tab.rename_destination = None;
        tab.path = destination.clone();
        tab.read_epoch += 1;
        tab.storage_error = warning;
        if let Some(session) = &mut tab.content {
            // The file moved, so rebase the saved snapshot and keep the working document.
            session.baseline = baseline;
        }
        for project in &mut self.projects {
            for reference in &mut project.tabs {
                if reference.document == id && reference.path == source {
                    reference.path = destination.clone();
                }
            }
            for path in &mut project.available {
                if path == source {
                    *path = destination.clone();
                }
            }
        }
    }
}
