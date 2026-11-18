use super::*;
use crate::{
    i18n::tr,
    session::{QuoteStyle, token_content},
};
use env_core::{BlockKind, Document};
use gpui_kit::component::input::{InputEvent, InputState, TextareaState};

pub(super) struct NewVariableRow {
    pub name: Entity<InputState>,
    pub value: Entity<TextareaState>,
    pub focus: FocusHandle,
    pub revealed: bool,
    pub paste_issue: Option<PasteIssue>,
    _subscriptions: [Subscription; 2],
}

pub(super) struct PasteIssue {
    pub error: EditError,
    pub fragment: Option<Document>,
}

fn plain_assignment(fragment: &Document) -> Option<(&str, &str)> {
    let mut blocks = fragment
        .blocks()
        .iter()
        .filter(|block| !matches!(block.kind, BlockKind::Blank));
    let block = blocks.next()?;
    let BlockKind::Assignment(entry) = &block.kind else {
        return None;
    };
    if blocks.next().is_some()
        || entry.commented
        || entry.inline_comment.is_some()
        || !fragment
            .text(block.range.start..entry.name.start)?
            .trim()
            .is_empty()
    {
        return None;
    }
    Some((
        fragment.text(entry.name.clone())?,
        fragment.text(entry.value.clone())?,
    ))
}

impl AppWindow {
    pub(super) fn begin_new_variable(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialog.is_some() || self.closing {
            return;
        }
        self.finish_editor(false, window, cx);
        let quote_style = self.preferences.read(cx).default_quote_style();
        let Some(tab) = self.active_tab_mut() else {
            return;
        };
        tab.sync_document_list();
        let Some(session) = tab.content.as_mut() else {
            return;
        };
        let tab_id = tab.id;
        if session.begin_new_variable(quote_style) {
            let name =
                cx.new(|cx| InputState::new(window, cx).placeholder(tr!("variable.name_example")));
            let value = cx.new(|cx| {
                TextareaState::new(window, cx)
                    .auto_grow(1, usize::MAX)
                    .soft_wrap(false)
                    .submit_on_enter(true)
            });
            let name_changed = cx.subscribe_in(
                &name,
                window,
                move |this, state, event, window, cx| match event {
                    InputEvent::Change => this.update_new_variable(tab_id, None, cx),
                    InputEvent::PressEnter { shift: false, .. }
                        if state.update(cx, |state, cx| {
                            state.marked_text_range(window, cx).is_none()
                        }) =>
                    {
                        this.apply_new_variable(tab_id, false, window, cx);
                    }
                    _ => {}
                },
            );
            let value_changed = cx.subscribe_in(
                &value,
                window,
                move |this, state, event, window, cx| match event {
                    InputEvent::Change => this.update_new_variable(tab_id, None, cx),
                    InputEvent::PressEnter { shift: false, .. }
                        if state.update(cx, |state, cx| {
                            state.marked_text_range(window, cx).is_none()
                        }) =>
                    {
                        this.apply_new_variable(tab_id, false, window, cx);
                    }
                    _ => {}
                },
            );
            tab.new_variable = Some(NewVariableRow {
                name,
                value,
                focus: cx.focus_handle(),
                revealed: true,
                paste_issue: None,
                _subscriptions: [name_changed, value_changed],
            });
            tab.sync_document_list();
        }
        self.focus_new_variable(tab_id, false, window, cx);
    }

    pub(super) fn focus_new_variable(
        &mut self,
        tab_id: u64,
        value: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tab_mut(tab_id) else {
            return;
        };
        let Some(session) = &tab.content else { return };
        let Some(view) = &mut tab.new_variable else {
            return;
        };
        let index = session.document.blocks().len();
        let viewport = tab.list.viewport_bounds();
        if !tab.list.bounds_for_item(index).is_some_and(|bounds| {
            bounds.top() >= viewport.top()
                && bounds.top() + px(design::FIELD_HEIGHT) <= viewport.bottom()
        }) {
            tab.list.scroll_to(ListOffset {
                item_ix: index,
                offset_in_item: px(0.),
            });
        }
        if value {
            view.revealed = true;
            view.value.update(cx, |state, cx| state.focus(window, cx));
        } else {
            view.name.update(cx, |state, cx| state.focus(window, cx));
        }
        cx.notify();
    }

    pub(super) fn update_new_variable(
        &mut self,
        tab_id: u64,
        style: Option<QuoteStyle>,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tab_mut(tab_id) else {
            return;
        };
        let (Some(row), Some(session)) = (&mut tab.new_variable, &mut tab.content) else {
            return;
        };
        let Some(draft) = session.new_variable() else {
            return;
        };
        if row.paste_issue.take().is_some() {
            tab.list.remeasure();
        }
        session.update_new_variable(
            row.name.read(cx).value().to_string(),
            row.value.read(cx).value().to_string(),
            style.unwrap_or(draft.quote_style),
        );
        self.error = None;
        self.notice = None;
        cx.notify();
    }

    pub(super) fn paste_new_variables(
        &mut self,
        id: u64,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.active_tab().filter(|tab| tab.id == id) else {
            return;
        };
        let Some(row) = &tab.new_variable else { return };
        let fragment = Document::from_bytes(text.into_bytes());
        if let Some((name, token)) = plain_assignment(&fragment) {
            let style = QuoteStyle::from_token(token);
            row.name
                .update(cx, |state, cx| state.set_value(name.to_owned(), window, cx));
            row.value.update(cx, |state, cx| {
                state.set_value(token_content(token).to_owned(), window, cx)
            });
            self.update_new_variable(id, Some(style), cx);
            self.focus_new_variable(id, true, window, cx);
        } else {
            self.append_pasted_variables(id, fragment, false, window, cx);
        }
    }

    pub(super) fn confirm_pasted_duplicates(
        &mut self,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(fragment) = self
            .tab(id)
            .and_then(|tab| tab.new_variable.as_ref())
            .and_then(|row| row.paste_issue.as_ref())
            .and_then(|issue| issue.fragment.clone())
        {
            self.append_pasted_variables(id, fragment, true, window, cx);
        }
    }

    fn append_pasted_variables(
        &mut self,
        id: u64,
        fragment: Document,
        allow_duplicates: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.active_tab_mut().filter(|tab| tab.id == id) else {
            return;
        };
        let (Some(row), Some(session)) = (&mut tab.new_variable, &mut tab.content) else {
            return;
        };
        match session.append_variables(&fragment, allow_duplicates) {
            Ok(()) => {
                tab.sync_document_list();
                self.error = None;
                self.notice = None;
                self.focus.focus(window, cx);
            }
            Err(SessionError::NewVariable(error)) => {
                let duplicate = matches!(
                    error,
                    EditError::DuplicateName { .. } | EditError::DuplicateFragmentName
                );
                row.paste_issue = Some(PasteIssue {
                    error,
                    fragment: duplicate.then_some(fragment),
                });
                tab.list.remeasure();
            }
            Err(error) => self.show_session_error(error, window, cx),
        }
        cx.notify();
    }

    pub(super) fn apply_new_variable(
        &mut self,
        tab_id: u64,
        allow_duplicate: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.finish_editor(false, window, cx);
        let Some(tab) = self.tab_mut(tab_id) else {
            return;
        };
        let Some(session) = tab.content.as_mut() else {
            return;
        };
        match session.confirm_new_variable(allow_duplicate) {
            Ok(Some(id)) => {
                if tab
                    .new_variable
                    .as_ref()
                    .is_some_and(|row| row.revealed != tab.revealed)
                {
                    tab.exceptions.insert(id);
                }
                tab.sync_document_list();
                self.error = None;
                self.focus.focus(window, cx);
            }
            Err(error) => self.show_session_error(error, window, cx),
            Ok(None) => {}
        }
        cx.notify();
    }

    pub(super) fn toggle_new_variable(
        &mut self,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tab_mut(tab_id) else {
            return;
        };
        let Some(row) = &mut tab.new_variable else {
            return;
        };
        row.revealed = !row.revealed;
        if !row.revealed && row.value.read(cx).focus_handle(cx).is_focused(window) {
            row.name.update(cx, |state, cx| state.focus(window, cx));
        }
        tab.list.remeasure();
        cx.notify();
    }

    pub(super) fn navigate_new_variable(
        &mut self,
        tab_id: u64,
        backwards: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(row) = self.tab(tab_id).and_then(|tab| tab.new_variable.as_ref()) else {
            return;
        };
        if row.is_composing(window, cx) {
            cx.propagate();
        } else if !backwards && row.name.read(cx).focus_handle(cx).is_focused(window) {
            self.focus_new_variable(tab_id, true, window, cx);
        } else if backwards && row.value.read(cx).focus_handle(cx).is_focused(window) {
            self.focus_new_variable(tab_id, false, window, cx);
        } else if backwards {
            window.focus_prev(cx);
        } else {
            window.focus_next(cx);
        }
    }

    pub(super) fn cancel_new_variable(
        &mut self,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tab_mut(tab_id) else {
            return;
        };
        if tab
            .new_variable
            .as_ref()
            .is_some_and(|row| row.is_composing(window, cx))
        {
            cx.propagate();
            return;
        }
        if let Some(session) = tab.content.as_mut() {
            session.cancel_new_variable();
            tab.sync_document_list();
        }
        self.focus.focus(window, cx);
        cx.notify();
    }
}

impl NewVariableRow {
    fn is_composing(&self, window: &mut Window, cx: &mut App) -> bool {
        if self.name.read(cx).focus_handle(cx).is_focused(window) {
            self.name.update(cx, |state, cx| {
                state.marked_text_range(window, cx).is_some()
            })
        } else {
            self.value.read(cx).focus_handle(cx).is_focused(window)
                && self.value.update(cx, |state, cx| {
                    state.marked_text_range(window, cx).is_some()
                })
        }
    }
}
