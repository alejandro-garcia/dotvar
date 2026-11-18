use super::tab_rename::FileCreationSource;
use super::*;
use crate::i18n::tr;
use crate::session::SaveRequest;

impl AppWindow {
    pub(super) fn save_active(
        &mut self,
        save_as: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.dialog.is_some() || self.file_trash.is_some() {
            return;
        }
        self.finish_editor(false, window, cx);
        let Some(id) = self.active_tab().map(|t| t.id) else {
            return;
        };
        if save_as {
            self.choose_save_as(id, window, cx);
        } else {
            self.save_tab(id, None, window, cx);
        }
    }

    fn choose_save_as(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tab_mut(id) else { return };
        if tab.rename_destination.is_some() {
            self.error = Some(UiError::Message("tab.rename_pending"));
            cx.notify();
            return;
        }
        let Some(session) = tab.content.as_mut() else {
            return;
        };
        if session.is_saving() {
            return;
        }
        if let Err(error) = session.confirm_all() {
            self.show_session_error(error, window, cx);
            return;
        }
        let path = tab.path.clone();
        let receiver = cx.prompt_for_new_path(
            path.parent().unwrap_or(std::path::Path::new("/")),
            path.file_name().and_then(|n| n.to_str()),
        );
        let handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let result = receiver.await;
            let _ = handle.update(cx, |_, window, cx| {
                let _ = this.update(cx, |this, cx| match result {
                    Ok(Ok(Some(path))) if model::is_env_file(&path) => {
                        this.save_tab(id, Some(path), window, cx)
                    }
                    Ok(Ok(Some(_))) => {
                        this.error = Some(UiError::Message("file.invalid_name"));
                        cx.notify();
                    }
                    Ok(Ok(None)) => {}
                    _ => {
                        this.error = Some(UiError::Message("file.save_picker_failed"));
                        cx.notify();
                    }
                });
            });
        })
        .detach();
    }

    fn save_tab(
        &mut self,
        id: u64,
        destination: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.file_trash.is_some() {
            return;
        }
        let aliases: Vec<_> = self
            .projects
            .iter()
            .flat_map(|p| p.tabs.iter())
            .filter(|r| r.document == id)
            .map(|r| r.path.clone())
            .collect();
        let Some(tab) = self.tab_mut(id) else {
            self.advance_close(window, cx);
            return;
        };
        if tab.rename_destination.is_some() {
            self.error = Some(UiError::Message("tab.rename_pending"));
            cx.notify();
            return;
        }
        if destination.is_none()
            && tab
                .storage_error
                .as_ref()
                .is_some_and(storage::StorageError::is_uncertain)
        {
            self.closing = false;
            self.close_queue.clear();
            cx.notify();
            return;
        }
        if destination.is_none()
            && tab
                .content
                .as_ref()
                .is_some_and(|s| s.baseline.chosen_path != tab.path)
        {
            tab.storage_error = Some(storage::StorageError::Conflict);
            self.closing = false;
            self.close_queue.clear();
            cx.notify();
            return;
        }
        let Some(session) = tab.content.as_mut() else {
            self.advance_close(window, cx);
            return;
        };
        let request = match session.prepare_save(destination.is_some()) {
            Ok(Some(request)) => request,
            Ok(None) => {
                self.advance_close(window, cx);
                return;
            }
            Err(error) => {
                self.closing = false;
                self.close_queue.clear();
                self.select_tab(id, window, cx);
                self.show_session_error(error, window, cx);
                return;
            }
        };
        tab.error = None;
        tab.storage_error = None;
        self.error = None;
        let save_as = destination.is_some();
        let job = cx.background_executor().spawn(async move {
            let result = match destination {
                Some(path) => storage::save_as(path, &request.bytes),
                None => storage::validate_aliases(&request.baseline, &aliases)
                    .and_then(|()| storage::save(&request.baseline, &request.bytes)),
            };
            (request, result)
        });
        let handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let (request, result) = job.await;
            let _ = handle.update(cx, |_, window, cx| {
                let _ = this.update(cx, |this, cx| {
                    this.finish_save(id, save_as, request, result, window, cx)
                });
            });
        })
        .detach();
        cx.notify();
    }

    fn finish_save(
        &mut self,
        id: u64,
        save_as: bool,
        request: SaveRequest,
        result: Result<storage::FileBaseline, storage::StorageError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tab_mut(id) else { return };
        let Some(session) = tab.content.as_mut() else {
            return;
        };
        match result {
            Ok(baseline) => {
                if let Err(error) = session.complete_save(&request, baseline) {
                    session.fail_save(&request);
                    tab.error = Some(UiError::Session(error));
                    self.closing = false;
                    self.close_queue.clear();
                    cx.notify();
                    return;
                }
                tab.path = session.baseline.chosen_path.clone();
                tab.storage_error = None;
                if save_as {
                    self.move_saved_tab(id);
                }
                self.persist(cx);
                self.refresh_watches(cx);
                self.scan_external(cx);
                self.advance_close(window, cx);
            }
            Err(error) => {
                session.fail_save(&request);
                tab.storage_error = Some(error);
                self.closing = false;
                self.close_queue.clear();
            }
        }
        cx.notify();
    }

    fn move_saved_tab(&mut self, id: u64) {
        let Some((old_project, _)) = self.location(id) else {
            return;
        };
        let Some(root) = self
            .tab(id)
            .and_then(|t| t.path.parent())
            .map(|p| p.to_path_buf())
        else {
            return;
        };
        let _ = old_project;
        // Save As changes the shared document; every reference follows it.
        for p in &mut self.projects {
            p.tabs.retain(|tab| tab.document != id);
            p.active = p.active.min(p.tabs.len().saturating_sub(1));
        }
        let target = if let Some(p) = self.projects.iter().position(|p| p.root == root) {
            p
        } else {
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
        };
        self.projects[target].tabs.push(TabReference {
            document: id,
            path: self.documents[&id].path.clone(),
        });
        self.projects[target].active = self.projects[target].tabs.len() - 1;
        self.active_project = target;
    }

    pub(super) fn create_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialog.is_some() {
            return;
        }
        self.finish_editor(false, window, cx);
        let root = self
            .projects
            .get(self.active_project)
            .map(|p| p.root.clone())
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_else(std::env::temp_dir);
        let receiver = cx.prompt_for_new_path(&root, Some(".env"));
        let handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let picked = receiver.await;
            let _ = handle.update(cx, |_, window, cx| {
                let _ = this.update(cx, |this, cx| {
                    let result = match picked {
                        Ok(Ok(Some(path))) if model::is_env_file(&path) => {
                            this.start_file_creation(path, FileCreationSource::Menu, window, cx)
                        }
                        Ok(Ok(None)) => return,
                        _ => Err(UiError::Message("file.invalid_new")),
                    };
                    if let Err(error) = result {
                        this.error = Some(error);
                        cx.notify();
                    }
                });
            });
        })
        .detach();
    }

    pub(super) fn reload(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.finish_editor(false, window, cx);
        let Some(tab) = self.tab(id) else { return };
        let Some(session) = tab.content.as_ref() else {
            return;
        };
        if session.is_saving() || tab.rename_destination.is_some() {
            return;
        }
        let expected_work = session.work_revision();
        let expected = session.baseline.clone();
        let original_path = tab.path.clone();
        let path = self
            .projects
            .get(self.active_project)
            .and_then(|p| p.tabs.get(p.active))
            .filter(|r| r.document == id)
            .map(|r| r.path.clone())
            .unwrap_or_else(|| original_path.clone());
        let receiver = window.prompt(
            PromptLevel::Warning,
            &tr!("file.reload_title"),
            Some(&tr!("file.reload_detail")),
            &[
                PromptButton::new(tr!("file.reload")),
                PromptButton::cancel(tr!("action.cancel")),
            ],
            cx,
        );
        let handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            if receiver.await.unwrap_or(1) != 0 {
                return;
            }
            let read_path = path.clone();
            let result = cx
                .background_executor()
                .spawn(async move { storage::open(read_path) })
                .await;
            let _ = handle.update(cx, |_, window, cx| {
                let _ = this.update(cx, |this, cx| {
                    if this.editor.as_ref().is_some_and(|e| e.tab == id) {
                        this.finish_editor(false, window, cx);
                    }
                    if !this.tab(id).is_some_and(|tab| {
                        tab.path == original_path
                            && tab.rename_destination.is_none()
                            && tab.content.as_ref().is_some_and(|s| {
                                s.can_reload(expected_work, &expected.bytes)
                                    && s.baseline.same_reference(&expected)
                            })
                    }) {
                        this.error = Some(UiError::Message("file.reload_stale"));
                        cx.notify();
                        return;
                    }
                    match result {
                        Ok(baseline)
                            if path != original_path
                                && baseline.resolved_path != expected.resolved_path =>
                        {
                            this.rebind_reference(id, path, baseline)
                        }
                        Ok(baseline) => {
                            if let Some(tab) = this.tab_mut(id) {
                                tab.read_epoch += 1;
                                tab.path = baseline.chosen_path.clone();
                                tab.content = Some(DocumentSession::new(baseline));
                                tab.storage_error = None;
                                tab.error = None;
                                tab.revealed = false;
                                tab.exceptions.clear();
                            }
                        }
                        Err(error) => {
                            if let Some(tab) = this.tab_mut(id) {
                                tab.storage_error = Some(error);
                            }
                        }
                    }
                    this.persist(cx);
                    this.refresh_watches(cx);
                    cx.notify();
                });
            });
        })
        .detach();
    }

    pub(crate) fn request_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.request_close_target(CloseTarget::Window, window, cx);
    }
    pub(super) fn request_close_target(
        &mut self,
        target: CloseTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.closing || self.config_closing || self.file_trash.is_some() {
            return;
        }
        if self.creating_file.is_some() && !matches!(target, CloseTarget::Tab { .. }) {
            self.error = Some(UiError::Message("file.create_pending"));
            cx.notify();
            return;
        }
        if let Some(dialog) = self.dialog.as_mut() {
            dialog.set_error(UiError::Message("close.pending_variable"));
            cx.notify();
            return;
        }
        self.finish_editor(false, window, cx);
        let ids = self.target_documents(&target);
        if ids.iter().any(|id| {
            self.tab(*id)
                .is_some_and(|tab| tab.rename_destination.is_some())
        }) {
            self.error = Some(UiError::Message("tab.rename_pending"));
            cx.notify();
            return;
        }
        if ids
            .iter()
            .any(|id| self.tab(*id).is_some_and(FileTab::saving))
        {
            self.error = Some(UiError::Message("close.pending_save"));
            cx.notify();
            return;
        }
        let dirty: Vec<_> = ids
            .into_iter()
            .filter(|id| self.tab(*id).is_some_and(FileTab::dirty))
            .collect();
        if dirty.is_empty() {
            self.finish_close_target(target, window, cx);
            return;
        }
        self.closing = true;
        self.close_target = Some(target.clone());
        let receiver = window.prompt(
            PromptLevel::Warning,
            &tr!("close.save_title"),
            Some(&tr!("close.save_detail")),
            &[
                PromptButton::new(tr!("file.save")),
                PromptButton::new(tr!("close.discard")),
                PromptButton::cancel(tr!("action.cancel")),
            ],
            cx,
        );
        let handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let answer = receiver.await.unwrap_or(2);
            let _ = handle.update(cx, |_, window, cx| {
                let _ = this.update(cx, |this, cx| match answer {
                    0 => {
                        for id in &dirty {
                            let result = this
                                .tab_mut(*id)
                                .and_then(|t| t.content.as_mut())
                                .map(DocumentSession::confirm_all);
                            if let Some(Err(error)) = result {
                                this.closing = false;
                                this.close_target = None;
                                this.close_queue.clear();
                                this.select_tab(*id, window, cx);
                                this.show_session_error(error, window, cx);
                                return;
                            }
                        }
                        this.close_queue = dirty.into_iter().rev().collect();
                        this.advance_close(window, cx);
                    }
                    1 => {
                        this.closing = false;
                        this.close_target = None;
                        this.finish_close_target(target, window, cx);
                    }
                    _ => {
                        this.closing = false;
                        this.close_target = None;
                        this.close_queue.clear();
                        cx.notify();
                    }
                });
            });
        })
        .detach();
    }
    fn advance_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.closing {
            return;
        }
        if let Some(id) = self.close_queue.pop() {
            self.save_tab(id, None, window, cx);
        } else {
            self.closing = false;
            if let Some(target) = self.close_target.take() {
                self.request_close_target(target, window, cx);
            }
        }
    }
}
