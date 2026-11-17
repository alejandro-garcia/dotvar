//! Lossless, conservative analysis of dotenv source documents.
//!
//! Values are source tokens: this crate never expands variables, decodes escapes,
//! executes shell expressions, normalizes line endings, or touches the filesystem.

mod document;
mod edit;
mod parser;

pub use document::{Assignment, Block, BlockKind, Diagnostic, Document, EntryId, TextField};
pub use edit::{EditError, HISTORY_MAX_BYTES, HISTORY_MAX_OPERATIONS};
