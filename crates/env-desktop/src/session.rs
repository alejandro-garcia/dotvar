//! In-memory document work. Drafts and history are never persisted separately.

use crate::i18n::{edit_error_message, tr};
use crate::storage::FileBaseline;
use env_core::{BlockKind, Document, EditError, EntryId, TextField};
use std::{collections::BTreeMap, fmt, sync::Arc};

pub struct Draft {
    pub text: String,
    pub error: Option<EditError>,
    content: Option<String>,
    quote_style: Option<QuoteStyle>,
}

pub struct NewVariableDraft {
    pub name: String,
    pub value: String,
    pub quote_style: QuoteStyle,
    pub error: Option<EditError>,
}

impl NewVariableDraft {
    fn has_content(&self) -> bool {
        !self.name.is_empty() || !self.value.is_empty()
    }

    pub fn token(&self) -> String {
        self.quote_style.wrap(&self.value)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuoteStyle {
    Unquoted,
    #[default]
    Double,
    Single,
}

impl QuoteStyle {
    pub(crate) fn from_token(token: &str) -> Self {
        if token_content(token) == token {
            Self::Unquoted
        } else if token.starts_with('"') {
            Self::Double
        } else {
            Self::Single
        }
    }

    pub fn delimiter(self) -> Option<&'static str> {
        match self {
            Self::Unquoted => None,
            Self::Double => Some("\""),
            Self::Single => Some("'"),
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Unquoted => Self::Double,
            Self::Double => Self::Single,
            Self::Single => Self::Unquoted,
        }
    }

    pub fn label(self) -> std::borrow::Cow<'static, str> {
        match self {
            Self::Unquoted => tr!("editor.quote_unquoted"),
            Self::Double => tr!("editor.quote_double"),
            Self::Single => tr!("editor.quote_single"),
        }
    }

    fn wrap(self, content: &str) -> String {
        match self.delimiter() {
            Some(quote) => format!("{quote}{content}{quote}"),
            None => content.to_owned(),
        }
    }
}

/// Only delimiters are hidden. Escapes and variable references remain source text.
pub(crate) fn token_content(token: &str) -> &str {
    if token.len() >= 2
        && matches!(token.as_bytes()[0], b'\'' | b'"')
        && token.as_bytes().last() == token.as_bytes().first()
    {
        &token[1..token.len() - 1]
    } else {
        token
    }
}

#[derive(Debug)]
pub enum SessionError {
    Edit { entry: EntryId, error: EditError },
    TextEdit { field: TextField, error: EditError },
    NewVariable(EditError),
    NewComment(EditError),
    Reorder(EditError),
    Busy,
    StaleRenameTarget,
    StaleSave,
    UnsupportedEncoding,
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Edit { error, .. }
            | Self::TextEdit { error, .. }
            | Self::NewVariable(error)
            | Self::NewComment(error)
            | Self::Reorder(error) => edit_error_message(error),
            Self::Busy => tr!("session.busy"),
            Self::StaleRenameTarget => tr!("edit_error.stale_revision"),
            Self::StaleSave => tr!("session.stale_save"),
            Self::UnsupportedEncoding => tr!("session.copy_unsupported_encoding"),
        };
        f.write_str(&message)
    }
}

impl std::error::Error for SessionError {}

pub struct SaveRequest {
    pub baseline: FileBaseline,
    pub bytes: Vec<u8>,
    pub revision: u64,
    ticket: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalUpdate {
    /// Working content is unchanged; an identical-byte baseline may be refreshed.
    Unchanged,
    /// Clean content was replaced from disk, including its entry IDs/history.
    Reloaded,
    /// Pending work is retained; an explicit reload or Save As is required.
    Conflict,
    /// A save is in flight. Schedule another read after completion.
    Busy,
}

pub struct RenameTarget {
    document_identity: Arc<()>,
    entry: EntryId,
}

pub struct DocumentSession {
    pub document: Document,
    // Replace this marker whenever a fresh parse can reuse occurrence IDs.
    document_identity: Arc<()>,
    pub baseline: FileBaseline,
    drafts: BTreeMap<EntryId, Draft>,
    text_drafts: BTreeMap<TextField, Draft>,
    new_variable: Option<NewVariableDraft>,
    next_ticket: u64,
    in_flight: Option<u64>,
    draft_epoch: u64,
}

impl DocumentSession {
    pub fn new(baseline: FileBaseline) -> Self {
        Self {
            document: Document::from_bytes(baseline.bytes.clone()),
            document_identity: Arc::new(()),
            baseline,
            drafts: BTreeMap::new(),
            text_drafts: BTreeMap::new(),
            new_variable: None,
            next_ticket: 0,
            in_flight: None,
            draft_epoch: 0,
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.document.bytes() != self.baseline.bytes
            || !self.drafts.is_empty()
            || !self.text_drafts.is_empty()
            || self
                .new_variable
                .as_ref()
                .is_some_and(NewVariableDraft::has_content)
    }

    pub fn is_saving(&self) -> bool {
        self.in_flight.is_some()
    }

    pub fn work_revision(&self) -> (u64, u64) {
        (self.document.revision(), self.draft_epoch)
    }

    pub fn can_reload(&self, expected_work: (u64, u64), expected_saved: &[u8]) -> bool {
        !self.is_saving()
            && self.work_revision() == expected_work
            && self.baseline.bytes == expected_saved
    }

    /// Apply a current external snapshot. The caller must reject reads started
    /// before a later save/reload/path change; their result may already be stale.
    /// Identical bytes at the same chosen/resolved path preserve IDs and history.
    pub fn reconcile_external(&mut self, baseline: FileBaseline) -> ExternalUpdate {
        if self.is_saving() {
            return ExternalUpdate::Busy;
        }
        if self.baseline.same_reference(&baseline) {
            return ExternalUpdate::Unchanged;
        }
        if self.baseline.chosen_path == baseline.chosen_path
            && self.baseline.resolved_path == baseline.resolved_path
            && self.baseline.bytes == baseline.bytes
        {
            self.baseline = baseline;
            return ExternalUpdate::Unchanged;
        }
        if self.is_dirty() {
            return ExternalUpdate::Conflict;
        }
        self.document = Document::from_bytes(baseline.bytes.clone());
        self.document_identity = Arc::new(());
        self.baseline = baseline;
        self.drafts.clear();
        self.text_drafts.clear();
        if self.document.is_read_only() {
            self.new_variable = None;
        }
        self.draft_epoch = self.draft_epoch.wrapping_add(1);
        ExternalUpdate::Reloaded
    }

    pub fn text_draft(&self, field: TextField) -> Option<&Draft> {
        self.text_drafts.get(&field)
    }

    pub fn field_text(&self, field: TextField) -> Option<&str> {
        if let Some(draft) = self.text_draft(field) {
            return Some(&draft.text);
        }
        self.document.text(self.document.text_field_range(field)?)
    }

    pub fn set_text_draft(&mut self, field: TextField, text: String) -> Result<(), SessionError> {
        if self.document.is_read_only() {
            return Err(SessionError::TextEdit {
                field,
                error: EditError::ReadOnly,
            });
        }
        let range = self
            .document
            .text_field_range(field)
            .ok_or(SessionError::TextEdit {
                field,
                error: EditError::UnsafeBoundary,
            })?;
        if self.field_text(field) == Some(text.as_str()) {
            return Ok(());
        }
        self.draft_epoch = self.draft_epoch.wrapping_add(1);
        if self.document.text(range) == Some(text.as_str()) {
            self.text_drafts.remove(&field);
        } else {
            let error = self.document.validate_text_field(field, &text).err();
            self.text_drafts.insert(
                field,
                Draft {
                    text,
                    error,
                    content: None,
                    quote_style: None,
                },
            );
        }
        Ok(())
    }

    pub fn cancel_text_draft(&mut self, field: TextField) {
        if self.text_drafts.remove(&field).is_some() {
            self.draft_epoch = self.draft_epoch.wrapping_add(1);
        }
    }

    pub fn confirm_text_field(&mut self, field: TextField) -> Result<(), SessionError> {
        let Some(draft) = self.text_drafts.get_mut(&field) else {
            return Ok(());
        };
        match self
            .document
            .set_text_field(field, &draft.text, self.document.revision())
        {
            Ok(_) => {
                self.text_drafts.remove(&field);
                Ok(())
            }
            Err(error) => {
                draft.error = Some(error.clone());
                Err(SessionError::TextEdit { field, error })
            }
        }
    }

    fn confirm_text_fields(&mut self) -> Result<(), SessionError> {
        let fields: Vec<_> = self.text_drafts.keys().copied().collect();
        let mut first_error = None;
        for field in fields {
            if let Err(error) = self.confirm_text_field(field) {
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    pub fn draft(&self, id: EntryId) -> Option<&Draft> {
        self.drafts.get(&id)
    }

    pub fn value(&self, id: EntryId) -> Option<&str> {
        if let Some(draft) = self.drafts.get(&id) {
            return Some(&draft.text);
        }
        self.document.entry_value(id)
    }

    /// Shared by the preview and editor so focusing a field never reveals syntax.
    pub fn value_content(&self, id: EntryId) -> Option<&str> {
        if let Some(content) = self.draft(id).and_then(|draft| draft.content.as_deref()) {
            return Some(content);
        }
        self.value(id).map(token_content)
    }

    pub fn quote_style(&self, id: EntryId) -> Option<QuoteStyle> {
        if let Some(style) = self.draft(id).and_then(|draft| draft.quote_style) {
            return Some(style);
        }
        self.value(id).map(QuoteStyle::from_token)
    }

    /// Typing never changes the delimiter; the quote control owns that choice.
    pub fn set_content_draft(&mut self, id: EntryId, content: String) -> Result<(), SessionError> {
        let style = self.quote_style(id).ok_or(SessionError::Edit {
            entry: id,
            error: EditError::MissingEntry(id),
        })?;
        self.set_styled_draft(id, content, style)
    }

    pub fn set_quote_style(&mut self, id: EntryId, style: QuoteStyle) -> Result<(), SessionError> {
        let content = self
            .value_content(id)
            .ok_or(SessionError::Edit {
                entry: id,
                error: EditError::MissingEntry(id),
            })?
            .to_owned();
        self.set_styled_draft(id, content, style)
    }

    fn set_styled_draft(
        &mut self,
        id: EntryId,
        content: String,
        style: QuoteStyle,
    ) -> Result<(), SessionError> {
        if self.document.is_read_only() {
            return Err(SessionError::Edit {
                entry: id,
                error: EditError::ReadOnly,
            });
        }
        let original = self.document.entry_value(id).ok_or(SessionError::Edit {
            entry: id,
            error: EditError::MissingEntry(id),
        })?;
        let token = style.wrap(&content);
        let matches_style = QuoteStyle::from_token(&token) == style;
        if matches_style && original == token {
            self.cancel_draft(id);
            return Ok(());
        }
        if self.draft(id).is_some_and(|draft| {
            draft.text == token
                && draft.content.as_deref() == Some(content.as_str())
                && draft.quote_style == Some(style)
        }) {
            return Ok(());
        }
        let error = if !matches_style {
            Some(EditError::InvalidValue)
        } else {
            self.document.validate_value(id, &token).err()
        };
        // Keep typed content and the explicitly selected style even if the
        // token is invalid, or equals the source but reinterprets its quotes.
        self.draft_epoch = self.draft_epoch.wrapping_add(1);
        self.drafts.insert(
            id,
            Draft {
                text: token,
                error,
                content: Some(content),
                quote_style: Some(style),
            },
        );
        Ok(())
    }

    /// A draft is dirty immediately, even before blur/Enter, including invalid text.
    pub fn set_draft(&mut self, id: EntryId, text: String) -> Result<(), SessionError> {
        if self.document.is_read_only() {
            return Err(SessionError::Edit {
                entry: id,
                error: EditError::ReadOnly,
            });
        }
        if self.value(id) == Some(text.as_str()) {
            return Ok(());
        }
        let original = self.document.entry_value(id).ok_or(SessionError::Edit {
            entry: id,
            error: EditError::MissingEntry(id),
        })?;
        self.draft_epoch = self.draft_epoch.wrapping_add(1);
        if original == text {
            self.drafts.remove(&id);
        } else {
            let error = self.document.validate_value(id, &text).err();
            self.drafts.insert(
                id,
                Draft {
                    text,
                    error,
                    content: None,
                    quote_style: None,
                },
            );
        }
        Ok(())
    }

    pub fn cancel_draft(&mut self, id: EntryId) {
        if self.drafts.remove(&id).is_some() {
            self.draft_epoch = self.draft_epoch.wrapping_add(1);
        }
    }

    pub fn confirm_entry(&mut self, id: EntryId) -> Result<(), SessionError> {
        let Some(draft) = self.drafts.get_mut(&id) else {
            return Ok(());
        };
        if let Some(error) = &draft.error {
            return Err(SessionError::Edit {
                entry: id,
                error: error.clone(),
            });
        }
        let result = self
            .document
            .set_value(id, &draft.text, self.document.revision());
        match result {
            Ok(_) => {
                self.drafts.remove(&id);
                Ok(())
            }
            Err(error) => {
                draft.error = Some(error.clone());
                Err(SessionError::Edit { entry: id, error })
            }
        }
    }

    /// Confirm valid fields and keep every invalid draft with its diagnosis.
    pub fn confirm_all(&mut self) -> Result<(), SessionError> {
        let ids: Vec<_> = self
            .document
            .blocks()
            .iter()
            .filter_map(|block| {
                if let BlockKind::Assignment(entry) = &block.kind {
                    self.drafts.contains_key(&entry.id).then_some(entry.id)
                } else {
                    None
                }
            })
            .collect();
        let mut first_error = self.confirm_text_fields().err();
        for id in ids {
            if let Err(error) = self.confirm_entry(id)
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        if let Some(error) = first_error {
            return Err(error);
        }
        if self
            .new_variable
            .as_ref()
            .is_some_and(NewVariableDraft::has_content)
        {
            self.confirm_new_variable(false)?;
        }
        Ok(())
    }

    /// Confirm only this field and return its exact source token. Visibility is
    /// deliberately outside this API: an explicit copy never copies a mask.
    pub fn copy_token(&mut self, id: EntryId) -> Result<String, SessionError> {
        if self.document.is_read_only() {
            return Err(SessionError::UnsupportedEncoding);
        }
        self.confirm_entry(id)?;
        self.value(id).map(str::to_owned).ok_or(SessionError::Edit {
            entry: id,
            error: EditError::MissingEntry(id),
        })
    }

    /// Return the whole confirmed working source, without writing or changing
    /// the saved baseline. Invalid drafts stop copying; callers must leave the
    /// clipboard unchanged on error rather than copy a partial document.
    pub fn copy_document(&mut self) -> Result<String, SessionError> {
        self.confirm_all()?;
        std::str::from_utf8(self.document.bytes())
            .map(str::to_owned)
            .map_err(|_| SessionError::UnsupportedEncoding)
    }

    pub fn move_block(
        &mut self,
        from: usize,
        to: usize,
        expected_work: (u64, u64),
    ) -> Result<bool, SessionError> {
        if self.work_revision() != expected_work {
            return Err(SessionError::Reorder(EditError::StaleRevision {
                expected: expected_work.0,
                actual: self.document.revision(),
            }));
        }
        // Resolve row-indexed comment drafts before their positions change.
        self.confirm_all()?;
        self.document
            .move_block(from, to, self.document.revision())
            .map_err(SessionError::Reorder)
    }

    pub fn capture_rename_target(&self, entry: EntryId) -> RenameTarget {
        RenameTarget {
            document_identity: self.document_identity.clone(),
            entry,
        }
    }

    pub fn rename_captured(
        &mut self,
        target: &RenameTarget,
        name: &str,
    ) -> Result<EntryId, SessionError> {
        if !Arc::ptr_eq(&self.document_identity, &target.document_identity) {
            return Err(SessionError::StaleRenameTarget);
        }
        self.rename(target.entry, name)?;
        Ok(target.entry)
    }

    pub fn rename(&mut self, id: EntryId, name: &str) -> Result<(), SessionError> {
        self.confirm_text_fields()?;
        self.confirm_entry(id)?;
        self.document
            .rename_entry(id, name, self.document.revision())
            .map(|_| ())
            .map_err(|error| SessionError::Edit { entry: id, error })
    }

    pub fn set_commented(&mut self, id: EntryId, commented: bool) -> Result<(), SessionError> {
        self.confirm_text_fields()?;
        self.confirm_entry(id)?;
        self.document
            .set_commented(id, commented, self.document.revision())
            .map(|_| ())
            .map_err(|error| SessionError::Edit { entry: id, error })
    }

    pub fn new_variable(&self) -> Option<&NewVariableDraft> {
        self.new_variable.as_ref()
    }

    pub fn add_comment(&mut self) -> Result<TextField, SessionError> {
        self.document
            .add_comment(self.document.revision())
            .map(TextField::Comment)
            .map_err(SessionError::NewComment)
    }

    pub fn begin_new_variable(&mut self, quote_style: QuoteStyle) -> bool {
        if self.document.is_read_only() || self.new_variable.is_some() {
            return false;
        }
        self.new_variable = Some(NewVariableDraft {
            name: String::new(),
            value: String::new(),
            quote_style,
            error: None,
        });
        self.draft_epoch = self.draft_epoch.wrapping_add(1);
        true
    }

    pub fn update_new_variable(&mut self, name: String, value: String, quote_style: QuoteStyle) {
        let Some(draft) = self.new_variable.as_mut() else {
            return;
        };
        if draft.name != name || draft.value != value || draft.quote_style != quote_style {
            draft.name = name;
            draft.value = value;
            draft.quote_style = quote_style;
            draft.error = None;
            self.draft_epoch = self.draft_epoch.wrapping_add(1);
        }
    }

    pub fn cancel_new_variable(&mut self) {
        if self.new_variable.take().is_some() {
            self.draft_epoch = self.draft_epoch.wrapping_add(1);
        }
    }

    pub fn append_variables(
        &mut self,
        fragment: &Document,
        allow_duplicates: bool,
    ) -> Result<(), SessionError> {
        self.confirm_text_fields()?;
        self.document
            .append_entries(fragment, allow_duplicates, self.document.revision())
            .map_err(SessionError::NewVariable)?;
        self.cancel_new_variable();
        Ok(())
    }

    pub fn confirm_new_variable(
        &mut self,
        allow_duplicate: bool,
    ) -> Result<Option<EntryId>, SessionError> {
        self.confirm_text_fields()?;
        let Some(draft) = self.new_variable.as_mut() else {
            return Ok(None);
        };
        // An explicit override only applies to the draft that reported a duplicate.
        let allow_duplicate =
            allow_duplicate && matches!(draft.error, Some(EditError::DuplicateName { .. }));
        match self.document.add_entry(
            &draft.name,
            &draft.token(),
            allow_duplicate,
            self.document.revision(),
        ) {
            Ok(id) => {
                self.cancel_new_variable();
                Ok(Some(id))
            }
            Err(error) => {
                draft.error = Some(error.clone());
                Err(SessionError::NewVariable(error))
            }
        }
    }

    pub fn remove(&mut self, id: EntryId) -> Result<(), SessionError> {
        self.confirm_text_fields()?;
        self.confirm_entry(id)?;
        self.document
            .remove_entry(id, self.document.revision())
            .map_err(|error| SessionError::Edit { entry: id, error })?;
        self.drafts.remove(&id);
        Ok(())
    }

    pub fn undo(&mut self) -> Result<bool, SessionError> {
        self.confirm_all()?;
        Ok(self.document.undo())
    }

    pub fn redo(&mut self) -> Result<bool, SessionError> {
        self.confirm_all()?;
        Ok(self.document.redo())
    }

    /// Capture an immutable revision. Later typing must remain dirty after completion.
    pub fn prepare_save(&mut self, save_as: bool) -> Result<Option<SaveRequest>, SessionError> {
        if self.is_saving() {
            return Err(SessionError::Busy);
        }
        self.confirm_all()?;
        if !save_as && !self.is_dirty() {
            return Ok(None);
        }
        self.next_ticket = self
            .next_ticket
            .checked_add(1)
            .ok_or(SessionError::StaleSave)?;
        self.in_flight = Some(self.next_ticket);
        Ok(Some(SaveRequest {
            baseline: self.baseline.clone(),
            bytes: self.document.bytes().to_vec(),
            revision: self.document.revision(),
            ticket: self.next_ticket,
        }))
    }

    pub fn complete_save(
        &mut self,
        request: &SaveRequest,
        result: FileBaseline,
    ) -> Result<(), SessionError> {
        if self.in_flight != Some(request.ticket) || result.bytes != request.bytes {
            return Err(SessionError::StaleSave);
        }
        self.in_flight = None;
        self.baseline = result;
        Ok(())
    }

    pub fn fail_save(&mut self, request: &SaveRequest) {
        if self.in_flight == Some(request.ticket) {
            self.in_flight = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage;

    fn session() -> (tempfile::TempDir, DocumentSession, EntryId, EntryId) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        std::fs::write(&path, b"# before\r\nA=one # tail\r\nB=two\r\n").unwrap();
        let session = DocumentSession::new(storage::open(path).unwrap());
        let ids: Vec<_> = session
            .document
            .blocks()
            .iter()
            .filter_map(|b| match &b.kind {
                BlockKind::Assignment(e) => Some(e.id),
                _ => None,
            })
            .collect();
        (dir, session, ids[0], ids[1])
    }

    fn quoted_session(source: &[u8]) -> (tempfile::TempDir, DocumentSession) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        std::fs::write(&path, source).unwrap();
        let session = DocumentSession::new(storage::open(path).unwrap());
        (dir, session)
    }

    #[test]
    fn commenting_commits_the_value_draft_and_saves_a_reopenable_assignment() {
        let (dir, mut session, id, _) = session();
        let path = dir.path().join(".env");
        session.set_draft(id, "'pending'".into()).unwrap();
        session.set_commented(id, true).unwrap();
        assert!(session.is_dirty());
        assert_eq!(session.copy_token(id).unwrap(), "'pending'");
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"# before\r\nA=one # tail\r\nB=two\r\n"
        );
        let request = session.prepare_save(false).unwrap().unwrap();
        storage::save(&request.baseline, &request.bytes).unwrap();
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"# before\r\n# A='pending' # tail\r\nB=two\r\n"
        );

        let mut reopened = DocumentSession::new(storage::open(path).unwrap());
        assert!(reopened.document.entry(id).unwrap().commented);
        assert_eq!(reopened.value_content(id), Some("pending"));
        reopened.set_commented(id, false).unwrap();
        assert_eq!(
            reopened.document.bytes(),
            b"# before\r\nA='pending' # tail\r\nB=two\r\n"
        );
    }

    #[test]
    fn adding_a_comment_keeps_the_variable_draft_and_saves_both() {
        let source = b"# before\r\nA=one\r\n";
        let (dir, mut session) = quoted_session(source);
        let path = dir.path().join(".env");
        assert!(session.begin_new_variable(QuoteStyle::Unquoted));
        session.update_new_variable("B".into(), "two".into(), QuoteStyle::Unquoted);

        let field = session.add_comment().unwrap();
        assert_eq!(field, TextField::Comment(2));
        assert_eq!(session.field_text(field), Some(" "));
        assert_eq!(session.new_variable().unwrap().name, "B");
        assert_eq!(std::fs::read(&path).unwrap(), source);

        session.set_text_draft(field, " Nota".into()).unwrap();
        let save = session.prepare_save(false).unwrap().unwrap();
        let baseline = storage::save(&save.baseline, &save.bytes).unwrap();
        session.complete_save(&save, baseline).unwrap();
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"# before\r\nA=one\r\n# Nota\r\nB=two\r\n"
        );
        assert!(!session.is_dirty());
    }

    #[test]
    fn new_variable_stays_in_memory_until_confirmation_and_appends_one_undoable_entry() {
        for (style, expected) in [
            (
                QuoteStyle::Double,
                "# before\r\nA=one\r\nNEW_KEY=\"with space\"",
            ),
            (
                QuoteStyle::Single,
                "# before\r\nA=one\r\nNEW_KEY='with space'",
            ),
            (
                QuoteStyle::Unquoted,
                "# before\r\nA=one\r\nNEW_KEY=with space",
            ),
        ] {
            let (_dir, mut s) = quoted_session(b"# before\r\nA=one");
            assert!(s.begin_new_variable(style));
            assert!(!s.is_dirty());
            assert!(s.prepare_save(false).unwrap().is_none());
            assert!(!s.begin_new_variable(style.next()));
            let selected = s.new_variable().unwrap().quote_style;
            assert_eq!(selected, style);

            s.update_new_variable("NEW_KEY".into(), "with space".into(), selected);
            assert!(s.is_dirty());
            assert_eq!(s.document.bytes(), b"# before\r\nA=one");
            let save = s.prepare_save(false).unwrap().unwrap();
            assert_eq!(save.bytes, expected.as_bytes());
            assert!(s.new_variable().is_none());
            assert!(s.undo().unwrap());
            assert_eq!(s.document.bytes(), b"# before\r\nA=one");
        }
    }

    #[test]
    fn new_variable_validation_preserves_the_draft_and_requires_fresh_duplicate_consent() {
        let (_dir, mut s, a, b) = session();
        assert!(s.begin_new_variable(QuoteStyle::Unquoted));
        s.update_new_variable("01-invalid".into(), "value".into(), QuoteStyle::Unquoted);
        assert!(matches!(
            s.copy_document(),
            Err(SessionError::NewVariable(EditError::InvalidName))
        ));
        assert_eq!(s.new_variable().unwrap().value, "value");

        s.update_new_variable("A".into(), "value".into(), QuoteStyle::Unquoted);
        assert!(
            matches!(s.confirm_new_variable(false), Err(SessionError::NewVariable(EditError::DuplicateName { existing })) if existing == a)
        );
        s.update_new_variable("B".into(), "value".into(), QuoteStyle::Unquoted);
        assert!(
            matches!(s.confirm_new_variable(true), Err(SessionError::NewVariable(EditError::DuplicateName { existing })) if existing == b)
        );
        assert!(s.confirm_new_variable(true).unwrap().is_some());
        assert_eq!(
            s.document.bytes(),
            b"# before\r\nA=one # tail\r\nB=two\r\nB=value\r\n"
        );
    }

    #[test]
    fn row_reorder_confirms_pending_text_saves_and_undoes_one_movement() {
        let (_dir, mut s, a, _) = session();
        s.set_text_draft(TextField::Comment(0), " changed".into())
            .unwrap();
        s.set_text_draft(TextField::Name(a), "RENAMED".into())
            .unwrap();
        let work = s.work_revision();
        assert!(s.move_block(0, 2, work).unwrap());
        let expected = b"RENAMED=one # tail\r\nB=two\r\n# changed\r\n";
        assert_eq!(s.document.bytes(), expected);
        let request = s.prepare_save(false).unwrap().unwrap();
        let disk = storage::save(&request.baseline, &request.bytes).unwrap();
        s.complete_save(&request, disk).unwrap();
        assert!(!s.is_dirty());
        assert!(s.undo().unwrap());
        assert_eq!(
            s.document.bytes(),
            b"# changed\r\nRENAMED=one # tail\r\nB=two\r\n"
        );
        assert!(s.redo().unwrap());
        assert_eq!(s.document.bytes(), expected);
    }

    #[test]
    fn row_reorder_never_discards_invalid_or_concurrent_work() {
        let (_dir, mut s, a, _) = session();
        s.set_text_draft(TextField::Name(a), "BAD NAME".into())
            .unwrap();
        let before = s.document.bytes().to_vec();
        assert!(s.move_block(0, 2, s.work_revision()).is_err());
        assert_eq!(s.document.bytes(), before);
        assert_eq!(s.field_text(TextField::Name(a)), Some("BAD NAME"));
        s.cancel_text_draft(TextField::Name(a));
        let work = s.work_revision();
        s.set_content_draft(a, "later".into()).unwrap();
        assert!(matches!(
            s.move_block(0, 2, work),
            Err(SessionError::Reorder(EditError::StaleRevision { .. }))
        ));
        assert_eq!(s.document.bytes(), before);
        assert_eq!(s.value_content(a), Some("later"));
    }

    #[test]
    fn inline_text_drafts_save_exact_source_and_participate_in_history() {
        let (_dir, mut s, a, _) = session();
        let before = s.document.bytes().to_vec();
        for (field, text) in [
            (TextField::Name(a), "RENAMED"),
            (TextField::Comment(0), " configuración"),
            (TextField::InlineComment(a), " nuevo"),
        ] {
            s.set_text_draft(field, text.into()).unwrap();
        }
        s.set_content_draft(a, "edited".into()).unwrap();
        assert!(s.is_dirty());
        let save = s.prepare_save(false).unwrap().unwrap();
        assert_eq!(
            save.bytes,
            "# configuración\r\nRENAMED=edited # nuevo\r\nB=two\r\n".as_bytes()
        );
        let baseline = storage::save(&save.baseline, &save.bytes).unwrap();
        s.complete_save(&save, baseline).unwrap();
        assert!(!s.is_dirty());
        for _ in 0..4 {
            assert!(s.undo().unwrap());
        }
        assert_eq!(s.document.bytes(), before);
        for _ in 0..4 {
            assert!(s.redo().unwrap());
        }
        assert!(!s.is_dirty());
    }

    #[test]
    fn invalid_text_survives_other_edits_blocks_save_and_can_be_cancelled() {
        let (_dir, mut s, a, b) = session();
        let name = TextField::Name(a);
        s.set_text_draft(name, "BAD NAME".into()).unwrap();
        s.set_text_draft(TextField::Comment(0), " changed".into())
            .unwrap();
        s.set_content_draft(b, "later".into()).unwrap();
        assert!(
            matches!(s.prepare_save(false), Err(SessionError::TextEdit { field, .. }) if field == name)
        );
        assert_eq!(s.field_text(name), Some("BAD NAME"));
        assert!(s.text_draft(name).unwrap().error.is_some());
        assert!(s.copy_document().is_err());
        s.cancel_text_draft(name);
        assert_eq!(
            s.copy_document().unwrap(),
            "# changed\r\nA=one # tail\r\nB=later\r\n"
        );
        s.set_text_draft(TextField::InlineComment(a), "bad\nINJECTED=yes".into())
            .unwrap();
        assert!(s.remove(a).is_err());
        assert!(s.document.entry(a).is_some());
        s.cancel_text_draft(TextField::InlineComment(a));
        s.set_text_draft(name, "A".into()).unwrap();
        assert!(s.text_draft(name).is_none());
    }

    #[test]
    fn content_hides_only_outer_quotes_without_dirtying_or_decoding_source() {
        let source = b"A=plain\r\nB='${A} # literal' # tail\r\nC=\"say \\\"hi\\\"\\n${A}\"\r\nD=''\r\nE=\"\"\r\nF=\r\nG='line one\r\nline two'\r\n";
        let (_dir, mut s) = quoted_session(source);
        let expected = [
            "plain",
            "${A} # literal",
            r#"say \"hi\"\n${A}"#,
            "",
            "",
            "",
            "line one\r\nline two",
        ];
        for (index, content) in expected.into_iter().enumerate() {
            let id = EntryId(index as u64 + 1);
            assert_eq!(s.value_content(id), Some(content));
            s.set_content_draft(id, content.into()).unwrap();
            s.confirm_entry(id).unwrap();
        }
        assert_eq!(s.document.bytes(), source);
        assert_eq!(s.document.revision(), 0);
        assert!(!s.is_dirty());
        assert!(s.prepare_save(false).unwrap().is_none());
    }

    #[test]
    fn editing_content_preserves_each_quote_style_comments_and_terminators_on_disk() {
        let source = b"\xef\xbb\xbf# before\r\nA = 'old' # tail\r\nB=\"old\"\r\nC=old\r\n";
        let (_dir, mut s) = quoted_session(source);
        for id in [EntryId(1), EntryId(2), EntryId(3)] {
            s.set_content_draft(id, "new=value".into()).unwrap();
            assert_eq!(s.value_content(id), Some("new=value"));
        }
        let request = s.prepare_save(false).unwrap().unwrap();
        let expected =
            b"\xef\xbb\xbf# before\r\nA = 'new=value' # tail\r\nB=\"new=value\"\r\nC=new=value\r\n";
        assert_eq!(request.bytes, expected);
        let disk = storage::save(&request.baseline, &request.bytes).unwrap();
        s.complete_save(&request, disk).unwrap();
        assert_eq!(std::fs::read(&s.baseline.chosen_path).unwrap(), expected);
        assert!(!s.is_dirty());
        for _ in 0..3 {
            assert!(s.undo().unwrap());
        }
        assert_eq!(s.document.bytes(), source);
        assert_eq!(s.value_content(EntryId(1)), Some("old"));
        for _ in 0..3 {
            assert!(s.redo().unwrap());
        }
        assert_eq!(s.document.bytes(), expected);
    }

    #[test]
    fn empty_content_keeps_existing_delimiters_and_copy_keeps_the_source_token() {
        let (_dir, mut s) = quoted_session(b"A='old'\nB=\"old\"\nC=old\n");
        for id in [EntryId(1), EntryId(2), EntryId(3)] {
            s.set_content_draft(id, "".into()).unwrap();
        }
        assert_eq!(s.copy_token(EntryId(1)).unwrap(), "''");
        assert_eq!(s.copy_token(EntryId(2)).unwrap(), "\"\"");
        assert_eq!(s.copy_document().unwrap(), "A=''\nB=\"\"\nC=\n");
        assert_eq!(s.value_content(EntryId(1)), Some(""));
    }

    #[test]
    fn typing_keeps_unquoted_style_and_the_control_can_resolve_invalid_content() {
        for (content, style, token) in [
            ("plain", QuoteStyle::Unquoted, "plain"),
            ("two words", QuoteStyle::Unquoted, "two words"),
            ("  spaced  ", QuoteStyle::Double, "\"  spaced  \""),
            ("has # hash", QuoteStyle::Double, "\"has # hash\""),
            (
                "line one\nline two",
                QuoteStyle::Double,
                "\"line one\nline two\"",
            ),
            (
                "'literal quotes'",
                QuoteStyle::Double,
                "\"'literal quotes'\"",
            ),
            (
                "\"literal quotes\"",
                QuoteStyle::Single,
                "'\"literal quotes\"'",
            ),
        ] {
            let (_dir, mut s, a, _) = session();
            s.set_content_draft(a, content.into()).unwrap();
            assert_eq!(s.value_content(a), Some(content));
            assert_eq!(s.quote_style(a), Some(QuoteStyle::Unquoted));
            if style != QuoteStyle::Unquoted {
                assert!(s.confirm_entry(a).is_err());
                s.set_quote_style(a, style).unwrap();
            }
            assert_eq!(s.copy_token(a).unwrap(), token);
            assert_eq!(s.value_content(a), Some(content));
            assert!(s.copy_document().unwrap().contains(" # tail\r\nB=two\r\n"));
        }
    }

    #[test]
    fn quote_control_cycles_three_styles_without_changing_content_or_neighbours() {
        let source = b"\xef\xbb\xbfA = plain # tail\r\nA='neighbour'\r\n";
        let (_dir, mut s) = quoted_session(source);
        let a = EntryId(1);
        for (style, token) in [
            (QuoteStyle::Double, "\"plain\""),
            (QuoteStyle::Single, "'plain'"),
            (QuoteStyle::Unquoted, "plain"),
        ] {
            assert_eq!(s.quote_style(a).unwrap().next(), style);
            s.set_quote_style(a, style).unwrap();
            assert_eq!(s.quote_style(a), Some(style));
            assert_eq!(s.value_content(a), Some("plain"));
            assert_eq!(s.copy_token(a).unwrap(), token);
            assert!(
                s.copy_document()
                    .unwrap()
                    .ends_with(" # tail\r\nA='neighbour'\r\n")
            );
        }
        assert_eq!(s.document.bytes(), source);
        assert!(!s.is_dirty());
        assert!(s.undo().unwrap());
        assert_eq!(s.quote_style(a), Some(QuoteStyle::Single));
        assert!(s.undo().unwrap());
        assert_eq!(s.quote_style(a), Some(QuoteStyle::Double));
        assert!(s.redo().unwrap());
        assert_eq!(s.quote_style(a), Some(QuoteStyle::Single));
    }

    #[test]
    fn invalid_quote_choice_survives_typing_and_can_be_cycled_back_without_loss() {
        let (_dir, mut s) = quoted_session(b"A='line one\nline two' # tail\n");
        let a = EntryId(1);
        s.set_quote_style(a, QuoteStyle::Unquoted).unwrap();
        assert_eq!(s.quote_style(a), Some(QuoteStyle::Unquoted));
        assert!(s.prepare_save(false).is_err());
        s.set_content_draft(a, "changed\nline two".into()).unwrap();
        assert_eq!(s.quote_style(a), Some(QuoteStyle::Unquoted));
        assert!(s.confirm_entry(a).is_err());
        s.set_quote_style(a, QuoteStyle::Double).unwrap();
        let request = s.prepare_save(false).unwrap().unwrap();
        assert_eq!(request.bytes, b"A=\"changed\nline two\" # tail\n");
        assert_eq!(s.value_content(a), Some("changed\nline two"));
    }

    #[test]
    fn typing_quotes_cannot_reinterpret_content_as_delimiters_even_for_identical_source_bytes() {
        let (_dir, mut s) = quoted_session(b"A=\"old\"\n");
        let a = EntryId(1);
        s.set_quote_style(a, QuoteStyle::Unquoted).unwrap();
        s.set_content_draft(a, "\"old\"".into()).unwrap();
        assert_eq!(s.value_content(a), Some("\"old\""));
        assert_eq!(s.quote_style(a), Some(QuoteStyle::Unquoted));
        assert!(s.confirm_entry(a).is_err());
        assert!(s.is_dirty());
        s.set_quote_style(a, QuoteStyle::Single).unwrap();
        assert_eq!(s.copy_token(a).unwrap(), "'\"old\"'");
        assert_eq!(s.value_content(a), Some("\"old\""));
    }

    #[test]
    fn selected_quote_style_persists_after_confirm_and_empty_style_changes_are_real_edits() {
        let (_dir, mut s) = quoted_session(b"A=\nB=keep\n");
        let a = EntryId(1);
        s.set_quote_style(a, QuoteStyle::Double).unwrap();
        s.confirm_entry(a).unwrap();
        assert_eq!(s.value(a), Some("\"\""));
        assert!(s.is_dirty());
        s.set_content_draft(a, "typed".into()).unwrap();
        assert_eq!(s.copy_token(a).unwrap(), "\"typed\"");
        s.set_quote_style(a, QuoteStyle::Single).unwrap();
        s.cancel_draft(a);
        assert_eq!(s.quote_style(a), Some(QuoteStyle::Double));
    }

    #[test]
    fn incompatible_content_retains_its_draft_and_cannot_change_quotes_or_inject_source() {
        for (source, content) in [
            ("A=old # tail\nB=keep\n", "\"can't\""),
            ("A='old' # tail\nB=keep\n", "can't"),
            (
                "A=\"old\" # tail\nB=keep\n",
                "bad\" # comment\nINJECTED=value",
            ),
            ("A=\"old\" # tail\nB=keep\n", "trailing\\"),
        ] {
            let (_dir, mut s) = quoted_session(source.as_bytes());
            let id = EntryId(1);
            s.set_content_draft(id, content.into()).unwrap();
            assert_eq!(s.value_content(id), Some(content));
            assert!(s.draft(id).unwrap().error.is_some());
            assert!(s.prepare_save(false).is_err());
            assert!(s.copy_token(id).is_err());
            assert_eq!(s.document.bytes(), source.as_bytes());
            assert_eq!(s.value_content(id), Some(content));
            s.cancel_draft(id);
            assert_eq!(s.value_content(id), Some("old"));
            assert!(!s.is_dirty());
        }
    }

    #[test]
    fn quoted_multiline_edits_preserve_literal_escapes_and_reverting_clears_the_draft() {
        let source = b"A=\"first\r\nsecond\\n${REF}\" # tail\r\n";
        let (_dir, mut s) = quoted_session(source);
        let id = EntryId(1);
        s.set_content_draft(id, "changed\r\nsecond\\n${REF}".into())
            .unwrap();
        s.set_content_draft(id, "first\r\nsecond\\n${REF}".into())
            .unwrap();
        assert!(!s.is_dirty());
        s.set_content_draft(id, "changed\r\nsecond\\n${REF}".into())
            .unwrap();
        assert_eq!(
            s.copy_document().unwrap(),
            "A=\"changed\r\nsecond\\n${REF}\" # tail\r\n"
        );
        assert!(s.undo().unwrap());
        assert_eq!(s.document.bytes(), source);
    }

    #[test]
    fn copy_token_confirms_only_its_literal_draft_without_saving_or_touching_other_drafts() {
        let (_dir, mut s, a, b) = session();
        let saved = s.baseline.clone();
        s.set_draft(a, "'literal # token'".into()).unwrap();
        s.set_draft(b, "invalid\nINJECTED=value".into()).unwrap();
        assert_eq!(s.copy_token(a).unwrap(), "'literal # token'");
        assert!(s.draft(a).is_none());
        assert_eq!(s.value(b), Some("invalid\nINJECTED=value"));
        assert!(s.draft(b).unwrap().error.is_some());
        assert!(s.baseline.same_reference(&saved));
        assert_eq!(std::fs::read(&saved.chosen_path).unwrap(), saved.bytes);
        assert!(!s.is_saving());
        assert!(s.is_dirty());
        s.cancel_draft(b);
        assert!(s.undo().unwrap());
        assert_eq!(s.value(a), Some("one"));
        assert!(!s.is_dirty());
    }

    #[test]
    fn invalid_token_copy_fails_without_confirming_other_fields_or_losing_its_text() {
        let (_dir, mut s, a, b) = session();
        let original = s.document.bytes().to_vec();
        s.set_draft(a, "invalid\nINJECTED=value".into()).unwrap();
        s.set_draft(b, "pending".into()).unwrap();
        assert!(matches!(s.copy_token(a), Err(SessionError::Edit { entry, .. }) if entry == a));
        assert_eq!(s.value(a), Some("invalid\nINJECTED=value"));
        assert_eq!(s.value(b), Some("pending"));
        assert!(s.draft(b).is_some());
        assert_eq!(s.document.bytes(), original);
        assert_eq!(s.document.revision(), 0);
        assert!(matches!(
            s.copy_token(EntryId(999)),
            Err(SessionError::Edit {
                error: EditError::MissingEntry(EntryId(999)),
                ..
            })
        ));
    }

    #[test]
    fn document_copy_returns_whole_working_source_with_bom_comments_and_exact_terminators() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        let source = b"\xef\xbb\xbf# before\r\nA=one # tail\r\n\r\nB=two\n# after";
        std::fs::write(&path, source).unwrap();
        let mut s = DocumentSession::new(storage::open(path.clone()).unwrap());
        s.set_draft(EntryId(1), "\"line one\r\nline two\"".into())
            .unwrap();
        s.set_draft(EntryId(2), "".into()).unwrap();
        let copied = s.copy_document().unwrap();
        assert_eq!(
            copied.as_bytes(),
            b"\xef\xbb\xbf# before\r\nA=\"line one\r\nline two\" # tail\r\n\r\nB=\n# after"
        );
        assert_eq!(s.document.bytes(), copied.as_bytes());
        assert_eq!(std::fs::read(path).unwrap(), source);
        assert_eq!(s.baseline.bytes, source);
        assert!(s.drafts.is_empty());
        assert!(s.is_dirty());
        let revision = s.document.revision();
        assert_eq!(s.copy_document().unwrap(), copied);
        assert_eq!(s.document.revision(), revision);
        assert!(s.undo().unwrap());
        assert!(s.undo().unwrap());
        assert!(!s.is_dirty());
    }

    #[test]
    fn document_copy_rejects_invalid_drafts_and_non_utf8_without_lossy_or_partial_text() {
        let (_dir, mut s, a, b) = session();
        let saved = s.baseline.clone();
        s.set_draft(a, "pending_valid".into()).unwrap();
        s.set_draft(b, "invalid\nINJECTED=value".into()).unwrap();
        assert!(matches!(s.copy_document(), Err(SessionError::Edit { entry, .. }) if entry == b));
        assert_eq!(s.value(a), Some("pending_valid"));
        assert_eq!(s.value(b), Some("invalid\nINJECTED=value"));
        assert!(s.draft(b).unwrap().error.is_some());
        assert!(s.baseline.same_reference(&saved));
        assert_eq!(std::fs::read(&saved.chosen_path).unwrap(), saved.bytes);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("invalid.env");
        let bytes = b"VALUE=\xff\r\n";
        std::fs::write(&path, bytes).unwrap();
        let mut unsupported = DocumentSession::new(storage::open(path.clone()).unwrap());
        assert!(matches!(
            unsupported.copy_document(),
            Err(SessionError::UnsupportedEncoding)
        ));
        assert!(matches!(
            unsupported.copy_token(EntryId(1)),
            Err(SessionError::UnsupportedEncoding)
        ));
        assert_eq!(unsupported.document.bytes(), bytes);
        assert_eq!(std::fs::read(path).unwrap(), bytes);
        assert!(!unsupported.is_dirty());
        let message = SessionError::UnsupportedEncoding.to_string();
        assert!(!message.contains("VALUE"));
        assert!(!message.contains('\u{fffd}'));
    }

    #[test]
    fn copying_during_save_uses_current_work_without_advancing_the_saved_snapshot() {
        let (_dir, mut s, a, _) = session();
        s.set_draft(a, "save_snapshot".into()).unwrap();
        let request = s.prepare_save(false).unwrap().unwrap();
        s.set_draft(a, "later_work".into()).unwrap();
        assert_eq!(s.copy_token(a).unwrap(), "later_work");
        assert!(s.copy_document().unwrap().contains("A=later_work # tail"));
        assert!(s.is_saving());
        let saved = storage::save(&request.baseline, &request.bytes).unwrap();
        s.complete_save(&request, saved).unwrap();
        assert!(s.is_dirty());
        assert_eq!(s.value(a), Some("later_work"));
        assert!(s.undo().unwrap());
        assert!(!s.is_dirty());
        assert_eq!(s.value(a), Some("save_snapshot"));
    }

    #[test]
    fn invalid_drafts_survive_other_fields_and_prevent_any_save_snapshot() {
        let (_dir, mut s, a, b) = session();
        s.set_draft(a, "bad\nINJECTED=oops".into()).unwrap();
        s.set_draft(b, "changed".into()).unwrap();
        assert!(s.is_dirty());
        assert!(s.prepare_save(false).is_err());
        assert!(!s.is_saving());
        assert_eq!(s.value(a), Some("bad\nINJECTED=oops"));
        assert!(s.draft(a).unwrap().error.is_some());
        assert_eq!(s.value(b), Some("changed"));
        assert!(s.draft(b).is_none());
        assert_eq!(
            s.document.bytes(),
            b"# before\r\nA=one # tail\r\nB=changed\r\n"
        );
        s.cancel_draft(a);
        assert_eq!(s.value(a), Some("one"));
        assert!(s.is_dirty());
    }

    #[test]
    fn save_keeps_history_and_later_edits_dirty() {
        let (_dir, mut s, a, _) = session();
        s.set_draft(a, "saved".into()).unwrap();
        let request = s.prepare_save(false).unwrap().unwrap();
        assert!(matches!(s.prepare_save(false), Err(SessionError::Busy)));
        s.set_draft(a, "later".into()).unwrap();
        let disk = storage::save(&request.baseline, &request.bytes).unwrap();
        s.complete_save(&request, disk).unwrap();
        assert!(s.is_dirty());
        assert_eq!(s.value(a), Some("later"));
        s.confirm_all().unwrap();
        assert!(s.undo().unwrap());
        assert!(!s.is_dirty());
        assert!(s.undo().unwrap());
        assert!(s.is_dirty());
        assert!(s.redo().unwrap());
        assert!(!s.is_dirty());
    }

    #[test]
    fn cancelling_one_draft_and_failing_save_never_discards_other_work() {
        let (_dir, mut s, a, b) = session();
        s.set_draft(a, "cancel me".into()).unwrap();
        s.set_draft(b, "keep me".into()).unwrap();
        s.cancel_draft(a);
        assert_eq!(s.value(a), Some("one"));
        assert_eq!(s.value(b), Some("keep me"));
        let request = s.prepare_save(false).unwrap().unwrap();
        std::fs::write(&request.baseline.chosen_path, b"EXTERNAL=changed\n").unwrap();
        assert!(storage::save(&request.baseline, &request.bytes).is_err());
        s.fail_save(&request);
        assert!(!s.is_saving());
        assert!(s.is_dirty());
        assert_eq!(s.value(b), Some("keep me"));
    }

    #[test]
    fn identical_draft_and_undo_to_baseline_are_clean_without_save() {
        let (_dir, mut s, a, _) = session();
        s.set_draft(a, "one".into()).unwrap();
        assert!(!s.is_dirty());
        assert!(s.prepare_save(false).unwrap().is_none());
        s.set_draft(a, "changed".into()).unwrap();
        s.confirm_all().unwrap();
        assert!(s.undo().unwrap());
        assert!(!s.is_dirty());
    }

    #[test]
    fn reload_snapshot_cannot_discard_a_later_invalid_draft_or_a_pending_save() {
        let (_dir, mut s, a, _) = session();
        let expected = s.work_revision();
        let saved = s.baseline.bytes.clone();
        assert!(s.can_reload(expected, &saved));
        s.set_draft(a, "bad\nINJECTED=x".into()).unwrap();
        assert!(!s.can_reload(expected, &saved));
        s.cancel_draft(a);
        let expected = s.work_revision();
        s.set_draft(a, "new".into()).unwrap();
        let request = s.prepare_save(false).unwrap().unwrap();
        assert!(!s.can_reload(s.work_revision(), &saved));
        s.fail_save(&request);
        assert!(!s.can_reload(expected, &saved));
    }

    #[test]
    fn remove_never_discards_an_invalid_draft_and_undo_restores_a_valid_edit() {
        let (_dir, mut s, a, _) = session();
        let original = s.document.bytes().to_vec();
        s.set_draft(a, "bad\nINJECTED=x".into()).unwrap();
        assert!(s.remove(a).is_err());
        assert_eq!(s.value(a), Some("bad\nINJECTED=x"));
        assert_eq!(s.document.bytes(), original);
        s.set_draft(a, "edited".into()).unwrap();
        s.remove(a).unwrap();
        assert!(s.undo().unwrap());
        assert_eq!(s.value(a), Some("edited"));
    }

    #[test]
    fn own_save_event_keeps_later_invalid_draft_and_saved_history() {
        let (_dir, mut s, a, _) = session();
        s.set_draft(a, "saved".into()).unwrap();
        let request = s.prepare_save(false).unwrap().unwrap();
        let baseline = storage::save(&request.baseline, &request.bytes).unwrap();
        s.complete_save(&request, baseline).unwrap();
        s.set_draft(a, "invalid\nINJECTED=value".into()).unwrap();
        let revision = s.work_revision();
        let disk = storage::open(s.baseline.chosen_path.clone()).unwrap();
        assert_eq!(s.reconcile_external(disk), ExternalUpdate::Unchanged);
        assert_eq!(s.work_revision(), revision);
        assert_eq!(s.value(a), Some("invalid\nINJECTED=value"));
        assert!(s.draft(a).unwrap().error.is_some());
        s.cancel_draft(a);
        assert!(s.undo().unwrap());
        assert_eq!(s.value(a), Some("one"));
        assert!(s.is_dirty());
    }

    #[test]
    fn external_edit_keeps_dirty_bytes_invalid_drafts_and_history() {
        let (_dir, mut s, a, b) = session();
        s.set_draft(a, "ours".into()).unwrap();
        s.confirm_entry(a).unwrap();
        s.set_draft(b, "invalid\nINJECTED=value".into()).unwrap();
        let bytes = s.document.bytes().to_vec();
        let revision = s.work_revision();
        let saved = s.baseline.clone();
        std::fs::write(&saved.chosen_path, b"EXTERNAL=changed\n").unwrap();
        let disk = storage::open(saved.chosen_path.clone()).unwrap();
        assert_eq!(s.reconcile_external(disk), ExternalUpdate::Conflict);
        assert!(s.baseline.same_reference(&saved));
        assert_eq!(s.document.bytes(), bytes);
        assert_eq!(s.work_revision(), revision);
        assert_eq!(s.value(b), Some("invalid\nINJECTED=value"));
        s.cancel_draft(b);
        assert!(s.undo().unwrap());
        assert_eq!(s.value(a), Some("one"));
    }

    #[test]
    fn clean_external_change_reloads_exact_bytes_and_discards_old_history() {
        let (_dir, mut s, a, _) = session();
        s.set_draft(a, "temporary".into()).unwrap();
        s.confirm_entry(a).unwrap();
        s.undo().unwrap();
        assert!(!s.is_dirty());
        assert!(s.document.can_redo());
        let external = b"\xef\xbb\xbf# external\r\nEXTERNAL='value'\r\n";
        std::fs::write(&s.baseline.chosen_path, external).unwrap();
        let disk = storage::open(s.baseline.chosen_path.clone()).unwrap();
        assert_eq!(s.reconcile_external(disk), ExternalUpdate::Reloaded);
        assert_eq!(s.document.bytes(), external);
        assert_eq!(s.baseline.bytes, external);
        assert!(!s.is_dirty());
        assert!(!s.document.can_undo());
        assert!(!s.document.can_redo());
    }

    fn assert_rename_rejected_after_reload(original: &[u8], external: &[u8]) {
        let (_dir, mut s) = quoted_session(original);
        let target = s.capture_rename_target(s.document.entry(EntryId(2)).unwrap().id);
        assert_eq!(s.document.revision(), 0);
        std::fs::write(&s.baseline.chosen_path, external).unwrap();
        let disk = storage::open(s.baseline.chosen_path.clone()).unwrap();
        assert_eq!(s.reconcile_external(disk), ExternalUpdate::Reloaded);
        let revision = s.document.revision();
        assert_eq!(revision, 0);

        let result = s.rename_captured(&target, "RENAMED");

        assert_eq!(s.document.bytes(), external);
        assert_eq!(s.document.revision(), revision);
        assert!(!s.document.can_undo());
        assert!(!s.document.can_redo());
        assert!(matches!(result, Err(SessionError::StaleRenameTarget)));
    }

    #[test]
    fn rename_after_reload_rejects_reused_id() {
        assert_rename_rejected_after_reload(
            b"A=first\nB=second\n",
            b"NEW=inserted\nA=first\nB=second\n",
        );
    }

    #[test]
    fn rename_after_reload_rejects_reused_id_with_duplicate_names() {
        assert_rename_rejected_after_reload(
            b"A=first\nA=second\n",
            b"NEW=inserted\nA=first\nA=second\n",
        );
    }

    #[test]
    fn rename_after_reload_allows_unchanged_document() {
        let (_dir, mut s) = quoted_session(b"A=first\nA=second\n");
        let second = EntryId(2);
        let target = s.capture_rename_target(second);

        assert_eq!(s.rename_captured(&target, "RENAMED").unwrap(), second);
        assert_eq!(s.document.bytes(), b"A=first\nRENAMED=second\n");
    }

    #[test]
    fn rename_after_reload_allows_identical_byte_rebase() {
        let (dir, mut s) = quoted_session(b"A=first\nB=second\n");
        let second = EntryId(2);
        let target = s.capture_rename_target(second);
        let replacement = dir.path().join("replacement");
        std::fs::write(&replacement, &s.baseline.bytes).unwrap();
        std::fs::rename(replacement, &s.baseline.chosen_path).unwrap();
        let disk = storage::open(s.baseline.chosen_path.clone()).unwrap();
        assert!(!disk.same_reference(&s.baseline));
        assert_eq!(s.reconcile_external(disk), ExternalUpdate::Unchanged);

        assert_eq!(s.rename_captured(&target, "RENAMED").unwrap(), second);
        assert_eq!(s.document.bytes(), b"A=first\nRENAMED=second\n");
    }

    #[test]
    fn rename_after_reload_allows_edits_and_save_in_same_document() {
        let (_dir, mut s, first, second) = session();
        let target = s.capture_rename_target(second);
        s.set_draft(first, "changed".into()).unwrap();
        let request = s.prepare_save(false).unwrap().unwrap();
        let disk = storage::save(&request.baseline, &request.bytes).unwrap();
        s.complete_save(&request, disk).unwrap();

        assert_eq!(s.rename_captured(&target, "RENAMED").unwrap(), second);
        assert_eq!(
            s.document.bytes(),
            b"# before\r\nA=changed # tail\r\nRENAMED=two\r\n"
        );
    }

    #[test]
    fn rename_after_reload_rejects_new_session_before_confirming_drafts() {
        let (_dir, original, _, second) = session();
        let target = original.capture_rename_target(second);
        let bytes = original.document.bytes();
        let mut reopened = DocumentSession::new(original.baseline.clone());
        assert_eq!(reopened.document.revision(), original.document.revision());
        assert_eq!(reopened.document.entry(second).unwrap().id, second);
        let comment = TextField::Comment(0);
        reopened
            .set_text_draft(comment, " pending comment".into())
            .unwrap();
        reopened.set_draft(second, "pending value".into()).unwrap();
        let revision = reopened.work_revision();

        assert!(matches!(
            reopened.rename_captured(&target, "RENAMED"),
            Err(SessionError::StaleRenameTarget)
        ));
        assert_eq!(reopened.document.bytes(), bytes);
        assert_eq!(reopened.work_revision(), revision);
        assert!(!reopened.document.can_undo());
        assert!(!reopened.document.can_redo());
        assert_eq!(
            reopened.text_draft(comment).unwrap().text,
            " pending comment"
        );
        assert_eq!(reopened.draft(second).unwrap().text, "pending value");
    }

    #[test]
    fn identical_byte_replacement_rebases_identity_without_losing_work() {
        let (dir, mut s, a, b) = session();
        s.set_draft(a, "ours".into()).unwrap();
        s.confirm_entry(a).unwrap();
        s.set_draft(b, "invalid\nINJECTED=value".into()).unwrap();
        let revision = s.work_revision();
        let old = s.baseline.clone();
        let replacement = dir.path().join("replacement");
        std::fs::write(&replacement, &old.bytes).unwrap();
        std::fs::rename(&replacement, &old.chosen_path).unwrap();
        let disk = storage::open(old.chosen_path.clone()).unwrap();
        #[cfg(unix)]
        assert_ne!(disk.identity(), old.identity());
        assert!(!disk.same_reference(&old));
        assert_eq!(
            s.reconcile_external(disk.clone()),
            ExternalUpdate::Unchanged
        );
        assert!(s.baseline.same_reference(&disk));
        assert_eq!(s.work_revision(), revision);
        assert_eq!(s.value(b), Some("invalid\nINJECTED=value"));
        s.cancel_draft(b);
        assert!(s.undo().unwrap());
        assert!(!s.is_dirty());
    }

    #[test]
    fn external_read_during_save_never_changes_baseline_or_work() {
        let (_dir, mut s, a, _) = session();
        s.set_draft(a, "saved".into()).unwrap();
        let request = s.prepare_save(false).unwrap().unwrap();
        let original = s.baseline.clone();
        s.set_draft(a, "later".into()).unwrap();
        std::fs::write(&s.baseline.chosen_path, b"EXTERNAL=changed\n").unwrap();
        let disk = storage::open(s.baseline.chosen_path.clone()).unwrap();
        assert_eq!(s.reconcile_external(disk), ExternalUpdate::Busy);
        assert!(s.baseline.same_reference(&original));
        assert_eq!(s.value(a), Some("later"));
        assert!(s.is_saving());
        s.fail_save(&request);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_retarget_with_identical_bytes_is_still_a_dirty_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first.env");
        let second = dir.path().join("second.env");
        let link = dir.path().join("alias.env");
        std::fs::write(&first, b"VALUE=same\n").unwrap();
        std::fs::write(&second, b"VALUE=same\n").unwrap();
        std::os::unix::fs::symlink(&first, &link).unwrap();
        let mut s = DocumentSession::new(storage::open(link.clone()).unwrap());
        let entry = match &s.document.blocks()[0].kind {
            BlockKind::Assignment(entry) => entry.id,
            _ => panic!("assignment expected"),
        };
        s.set_draft(entry, "ours".into()).unwrap();
        let original = s.baseline.clone();
        std::fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(&second, &link).unwrap();
        let disk = storage::open(link).unwrap();
        assert_eq!(s.reconcile_external(disk), ExternalUpdate::Conflict);
        assert!(s.baseline.same_reference(&original));
        assert_eq!(s.value(entry), Some("ours"));
    }
}
