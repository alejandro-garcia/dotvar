use super::*;
use crate::i18n::tr;
use gpui_kit::component::input::{Input, InputEvent, InputState};

pub(super) struct SearchBox {
    pub document: u64,
    pub state: Entity<InputState>,
    _subscription: Subscription,
}

pub(super) fn toggle_all_value_visibility(
    revealed: &mut bool,
    exceptions: &mut HashSet<EntryId>,
    pending_revealed: Option<&mut bool>,
) {
    *revealed = !*revealed;
    exceptions.clear();
    if let Some(pending_revealed) = pending_revealed {
        *pending_revealed = *revealed;
    }
}

impl AppWindow {
    pub(super) fn copy_entry(
        &mut self,
        tab: u64,
        entry: EntryId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.finish_editor(false, window, cx);
        self.notice = None;
        if let Some(tab) = self.tab_mut(tab) {
            tab.copied.remove(&Some(entry));
        }
        let result = self
            .tab_mut(tab)
            .and_then(|t| t.content.as_mut())
            .map(|s| s.copy_value(entry));
        match result {
            Some(Ok(text)) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                self.mark_copied(tab, Some(entry), cx);
            }
            Some(Err(error)) => {
                self.select_tab(tab, window, cx);
                self.show_session_error(error, window, cx);
            }
            None => {}
        }
        cx.notify();
    }
    pub(super) fn copy_document(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialog.is_some() {
            return;
        }
        self.finish_editor(false, window, cx);
        self.notice = None;
        if let Some(tab) = self.active_tab_mut() {
            tab.copied.remove(&None);
        }
        let result = self
            .active_tab_mut()
            .and_then(|t| t.content.as_mut())
            .map(DocumentSession::copy_document);
        match result {
            Some(Ok(text)) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                if let Some(tab) = self.active_tab().map(|t| t.id) {
                    self.mark_copied(tab, None, cx);
                }
            }
            Some(Err(error)) => self.show_session_error(error, window, cx),
            None => {}
        }
        cx.notify();
    }
    fn mark_copied(&mut self, tab: u64, entry: Option<EntryId>, cx: &mut Context<Self>) {
        let stamp = std::time::Instant::now();
        if let Some(tab) = self.tab_mut(tab) {
            tab.copied.insert(entry, stamp);
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(1500))
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Some(tab) = this.tab_mut(tab)
                    && tab.copied.get(&entry) == Some(&stamp)
                {
                    tab.copied.remove(&entry);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialog.is_some() {
            return;
        }
        self.finish_editor(false, window, cx);
        if let Some(search) = &self.search {
            search.state.update(cx, |s, cx| s.focus(window, cx));
            return;
        }
        let Some(document) = self.active_tab().map(|t| t.id) else {
            return;
        };
        let query = self
            .active_tab()
            .map(|t| t.query.clone())
            .unwrap_or_default();
        let state = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(query)
                .placeholder(tr!("variable.name"))
        });
        let subscription = cx.subscribe_in(&state, window, move |this, state, event, _, cx| {
            if matches!(event, InputEvent::Change)
                && this.active_tab().is_some_and(|t| t.id == document)
            {
                if let Some(tab) = this.active_tab_mut() {
                    tab.query = state.read(cx).value().to_string();
                    tab.matched = None;
                }
                this.find_match(false, cx);
                cx.notify();
            }
        });
        state.update(cx, |s, cx| s.focus(window, cx));
        self.search = Some(SearchBox {
            document,
            state,
            _subscription: subscription,
        });
        cx.notify();
    }
    pub(super) fn find_match(&mut self, backwards: bool, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab_mut() else {
            return;
        };
        let Some(s) = &tab.content else { return };
        let matches = crate::presentation::key_matches(&s.document, &tab.query);
        tab.matched = crate::presentation::adjacent_match(&matches, tab.matched, backwards);
        if let Some(id) = tab.matched
            && let Some(index) = s
                .document
                .blocks()
                .iter()
                .position(|b| matches!(&b.kind,env_core::BlockKind::Assignment(e)if e.id==id))
        {
            tab.list.scroll_to(ListOffset {
                item_ix: index,
                offset_in_item: px(0.),
            });
        }
        cx.notify();
    }
    /// Enter in the search box: edit the value of the current (or first)
    /// match through the same path as clicking a value. The search box stays
    /// open, as it does when a value is clicked. Does nothing without matches.
    pub(super) fn edit_found_value(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab() else { return };
        let Some(session) = &tab.content else { return };
        let Some(entry) =
            crate::presentation::match_to_edit(&session.document, &tab.query, tab.matched)
        else {
            return;
        };
        let tab_id = tab.id;
        if let Some(tab) = self.tab_mut(tab_id) {
            tab.matched = Some(entry);
        }
        self.begin_editor(tab_id, entry, window, cx);
    }
    pub(super) fn search_view(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let search = self.search.as_ref()?;
        let count = self
            .active_tab()
            .and_then(|t| {
                t.content
                    .as_ref()
                    .map(|s| crate::presentation::key_matches(&s.document, &t.query).len())
            })
            .unwrap_or(0);
        Some(
            div()
                .px(px(15.))
                .py_1()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div().w(px(320.)).child(
                        Input::new(&search.state)
                            .small()
                            .aria_label(tr!("variable.find_by_name")),
                    ),
                )
                .child(
                    Label::new(if count == 1 {
                        tr!("search.one_match", count = count)
                    } else {
                        tr!("search.many_matches", count = count)
                    })
                    .text_xs()
                    .text_color(rgb(SECONDARY)),
                )
                .child(
                    Button::new("previous-match")
                        .ghost()
                        .small()
                        .label(tr!("action.previous"))
                        .disabled(count == 0)
                        .on_click(cx.listener(|this, _, _, cx| this.find_match(true, cx))),
                )
                .child(
                    Button::new("next-match")
                        .ghost()
                        .small()
                        .label(tr!("action.next"))
                        .disabled(count == 0)
                        .on_click(cx.listener(|this, _, _, cx| this.find_match(false, cx))),
                )
                .child(
                    Button::new("close-search")
                        .ghost()
                        .small()
                        .label(tr!("search.close"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.search = None;
                            this.focus.focus(window, cx);
                            cx.notify();
                        })),
                )
                .into_any_element(),
        )
    }
    pub(super) fn cycle_tab(
        &mut self,
        backwards: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(project) = self.projects.get(self.active_project)
            && project.tabs.len() > 1
        {
            let count = project.tabs.len();
            let step = if backwards { count - 1 } else { 1 };
            let destination = &project.tabs[(project.active + step) % count];
            if self
                .tab(destination.document)
                .is_some_and(|tab| tab.content.is_some())
            {
                self.record_interaction("loaded_tab");
            }
        }
        self.cancel_tab_name_edit(window, cx);
        self.finish_editor(false, window, cx);
        self.search = None;
        self.notice = None;
        if let Some(p) = self.projects.get_mut(self.active_project)
            && !p.tabs.is_empty()
        {
            p.active = if backwards {
                (p.active + p.tabs.len() - 1) % p.tabs.len()
            } else {
                (p.active + 1) % p.tabs.len()
            };
            p.scroll.scroll_to_item(p.active);
        }
        self.load_active(cx);
        self.persist(cx);
        self.refresh_watches(cx);
        self.focus.focus(window, cx);
        cx.notify();
    }
    pub(super) fn toggle_row(
        &mut self,
        tab: u64,
        id: EntryId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.finish_editor(false, window, cx);
        if let Some(tab) = self.tab_mut(tab) {
            if !tab.exceptions.remove(&id) {
                tab.exceptions.insert(id);
            }
            tab.list.remeasure();
        }
        cx.notify();
    }
    pub(super) fn toggle_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.finish_editor(false, window, cx);
        if let Some(tab) = self.active_tab_mut() {
            toggle_all_value_visibility(
                &mut tab.revealed,
                &mut tab.exceptions,
                tab.new_variable.as_mut().map(|row| &mut row.revealed),
            );
            if let Some(row) = &tab.new_variable {
                row.redirect_hidden_value_focus(window, cx);
            }
            tab.list.remeasure();
        }
        cx.notify();
    }
    pub(super) fn detach_editor(&mut self) {
        if let Some(editor) = self.editor.take() {
            if let Some(tab) = self.tab(editor.tab)
                && editor.row < tab.list.item_count()
            {
                tab.set_row_focus(editor.row, None);
            }
            self.needs_root_focus = true;
        }
    }
}
