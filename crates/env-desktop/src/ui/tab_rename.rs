use super::*;
use gpui_kit::component::input::{Input, InputState};
use std::path::Path;

const DEFAULT_FILENAME: &str = ".env";

pub(super) struct PendingFileCreation {
    chosen_path: PathBuf,
    resolved_path: Option<PathBuf>,
}

impl PendingFileCreation {
    fn confirm_destination(
        &mut self,
        projects: &[Project],
        documents: &BTreeMap<u64, FileTab>,
        resolved_path: PathBuf,
    ) -> Result<(), storage::StorageError> {
        if [&self.chosen_path, &resolved_path]
            .into_iter()
            .any(|path| is_file_destination_taken(projects, documents, None, path))
        {
            return Err(storage::StorageError::AlreadyExists);
        }
        self.resolved_path = Some(resolved_path);
        Ok(())
    }
}

fn resolve_create_destination(destination: &Path) -> Result<PathBuf, storage::StorageError> {
    let parent = destination
        .parent()
        .ok_or(storage::StorageError::InvalidPath)?;
    let name = destination
        .file_name()
        .ok_or(storage::StorageError::InvalidPath)?;
    Ok(std::fs::canonicalize(parent)?.join(name))
}

fn reserve_create_destination(
    projects: &[Project],
    documents: &BTreeMap<u64, FileTab>,
    creating_file: &mut Option<PendingFileCreation>,
    destination: &Path,
) -> Result<(), UiError> {
    if is_file_destination_taken(projects, documents, creating_file.as_ref(), destination) {
        return Err(UiError::Storage(storage::StorageError::AlreadyExists));
    }
    if creating_file.is_some() {
        return Err(UiError::Message("file.create_pending"));
    }
    *creating_file = Some(PendingFileCreation {
        chosen_path: destination.to_path_buf(),
        resolved_path: None,
    });
    Ok(())
}

fn is_file_destination_taken(
    projects: &[Project],
    documents: &BTreeMap<u64, FileTab>,
    creating_file: Option<&PendingFileCreation>,
    destination: &Path,
) -> bool {
    is_pending_file_destination(documents, creating_file, destination)
        || projects.iter().any(|project| {
            project
                .tabs
                .iter()
                .any(|reference| reference.path == destination)
        })
        || documents.values().any(|tab| {
            tab.path == destination
                || tab.content.as_ref().is_some_and(|session| {
                    session.baseline.chosen_path == destination
                        || session.baseline.resolved_path == destination
                })
        })
}

fn is_pending_file_destination(
    documents: &BTreeMap<u64, FileTab>,
    creating_file: Option<&PendingFileCreation>,
    destination: &Path,
) -> bool {
    creating_file.is_some_and(|pending| {
        pending.chosen_path == destination || pending.resolved_path.as_deref() == Some(destination)
    }) || documents
        .values()
        .any(|tab| tab.rename_destination.as_deref() == Some(destination))
}

fn finish_create_destination(
    projects: &[Project],
    documents: &BTreeMap<u64, FileTab>,
    creating_file: &mut Option<PendingFileCreation>,
    destination: &Path,
    result: Result<storage::FileBaseline, storage::StorageError>,
) -> Option<Result<storage::FileBaseline, UiError>> {
    if !creating_file
        .as_ref()
        .is_some_and(|pending| pending.chosen_path == destination)
    {
        return None;
    }
    // open_project_file skips reserved paths, including this request's own path.
    *creating_file = None;
    Some(result.map_err(UiError::Storage).and_then(|baseline| {
        if is_file_destination_taken(projects, documents, None, &baseline.chosen_path)
            || (baseline.resolved_path != baseline.chosen_path
                && is_file_destination_taken(projects, documents, None, &baseline.resolved_path))
            || documents
                .values()
                .any(|tab| tab.matches_baseline(&baseline))
        {
            return Err(UiError::Storage(storage::StorageError::AlreadyExists));
        }
        Ok(baseline)
    }))
}

pub(super) enum FileCreationSource {
    Menu,
    Inline {
        project: u64,
        editor: WeakEntity<InputState>,
    },
}

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
        is_pending_file_destination(&self.documents, self.creating_file.as_ref(), path)
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
        let name = draft.name.read(cx).value();
        let destination = storage::file_destination(&project.root, &name)
            .ok_or(UiError::Message("file.invalid_name"))?;

        let source = FileCreationSource::Inline {
            project: project.id,
            editor: draft.name.downgrade(),
        };
        self.start_file_creation(destination, source, window, cx)
    }

    pub(super) fn start_file_creation(
        &mut self,
        destination: PathBuf,
        source: FileCreationSource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), UiError> {
        if self.opening || self.closing || self.config_closing || self.dialog.is_some() {
            return Err(UiError::Message("tab.rename_busy"));
        }
        reserve_create_destination(
            &self.projects,
            &self.documents,
            &mut self.creating_file,
            &destination,
        )?;
        if let FileCreationSource::Inline { .. } = &source
            && let Some(draft) = &mut self.tab_name_editor
        {
            draft.error = None;
        }
        let chosen_path = destination.clone();
        let resolve = cx
            .background_executor()
            .spawn(async move { resolve_create_destination(&chosen_path) });
        let handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let resolved = resolve.await;
            let Ok(Ok(job)) = handle.update(cx, |_, _, cx| {
                this.update(cx, |this, cx| {
                    let pending = this
                        .creating_file
                        .as_mut()
                        .filter(|pending| pending.chosen_path == destination)
                        .ok_or(storage::StorageError::Conflict)?;
                    pending.confirm_destination(&this.projects, &this.documents, resolved?)?;
                    let path = destination.clone();
                    Ok(cx
                        .background_executor()
                        .spawn(async move { storage::create_empty(path) }))
                })
            }) else {
                return;
            };
            let result = match job {
                Ok(write) => write.await,
                Err(error) => Err(error),
            };
            let _ = handle.update(cx, |_, window, cx| {
                let _ = this.update(cx, |this, cx| {
                    let Some(result) = finish_create_destination(
                        &this.projects,
                        &this.documents,
                        &mut this.creating_file,
                        &destination,
                        result,
                    ) else {
                        return;
                    };
                    let current_inline = this.tab_name_editor.as_ref().is_some_and(|draft| {
                        matches!(&source, FileCreationSource::Inline { editor, .. }
                                if draft.name.downgrade() == *editor)
                    });
                    match result {
                        Ok(baseline) => {
                            let (index, activate) = match source {
                                FileCreationSource::Menu => {
                                    let index = this.project_for_created_file(&baseline);
                                    this.active_project = index;
                                    this.error = None;
                                    this.focus.focus(window, cx);
                                    (index, true)
                                }
                                FileCreationSource::Inline { project, .. } => {
                                    let Some(index) =
                                        this.projects.iter().position(|p| p.id == project)
                                    else {
                                        return;
                                    };
                                    (index, current_inline)
                                }
                            };
                            if current_inline {
                                this.cancel_tab_name_edit(window, cx);
                            }
                            this.attach_created_file(index, baseline, activate, cx);
                        }
                        Err(error) => {
                            if current_inline && let Some(draft) = &mut this.tab_name_editor {
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

    fn project_for_created_file(&mut self, baseline: &storage::FileBaseline) -> usize {
        let root = baseline
            .resolved_path
            .parent()
            .unwrap_or(Path::new("/"))
            .to_path_buf();
        if let Some(index) = self
            .projects
            .iter()
            .position(|project| project.root == root)
        {
            return index;
        }
        self.next_project += 1;
        self.projects.push(Project {
            id: self.next_project,
            root,
            tabs: Vec::new(),
            active: 0,
            available: Vec::new(),
            missing: false,
            scroll: ScrollHandle::new(),
        });
        self.projects.len() - 1
    }

    fn attach_created_file(
        &mut self,
        index: usize,
        baseline: storage::FileBaseline,
        activate: bool,
        cx: &mut Context<Self>,
    ) {
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
        if session.is_saving()
            || self.closing
            || self.config_closing
            || self.opening
            || self.file_trash.is_some()
        {
            return Err(UiError::Message("tab.rename_busy"));
        }
        if is_file_destination_taken(
            &self.projects,
            &self.documents,
            self.creating_file.as_ref(),
            &destination,
        ) {
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

#[cfg(test)]
mod tests {
    use super::{
        BTreeMap, DocumentSession, FileTab, Path, Project, ScrollHandle, TabReference, UiError,
        finish_create_destination, is_pending_file_destination, reserve_create_destination,
        resolve_create_destination, storage,
    };
    use crate::session::QuoteStyle;
    use env_core::BlockKind;

    fn project(id: u64, root: &Path, tabs: Vec<TabReference>) -> Project {
        Project {
            id,
            root: root.to_path_buf(),
            tabs,
            active: 0,
            available: Vec::new(),
            missing: false,
            scroll: ScrollHandle::new(),
        }
    }

    fn reference(document: u64, path: &Path) -> TabReference {
        TabReference {
            document,
            path: path.to_path_buf(),
        }
    }

    #[test]
    fn create_destination_rejects_an_open_dirty_file_removed_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        std::fs::write(&path, b"A=before\nB=sample\n").unwrap();
        let mut session = DocumentSession::new(storage::open(path.clone()).unwrap());
        let ids: Vec<_> = session
            .document
            .blocks()
            .iter()
            .filter_map(|block| match &block.kind {
                BlockKind::Assignment(entry) => Some(entry.id),
                _ => None,
            })
            .collect();
        let (a, b) = (ids[0], ids[1]);
        session.set_draft(a, "after".into()).unwrap();
        session.confirm_entry(a).unwrap();
        let invalid = "invalid\nEXAMPLE=sample";
        session.set_draft(b, invalid.into()).unwrap();
        assert!(session.draft(b).unwrap().error.is_some());
        let revision = session.work_revision();
        let mut tab = FileTab::new(1, path.clone());
        tab.content = Some(session);
        let mut documents = BTreeMap::from([(tab.id, tab)]);
        let projects = [project(1, dir.path(), vec![reference(1, &path)])];
        std::fs::remove_file(&path).unwrap();

        let mut creating_file = None;
        let result = reserve_create_destination(&projects, &documents, &mut creating_file, &path);
        if result.is_ok() {
            storage::create_empty(path.clone()).unwrap();
        }
        assert!(
            matches!(
                result,
                Err(UiError::Storage(storage::StorageError::AlreadyExists))
            ),
            "create admitted the open destination and left a file on disk: {}",
            path.exists()
        );
        assert!(!path.exists());
        assert!(creating_file.is_none());
        assert_eq!(projects[0].tabs.len(), 1);
        let reference = &projects[0].tabs[0];
        assert_eq!(reference.path, path);
        assert_eq!(reference.document, 1);
        let tab = documents.get_mut(&reference.document).unwrap();
        assert_eq!(tab.id, 1);
        let session = tab.content.as_mut().unwrap();
        assert_eq!(session.work_revision(), revision);
        assert_eq!(session.document.bytes(), b"A=after\nB=sample\n");
        assert_eq!(session.draft(b).unwrap().text, invalid);
        assert!(session.draft(b).unwrap().error.is_some());
        session.cancel_draft(b);
        assert!(session.undo().unwrap());
        assert_eq!(session.document.bytes(), b"A=before\nB=sample\n");
    }

    #[test]
    fn create_destination_accepts_a_free_path_and_keeps_its_reservation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        let other = dir.path().join(".env.other");
        let documents = BTreeMap::new();
        let mut pending = None;

        assert!(reserve_create_destination(&[], &documents, &mut pending, &path).is_ok());
        assert!(!path.exists());
        assert!(matches!(
            reserve_create_destination(&[], &documents, &mut pending, &path),
            Err(UiError::Storage(storage::StorageError::AlreadyExists))
        ));
        assert!(matches!(
            reserve_create_destination(&[], &documents, &mut pending, &other),
            Err(UiError::Message("file.create_pending"))
        ));
        assert!(
            finish_create_destination(
                &[],
                &documents,
                &mut pending,
                &other,
                Err(storage::StorageError::InvalidPath),
            )
            .is_none()
        );
        assert!(is_pending_file_destination(
            &documents,
            pending.as_ref(),
            &path
        ));
        assert!(!other.exists());

        let result = storage::create_empty(path.clone());
        let finished = finish_create_destination(&[], &documents, &mut pending, &path, result);
        assert!(matches!(finished, Some(Ok(baseline)) if baseline.chosen_path == path));
        assert!(pending.is_none());
        assert!(std::fs::read(path).unwrap().is_empty());
    }

    #[test]
    fn create_destination_rejects_a_picker_alias_before_writing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let project_root = root.join("project");
        std::fs::create_dir(&project_root).unwrap();
        let alias_root = root.join("picker-alias");
        std::os::unix::fs::symlink(&project_root, &alias_root).unwrap();
        let path = project_root.join(".env");
        let picked = alias_root.join(".env");
        let tab = FileTab::new(1, path.clone());
        let documents = BTreeMap::from([(tab.id, tab)]);
        let projects = [project(1, &project_root, vec![reference(1, &path)])];
        let mut pending = None;
        assert!(reserve_create_destination(&projects, &documents, &mut pending, &picked).is_ok());
        let resolved = resolve_create_destination(&picked).unwrap();
        assert_eq!(resolved, path);
        let confirmation = pending
            .as_mut()
            .unwrap()
            .confirm_destination(&projects, &documents, resolved);
        let result = confirmation.and_then(|()| storage::create_empty(picked.clone()));
        assert!(matches!(
            finish_create_destination(&projects, &documents, &mut pending, &picked, result),
            Some(Err(UiError::Storage(storage::StorageError::AlreadyExists)))
        ));
        assert!(pending.is_none());
        assert!(!path.exists());
        assert_eq!(projects[0].tabs.len(), 1);
    }

    #[test]
    fn create_destination_rejects_other_project_references_and_pending_renames() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("second/.env");
        let rename = dir.path().join("second/.env.rename");
        let mut tab = FileTab::new(1, dir.path().join("second/.env.original"));
        tab.rename_destination = Some(rename.clone());
        let documents = BTreeMap::from([(tab.id, tab)]);
        let projects = [
            project(1, &dir.path().join("first"), vec![]),
            project(2, &dir.path().join("second"), vec![reference(1, &path)]),
        ];
        let mut pending = None;

        for destination in [&path, &rename] {
            assert!(matches!(
                reserve_create_destination(&projects, &documents, &mut pending, destination),
                Err(UiError::Storage(storage::StorageError::AlreadyExists))
            ));
            assert!(!destination.exists());
            assert!(pending.is_none());
        }
        assert_eq!(projects[1].tabs[0].path, path);
    }

    #[test]
    fn create_destination_rejects_retained_chosen_path_after_alias_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        let alias = dir.path().join(".env.alias");
        std::fs::write(&path, b"SAMPLE=original\n").unwrap();
        let baseline = storage::open(path.clone()).unwrap();
        std::fs::hard_link(&path, &alias).unwrap();
        std::fs::remove_file(&path).unwrap();
        let mut tab = FileTab::new(1, alias.clone());
        tab.content = Some(DocumentSession::new(baseline));
        let documents = BTreeMap::from([(tab.id, tab)]);
        let projects = [project(1, dir.path(), vec![reference(1, &alias)])];
        let mut pending = None;

        assert!(matches!(
            reserve_create_destination(&projects, &documents, &mut pending, &path),
            Err(UiError::Storage(storage::StorageError::AlreadyExists))
        ));
        assert!(!path.exists());
        assert!(pending.is_none());
        assert_eq!(projects[0].tabs[0].path, alias);
        assert_eq!(std::fs::read(alias).unwrap(), b"SAMPLE=original\n");
    }

    #[test]
    fn create_destination_releases_after_failure_and_can_retry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        let documents = BTreeMap::new();
        let mut pending = None;

        assert!(reserve_create_destination(&[], &documents, &mut pending, &path).is_ok());
        std::fs::write(&path, b"SAMPLE=created-elsewhere\n").unwrap();
        let result = storage::create_empty(path.clone());
        assert!(matches!(
            finish_create_destination(&[], &documents, &mut pending, &path, result),
            Some(Err(UiError::Storage(storage::StorageError::AlreadyExists)))
        ));
        assert!(pending.is_none());
        assert_eq!(std::fs::read(&path).unwrap(), b"SAMPLE=created-elsewhere\n");

        std::fs::remove_file(&path).unwrap();
        assert!(reserve_create_destination(&[], &documents, &mut pending, &path).is_ok());
        let result = storage::create_empty(path.clone());
        assert!(matches!(
            finish_create_destination(&[], &documents, &mut pending, &path, result),
            Some(Ok(_))
        ));
        assert!(pending.is_none());
        assert!(std::fs::read(path).unwrap().is_empty());
    }

    #[test]
    fn create_destination_preserves_a_session_claimed_during_creation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        let mut documents = BTreeMap::new();
        let mut pending = None;
        assert!(reserve_create_destination(&[], &documents, &mut pending, &path).is_ok());
        let result = storage::create_empty(path.clone());
        assert!(result.is_ok());

        let mut session = DocumentSession::new(storage::open(path.clone()).unwrap());
        assert!(session.begin_new_variable(QuoteStyle::Unquoted));
        session.update_new_variable("SAMPLE".into(), "retained".into(), QuoteStyle::Unquoted);
        let mut tab = FileTab::new(1, path.clone());
        tab.content = Some(session);
        documents.insert(tab.id, tab);
        let projects = [project(1, dir.path(), vec![reference(1, &path)])];

        assert!(matches!(
            finish_create_destination(&projects, &documents, &mut pending, &path, result),
            Some(Err(UiError::Storage(storage::StorageError::AlreadyExists)))
        ));
        assert!(pending.is_none());
        assert_eq!(projects[0].tabs.len(), 1);
        let session = documents[&1].content.as_ref().unwrap();
        assert_eq!(session.new_variable().unwrap().value, "retained");
        assert!(std::fs::read(path).unwrap().is_empty());
    }
}
