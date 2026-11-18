use super::*;
use crate::i18n::tr;
use crate::{
    config::{Catalog, ConfigStore, ProjectConfig},
    session::ExternalUpdate,
};

enum OpenPathKind {
    FileOrFolder,
    Folder,
    EnvFile,
}

impl AppWindow {
    pub(super) fn select_project(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self.projects.get(index) else {
            return;
        };
        let reopen = project
            .tabs
            .is_empty()
            .then(|| (project.root.clone(), project.id));
        self.cancel_tab_name_edit(window, cx);
        self.finish_editor(false, window, cx);
        self.search = None;
        self.notice = None;
        self.error = None;
        self.active_project = index;
        self.focus.focus(window, cx);
        if let Some((root, id)) = reopen {
            self.open_path(root, OpenPathKind::Folder, Some(id), cx);
        } else {
            self.load_active(cx);
            self.persist(cx);
            self.refresh_watches(cx);
            self.scan_external(cx);
        }
        cx.notify();
    }

    pub(super) fn restore(&mut self, initial: Option<PathBuf>, cx: &mut Context<Self>) {
        let job = cx.background_executor().spawn(async move {
            let store = ConfigStore::platform_default();
            let result = store.as_ref().map_err(|e| *e).and_then(ConfigStore::load);
            let catalog = result.as_ref().ok().cloned().unwrap_or_default();
            let discovered: Vec<_> = catalog
                .projects
                .iter()
                .map(|p| model::discover(&p.root).ok())
                .collect();
            (store.ok(), result, discovered)
        });
        cx.spawn(async move |this, cx| {
            let (store, result, discovered) = job.await;
            let _ = this.update(cx, |this, cx| {
                this.config = store;
                match result {
                    Ok(catalog) => {
                        for (p, available) in catalog.projects.into_iter().zip(discovered) {
                            this.next_project = this.next_project.max(p.id);
                            let tabs: Vec<_> = p
                                .tabs
                                .into_iter()
                                .map(|path| TabReference {
                                    document: this.make_tab(path.clone()),
                                    path,
                                })
                                .collect();
                            let active = p
                                .active_tab
                                .and_then(|path| tabs.iter().position(|r| r.path == path))
                                .unwrap_or(0);
                            this.projects.push(Project {
                                id: p.id,
                                root: p.root,
                                tabs,
                                active,
                                missing: available.is_none(),
                                available: available.unwrap_or_default(),
                                scroll: ScrollHandle::new(),
                            });
                        }
                        this.active_project = this
                            .projects
                            .iter()
                            .position(|p| Some(p.id) == catalog.active_project)
                            .unwrap_or(usize::MAX);
                    }
                    Err(error) => this.config_error = Some(error),
                }
                this.opening = false;
                this.start_watcher(cx);
                if let Some(path) = initial {
                    this.open_path(path, OpenPathKind::FileOrFolder, None, cx);
                } else if !this.open_next_external_file(cx) {
                    this.load_active(cx);
                    this.refresh_watches(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn catalog(&self) -> Catalog {
        Catalog {
            version: 1,
            projects: self
                .projects
                .iter()
                .map(|p| ProjectConfig {
                    id: p.id,
                    root: p.root.clone(),
                    tabs: p.tabs.iter().map(|r| r.path.clone()).collect(),
                    active_tab: p.tabs.get(p.active).map(|r| r.path.clone()),
                })
                .collect(),
            active_project: self.projects.get(self.active_project).map(|p| p.id),
        }
    }
    pub(super) fn persist(&mut self, cx: &mut Context<Self>) {
        if self.opening || self.config_error.is_some() {
            return;
        }
        self.config_pending = Some(self.catalog());
        if !self.config_saving {
            self.write_config(cx);
        }
    }
    pub(super) fn write_config(&mut self, cx: &mut Context<Self>) {
        let Some(catalog) = self.config_pending.take() else {
            return;
        };
        let Some(store) = self.config.clone() else {
            return;
        };
        self.config_saving = true;
        let job = cx
            .background_executor()
            .spawn(async move { store.save(&catalog) });
        cx.spawn(async move |this, cx| {
            let result = job.await;
            let _ = this.update(cx, |this, cx| {
                this.config_saving = false;
                if let Err(error) = result {
                    this.config_error = Some(error);
                    this.config_pending = None;
                }
                if this.config_pending.is_some() {
                    this.write_config(cx);
                }
                if this.config_closing && !this.config_saving {
                    this.config_closing = false;
                    if this.work_signature() != this.config_close_work {
                        this.config_window = None;
                        this.error = Some(UiError::Message("close.work_changed"));
                    } else if let Some(handle) = this.config_window.take() {
                        let _ = handle.update(cx, |_, window, _| window.remove_window());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn choose(&mut self, directory: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialog.is_some() || self.opening {
            return;
        }
        self.finish_editor(false, window, cx);
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: !directory,
            directories: true,
            multiple: false,
            prompt: Some(
                if directory {
                    tr!("project.add")
                } else {
                    tr!("file.open_or_folder")
                }
                .into(),
            ),
        });
        let kind = if directory {
            OpenPathKind::Folder
        } else {
            OpenPathKind::FileOrFolder
        };
        cx.spawn(async move |this, cx| match receiver.await {
            Ok(Ok(Some(paths))) => {
                if let Some(path) = paths.into_iter().next() {
                    let _ = this.update(cx, |this, cx| this.open_path(path, kind, None, cx));
                }
            }
            Ok(Ok(None)) => {}
            _ => {
                let _ = this.update(cx, |this, cx| {
                    this.error = Some(UiError::Message("project.picker_failed"));
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub(crate) fn open_external_files(
        &mut self,
        paths: impl IntoIterator<Item = PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.dialog.is_some() || self.closing || self.config_closing {
            return;
        }
        let previous_len = self.pending_external_files.len();
        self.pending_external_files
            .extend(paths.into_iter().filter(|path| model::is_env_file(path)));
        if self.pending_external_files.len() == previous_len {
            return;
        }
        self.close_palette(window, cx);
        self.finish_editor(false, window, cx);
        self.cancel_tab_name_edit(window, cx);
        self.notice = None;
        self.focus.focus(window, cx);
        self.open_next_external_file(cx);
    }

    fn open_next_external_file(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.opening
            && let Some(path) = self.pending_external_files.pop_front()
        {
            self.open_path(path, OpenPathKind::EnvFile, None, cx);
            return true;
        }
        false
    }

    fn open_path(
        &mut self,
        path: PathBuf,
        kind: OpenPathKind,
        reopening_project: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        self.open_request += 1;
        let request = self.open_request;
        self.opening = true;
        self.error = None;
        let previous_root = reopening_project.map(|id| (id, path.clone()));
        let job = cx.background_executor().spawn(async move {
            let path = if path.is_absolute() {
                path
            } else {
                std::env::current_dir()
                    .map_err(|_| UiError::Message("project.resolve_path_failed"))?
                    .join(path)
            };
            let directory = match kind {
                OpenPathKind::FileOrFolder => path.is_dir(),
                OpenPathKind::Folder => true,
                OpenPathKind::EnvFile if path.is_file() => false,
                OpenPathKind::EnvFile => return Err(UiError::Message("file.invalid_existing")),
            };
            let root = if directory {
                std::fs::canonicalize(&path)
                    .map_err(|_| UiError::Message("project.folder_inaccessible"))?
            } else {
                if !model::is_env_file(&path) {
                    return Err(UiError::Message("file.invalid_existing"));
                }
                let parent = path
                    .parent()
                    .ok_or(UiError::Message("project.no_parent_folder"))?;
                std::fs::canonicalize(parent)
                    .map_err(|_| UiError::Message("project.folder_inaccessible"))?
            };
            let paths = model::discover(&root)
                .map_err(|_| UiError::Message("project.folder_read_failed"))?;
            let explicit = if directory {
                None
            } else {
                Some(
                    root.join(
                        path.file_name()
                            .ok_or(UiError::Message("file.invalid_filename"))?,
                    ),
                )
            };
            Ok((root, paths, explicit))
        });
        cx.spawn(async move |this, cx| {
            let result = job.await;
            let _ = this.update(cx, |this, cx| {
                if this.open_request != request {
                    return;
                }
                this.opening = false;
                if let Some((id, root)) = &previous_root
                    && !this
                        .projects
                        .get(this.active_project)
                        .is_some_and(|p| p.id == *id && p.root == *root)
                {
                    this.open_next_external_file(cx);
                    cx.notify();
                    return;
                }
                match result {
                    Ok((root, available, explicit)) => {
                        let p = if let Some(p) = this.projects.iter().position(|p| p.root == root) {
                            this.projects[p].available = available;
                            this.projects[p].missing = false;
                            if explicit.is_none() && this.projects[p].tabs.is_empty() {
                                for path in this.projects[p].available.clone() {
                                    this.open_project_file(p, path);
                                }
                                this.projects[p].active = 0;
                            }
                            p
                        } else {
                            let paths: Vec<_> = available
                                .iter()
                                .filter(|path| !this.is_pending_file_destination(path))
                                .cloned()
                                .collect();
                            let tabs = paths
                                .into_iter()
                                .map(|path| TabReference {
                                    document: this.make_tab(path.clone()),
                                    path,
                                })
                                .collect();
                            this.next_project += 1;
                            this.projects.push(Project {
                                id: this.next_project,
                                root,
                                tabs,
                                active: 0,
                                available,
                                missing: false,
                                scroll: ScrollHandle::new(),
                            });
                            this.projects.len() - 1
                        };
                        this.active_project = p;
                        if let Some(path) = explicit {
                            this.open_project_file(p, path);
                        }
                        this.load_active(cx);
                        this.persist(cx);
                        this.refresh_watches(cx);
                        this.scan_external(cx);
                    }
                    Err(_) if previous_root.is_some() => {
                        let project = &mut this.projects[this.active_project];
                        project.missing = true;
                        project.available.clear();
                        this.refresh_watches(cx);
                    }
                    Err(error) => {
                        this.error = Some(error);
                        this.load_active(cx);
                        this.refresh_watches(cx);
                    }
                }
                this.open_next_external_file(cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    pub(super) fn open_project_file(&mut self, p: usize, path: PathBuf) {
        if let Some(index) = self.projects[p].tabs.iter().position(|r| r.path == path) {
            self.projects[p].active = index;
            return;
        }
        if self.is_pending_file_destination(&path) {
            return;
        }
        let document = self.make_tab(path.clone());
        self.projects[p].tabs.push(TabReference { document, path });
        self.projects[p].active = self.projects[p].tabs.len() - 1;
    }
    pub(super) fn rebind_reference(
        &mut self,
        old: u64,
        path: PathBuf,
        baseline: storage::FileBaseline,
    ) {
        let project = self
            .projects
            .iter()
            .position(|p| p.tabs.iter().any(|r| r.document == old && r.path == path));
        let id = self.make_tab(path.clone());
        for p in &mut self.projects {
            for reference in &mut p.tabs {
                if reference.document == old && reference.path == path {
                    reference.document = id;
                }
            }
        }
        if self.editor.as_ref().is_some_and(|e| e.tab == old) {
            self.detach_editor();
        }
        self.attach_loaded(id, baseline);
        // Keep the original working buffer accessible if the changed alias was its last reference.
        if !self
            .projects
            .iter()
            .any(|p| p.tabs.iter().any(|r| r.document == old))
            && let Some(p) = project
            && let Some(tab) = self.tab(old)
        {
            let path = tab.path.clone();
            self.projects[p].tabs.push(TabReference {
                document: old,
                path,
            });
        }
    }
    pub(super) fn files_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let available = self
            .projects
            .get(self.active_project)
            .map(|p| p.available.clone())
            .unwrap_or_default();
        let weak = cx.weak_entity();
        Button::new("available-files")
            .custom(
                ButtonCustomVariant::new(cx)
                    .hover(rgb(design::SELECTOR_BG).into())
                    .active(rgb(design::SELECTOR_BG).into()),
            )
            .w(px(26.))
            .h(px(26.))
            .p_0()
            .border_0()
            .rounded(px(8.))
            .child(
                svg()
                    .path(Asset::Dots.path())
                    .size(px(18.))
                    .flex_shrink_0()
                    .text_color(rgb(design::SECONDARY)),
            )
            .accessibility_label(tr!("file.actions"))
            .tooltip(tr!("file.actions"))
            .dropdown_menu(move |mut menu, _, _| {
                for path in &available {
                    let path = path.clone();
                    let weak = weak.clone();
                    let name =
                        SharedString::from(path.file_name().unwrap_or_default().to_string_lossy());
                    let icon = design::file_icon(&path, true);
                    menu = menu.item(
                        PopupMenuItem::element(move |_, _| {
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(design::asset(icon))
                                .child(Text::new("file-name".into(), name.clone()))
                        })
                        .on_click(move |_, window, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.cancel_tab_name_edit(window, cx);
                                this.finish_editor(false, window, cx);
                                let p = this.active_project;
                                this.open_project_file(p, path.clone());
                                this.load_active(cx);
                                this.persist(cx);
                                this.refresh_watches(cx);
                                cx.notify();
                            });
                        }),
                    );
                }
                menu.separator()
                    .item(PopupMenuItem::new(tr!("file.save")).action(Box::new(Save)))
                    .item(PopupMenuItem::new(tr!("file.save_as")).action(Box::new(SaveAs)))
                    .item(
                        PopupMenuItem::new(tr!("variable.add_action"))
                            .action(Box::new(AddVariable)),
                    )
                    .item(PopupMenuItem::new(tr!("variable.find")).action(Box::new(FindKey)))
                    .item(PopupMenuItem::new(tr!("tab.close")).action(Box::new(CloseTab)))
                    .item(PopupMenuItem::new(tr!("tab.move_left")).action(Box::new(MoveTabLeft)))
                    .item(PopupMenuItem::new(tr!("tab.move_right")).action(Box::new(MoveTabRight)))
            })
            .into_any_element()
    }
    pub(super) fn move_tab(&mut self, right: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.new_file_draft().is_some() {
            return;
        }
        self.finish_editor(false, window, cx);
        if let Some(p) = self.projects.get_mut(self.active_project) {
            let next = if right {
                p.active.checked_add(1)
            } else {
                p.active.checked_sub(1)
            };
            if let Some(next) = next.filter(|n| *n < p.tabs.len()) {
                p.tabs.swap(p.active, next);
                p.active = next;
            }
        }
        self.persist(cx);
        cx.notify();
    }
    pub(super) fn close_active_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.new_file_draft().is_some() {
            if self.creating_file.is_none() {
                self.cancel_tab_name_edit(window, cx);
            }
            return;
        }
        if let Some(p) = self.projects.get(self.active_project)
            && let Some(document) = p.tabs.get(p.active)
        {
            self.request_close_target(
                CloseTarget::Tab {
                    project: p.id,
                    document: document.document,
                    path: document.path.clone(),
                },
                window,
                cx,
            );
        }
    }
    pub(super) fn remove_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(p) = self.projects.get(self.active_project) {
            self.request_close_target(CloseTarget::Project(p.id), window, cx);
        }
    }
    pub(super) fn target_documents(&self, target: &CloseTarget) -> Vec<u64> {
        match target {
            CloseTarget::Window => self.documents.keys().copied().collect(),
            CloseTarget::Tab {
                project,
                document,
                path,
            } => {
                if self.projects.iter().any(|p| {
                    p.tabs
                        .iter()
                        .any(|r| r.document == *document && (p.id != *project || r.path != *path))
                }) {
                    Vec::new()
                } else {
                    vec![*document]
                }
            }
            CloseTarget::Project(project) => self
                .projects
                .iter()
                .find(|p| p.id == *project)
                .map(|p| {
                    p.tabs
                        .iter()
                        .map(|r| r.document)
                        .filter(|id| {
                            !self.projects.iter().any(|p| {
                                p.id != *project && p.tabs.iter().any(|r| r.document == *id)
                            })
                        })
                        .collect::<HashSet<_>>()
                        .into_iter()
                        .collect()
                })
                .unwrap_or_default(),
        }
    }
    pub(super) fn finish_close_target(
        &mut self,
        target: CloseTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match target {
            CloseTarget::Window => {
                self.persist(cx);
                if self.config_saving {
                    self.config_closing = true;
                    self.wait_config_close(window, cx);
                } else {
                    window.remove_window();
                }
                return;
            }
            CloseTarget::Tab { project, path, .. } => {
                if let Some(p) = self.projects.iter_mut().find(|p| p.id == project) {
                    p.tabs.retain(|r| r.path != path);
                    p.active = p.active.min(p.tabs.len().saturating_sub(1));
                }
                if self
                    .projects
                    .get(self.active_project)
                    .is_some_and(|p| p.id == project && p.tabs.is_empty())
                {
                    self.active_project = usize::MAX;
                    self.search = None;
                    self.notice = None;
                    self.focus.focus(window, cx);
                }
            }
            CloseTarget::Project(id) => {
                self.projects.retain(|p| p.id != id);
                self.active_project = self
                    .active_project
                    .min(self.projects.len().saturating_sub(1));
            }
        }
        let refs: HashSet<_> = self
            .projects
            .iter()
            .flat_map(|p| p.tabs.iter().map(|r| r.document))
            .collect();
        self.documents.retain(|id, _| refs.contains(id));
        self.load_active(cx);
        self.persist(cx);
        self.refresh_watches(cx);
        cx.notify();
    }
    fn work_signature(&self) -> Vec<(u64, Option<(u64, u64)>)> {
        self.documents
            .values()
            .map(|t| (t.id, t.content.as_ref().map(DocumentSession::work_revision)))
            .collect()
    }
    pub(super) fn wait_config_close(&mut self, window: &mut Window, _cx: &mut Context<Self>) {
        self.config_window = Some(window.window_handle());
        self.config_close_work = self.work_signature();
    }
    pub(super) fn locate_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.creating_file.is_some() {
            self.error = Some(UiError::Message("file.create_pending"));
            cx.notify();
            return;
        }
        if self
            .documents
            .values()
            .any(|tab| tab.rename_destination.is_some())
        {
            self.error = Some(UiError::Message("tab.rename_pending"));
            cx.notify();
            return;
        }
        if self.documents.values().any(FileTab::saving) {
            self.error = Some(UiError::Message("project.locate_while_saving"));
            cx.notify();
            return;
        }
        if self.dialog.is_some() || self.closing || self.opening {
            return;
        }
        self.finish_editor(false, window, cx);
        let Some(p) = self.projects.get(self.active_project) else {
            return;
        };
        let (id, old) = (p.id, p.root.clone());
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(tr!("project.locate").into()),
        });
        let handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let result = cx
                .background_executor()
                .spawn(async move {
                    let root = std::fs::canonicalize(path)?;
                    let available = model::discover(&root)?;
                    Ok::<_, std::io::Error>((root, available))
                })
                .await;
            let _ = handle.update(cx, |_, window, cx| {
                let _ = this.update(cx, |this, cx| {
                    if this.creating_file.is_some() {
                        this.error = Some(UiError::Message("file.create_pending"));
                        cx.notify();
                        return;
                    }
                    if this
                        .documents
                        .values()
                        .any(|tab| tab.rename_destination.is_some())
                    {
                        this.error = Some(UiError::Message("tab.rename_pending"));
                        cx.notify();
                        return;
                    }
                    this.finish_editor(false, window, cx);
                    let Some(p) = this
                        .projects
                        .iter()
                        .position(|p| p.id == id && p.root == old)
                    else {
                        return;
                    };
                    match result {
                        Ok((root, available)) => {
                            if this
                                .projects
                                .iter()
                                .any(|other| other.id != id && other.root == root)
                            {
                                this.error = Some(UiError::Message("project.already_in_catalog"));
                                cx.notify();
                                return;
                            }
                            let refs = this.projects[p].tabs.clone();
                            for reference in refs {
                                if let Ok(relative) = reference.path.strip_prefix(&old) {
                                    let path = root.join(relative);
                                    if let Some(r) = this.projects[p]
                                        .tabs
                                        .iter_mut()
                                        .find(|r| r.path == reference.path)
                                    {
                                        r.path = path.clone();
                                    }
                                    if let Some(tab) = this.tab_mut(reference.document)
                                        && tab.path == reference.path
                                    {
                                        tab.path = path;
                                        tab.read_epoch += 1;
                                        tab.loading = false;
                                        if tab.content.is_some() {
                                            tab.storage_error =
                                                Some(storage::StorageError::Conflict);
                                        } else {
                                            tab.error = None;
                                        }
                                    }
                                }
                            }
                            this.projects[p].root = root;
                            this.projects[p].available = available;
                            if this.projects[p].missing && this.projects[p].tabs.is_empty() {
                                for path in this.projects[p].available.clone() {
                                    this.open_project_file(p, path);
                                }
                                this.projects[p].active = 0;
                            }
                            this.projects[p].missing = false;
                            this.error = None;
                            this.load_active(cx);
                            this.persist(cx);
                            this.refresh_watches(cx);
                            this.scan_external(cx);
                        }
                        Err(_) => this.error = Some(UiError::Message("project.folder_read_failed")),
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }
    pub(super) fn reset_catalog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.config_saving {
            return;
        }
        let receiver = window.prompt(
            PromptLevel::Warning,
            &tr!("catalog.reset_title"),
            Some(&tr!("catalog.reset_detail")),
            &[
                PromptButton::new(tr!("catalog.reset_confirm")),
                PromptButton::cancel(tr!("action.cancel")),
            ],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if receiver.await.unwrap_or(1) != 0 {
                return;
            }
            let Some(store) = this
                .update(cx, |this, _| this.config.clone())
                .ok()
                .flatten()
            else {
                return;
            };
            let result = cx
                .background_executor()
                .spawn(async move { store.reset(&Catalog::default()) })
                .await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.config_error = None;
                        this.persist(cx);
                    }
                    Err(e) => this.config_error = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn start_watcher(&mut self, cx: &mut Context<Self>) {
        // Every wake scans the current scope; one queued wake covers any burst
        // while UI is busy, without retaining an unbounded batch backlog.
        let (sender, receiver) = async_channel::bounded(1);
        let job = cx.background_executor().spawn(async move {
            crate::watch::WatchService::new(move |batch| {
                let _ = sender.try_send(batch);
            })
        });
        cx.spawn(async move |this, cx| {
            match job.await {
                Ok(watcher) => {
                    let _ = this.update(cx, |this, cx| {
                        this.watcher = Some(Arc::new(Mutex::new(watcher)));
                        this.refresh_watches(cx);
                    });
                }
                Err(_) => {
                    let _ = this.update(cx, |this, cx| {
                        this.error = Some(UiError::Message("watch.external_changes"));
                        cx.notify();
                    });
                    return;
                }
            }
            while receiver.recv().await.is_ok() {
                if this.update(cx, |this, cx| this.scan_external(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
    }
    pub(super) fn refresh_watches(&mut self, cx: &mut Context<Self>) {
        if self.watcher.is_none() {
            return;
        }
        let mut roots = Vec::new();
        if let Some(p) = self.projects.get(self.active_project) {
            roots.push(p.root.clone());
            if let Some(parent) = p.root.parent() {
                roots.push(parent.to_path_buf());
            }
        }
        for tab in self.documents.values() {
            if let Some(parent) = tab.path.parent() {
                roots.push(parent.to_path_buf());
            }
            if let Some(session) = &tab.content
                && let Some(parent) = session.baseline.resolved_path.parent()
            {
                roots.push(parent.to_path_buf());
            }
        }
        for p in &self.projects {
            for reference in &p.tabs {
                if let Some(parent) = reference.path.parent() {
                    roots.push(parent.to_path_buf());
                }
            }
        }
        roots.sort();
        roots.dedup();
        self.reconcile_watches(roots, cx);
    }
    fn reconcile_watches(&mut self, roots: Vec<PathBuf>, cx: &mut Context<Self>) {
        if self.watch_reconciling {
            self.watch_pending_roots = Some(roots);
            return;
        }
        let Some(watcher) = self.watcher.clone() else {
            return;
        };
        self.watch_reconciling = true;
        let expected = roots.clone();
        let job = cx.background_executor().spawn(async move {
            let mut watcher = watcher.lock().map_err(|_| ())?;
            let existing = roots.into_iter().filter(|p| p.is_dir()).collect::<Vec<_>>();
            watcher.reconcile(existing).map_err(|_| ())?;
            Ok::<_, ()>(watcher.watched_roots().iter().cloned().collect::<Vec<_>>())
        });
        cx.spawn(async move |this, cx| {
            let result = job.await;
            let _ = this.update(cx, |this, cx| {
                this.watch_reconciling = false;
                match result {
                    Ok(actual) => this.watch_roots = actual,
                    Err(()) => this.error = Some(UiError::Message("watch.folder")),
                }
                if let Some(next) = this.watch_pending_roots.take()
                    && next != expected
                {
                    this.reconcile_watches(next, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn scan_external(&mut self, cx: &mut Context<Self>) {
        if self.watch_scanning {
            self.watch_pending = true;
            return;
        }
        self.watch_scanning = true;
        let project = self
            .projects
            .get(self.active_project)
            .map(|p| (p.id, p.root.clone()));
        let reads: Vec<_> = self
            .documents
            .values_mut()
            .filter_map(|t| {
                let s = t.content.as_ref()?;
                if s.is_saving() || t.rename_destination.is_some() {
                    return None;
                }
                t.read_epoch += 1;
                Some((t.id, t.read_epoch, t.path.clone(), s.baseline.clone()))
            })
            .collect();
        let aliases: Vec<_> = self
            .projects
            .iter()
            .flat_map(|p| p.tabs.iter())
            .filter_map(|r| {
                self.tab(r.document)
                    .filter(|t| t.rename_destination.is_none())
                    .and_then(|t| t.content.as_ref())
                    .filter(|s| s.baseline.chosen_path != r.path)
                    .map(|s| (r.document, r.path.clone(), s.baseline.clone()))
            })
            .collect();
        let job = cx.background_executor().spawn(async move {
            let alias_results: Vec<_> = aliases
                .into_iter()
                .map(|(id, path, expected)| {
                    let result = storage::open(path.clone());
                    (id, path, expected, result)
                })
                .collect();
            let discovered = project.map(|(id, root)| {
                let result = model::discover(&root);
                (id, root, result)
            });
            let results: Vec<_> = reads
                .into_iter()
                .map(|(id, epoch, path, baseline)| {
                    let result = storage::open(path);
                    (id, epoch, baseline, result)
                })
                .collect();
            (discovered, results, alias_results)
        });
        cx.spawn(async move |this, cx| {
            let (discovered, results, alias_results) = job.await;
            let _ = this.update(cx, |this, cx| {
                this.watch_scanning = false;
                if let Some((id, root, result)) = discovered
                    && let Some(p) = this
                        .projects
                        .iter_mut()
                        .find(|p| p.id == id && p.root == root)
                {
                    match result {
                        Ok(paths) => {
                            p.available = paths;
                            p.missing = false;
                        }
                        Err(_) => {
                            p.missing = true;
                            p.available.clear();
                        }
                    }
                }
                for (id, epoch, expected, result) in results {
                    let active_editor = this.editor.as_ref().is_some_and(|e| e.tab == id);
                    let Some(tab) = this.tab_mut(id) else {
                        continue;
                    };
                    if tab.read_epoch != epoch || tab.rename_destination.is_some() {
                        continue;
                    }
                    let Some(session) = tab.content.as_mut() else {
                        continue;
                    };
                    if !session.baseline.same_reference(&expected) || session.is_saving() {
                        continue;
                    }
                    match result {
                        Ok(baseline) => {
                            // Explicitly relocating a folder may rebase only identical saved bytes.
                            if baseline.chosen_path != session.baseline.chosen_path
                                && baseline.bytes == session.baseline.bytes
                                && baseline.identity() == session.baseline.identity()
                            {
                                session.baseline = baseline;
                                tab.storage_error = None;
                            } else {
                                match session.reconcile_external(baseline) {
                                    ExternalUpdate::Reloaded => {
                                        tab.storage_error = None;
                                        tab.error = None;
                                        tab.revealed = false;
                                        tab.exceptions.clear();
                                        if active_editor {
                                            this.detach_editor();
                                        }
                                    }
                                    ExternalUpdate::Conflict => {
                                        tab.storage_error = Some(storage::StorageError::Conflict)
                                    }
                                    ExternalUpdate::Unchanged => {
                                        if !tab
                                            .storage_error
                                            .as_ref()
                                            .is_some_and(storage::StorageError::is_uncertain)
                                        {
                                            tab.storage_error = None;
                                        }
                                    }
                                    ExternalUpdate::Busy => {}
                                }
                            }
                        }
                        Err(error) => tab.storage_error = Some(error),
                    }
                }
                for (id, path, expected, result) in alias_results {
                    if !this
                        .projects
                        .iter()
                        .any(|p| p.tabs.iter().any(|r| r.document == id && r.path == path))
                    {
                        continue;
                    }
                    let Some(tab) = this.tab(id) else { continue };
                    if tab.rename_destination.is_some() {
                        continue;
                    }
                    let Some(session) = tab.content.as_ref() else {
                        continue;
                    };
                    if !session.baseline.same_reference(&expected) || session.is_saving() {
                        continue;
                    }
                    let dirty = session.is_dirty();
                    match result {
                        Ok(actual)
                            if actual.identity() == expected.identity()
                                && actual.bytes == expected.bytes => {}
                        Ok(actual) if !dirty && actual.resolved_path != expected.resolved_path => {
                            this.rebind_reference(id, path, actual);
                            this.persist(cx);
                        }
                        Ok(_) => {
                            if let Some(tab) = this.tab_mut(id) {
                                tab.storage_error = Some(storage::StorageError::Conflict);
                            }
                        }
                        Err(error) => {
                            if let Some(tab) = this.tab_mut(id) {
                                tab.storage_error = Some(error);
                            }
                        }
                    }
                }
                if this.watch_pending {
                    this.watch_pending = false;
                    this.scan_external(cx);
                }
                this.refresh_watches(cx);
                cx.notify();
            });
        })
        .detach();
    }
}
