use super::*;
use crate::i18n::tr;
use crate::session::SessionError;
use env_core::{EditError, TextField};
use gpui_kit::component::input::{Input, InputEvent, InputState, TextareaState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EditorTarget {
    Value(EntryId),
    Text(TextField),
}

impl EditorTarget {
    fn row(self, session: &DocumentSession) -> Option<usize> {
        let id = match self {
            Self::Text(TextField::Comment(row)) => return Some(row),
            Self::Value(id) | Self::Text(TextField::Name(id) | TextField::InlineComment(id)) => id,
        };
        session
            .document
            .blocks()
            .iter()
            .position(|b| matches!(&b.kind, env_core::BlockKind::Assignment(e) if e.id == id))
    }
}

pub(super) struct ActiveEditor {
    pub tab: u64,
    pub target: EditorTarget,
    pub row: usize,
    pub state: Entity<TextareaState>,
    source_prefix: String,
    changed: bool,
    _subscription: Subscription,
}

/// Keep source spacing after `#` outside the visible comment editor.
pub(super) fn comment_content(text: &str) -> (&str, &str) {
    let content = text.trim_start_matches([' ', '\t']);
    text.split_at(text.len() - content.len())
}

pub(super) struct RenameDialog {
    tab: u64,
    entry: EntryId,
    name: Entity<InputState>,
    error: Option<UiError>,
}

impl RenameDialog {
    pub(super) fn set_error(&mut self, error: UiError) {
        self.error = Some(error);
    }

    pub(super) fn refresh_language(&self, window: &mut Window, cx: &mut App) {
        self.name.update(cx, |state, cx| {
            state.set_placeholder(tr!("variable.name_example"), window, cx);
        });
    }
}

impl AppWindow {
    pub(super) fn begin_editor(
        &mut self,
        tab_id: u64,
        id: EntryId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.begin_field_editor(tab_id, EditorTarget::Value(id), None, window, cx);
    }

    pub(super) fn begin_text_editor(
        &mut self,
        tab_id: u64,
        field: TextField,
        caret: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.begin_field_editor(tab_id, EditorTarget::Text(field), caret, window, cx);
    }

    fn begin_field_editor(
        &mut self,
        tab_id: u64,
        target: EditorTarget,
        caret: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .editor
            .as_ref()
            .is_some_and(|e| e.tab == tab_id && e.target == target)
        {
            if let Some(e) = &self.editor {
                e.state.update(cx, |s, cx| {
                    if let Some(offset) = caret {
                        s.set_selected_range(offset..offset, cx);
                    }
                    s.focus(window, cx);
                });
            }
            return;
        }
        self.finish_editor(false, window, cx);
        let Some(tab) = self.tab_mut(tab_id) else {
            return;
        };
        let Some(session) = tab.content.as_ref() else {
            return;
        };
        if session.document.is_read_only() {
            return;
        }
        let text = match target {
            EditorTarget::Value(id) => session.value_content(id),
            EditorTarget::Text(field) => session.field_text(field),
        };
        let Some(text) = text.map(str::to_owned) else {
            return;
        };
        let (source_prefix, text) = if matches!(target, EditorTarget::Text(TextField::Comment(_))) {
            let (prefix, content) = comment_content(&text);
            (prefix.to_owned(), content.to_owned())
        } else {
            (String::new(), text)
        };
        let draft_prefix = source_prefix.clone();
        let multiline = matches!(target, EditorTarget::Value(_)) && text.contains('\n');
        let Some(row) = target.row(session) else {
            return;
        };
        if let EditorTarget::Value(id) = target
            && !(tab.revealed ^ tab.exceptions.contains(&id))
            && !tab.exceptions.remove(&id)
        {
            tab.exceptions.insert(id);
        }
        // Check before registering the editor: splice_focusable makes this
        // row unmeasured. Clicking even a partially visible row keeps its place.
        let visible = tab.list.bounds_for_item(row).is_some_and(|b| {
            b.bottom() > px(design::TITLE_HEIGHT + design::TAB_HEIGHT + design::CONTENT_TOP)
                && b.top() < window.viewport_size().height
        });
        if !visible {
            tab.list.scroll_to(ListOffset {
                item_ix: row,
                offset_in_item: px(0.),
            });
        }
        if matches!(target, EditorTarget::Value(_)) {
            self.record_interaction("focus_value");
        }
        let state = cx.new(|cx| {
            let mut state = TextareaState::new(window, cx)
                // Match the unwrapped preview, including values over ten lines.
                .auto_grow(1, usize::MAX)
                .soft_wrap(false)
                .submit_on_enter(!multiline);
            // Hide source delimiters without adding a user edit or undo entry.
            state.set_value(text, window, cx);
            if let Some(offset) = caret {
                state.set_selected_range(offset..offset, cx);
            }
            state
        });
        let subscription =
            cx.subscribe_in(&state, window, move |this, state, event, window, cx| {
                if !this
                    .editor
                    .as_ref()
                    .is_some_and(|e| e.tab == tab_id && e.target == target)
                {
                    return;
                }
                match event {
                    InputEvent::Focus => cx.notify(),
                    InputEvent::Change => {
                        this.record_interaction("edit");
                        if let Some(editor) = this.editor.as_mut() {
                            editor.changed = true;
                        }
                        this.error = None;
                        this.notice = None;
                        let text = format!("{draft_prefix}{}", state.read(cx).value());
                        if let Some(session) = this.tab_mut(tab_id).and_then(|t| t.content.as_mut())
                        {
                            match target {
                                EditorTarget::Value(id) => {
                                    let _ = session.set_content_draft(id, text);
                                }
                                EditorTarget::Text(field) => {
                                    let _ = session.set_text_draft(field, text);
                                }
                            }
                        }
                        cx.notify();
                    }
                    InputEvent::Blur => {
                        if let Some(session) = this.tab_mut(tab_id).and_then(|t| t.content.as_mut())
                        {
                            match target {
                                EditorTarget::Value(id) => {
                                    let _ = session.confirm_entry(id);
                                }
                                EditorTarget::Text(field) => {
                                    let _ = session.confirm_text_field(field);
                                }
                            }
                        }
                        cx.notify();
                    }
                    InputEvent::PressEnter { secondary, shift }
                        if *secondary || (!multiline && !*shift) =>
                    {
                        if state.update(cx, |state, cx| {
                            state.marked_text_range(window, cx).is_some()
                        }) {
                            return;
                        }
                        this.finish_editor(false, window, cx);
                        this.focus.focus(window, cx);
                    }
                    _ => {}
                }
            });
        if let Some(tab) = self.tab(tab_id) {
            tab.set_row_focus(row, Some(state.read(cx).focus_handle(cx)));
        }
        state.update(cx, |s, cx| s.focus(window, cx));
        self.editor = Some(ActiveEditor {
            tab: tab_id,
            target,
            row,
            state,
            source_prefix,
            changed: false,
            _subscription: subscription,
        });
        cx.notify();
    }

    pub(super) fn cycle_quotes(
        &mut self,
        tab_id: u64,
        id: EntryId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self
            .editor
            .as_ref()
            .filter(|e| e.tab == tab_id && e.target == EditorTarget::Value(id))
        else {
            return;
        };
        let state = editor.state.clone();
        let content = state.read(cx).value().to_string();
        if let Some(session) = self.tab_mut(tab_id).and_then(|tab| tab.content.as_mut()) {
            let _ = session.set_content_draft(id, content);
            if let Some(style) = session.quote_style(id) {
                let _ = session.set_quote_style(id, style.next());
                // Valid transitions are ordinary reversible document edits. Invalid
                // choices remain visible drafts so the next click can fix them.
                let _ = session.confirm_entry(id);
            }
        }
        self.error = None;
        self.notice = None;
        state.update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    pub(super) fn finish_editor(
        &mut self,
        cancel: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.editor.take() else {
            return;
        };
        let text = format!("{}{}", editor.source_prefix, editor.state.read(cx).value());
        if let Some(tab) = self.tab(editor.tab)
            && editor.row < tab.list.item_count()
        {
            tab.set_row_focus(editor.row, None);
        }
        if let Some(session) = self.tab_mut(editor.tab).and_then(|t| t.content.as_mut()) {
            match editor.target {
                EditorTarget::Value(id) => {
                    if cancel {
                        session.cancel_draft(id);
                    } else {
                        if editor.changed {
                            let _ = session.set_content_draft(id, text);
                        }
                        let _ = session.confirm_entry(id);
                    }
                }
                EditorTarget::Text(field) => {
                    if cancel {
                        session.cancel_text_draft(field);
                    } else {
                        if editor.changed {
                            let _ = session.set_text_draft(field, text);
                        }
                        let _ = session.confirm_text_field(field);
                    }
                }
            }
        }
        if cancel {
            self.error = None;
        }
        self.focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn editor_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(search) = &self.search
            && search.state.read(cx).focus_handle(cx).is_focused(window)
        {
            if search.state.update(cx, |state, cx| {
                state.marked_text_range(window, cx).is_some()
            }) {
                return;
            }
            match event.keystroke.key.as_str() {
                "escape" => {
                    self.search = None;
                    self.focus.focus(window, cx);
                    cx.stop_propagation();
                    cx.notify();
                }
                "enter" => {
                    self.find_match(event.keystroke.modifiers.shift, cx);
                    cx.stop_propagation();
                }
                _ => {}
            }
            return;
        }
        if let Some(dialog) = self.dialog.as_ref() {
            if dialog.name.update(cx, |state, cx| {
                state.marked_text_range(window, cx).is_some()
            }) {
                return;
            }
            if event.keystroke.key == "escape" {
                self.dialog = None;
                self.focus.focus(window, cx);
                cx.stop_propagation();
                cx.notify();
            } else if event.keystroke.key == "enter"
                && (event.keystroke.modifiers.platform
                    || dialog.name.read(cx).focus_handle(cx).is_focused(window))
            {
                self.apply_rename(window, cx);
                cx.stop_propagation();
            }
            return;
        }
        let Some(editor) = &self.editor else { return };
        if !editor.state.read(cx).focus_handle(cx).is_focused(window) {
            return;
        }
        if editor.state.update(cx, |state, cx| {
            state.marked_text_range(window, cx).is_some()
        }) {
            return;
        }
        match event.keystroke.key.as_str() {
            "escape" => {
                self.finish_editor(true, window, cx);
                cx.stop_propagation();
            }
            "tab"
                if !event.keystroke.modifiers.control
                    && !event.keystroke.modifiers.alt
                    && !event.keystroke.modifiers.platform =>
            {
                self.navigate_field(event.keystroke.modifiers.shift, window, cx);
                cx.stop_propagation();
            }
            _ => {}
        }
    }

    pub(super) fn navigate_field(
        &mut self,
        backwards: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.editor.as_ref() else {
            cx.propagate();
            return;
        };
        if editor.state.update(cx, |state, cx| {
            state.marked_text_range(window, cx).is_some()
        }) {
            cx.propagate();
            return;
        }
        let (tab, current) = (editor.tab, editor.target);
        let fields: Vec<_> = self
            .tab(tab)
            .and_then(|t| t.content.as_ref())
            .map(|s| {
                s.document
                    .blocks()
                    .iter()
                    .enumerate()
                    .flat_map(|(row, b)| match &b.kind {
                        env_core::BlockKind::Assignment(e) => {
                            let mut fields = vec![
                                EditorTarget::Text(TextField::Name(e.id)),
                                EditorTarget::Value(e.id),
                            ];
                            if e.inline_comment.is_some() {
                                fields.push(EditorTarget::Text(TextField::InlineComment(e.id)));
                            }
                            fields
                        }
                        env_core::BlockKind::Comment => {
                            vec![EditorTarget::Text(TextField::Comment(row))]
                        }
                        _ => vec![],
                    })
                    .collect()
            })
            .unwrap_or_default();
        let next = fields
            .iter()
            .position(|field| *field == current)
            .and_then(|i| {
                if backwards {
                    i.checked_sub(1).and_then(|i| fields.get(i))
                } else {
                    fields.get(i + 1)
                }
            })
            .copied();
        if let Some(target) = next
            && let Some(tab) = self.tab(tab)
            && let Some(session) = &tab.content
            && let Some(row) = target.row(session)
        {
            tab.list.scroll_to_reveal_item(row);
        }
        self.finish_editor(false, window, cx);
        if let Some(target) = next {
            self.begin_field_editor(tab, target, None, window, cx);
        }
    }

    pub(super) fn history(&mut self, redo: bool, window: &mut Window, cx: &mut Context<Self>) {
        let local = self
            .search
            .as_ref()
            .is_some_and(|s| s.state.read(cx).focus_handle(cx).is_focused(window))
            || self
                .dialog
                .as_ref()
                .is_some_and(|d| d.name.read(cx).focus_handle(cx).is_focused(window))
            || self
                .active_tab()
                .and_then(|tab| tab.new_variable.as_ref())
                .is_some_and(|row| {
                    row.name.read(cx).focus_handle(cx).is_focused(window)
                        || row.value.read(cx).focus_handle(cx).is_focused(window)
                });
        if local {
            if redo {
                window.dispatch_action(Box::new(gpui_kit::component::input::Redo), cx);
            } else {
                window.dispatch_action(Box::new(gpui_kit::component::input::Undo), cx);
            }
            return;
        }
        if self.dialog.is_some() {
            return;
        }
        if let Some(editor) = &self.editor
            && editor.state.read(cx).focus_handle(cx).is_focused(window)
        {
            if redo {
                window.dispatch_action(Box::new(gpui_kit::component::input::Redo), cx);
            } else {
                window.dispatch_action(Box::new(gpui_kit::component::input::Undo), cx);
            }
            return;
        }
        self.finish_editor(false, window, cx);
        let result = self
            .active_tab_mut()
            .and_then(|t| t.content.as_mut())
            .map(|s| if redo { s.redo() } else { s.undo() });
        if let Some(Err(error)) = result {
            self.show_session_error(error, window, cx);
        }
        cx.notify();
    }

    pub(super) fn show_session_error(
        &mut self,
        error: SessionError,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(tab) = self.active_tab() {
            let id = tab.id;
            match &error {
                SessionError::Edit { entry, .. } => self.begin_editor(id, *entry, window, cx),
                SessionError::TextEdit { field, .. } => {
                    self.begin_text_editor(id, *field, None, window, cx)
                }
                SessionError::NewVariable(error) => {
                    self.focus_new_variable(
                        id,
                        matches!(error, EditError::InvalidValue),
                        window,
                        cx,
                    );
                    self.error = None;
                    cx.notify();
                    return;
                }
                _ => {}
            }
        }
        self.error = Some(UiError::Session(error));
        cx.notify();
    }

    pub(super) fn open_rename_dialog(
        &mut self,
        entry: EntryId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.finish_editor(false, window, cx);
        let Some(tab) = self.active_tab() else { return };
        let Some(session) = tab.content.as_ref() else {
            return;
        };
        if session.document.is_read_only() {
            return;
        }
        let tab_id = tab.id;
        let key = session
            .document
            .entry(entry)
            .and_then(|e| session.document.text(e.name.clone()))
            .unwrap_or("")
            .to_owned();
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(key)
                .placeholder(tr!("variable.name_example"))
        });
        name.update(cx, |s, cx| {
            s.focus(window, cx);
            s.select_all(window, cx);
        });
        self.dialog = Some(RenameDialog {
            tab: tab_id,
            entry,
            name,
            error: None,
        });
        cx.notify();
    }

    fn apply_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.dialog.as_ref() else {
            return;
        };
        let tab_id = dialog.tab;
        let entry = dialog.entry;
        let name = dialog.name.read(cx).value().to_string();
        let Some(session) = self.tab_mut(tab_id).and_then(|t| t.content.as_mut()) else {
            return;
        };
        match session.rename(entry, &name) {
            Ok(()) => {
                self.dialog = None;
                self.error = None;
                self.begin_editor(tab_id, entry, window, cx);
            }
            Err(error) => {
                if let Some(d) = self.dialog.as_mut() {
                    d.error = Some(UiError::Session(error));
                }
            }
        }
        cx.notify();
    }

    pub(super) fn remove_variable(
        &mut self,
        tab_id: u64,
        entry: EntryId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.finish_editor(false, window, cx);
        let Some(session) = self.tab_mut(tab_id).and_then(|t| t.content.as_mut()) else {
            return;
        };
        match session.remove(entry) {
            // The menu's trigger disappears with the row, so keep shortcuts on the editor.
            Ok(()) => self.needs_root_focus = true,
            Err(error) => self.show_session_error(error, window, cx),
        }
        cx.notify();
    }

    pub(super) fn dialog_view(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dialog = self.dialog.as_ref()?;
        Some(
            div()
                .id("variable-modal")
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(rgba(0x00000099))
                .occlude()
                .child(
                    div()
                        .w(px(540.))
                        .max_w_full()
                        .p_6()
                        .bg(rgb(CONTENT_BG))
                        .rounded_lg()
                        .flex()
                        .flex_col()
                        .gap_4()
                        .child(Label::new(tr!("variable.rename")))
                        .child(Input::new(&dialog.name).aria_label(tr!("variable.name")))
                        .when_some(
                            dialog.error.as_ref().map(UiError::message),
                            |this, error| this.child(Label::new(error).text_color(rgb(0xfda4af))),
                        )
                        .child(
                            div()
                                .flex()
                                .justify_end()
                                .gap_2()
                                .child(
                                    Button::new("cancel-variable")
                                        .ghost()
                                        .label(tr!("action.cancel"))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.dialog = None;
                                            this.focus.focus(window, cx);
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    Button::new("confirm-variable")
                                        .primary()
                                        .label(tr!("variable.rename_confirm"))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.apply_rename(window, cx)
                                        })),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }
}
