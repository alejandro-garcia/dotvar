use std::{collections::HashSet, fmt, ops::Range};

use crate::{
    Assignment, Block, BlockKind, Document, EntryId, TextField, parser::COMMENT_CONTINUATION,
};

pub const HISTORY_MAX_OPERATIONS: usize = 100;
pub const HISTORY_MAX_BYTES: usize = 8 * 1024 * 1024;

/// Diagnostics contain no source values, names, or document fragments.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EditError {
    ReadOnly,
    StaleRevision { expected: u64, actual: u64 },
    MissingEntry(EntryId),
    InvalidName,
    InvalidValue,
    InvalidComment,
    InvalidFragment,
    UnsafeBoundary,
    DuplicateName { existing: EntryId },
    DuplicateFragmentName,
    HistoryLimit,
    RevisionExhausted,
    IdentityExhausted,
}

impl EditError {
    pub fn message(&self) -> &'static str {
        match self {
            Self::ReadOnly => "El documento está en sólo lectura por su codificación.",
            Self::StaleRevision { .. } => {
                "La edición corresponde a una revisión anterior del documento."
            }
            Self::MissingEntry(_) => "La variable ya no está disponible en este documento.",
            Self::InvalidName => {
                "Usá un nombre con letras ASCII, números y guion bajo, sin comenzar con un número."
            }
            Self::InvalidValue => {
                "El valor no es válido para el formato de comillas de esta variable."
            }
            Self::InvalidComment => "El comentario debe ocupar una sola línea de texto.",
            Self::InvalidFragment => "El texto pegado debe contener variables .env válidas.",
            Self::UnsafeBoundary => {
                "No se puede modificar ese límite sin alterar otro fragmento del documento."
            }
            Self::DuplicateName { .. } => {
                "Ya existe una variable con ese nombre. Podés crear otra ocurrencia de forma explícita."
            }
            Self::DuplicateFragmentName => "El texto pegado repite un nombre de variable.",
            Self::HistoryLimit => {
                "La operación supera el límite de 8 MiB del historial reversible. No se modificó el documento."
            }
            Self::RevisionExhausted => "El documento alcanzó el límite de revisiones.",
            Self::IdentityExhausted => {
                "El documento alcanzó el límite de identidades de variables."
            }
        }
    }
}

impl fmt::Display for EditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for EditError {}

#[derive(Clone, Debug)]
pub(crate) struct HistoryEntry {
    start: usize,
    removed: Vec<u8>,
    inserted: Vec<u8>,
    before_ids: Vec<EntryId>,
    after_ids: Vec<EntryId>,
    accounted_bytes: usize,
}

impl HistoryEntry {
    fn cost(&self) -> usize {
        self.accounted_bytes
    }

    fn allocation_cost(&self) -> usize {
        std::mem::size_of::<Self>()
            .saturating_add(self.removed.capacity())
            .saturating_add(self.inserted.capacity())
            .saturating_add(
                (self.before_ids.capacity() + self.after_ids.capacity())
                    .saturating_mul(std::mem::size_of::<EntryId>()),
            )
    }
}

struct Candidate {
    bytes: Vec<u8>,
    blocks: Vec<Block>,
    range: Range<usize>,
    replacement: Vec<u8>,
}

impl Document {
    /// Move one complete source block to a final index, as one reversible patch.
    /// A missing final newline stays at EOF, even when the last item moves.
    pub fn move_block(
        &mut self,
        from: usize,
        to: usize,
        expected_revision: u64,
    ) -> Result<bool, EditError> {
        self.require_revision(expected_revision)?;
        if from >= self.blocks.len()
            || to >= self.blocks.len()
            || !matches!(
                self.blocks[from].kind,
                BlockKind::Assignment(_) | BlockKind::Comment
            )
        {
            return Err(EditError::UnsafeBoundary);
        }
        if from == to {
            return Ok(false);
        }
        let mut order: Vec<_> = (0..self.blocks.len()).collect();
        let moved = order.remove(from);
        order.insert(to, moved);
        let first = from.min(to);
        let last = from.max(to);
        let range = self.blocks[first].range.start..self.blocks[last].range.end;
        let missing_final_newline = last + 1 == self.blocks.len() && !self.bytes.ends_with(b"\n");
        let terminator = if missing_final_newline {
            let new_last = &self.bytes[self.blocks[order[last]].range.clone()];
            if new_last.ends_with(b"\r\n") {
                b"\r\n".as_slice()
            } else {
                b"\n".as_slice()
            }
        } else {
            b"".as_slice()
        };
        let mut replacement = Vec::with_capacity(range.len());
        let mut lengths = Vec::new();
        for (index, old_index) in order.iter().copied().enumerate().take(last + 1).skip(first) {
            let bytes = &self.bytes[self.blocks[old_index].range.clone()];
            let start = replacement.len();
            if missing_final_newline && index == last {
                replacement.extend_from_slice(
                    bytes
                        .strip_suffix(terminator)
                        .ok_or(EditError::UnsafeBoundary)?,
                );
            } else {
                replacement.extend_from_slice(bytes);
                if !bytes.ends_with(b"\n") {
                    replacement.extend_from_slice(terminator);
                }
            }
            lengths.push(replacement.len() - start);
        }
        let candidate = self.patch(range, replacement);
        if candidate.blocks.len() != self.blocks.len() {
            return Err(EditError::UnsafeBoundary);
        }
        for (index, old_index) in order.iter().copied().enumerate() {
            let old = &self.blocks[old_index];
            let new = &candidate.blocks[index];
            let expected_len = if (first..=last).contains(&index) {
                lengths[index - first]
            } else {
                old.range.len()
            };
            let (inserted, removed) = (new.range.start, old.range.start);
            if new.range.len() != expected_len
                || !same_kind_ranges(&old.kind, &new.kind, inserted, removed)
            {
                return Err(EditError::UnsafeBoundary);
            }
        }
        let ids = order
            .into_iter()
            .filter_map(|index| assignment(&self.blocks[index]).map(|e| e.id))
            .collect();
        self.commit(candidate, ids)
    }

    /// Text after the comment marker; indentation, `#` and terminators are preserved.
    pub fn text_field_range(&self, field: TextField) -> Option<Range<usize>> {
        match field {
            TextField::Name(id) => Some(self.entry(id)?.name.clone()),
            TextField::InlineComment(id) => {
                let range = self.entry(id)?.inline_comment.clone()?;
                Some(range.start + 1..range.end)
            }
            TextField::Comment(row) => {
                let block = self.blocks.get(row)?;
                if !matches!(block.kind, BlockKind::Comment) {
                    return None;
                }
                let text = self.text(block.range.clone())?;
                let start = block.range.start + text.find('#')? + 1;
                let end = block.range.start
                    + text
                        .strip_suffix("\r\n")
                        .or_else(|| text.strip_suffix('\n'))
                        .unwrap_or(text)
                        .len();
                Some(start..end)
            }
        }
    }

    pub fn validate_text_field(&self, field: TextField, text: &str) -> Result<(), EditError> {
        self.require_editable()?;
        match field {
            TextField::Name(id) => {
                self.find_entry(id)?;
                if valid_name(text) {
                    Ok(())
                } else {
                    Err(EditError::InvalidName)
                }
            }
            _ => self.comment_candidate(field, text).map(|_| ()),
        }
    }

    pub fn set_text_field(
        &mut self,
        field: TextField,
        text: &str,
        expected_revision: u64,
    ) -> Result<bool, EditError> {
        self.require_revision(expected_revision)?;
        if let TextField::Name(id) = field {
            return self.rename_entry(id, text, expected_revision);
        }
        let candidate = self.comment_candidate(field, text)?;
        self.commit(candidate, self.entry_ids())
    }

    fn comment_candidate(&self, field: TextField, text: &str) -> Result<Candidate, EditError> {
        if text.chars().any(|c| c.is_control() && c != '\t') {
            return Err(EditError::InvalidComment);
        }
        let range = self
            .text_field_range(field)
            .ok_or(EditError::UnsafeBoundary)?;
        let index = match field {
            TextField::Comment(row) => row,
            TextField::InlineComment(id) => self.find_entry(id)?.0,
            TextField::Name(_) => return Err(EditError::UnsafeBoundary),
        };
        let candidate = self.patch(range, text.as_bytes().to_vec());
        self.check_unchanged_blocks(&candidate, index)?;
        let old = &self.blocks[index];
        let new = &candidate.blocks[index];
        let inserted = candidate.replacement.len();
        let removed = candidate.range.len();
        if new.range != (old.range.start..shift(old.range.end, inserted, removed)) {
            return Err(EditError::UnsafeBoundary);
        }
        match (&old.kind, &new.kind) {
            (BlockKind::Comment, BlockKind::Comment) => {}
            (BlockKind::Assignment(old), BlockKind::Assignment(new))
                if old.commented == new.commented
                    && old.name == new.name
                    && old.value == new.value
                    && new.inline_comment
                        == old
                            .inline_comment
                            .as_ref()
                            .map(|r| r.start..shift(r.end, inserted, removed)) => {}
            _ => return Err(EditError::UnsafeBoundary),
        }
        Ok(candidate)
    }

    /// Checks a draft against the whole document without changing bytes/history.
    pub fn validate_value(&self, id: EntryId, token: &str) -> Result<(), EditError> {
        self.require_editable()?;
        self.value_candidate(id, token).map(|_| ())
    }

    pub fn set_value(
        &mut self,
        id: EntryId,
        token: &str,
        expected_revision: u64,
    ) -> Result<bool, EditError> {
        self.require_revision(expected_revision)?;
        let old = self.entry_value(id).ok_or(EditError::MissingEntry(id))?;
        if old == token {
            return Ok(false);
        }
        let candidate = self.value_candidate(id, token)?;
        let ids = self.entry_ids();
        self.commit(candidate, ids)
    }

    pub fn rename_entry(
        &mut self,
        id: EntryId,
        name: &str,
        expected_revision: u64,
    ) -> Result<bool, EditError> {
        self.require_revision(expected_revision)?;
        if !valid_name(name) {
            return Err(EditError::InvalidName);
        }
        let (index, block, entry) = self.find_entry(id)?;
        if self.text(entry.name.clone()) == Some(name) {
            return Ok(false);
        }
        let candidate = self.patch(entry.name.clone(), name.as_bytes().to_vec());
        self.check_unchanged_blocks(&candidate, index)?;
        let replacement = assignment(&candidate.blocks[index]).ok_or(EditError::UnsafeBoundary)?;
        if replacement.commented != entry.commented
            || replacement.name != (entry.name.start..entry.name.start + name.len())
            || replacement.value
                != shifted(
                    &entry.value,
                    candidate.replacement.len(),
                    candidate.range.len(),
                )
            || replacement.inline_comment
                != shift_optional(
                    &entry.inline_comment,
                    candidate.replacement.len(),
                    candidate.range.len(),
                )
            || candidate.blocks[index].range
                != (block.range.start
                    ..shift(
                        block.range.end,
                        candidate.replacement.len(),
                        candidate.range.len(),
                    ))
        {
            return Err(EditError::UnsafeBoundary);
        }
        self.commit(candidate, self.entry_ids())
    }

    /// Toggle one assignment without changing its identity or literal token.
    pub fn set_commented(
        &mut self,
        id: EntryId,
        commented: bool,
        expected_revision: u64,
    ) -> Result<bool, EditError> {
        self.require_revision(expected_revision)?;
        let (index, block, entry) = self.find_entry(id)?;
        if entry.commented == commented {
            return Ok(false);
        }
        let marker = self.bytes[block.range.start..entry.name.start]
            .iter()
            .position(|byte| !matches!(byte, b' ' | b'\t'))
            .map_or(entry.name.start, |offset| block.range.start + offset);
        let (range, replacement) = if commented {
            let source = &self.bytes[marker..block.range.end];
            let body = without_terminator(source);
            if body.contains(&b'\n') {
                let mut replacement = b"# ".to_vec();
                replacement.extend(comment_continuations(body));
                (marker..marker + body.len(), replacement)
            } else {
                (marker..marker, b"# ".to_vec())
            }
        } else {
            let after_hash = marker + 1;
            let start =
                after_hash + usize::from(matches!(self.bytes.get(after_hash), Some(b' ' | b'\t')));
            if entry.uncommented_value.is_some() {
                let body = without_terminator(&self.bytes[start..block.range.end]);
                let mut lines = body.split_inclusive(|byte| *byte == b'\n');
                let mut replacement = lines.next().ok_or(EditError::UnsafeBoundary)?.to_vec();
                for line in lines {
                    replacement.extend_from_slice(
                        line.strip_prefix(COMMENT_CONTINUATION)
                            .ok_or(EditError::UnsafeBoundary)?,
                    );
                }
                (marker..start + body.len(), replacement)
            } else {
                (marker..start, Vec::new())
            }
        };
        let candidate = self.patch(range, replacement);
        self.check_unchanged_blocks(&candidate, index)?;
        let changed = &candidate.blocks[index];
        let parsed = assignment(changed).ok_or(EditError::UnsafeBoundary)?;
        if parsed.commented != commented
            || candidate.bytes[parsed.name.clone()] != self.bytes[entry.name.clone()]
            || parsed.value_bytes(&candidate.bytes) != entry.value_bytes(&self.bytes)
            || changed.range
                != (block.range.start
                    ..shift(
                        block.range.end,
                        candidate.replacement.len(),
                        candidate.range.len(),
                    ))
        {
            return Err(EditError::UnsafeBoundary);
        }
        self.commit(candidate, self.entry_ids())
    }

    /// Appends one assignment, preserving the existing final-newline convention.
    pub fn add_entry(
        &mut self,
        name: &str,
        token: &str,
        allow_duplicate: bool,
        expected_revision: u64,
    ) -> Result<EntryId, EditError> {
        self.require_revision(expected_revision)?;
        if !valid_name(name) {
            return Err(EditError::InvalidName);
        }
        if !allow_duplicate
            && let Some(existing) = self.blocks.iter().find_map(|block| {
                let entry = assignment(block)?;
                (!entry.commented && self.text(entry.name.clone()) == Some(name))
                    .then_some(entry.id)
            })
        {
            return Err(EditError::DuplicateName { existing });
        }
        validate_new_token(name, token)?;
        let next_id = self
            .next_id
            .checked_add(1)
            .ok_or(EditError::IdentityExhausted)?;
        let id = EntryId(self.next_id);
        let empty = self.bytes.len() == usize::from(self.has_bom()) * 3;
        let newline = predominant_newline(&self.bytes);
        let needs_separator = !empty && !self.bytes.ends_with(b"\n");
        let preserve_final_newline = empty || self.bytes.ends_with(b"\n");
        let mut replacement = Vec::new();
        if needs_separator {
            replacement.extend_from_slice(newline);
        }
        let assignment_start = self.bytes.len() + replacement.len();
        replacement.extend_from_slice(name.as_bytes());
        replacement.push(b'=');
        replacement.extend_from_slice(token.as_bytes());
        if preserve_final_newline {
            replacement.extend_from_slice(newline);
        }
        let candidate = self.patch(self.bytes.len()..self.bytes.len(), replacement);
        self.check_appended_blocks(
            &candidate,
            1,
            if needs_separator { newline.len() } else { 0 },
        )?;
        let added = assignment(candidate.blocks.last().ok_or(EditError::UnsafeBoundary)?)
            .ok_or(EditError::UnsafeBoundary)?;
        if added.name != (assignment_start..assignment_start + name.len())
            || added.value
                != (assignment_start + name.len() + 1
                    ..assignment_start + name.len() + 1 + token.len())
            || added.inline_comment.is_some()
        {
            return Err(EditError::UnsafeBoundary);
        }
        let mut ids = self.entry_ids();
        ids.push(id);
        self.commit(candidate, ids)?;
        self.next_id = next_id;
        Ok(id)
    }

    /// Append a parsed dotenv fragment as a single undoable edit. Keep its
    /// source blocks, using the destination's separators between blocks.
    pub fn append_entries(
        &mut self,
        fragment: &Document,
        allow_duplicates: bool,
        expected_revision: u64,
    ) -> Result<Vec<EntryId>, EditError> {
        self.require_revision(expected_revision)?;
        let entries: Vec<_> = fragment.blocks.iter().filter_map(assignment).collect();
        if entries.is_empty()
            || fragment
                .blocks
                .iter()
                .any(|block| matches!(block.kind, BlockKind::Opaque(_)))
        {
            return Err(EditError::InvalidFragment);
        }
        if !allow_duplicates {
            let mut existing = std::collections::HashMap::new();
            for entry in self
                .blocks
                .iter()
                .filter_map(assignment)
                .filter(|entry| !entry.commented)
            {
                existing
                    .entry(&self.bytes[entry.name.clone()])
                    .or_insert(entry.id);
            }
            let mut names = HashSet::new();
            for entry in entries.iter().filter(|entry| !entry.commented) {
                let name = &fragment.bytes[entry.name.clone()];
                if let Some(&existing) = existing.get(name) {
                    return Err(EditError::DuplicateName { existing });
                }
                if !names.insert(name) {
                    return Err(EditError::DuplicateFragmentName);
                }
            }
        }

        let next_id = self
            .next_id
            .checked_add(entries.len() as u64)
            .ok_or(EditError::IdentityExhausted)?;
        let added_ids: Vec<_> = (self.next_id..next_id).map(EntryId).collect();
        let newline = predominant_newline(&self.bytes);
        let empty = self.bytes.len() == usize::from(self.has_bom()) * 3;
        let final_newline = empty || self.bytes.ends_with(b"\n");
        let mut replacement = Vec::new();
        if !final_newline {
            replacement.extend_from_slice(newline);
        }
        let separator_len = replacement.len();
        // A copied selection can end in empty lines. When the destination lacks
        // a final newline, stop after the last nonblank block to preserve that.
        let end = if final_newline {
            fragment.blocks.len()
        } else {
            fragment
                .blocks
                .iter()
                .rposition(|block| !matches!(block.kind, BlockKind::Blank))
                .unwrap()
                + 1
        };
        let blocks = &fragment.blocks[..end];
        let mut ranges = Vec::with_capacity(blocks.len());
        for (index, block) in blocks.iter().enumerate() {
            let start = self.bytes.len() + replacement.len();
            let source = &fragment.bytes[block.range.clone()];
            replacement.extend_from_slice(without_terminator(source));
            if index + 1 < blocks.len() || final_newline {
                replacement.extend_from_slice(newline);
            }
            ranges.push(start..self.bytes.len() + replacement.len());
        }
        let candidate = self.patch(self.bytes.len()..self.bytes.len(), replacement);
        self.check_appended_blocks(&candidate, blocks.len(), separator_len)?;
        for ((source, range), parsed) in blocks
            .iter()
            .zip(ranges)
            .zip(&candidate.blocks[self.blocks.len()..])
        {
            if range != parsed.range
                || !same_kind_ranges(&source.kind, &parsed.kind, range.start, source.range.start)
            {
                return Err(EditError::UnsafeBoundary);
            }
        }
        let mut ids = self.entry_ids();
        ids.extend_from_slice(&added_ids);
        self.commit(candidate, ids)?;
        self.next_id = next_id;
        Ok(added_ids)
    }

    fn check_appended_blocks(
        &self,
        candidate: &Candidate,
        added: usize,
        separator_len: usize,
    ) -> Result<(), EditError> {
        if candidate.blocks.len() != self.blocks.len() + added {
            return Err(EditError::UnsafeBoundary);
        }
        for (index, (old, new)) in self.blocks.iter().zip(&candidate.blocks).enumerate() {
            let extra = if index + 1 == self.blocks.len() {
                separator_len
            } else {
                0
            };
            if new.range != (old.range.start..old.range.end + extra)
                || !same_kind_ranges(&old.kind, &new.kind, 0, 0)
            {
                return Err(EditError::UnsafeBoundary);
            }
        }
        Ok(())
    }

    pub fn remove_entry(&mut self, id: EntryId, expected_revision: u64) -> Result<bool, EditError> {
        self.require_revision(expected_revision)?;
        let (index, block, entry) = self.find_entry(id)?;
        let mut replacement = Vec::new();
        if let Some(comment) = &entry.inline_comment {
            let indentation_end = self.bytes[block.range.clone()]
                .iter()
                .position(|byte| !matches!(byte, b' ' | b'\t'))
                .map_or(block.range.end, |relative| block.range.start + relative);
            replacement.extend_from_slice(&self.bytes[block.range.start..indentation_end]);
            replacement.extend_from_slice(&self.bytes[comment.clone()]);
            replacement.extend_from_slice(&self.bytes[comment.end..block.range.end]);
        }
        let keeps_comment = entry.inline_comment.is_some();
        let candidate = self.patch(block.range.clone(), replacement);
        if candidate.blocks.len() != self.blocks.len() - usize::from(!keeps_comment) {
            return Err(EditError::UnsafeBoundary);
        }
        for (old_index, old) in self.blocks.iter().enumerate() {
            if old_index == index {
                if keeps_comment
                    && (!matches!(
                        candidate.blocks[index].kind,
                        BlockKind::Comment
                            | BlockKind::Assignment(Assignment {
                                commented: true,
                                ..
                            })
                    ) || candidate.blocks[index].range
                        != (block.range.start..block.range.start + candidate.replacement.len()))
                {
                    return Err(EditError::UnsafeBoundary);
                }
                continue;
            }
            let new_index = old_index - usize::from(!keeps_comment && old_index > index);
            let new = &candidate.blocks[new_index];
            let (inserted, removed) = if old_index > index {
                (candidate.replacement.len(), candidate.range.len())
            } else {
                (0, 0)
            };
            if new.range != shifted(&old.range, inserted, removed)
                || !same_kind_ranges(&old.kind, &new.kind, inserted, removed)
            {
                return Err(EditError::UnsafeBoundary);
            }
        }
        let mut ids = self.entry_ids();
        // An inline comment that looks like a variable becomes a commented
        // assignment when it stands alone, with its own identity.
        if keeps_comment && assignment(&candidate.blocks[index]).is_some() {
            let next_id = self
                .next_id
                .checked_add(1)
                .ok_or(EditError::IdentityExhausted)?;
            let position = ids
                .iter()
                .position(|entry_id| *entry_id == id)
                .expect("removed assignment has an ID");
            ids[position] = EntryId(self.next_id);
            let changed = self.commit(candidate, ids)?;
            self.next_id = next_id;
            return Ok(changed);
        }
        ids.retain(|entry_id| *entry_id != id);
        self.commit(candidate, ids)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        if self.revision == u64::MAX {
            return false;
        }
        let Some(record) = self.undo.pop() else {
            return false;
        };
        self.bytes.splice(
            record.start..record.start + record.inserted.len(),
            record.removed.iter().copied(),
        );
        self.blocks = crate::parser::parse(&self.bytes, usize::from(self.has_bom()) * 3);
        assign_ids(&mut self.blocks, &record.before_ids);
        self.revision += 1;
        self.redo.push(record);
        true
    }

    pub fn redo(&mut self) -> bool {
        if self.revision == u64::MAX {
            return false;
        }
        let Some(record) = self.redo.pop() else {
            return false;
        };
        self.bytes.splice(
            record.start..record.start + record.removed.len(),
            record.inserted.iter().copied(),
        );
        self.blocks = crate::parser::parse(&self.bytes, usize::from(self.has_bom()) * 3);
        assign_ids(&mut self.blocks, &record.after_ids);
        self.revision += 1;
        self.undo.push(record);
        true
    }

    fn require_editable(&self) -> Result<(), EditError> {
        if self.is_read_only() {
            Err(EditError::ReadOnly)
        } else {
            Ok(())
        }
    }

    fn require_revision(&self, expected: u64) -> Result<(), EditError> {
        self.require_editable()?;
        if expected != self.revision {
            Err(EditError::StaleRevision {
                expected,
                actual: self.revision,
            })
        } else {
            Ok(())
        }
    }

    fn find_entry(&self, id: EntryId) -> Result<(usize, &Block, &Assignment), EditError> {
        self.blocks
            .iter()
            .enumerate()
            .find_map(|(index, block)| {
                let entry = assignment(block)?;
                (entry.id == id).then_some((index, block, entry))
            })
            .ok_or(EditError::MissingEntry(id))
    }

    fn entry_ids(&self) -> Vec<EntryId> {
        self.blocks
            .iter()
            .filter_map(assignment)
            .map(|entry| entry.id)
            .collect()
    }

    fn patch(&self, range: Range<usize>, replacement: Vec<u8>) -> Candidate {
        let mut bytes = Vec::with_capacity(self.bytes.len() - range.len() + replacement.len());
        bytes.extend_from_slice(&self.bytes[..range.start]);
        bytes.extend_from_slice(&replacement);
        bytes.extend_from_slice(&self.bytes[range.end..]);
        let blocks = crate::parser::parse(&bytes, usize::from(self.has_bom()) * 3);
        Candidate {
            bytes,
            blocks,
            range,
            replacement,
        }
    }

    fn value_candidate(&self, id: EntryId, token: &str) -> Result<Candidate, EditError> {
        let (index, block, entry) = self.find_entry(id)?;
        let mut replacement = if entry.commented {
            comment_continuations(token.as_bytes())
        } else {
            token.as_bytes().to_vec()
        };
        // An originally empty token may share its offset with `#`. Inserting
        // a value needs a separator; the existing prefix/comment stay intact.
        if !token.is_empty()
            && entry
                .inline_comment
                .as_ref()
                .is_some_and(|comment| comment.start == entry.value.end)
        {
            replacement.push(b' ');
        }
        let candidate = self.patch(entry.value.clone(), replacement);
        self.check_unchanged_blocks(&candidate, index)
            .map_err(|_| EditError::InvalidValue)?;
        let changed = assignment(&candidate.blocks[index]).ok_or(EditError::InvalidValue)?;
        if changed.commented != entry.commented
            || changed.name != entry.name
            || changed.value_bytes(&candidate.bytes) != Some(token.as_bytes())
            || changed.inline_comment
                != shift_optional(
                    &entry.inline_comment,
                    candidate.replacement.len(),
                    candidate.range.len(),
                )
            || candidate.blocks[index].range
                != (block.range.start
                    ..shift(
                        block.range.end,
                        candidate.replacement.len(),
                        candidate.range.len(),
                    ))
        {
            return Err(EditError::InvalidValue);
        }
        Ok(candidate)
    }

    fn check_unchanged_blocks(
        &self,
        candidate: &Candidate,
        changed_index: usize,
    ) -> Result<(), EditError> {
        if candidate.blocks.len() != self.blocks.len() {
            return Err(EditError::UnsafeBoundary);
        }
        for (index, old) in self.blocks.iter().enumerate() {
            if index == changed_index {
                continue;
            }
            let new = &candidate.blocks[index];
            let (inserted, removed) = if index > changed_index {
                (candidate.replacement.len(), candidate.range.len())
            } else {
                (0, 0)
            };
            if new.range != shifted(&old.range, inserted, removed)
                || !same_kind_ranges(&old.kind, &new.kind, inserted, removed)
            {
                return Err(EditError::UnsafeBoundary);
            }
        }
        Ok(())
    }

    fn commit(&mut self, mut candidate: Candidate, ids: Vec<EntryId>) -> Result<bool, EditError> {
        let revision = self
            .revision
            .checked_add(1)
            .ok_or(EditError::RevisionExhausted)?;
        if self.bytes == candidate.bytes && ids == self.entry_ids() {
            return Ok(false);
        }
        let mut record = HistoryEntry {
            start: candidate.range.start,
            removed: self.bytes[candidate.range].to_vec(),
            inserted: candidate.replacement,
            before_ids: self.entry_ids(),
            after_ids: ids,
            accounted_bytes: 0,
        };
        // A derived Document clone can shrink Vec capacities. Keep the reserved
        // accounting with its record so later drains/evictions subtract exactly
        // what this document's counter contains, even after a clone.
        record.accounted_bytes = record.allocation_cost();
        if record.cost() > HISTORY_MAX_BYTES {
            return Err(EditError::HistoryLimit);
        }
        for redo in self.redo.drain(..) {
            self.history_bytes -= redo.cost();
        }
        self.history_bytes += record.cost();
        assign_ids(&mut candidate.blocks, &record.after_ids);
        self.bytes = candidate.bytes;
        self.blocks = candidate.blocks;
        self.revision = revision;
        self.undo.push(record);
        while self.undo.len() > HISTORY_MAX_OPERATIONS || self.history_bytes > HISTORY_MAX_BYTES {
            let oldest = self.undo.remove(0);
            self.history_bytes -= oldest.cost();
        }
        Ok(true)
    }
}

fn without_terminator(source: &[u8]) -> &[u8] {
    source
        .strip_suffix(b"\r\n")
        .or_else(|| source.strip_suffix(b"\n"))
        .unwrap_or(source)
}

fn comment_continuations(source: &[u8]) -> Vec<u8> {
    let mut commented = Vec::with_capacity(source.len());
    for (index, line) in source.split(|byte| *byte == b'\n').enumerate() {
        if index > 0 {
            commented.push(b'\n');
            commented.extend_from_slice(COMMENT_CONTINUATION);
        }
        commented.extend_from_slice(line);
    }
    commented
}

fn assignment(block: &Block) -> Option<&Assignment> {
    match &block.kind {
        BlockKind::Assignment(entry) => Some(entry),
        _ => None,
    }
}

fn assign_ids(blocks: &mut [Block], ids: &[EntryId]) {
    let mut ids = ids.iter();
    for block in blocks {
        if let BlockKind::Assignment(entry) = &mut block.kind {
            entry.id = *ids.next().expect("validated assignment count");
        }
    }
    debug_assert!(ids.next().is_none());
}

fn valid_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes
        .first()
        .is_some_and(|first| first.is_ascii_alphabetic() || *first == b'_')
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
}

fn validate_new_token(name: &str, token: &str) -> Result<(), EditError> {
    let bytes = format!("{name}={token}").into_bytes();
    let parsed = crate::parser::parse(&bytes, 0);
    if parsed.len() != 1 {
        return Err(EditError::InvalidValue);
    }
    let entry = assignment(&parsed[0]).ok_or(EditError::InvalidValue)?;
    if entry.value != (name.len() + 1..bytes.len()) || entry.inline_comment.is_some() {
        return Err(EditError::InvalidValue);
    }
    Ok(())
}

fn predominant_newline(bytes: &[u8]) -> &'static [u8] {
    let (mut crlf, mut lf) = (0, 0);
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' {
            if index > 0 && bytes[index - 1] == b'\r' {
                crlf += 1;
            } else {
                lf += 1;
            }
        }
    }
    if crlf > lf { b"\r\n" } else { b"\n" }
}

fn shift(offset: usize, inserted: usize, removed: usize) -> usize {
    if inserted >= removed {
        offset + inserted - removed
    } else {
        offset - (removed - inserted)
    }
}

fn shifted(range: &Range<usize>, inserted: usize, removed: usize) -> Range<usize> {
    shift(range.start, inserted, removed)..shift(range.end, inserted, removed)
}

fn shift_optional(
    range: &Option<Range<usize>>,
    inserted: usize,
    removed: usize,
) -> Option<Range<usize>> {
    range
        .as_ref()
        .map(|range| shifted(range, inserted, removed))
}

fn same_kind_ranges(old: &BlockKind, new: &BlockKind, inserted: usize, removed: usize) -> bool {
    match (old, new) {
        (BlockKind::Blank, BlockKind::Blank) | (BlockKind::Comment, BlockKind::Comment) => true,
        (BlockKind::Opaque(old), BlockKind::Opaque(new)) => old == new,
        (BlockKind::Assignment(old), BlockKind::Assignment(new)) => {
            new.commented == old.commented
                && new.name == shifted(&old.name, inserted, removed)
                && new.value == shifted(&old.value, inserted, removed)
                && new.inline_comment == shift_optional(&old.inline_comment, inserted, removed)
        }
        _ => false,
    }
}
