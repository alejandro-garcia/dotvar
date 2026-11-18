use super::*;
use crate::i18n::tr;
use crate::session::{CommentTarget, RenameTarget, SessionError};
use env_core::{BlockKind, EditError, TextField};
use gpui_kit::component::input::{Input, InputEvent, InputState, TextareaState};
use std::cell::Cell;

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
    preview: Option<FieldPreview>,
    _subscription: Subscription,
}

#[derive(Clone)]
pub(super) struct FieldPreview {
    pub focus: FocusHandle,
    restoring_focus: Rc<Cell<bool>>,
}

/// Keep source spacing after `#` outside the visible comment editor.
pub(super) fn comment_content(text: &str) -> (&str, &str) {
    let content = text.trim_start_matches([' ', '\t']);
    text.split_at(text.len() - content.len())
}

pub(super) struct RenameDialog {
    tab: u64,
    target: RenameTarget,
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
    pub(super) fn field_preview(
        &self,
        tab_id: u64,
        target: EditorTarget,
        auto_edit: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> FieldPreview {
        let preview = window
            .use_keyed_state(format!("field-preview-{tab_id}-{target:?}"), cx, |_, cx| {
                FieldPreview {
                    focus: cx.focus_handle().tab_stop(true),
                    restoring_focus: Rc::new(Cell::new(false)),
                }
            })
            .read(cx)
            .clone();
        if !preview.focus.is_focused(window) {
            preview.restoring_focus.set(false);
        } else if auto_edit && window.last_input_was_keyboard() && !preview.restoring_focus.get() {
            // Run after layout so replacing the preview can safely update the list.
            let owner = cx.weak_entity();
            let entering = preview.clone();
            window.defer(cx, move |window, cx| {
                if entering.focus.is_focused(window) {
                    let _ = owner.update(cx, |this, cx| {
                        this.edit_preview(tab_id, target, entering, None, window, cx);
                    });
                }
            });
        }
        preview
    }

    pub(super) fn edit_preview(
        &mut self,
        tab_id: u64,
        target: EditorTarget,
        preview: FieldPreview,
        caret: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.begin_field_editor(tab_id, target, caret, window, cx);
        if let Some(editor) = &mut self.editor
            && editor.tab == tab_id
            && editor.target == target
        {
            editor.preview = Some(preview);
        }
    }

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

    pub(super) fn add_comment(&mut self, tab_id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialog.is_some()
            || self.closing
            || self.active_tab().map(|tab| tab.id) != Some(tab_id)
        {
            return;
        }
        self.finish_editor(false, window, cx);
        let Some(tab) = self.tab_mut(tab_id) else {
            return;
        };
        let Some(session) = tab.content.as_mut() else {
            return;
        };
        match session.add_comment() {
            Ok(field) => {
                tab.sync_document_list();
                self.error = None;
                self.notice = None;
                self.begin_text_editor(tab_id, field, None, window, cx);
            }
            Err(error) => self.show_session_error(error, window, cx),
        }
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
                        this.close_field_editor(false, window, cx);
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
            preview: None,
            _subscription: subscription,
        });
        cx.notify();
    }

    pub(super) fn cycle_quotes(
        &mut self,
        tab_id: u64,
        id: EntryId,
        refocus_editor: bool,
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
        if refocus_editor {
            state.update(cx, |state, cx| state.focus(window, cx));
        }
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
        let focused = editor.state.read(cx).focus_handle(cx).is_focused(window);
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
        if focused {
            self.focus.focus(window, cx);
        }
        cx.notify();
    }

    fn close_field_editor(&mut self, cancel: bool, window: &mut Window, cx: &mut Context<Self>) {
        let preview = self
            .editor
            .as_ref()
            .and_then(|editor| editor.preview.clone());
        self.finish_editor(cancel, window, cx);
        if let Some(preview) = preview {
            preview.restoring_focus.set(true);
            preview.focus.focus(window, cx);
        }
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
        if event.keystroke.key != "escape" {
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
        self.close_field_editor(true, window, cx);
        cx.stop_propagation();
    }

    pub(super) fn keyboard_document_row(
        &mut self,
        tab_id: u64,
        index: usize,
        content: AnyElement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let row_focus = window
            .use_keyed_state(format!("keyboard-row-{tab_id}-{index}"), cx, |_, cx| {
                cx.focus_handle()
            })
            .read(cx)
            .clone();
        let pending = self
            .tab_mut(tab_id)
            .and_then(|tab| tab.pending_row_focus.take_if(|(row, _)| *row == index));
        let trailing_anchor = pending
            .filter(|(_, backwards)| *backwards)
            .map(|_| cx.focus_handle());
        if let Some((_, backwards)) = pending {
            let anchor = trailing_anchor.as_ref().unwrap_or(&row_focus).clone();
            let previous = window.focused(cx);
            window.on_next_frame(move |window, cx| {
                if window.focused(cx) == previous && window.last_input_was_keyboard() {
                    anchor.focus(window, cx);
                    if backwards {
                        window.focus_prev(cx);
                    } else {
                        window.focus_next(cx);
                    }
                }
            });
        }
        let next_focus = row_focus.clone();
        let prev_focus = row_focus.clone();
        div()
            .id(("keyboard-row", index))
            .relative()
            .track_focus(&row_focus)
            .key_context("EnvDocumentRow")
            .on_action(cx.listener(move |this, _: &NextField, window, cx| {
                this.navigate_document_row(tab_id, index, &next_focus, false, window, cx);
            }))
            .on_action(cx.listener(move |this, _: &PreviousField, window, cx| {
                this.navigate_document_row(tab_id, index, &prev_focus, true, window, cx);
            }))
            .child(content)
            .when_some(trailing_anchor, |this, focus| {
                this.child(
                    div()
                        .id("end-of-row")
                        .absolute()
                        .size_0()
                        .track_focus(&focus),
                )
            })
            .into_any_element()
    }

    fn navigate_document_row(
        &mut self,
        tab_id: u64,
        index: usize,
        row_focus: &FocusHandle,
        backwards: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let composing = self.editor.as_ref().is_some_and(|editor| {
            editor.state.read(cx).focus_handle(cx).is_focused(window)
                && editor.state.update(cx, |state, cx| {
                    state.marked_text_range(window, cx).is_some()
                })
        }) || self
            .tab(tab_id)
            .and_then(|tab| tab.new_variable.as_ref())
            .is_some_and(|row| row.is_composing(window, cx));
        if composing {
            cx.propagate();
            return;
        }
        if backwards {
            window.focus_prev(cx);
        } else {
            window.focus_next(cx);
        }
        if row_focus.contains_focused(window, cx) {
            return;
        }
        let Some(tab) = self.tab_mut(tab_id) else {
            return;
        };
        let Some(session) = &tab.content else { return };
        let navigable = |row: usize| {
            session.document.blocks().get(row).is_none_or(|block| {
                matches!(block.kind, BlockKind::Assignment(_) | BlockKind::Comment)
            })
        };
        let next = if backwards {
            (0..index).rev().find(|&row| navigable(row))
        } else {
            (index + 1..tab.list.item_count()).find(|&row| navigable(row))
        };
        let Some(next) = next else { return };
        let viewport = tab.list.viewport_bounds();
        let bounds = tab.list.bounds_for_item(next);
        // GPUI registers tab stops for rendered rows. Mount the adjacent row
        // before continuing whenever native traversal reaches that boundary.
        if bounds.is_none_or(|row| row.top() >= viewport.bottom() || row.bottom() <= viewport.top())
        {
            tab.pending_row_focus = Some((next, backwards));
            if bounds.is_some_and(|row| row.size.height <= viewport.size.height) {
                tab.list.scroll_to_reveal_item(next);
            } else {
                tab.list.scroll_to(ListOffset {
                    item_ix: next,
                    offset_in_item: px(0.),
                });
            }
            cx.notify();
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
        let Some(key) = session
            .document
            .entry(entry)
            .and_then(|e| session.document.text(e.name.clone()))
        else {
            return;
        };
        let target = session.capture_rename_target(entry);
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(key.to_owned())
                .placeholder(tr!("variable.name_example"))
        });
        name.update(cx, |s, cx| {
            s.focus(window, cx);
            s.select_all(window, cx);
        });
        self.dialog = Some(RenameDialog {
            tab: tab_id,
            target,
            name,
            error: None,
        });
        cx.notify();
    }

    fn apply_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut dialog) = self.dialog.take() else {
            return;
        };
        let tab_id = dialog.tab;
        let name = dialog.name.read(cx).value();
        let result = self
            .tab_mut(tab_id)
            .and_then(|t| t.content.as_mut())
            .ok_or(SessionError::StaleRenameTarget)
            .and_then(|session| session.rename_captured(&dialog.target, &name));
        match result {
            Ok(entry) => {
                self.error = None;
                self.begin_editor(tab_id, entry, window, cx);
            }
            Err(SessionError::StaleRenameTarget) => {
                self.error = Some(UiError::Message("edit_error.stale_revision"));
                self.focus.focus(window, cx);
            }
            Err(error) => {
                dialog.set_error(UiError::Session(error));
                self.dialog = Some(dialog);
            }
        }
        cx.notify();
    }

    pub(super) fn set_variable_commented(
        &mut self,
        tab_id: u64,
        entry: EntryId,
        commented: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.finish_editor(false, window, cx);
        let Some(session) = self.tab_mut(tab_id).and_then(|t| t.content.as_mut()) else {
            return;
        };
        match session.set_commented(entry, commented) {
            Ok(()) => {
                self.error = None;
                self.notice = None;
            }
            Err(error) => self.show_session_error(error, window, cx),
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

    pub(super) fn remove_comment(
        &mut self,
        tab_id: u64,
        target: &CommentTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.active_tab().filter(|tab| tab.id == tab_id) else {
            return;
        };
        if !tab
            .content
            .as_ref()
            .is_some_and(|session| session.matches_comment_target(target))
        {
            self.error = Some(UiError::Message("edit_error.stale_revision"));
            cx.notify();
            return;
        }
        self.finish_editor(false, window, cx);
        let Some(session) = self.tab_mut(tab_id).and_then(|tab| tab.content.as_mut()) else {
            return;
        };
        match session.remove_comment(target.index) {
            Ok(()) => {
                self.error = None;
                self.notice = None;
                self.needs_root_focus = true;
            }
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
