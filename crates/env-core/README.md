# env-core

`env-core` parses and edits dotenv documents. It uses only the Rust standard library and operates on bytes supplied by the caller. `Document::from_bytes` keeps the original buffer, and `bytes()` returns the complete document, including its UTF-8 BOM and original line endings. Edits patch byte ranges in that buffer.

## Grammar and preservation

`blocks()` returns assignments, comments, blank lines, and opaque fragments in source order. An assignment has separate ranges for its name, literal value token, and optional trailing comment. Each occurrence has its own `EntryId`, even when names repeat. The ID stays attached to the occurrence through edits, moves, undo, and redo.

The parser recognizes ASCII names matching `[A-Za-z_][A-Za-z0-9_]*`, optional `export`, horizontal whitespace, empty values, and single- or double-quoted values spanning multiple lines. The value token includes its surrounding quotes. References and escapes stay literal. Double quotes support an escaped delimiter, and backslashes inside single quotes stay literal.

Outside quotes, `#` starts a trailing comment when preceded by a space or tab. `KEY=abc#def`, concatenated quote styles, shell operators, continuations, and names outside the grammar are preserved as opaque text. If the parser cannot determine where an unsupported construct ends, it leaves the remainder opaque. This includes unclosed quotes, backticks, shell groups, and heredocs.

Invalid UTF-8 produces a single opaque block and opens read-only. `text(range)` returns `None` for invalid UTF-8 or invalid boundaries.

## Editing model

- Mutations take an expected document revision. Stale revisions leave the document intact. Real edits, undo, and redo advance the revision. Reapplying the same name or value leaves the history intact.
- `validate_value` checks a new literal token against the document. It checks the surrounding syntax too, so a draft cannot accidentally consume a trailing comment or introduce another assignment. A multiline value requires quotes.
- `add_entry` follows the document's prevailing line ending and final newline. It requires explicit approval for duplicate names and rejects appends after an opaque tail with an uncertain boundary.
- `remove_entry` removes one assignment. Its trailing comment, if present, becomes a standalone comment in the same location.
- `undo()` and `redo()` restore both bytes and entry IDs. History retains up to 100 patches within an 8 MiB budget. An edit that exceeds that budget on its own fails before changing the document.

The desktop crate handles storage and determines whether a document has unsaved edits by comparing the working bytes with its saved snapshot. Core edit errors omit names and values from the document.

## Tests

```sh
cargo test -p env-core --locked
```

The `.fixture` files in `tests/fixtures` contain synthetic data.
