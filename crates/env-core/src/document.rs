use std::ops::Range;

/// Identity of an assignment within one document, independent of its name/offset.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EntryId(pub u64);

/// Editable text whose surrounding dotenv syntax stays outside the input.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum TextField {
    Name(EntryId),
    Comment(usize),
    InlineComment(EntryId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Assignment {
    pub id: EntryId,
    pub commented: bool,
    pub name: Range<usize>,
    /// The source span, including outer quotes and any continuation markers.
    pub value: Range<usize>,
    /// Starts at `#`; whitespace separating the comment remains outside the token.
    pub inline_comment: Option<Range<usize>>,
    pub(crate) uncommented_value: Option<String>,
}

impl Assignment {
    pub(crate) fn value_bytes<'a>(&'a self, bytes: &'a [u8]) -> Option<&'a [u8]> {
        if let Some(value) = &self.uncommented_value {
            return Some(value.as_bytes());
        }
        bytes.get(self.value.clone())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Diagnostic {
    UnsupportedEncoding,
    UnsupportedSyntax,
    AmbiguousComment,
    UnclosedQuote,
}

impl Diagnostic {
    /// Safe to display: diagnostics never include source values.
    pub fn message(self) -> &'static str {
        match self {
            Self::UnsupportedEncoding => {
                "Codificación no soportada. Se conservaron los bytes originales."
            }
            Self::UnsupportedSyntax => {
                "Fragmento conservado: sintaxis no soportada para edición estructurada."
            }
            Self::AmbiguousComment => {
                "Fragmento conservado: no se puede delimitar el comentario con seguridad."
            }
            Self::UnclosedQuote => "Fragmento conservado: comillas sin cerrar.",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BlockKind {
    Assignment(Assignment),
    Comment,
    Blank,
    Opaque(Diagnostic),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Block {
    /// Includes indentation, any final comment, and the original terminator.
    pub range: Range<usize>,
    pub kind: BlockKind,
}

/// Original bytes plus safe source ranges. Serialization is always byte exact.
///
/// Edits are bounded patches, never a reconstruction from variable names.
/// Documents whose source is not UTF-8 remain explicitly read-only.
#[derive(Clone, Debug)]
pub struct Document {
    pub(crate) bytes: Vec<u8>,
    pub(crate) blocks: Vec<Block>,
    bom: bool,
    read_only: bool,
    pub(crate) revision: u64,
    pub(crate) next_id: u64,
    pub(crate) undo: Vec<crate::edit::HistoryEntry>,
    pub(crate) redo: Vec<crate::edit::HistoryEntry>,
    pub(crate) history_bytes: usize,
}

impl Document {
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        let bom = bytes.starts_with(&[0xef, 0xbb, 0xbf]);
        let read_only = std::str::from_utf8(&bytes).is_err();
        let blocks = if read_only {
            vec![Block {
                range: 0..bytes.len(),
                kind: BlockKind::Opaque(Diagnostic::UnsupportedEncoding),
            }]
        } else {
            crate::parser::parse(&bytes, if bom { 3 } else { 0 })
        };
        let next_id = blocks
            .iter()
            .filter(|block| matches!(block.kind, BlockKind::Assignment(_)))
            .count() as u64
            + 1;
        Self {
            bytes,
            blocks,
            bom,
            read_only,
            revision: 0,
            next_id,
            undo: Vec::new(),
            redo: Vec::new(),
            history_bytes: 0,
        }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// Returns only an exact UTF-8 slice, never a lossy replacement string.
    pub fn text(&self, range: Range<usize>) -> Option<&str> {
        std::str::from_utf8(self.bytes.get(range)?).ok()
    }

    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    pub fn has_bom(&self) -> bool {
        self.bom
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn entry(&self, id: EntryId) -> Option<&Assignment> {
        self.blocks.iter().find_map(|block| match &block.kind {
            BlockKind::Assignment(entry) if entry.id == id => Some(entry),
            _ => None,
        })
    }

    /// Returns the literal token with any comment continuation markers removed.
    pub fn entry_value(&self, id: EntryId) -> Option<&str> {
        std::str::from_utf8(self.entry(id)?.value_bytes(&self.bytes)?).ok()
    }
}
