use super::*;
use gpui_kit::component::menu::{ContextMenu, ContextMenuExt};
use std::path::Path;

pub(super) enum FileTrash {
    Confirming,
    Moving(u64),
}

pub(super) fn tab_menu(
    mut tab: BaseTab,
    project: u64,
    document: u64,
    path: PathBuf,
    owner: WeakEntity<AppWindow>,
) -> ContextMenu<BaseTab> {
    // Key this by the reference so reordering tabs cannot retarget an open menu.
    tab.interactivity().element_id =
        Some(SharedString::from(format!("file-menu-{project}-{document}-{path:?}")).into());
    tab.context_menu(move |menu, _, _| {
        let owner = owner.clone();
        let path = path.clone();
        menu.item(
            document::delete_menu_item(tr!("file.delete")).on_click(move |_, window, cx| {
                let _ = owner.update(cx, |this, cx| {
                    this.request_file_trash(project, document, path.clone(), window, cx);
                });
            }),
        )
    })
}

impl AppWindow {
    pub(super) fn moving_to_trash(&self, id: u64) -> bool {
        matches!(self.file_trash, Some(FileTrash::Moving(document)) if document == id)
    }

    fn trash_target(&self, project: u64, id: u64, path: &Path) -> Option<&FileTab> {
        self.projects
            .iter()
            .find(|p| p.id == project)?
            .tabs
            .iter()
            .find(|r| r.document == id && r.path == path)?;
        self.tab(id)
    }

    fn request_file_trash(
        &mut self,
        project: u64,
        id: u64,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.file_trash.is_some() || self.dialog.is_some() || self.palette.is_some() {
            return;
        }
        let Some(tab) = self.trash_target(project, id, &path) else {
            return;
        };
        if self.opening
            || self.closing
            || self.config_closing
            || self.creating_file.is_some()
            || self.documents.values().any(FileTab::saving)
            || self
                .documents
                .values()
                .any(|t| t.rename_destination.is_some())
        {
            self.error = Some(UiError::Message("file.trash_busy"));
            cx.notify();
            return;
        }

        let baseline = tab.content.as_ref().map(|s| s.baseline.clone());
        self.file_trash = Some(FileTrash::Confirming);
        self.error = None;
        let read_path = path.clone();
        let job = cx.background_executor().spawn(async move {
            match baseline {
                Some(baseline) => storage::open_reference(&baseline, &read_path),
                None => storage::open(read_path),
            }
        });
        let handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let result = job.await;
            let prompt = handle.update(cx, |_, window, cx| {
                this.update(cx, |this, cx| {
                    let snapshot = match result {
                        Ok(snapshot) => snapshot,
                        Err(error) => {
                            this.file_trash = None;
                            this.error = Some(UiError::Storage(error));
                            cx.notify();
                            return None;
                        }
                    };
                    let Some(tab) = this.trash_target(project, id, &path) else {
                        this.file_trash = None;
                        cx.notify();
                        return None;
                    };
                    let dirty = tab.dirty();
                    let detail = if dirty {
                        tr!("file.trash_dirty_detail")
                    } else {
                        tr!("file.trash_detail")
                    };
                    let answer = window.prompt(
                        PromptLevel::Warning,
                        &tr!(
                            "file.trash_title",
                            name = path.file_name().unwrap_or_default().to_string_lossy()
                        ),
                        Some(&detail),
                        &[
                            PromptButton::cancel(tr!("action.cancel")),
                            PromptButton::new(tr!("file.trash")),
                        ],
                        cx,
                    );
                    Some((snapshot, dirty, answer))
                })
            });
            let Ok(Ok(Some((snapshot, dirty, answer)))) = prompt else {
                return;
            };
            let confirmed = answer.await.ok() == Some(1);
            let _ = handle.update(cx, |_, window, cx| {
                let _ = this.update(cx, |this, cx| {
                    this.file_trash = None;
                    if confirmed {
                        this.trash_confirmed(project, id, snapshot, dirty, window, cx);
                    }
                });
            });
        })
        .detach();
    }

    fn trash_confirmed(
        &mut self,
        project: u64,
        id: u64,
        snapshot: storage::FileBaseline,
        was_dirty: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = &snapshot.chosen_path;
        if self.editor.as_ref().is_some_and(|editor| editor.tab == id) {
            self.finish_editor(false, window, cx);
        }
        let Some(tab) = self.trash_target(project, id, path) else {
            return;
        };
        if !was_dirty && tab.dirty() {
            self.error = Some(UiError::Message("file.trash_dirty_retry"));
            cx.notify();
            return;
        }
        let alternatives: Vec<_> = self
            .projects
            .iter()
            .flat_map(|p| &p.tabs)
            .filter(|r| tab.path == *path && r.document == id && r.path != *path)
            .map(|r| r.path.clone())
            .collect();
        let tab = self.documents.get_mut(&id).unwrap();
        tab.read_epoch += 1;
        tab.loading = false;
        self.file_trash = Some(FileTrash::Moving(id));
        let job = cx.background_executor().spawn(async move {
            let result = storage::trash_file(&snapshot).map(|()| {
                alternatives
                    .iter()
                    .find_map(|path| storage::open_reference(&snapshot, path).ok())
            });
            (snapshot.chosen_path, result)
        });
        let handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let (path, result) = job.await;
            let _ = handle.update(cx, |_, window, cx| {
                let _ = this.update(cx, |this, cx| {
                    this.file_trash = None;
                    match result {
                        Ok(alternative) => {
                            if let Some(baseline) = alternative
                                && let Some(tab) = this.tab_mut(id)
                                && let Some(session) = &mut tab.content
                            {
                                tab.path = baseline.chosen_path.clone();
                                session.baseline = baseline;
                                tab.storage_error = None;
                            }
                            this.remove_trashed_file(project, &path, window, cx);
                        }
                        Err(error) => {
                            this.error = Some(UiError::Storage(error));
                            this.load_active(cx);
                        }
                    }
                    this.scan_external(cx);
                    cx.notify();
                });
            });
        })
        .detach();
        cx.notify();
    }

    fn remove_trashed_file(
        &mut self,
        project: u64,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .projects
            .get(self.active_project)
            .is_some_and(|p| p.tabs.get(p.active).is_some_and(|r| r.path == path))
        {
            self.search = None;
            self.notice = None;
            self.focus.focus(window, cx);
        }
        if self
            .tab_name_editor
            .as_ref()
            .is_some_and(|rename| rename.matches(project, path))
        {
            self.cancel_tab_name_edit(window, cx);
        }
        for project in &mut self.projects {
            project.remove_trashed_reference(path);
        }
        if self
            .projects
            .get(self.active_project)
            .is_some_and(|p| p.tabs.is_empty())
        {
            self.active_project = usize::MAX;
        }
        self.hovered_tab = None;
        self.hovered_close_tab = None;
        self.tab_scroll_anchor = None;
        self.finish_tab_removal(cx);
    }
}

impl Project {
    fn remove_trashed_reference(&mut self, path: &Path) {
        self.available.retain(|candidate| candidate != path);
        let Some(index) = self
            .tabs
            .iter()
            .position(|reference| reference.path == path)
        else {
            return;
        };
        self.tabs.remove(index);
        if index < self.active {
            self.active -= 1;
        }
        self.active = self.active.min(self.tabs.len().saturating_sub(1));
    }
}
