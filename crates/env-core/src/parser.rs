use crate::{Assignment, Block, BlockKind, Diagnostic, EntryId};
use std::ops::Range;

// Distinguish multiline value continuations from neighboring standalone comments.
pub(crate) const COMMENT_CONTINUATION: &[u8] = b"#| ";

pub(crate) fn parse(bytes: &[u8], mut offset: usize) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut next_id = 1;
    while offset < bytes.len() {
        let (content_end, line_end) = line_bounds(bytes, offset);
        let first = skip_space(bytes, offset, content_end);
        let (kind, end) = if first == content_end {
            (BlockKind::Blank, line_end)
        } else if bytes[first] == b'#' {
            parse_comment(bytes, first + 1, content_end, line_end, next_id)
        } else {
            parse_assignment(bytes, first, content_end, line_end, next_id)
        };
        if matches!(kind, BlockKind::Assignment(_)) {
            next_id += 1;
        }
        blocks.push(Block {
            range: offset..end,
            kind,
        });
        offset = end;
    }
    blocks
}

fn parse_comment(
    bytes: &[u8],
    after_marker: usize,
    content_end: usize,
    line_end: usize,
    id: u64,
) -> (BlockKind, usize) {
    let start = skip_space(bytes, after_marker, content_end);
    if start == content_end || !is_name_start(bytes[start]) {
        return (BlockKind::Comment, line_end);
    }
    match parse_assignment(&bytes[..line_end], start, content_end, line_end, id).0 {
        BlockKind::Assignment(mut entry) => {
            entry.commented = true;
            return (BlockKind::Assignment(entry), line_end);
        }
        BlockKind::Opaque(Diagnostic::UnclosedQuote)
            if bytes[line_end..].starts_with(COMMENT_CONTINUATION) => {}
        _ => return (BlockKind::Comment, line_end),
    }

    let mut source = bytes[start..line_end].to_vec();
    let mut starts = Vec::new();
    let mut cursor = line_end;
    while bytes[cursor..].starts_with(COMMENT_CONTINUATION) {
        starts.push(source.len());
        let payload = cursor + COMMENT_CONTINUATION.len();
        (_, cursor) = line_bounds(bytes, payload);
        source.extend_from_slice(&bytes[payload..cursor]);
    }
    let (kind, end) = parse_assignment(&source, 0, content_end - start, line_end - start, id);
    let BlockKind::Assignment(mut entry) = kind else {
        return (BlockKind::Comment, line_end);
    };
    let original_offset = |offset| {
        start
            + offset
            + starts.partition_point(|line_start| *line_start <= offset)
                * COMMENT_CONTINUATION.len()
    };
    let original_range =
        |range: Range<usize>| original_offset(range.start)..original_offset(range.end);
    // The end is exclusive, so a marker on the next line is outside this block.
    let original_end = start
        + end
        + starts.partition_point(|line_start| *line_start < end) * COMMENT_CONTINUATION.len();
    entry.commented = true;
    entry.uncommented_value = Some(
        std::str::from_utf8(&source[entry.value.clone()])
            .expect("dotenv source was validated as UTF-8")
            .to_owned(),
    );
    entry.name = original_range(entry.name);
    entry.value = original_range(entry.value);
    entry.inline_comment = entry.inline_comment.map(original_range);
    (BlockKind::Assignment(entry), original_end)
}

fn line_bounds(bytes: &[u8], start: usize) -> (usize, usize) {
    match bytes[start..].iter().position(|&b| b == b'\n') {
        Some(relative) => {
            let lf = start + relative;
            (
                if lf > start && bytes[lf - 1] == b'\r' {
                    lf - 1
                } else {
                    lf
                },
                lf + 1,
            )
        }
        None => (bytes.len(), bytes.len()),
    }
}

fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t')
}

fn skip_space(bytes: &[u8], mut pos: usize, end: usize) -> usize {
    while pos < end && is_space(bytes[pos]) {
        pos += 1;
    }
    pos
}

fn is_name_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_name_continue(byte: u8) -> bool {
    is_name_start(byte) || byte.is_ascii_digit()
}

fn opaque(diagnostic: Diagnostic, end: usize) -> (BlockKind, usize) {
    (BlockKind::Opaque(diagnostic), end)
}

fn unsupported_control(bytes: &[u8], pos: usize) -> bool {
    bytes[pos].is_ascii_control()
        && !matches!(bytes[pos], b'\t' | b'\n')
        && !(bytes[pos] == b'\r' && bytes.get(pos + 1) == Some(&b'\n'))
}

// An unsupported line is not necessarily a complete block. We do not parse
// shell, but must avoid exposing assignments inside its uncertain continuation.
// A balanced unknown line remains local; an unfinished delimiter/continuation
// or heredoc conservatively retains the rest of the document as one block.
fn unsupported_line(
    bytes: &[u8],
    start: usize,
    content_end: usize,
    line_end: usize,
) -> (BlockKind, usize) {
    opaque(
        Diagnostic::UnsupportedSyntax,
        unsupported_line_end(bytes, start, content_end, line_end, false),
    )
}

fn unsupported_line_end(
    bytes: &[u8],
    start: usize,
    content_end: usize,
    line_end: usize,
    recognize_trailing_comment: bool,
) -> usize {
    let mut pos = start;
    let mut quote = None;
    let mut parentheses = 0usize;
    while pos < content_end {
        let byte = bytes[pos];
        match quote {
            Some(b'\'') => {
                if byte == b'\'' {
                    quote = None;
                }
            }
            Some(delimiter) => {
                if byte == b'\\' {
                    if pos + 1 >= content_end {
                        return bytes.len();
                    }
                    pos += 1;
                } else if byte == delimiter {
                    quote = None;
                }
            }
            None => match byte {
                b'\'' | b'"' | b'`' => quote = Some(byte),
                b'(' => parentheses += 1,
                b')' => parentheses = parentheses.saturating_sub(1),
                b'<' if bytes.get(pos + 1) == Some(&b'<') => {
                    return bytes.len();
                }
                b'\\' => {
                    if pos + 1 >= content_end {
                        return bytes.len();
                    }
                    pos += 1;
                }
                _ if recognize_trailing_comment
                    && is_space(byte)
                    && bytes.get(pos + 1) == Some(&b'#') =>
                {
                    break;
                }
                _ => {}
            },
        }
        pos += 1;
    }
    if quote.is_some() || parentheses > 0 {
        bytes.len()
    } else {
        line_end
    }
}

fn parse_assignment(
    bytes: &[u8],
    mut pos: usize,
    content_end: usize,
    line_end: usize,
    id: u64,
) -> (BlockKind, usize) {
    let assignment_start = pos;
    if bytes[pos..content_end].starts_with(b"export")
        && pos + 6 < content_end
        && is_space(bytes[pos + 6])
    {
        pos = skip_space(bytes, pos + 6, content_end);
    }
    let name_start = pos;
    if pos == content_end || !is_name_start(bytes[pos]) {
        return unsupported_line(bytes, assignment_start, content_end, line_end);
    }
    pos += 1;
    while pos < content_end && is_name_continue(bytes[pos]) {
        pos += 1;
    }
    let name = name_start..pos;
    pos = skip_space(bytes, pos, content_end);
    if pos == content_end || bytes[pos] != b'=' {
        return unsupported_line(bytes, assignment_start, content_end, line_end);
    }
    pos = skip_space(bytes, pos + 1, content_end);
    let value_start = pos;
    let (value_end, inline_comment, end) =
        if pos < content_end && matches!(bytes[pos], b'\'' | b'"') {
            let quote = bytes[pos];
            let mut unsupported = false;
            pos += 1;
            loop {
                if pos == bytes.len() {
                    return opaque(Diagnostic::UnclosedQuote, bytes.len());
                }
                unsupported |= unsupported_control(bytes, pos);
                if quote == b'"' && bytes[pos] == b'\\' {
                    // Preserve escapes literally. Only delimiter recognition uses them.
                    pos += 1;
                    if pos < bytes.len() {
                        unsupported |= unsupported_control(bytes, pos);
                        pos += 1;
                    }
                    continue;
                }
                if bytes[pos] == quote {
                    break;
                }
                pos += 1;
            }
            let value_end = pos + 1;
            let (tail_end, end) = line_bounds(bytes, value_end);
            pos = skip_space(bytes, value_end, tail_end);
            if pos < tail_end && bytes[pos] != b'#' {
                let end = unsupported_line_end(bytes, value_end, tail_end, end, true);
                return opaque(Diagnostic::UnsupportedSyntax, end);
            }
            if unsupported {
                return opaque(Diagnostic::UnsupportedSyntax, end);
            }
            if pos < tail_end && pos == value_end {
                return opaque(Diagnostic::AmbiguousComment, end);
            }
            let comment = (pos < tail_end).then_some(pos..tail_end);
            (value_end, comment, end)
        } else {
            let mut comment = None;
            while pos < content_end {
                match bytes[pos] {
                    b'#' => {
                        // `KEY= #comment` is unambiguous; `KEY=#x` is not.
                        if pos > 0 && is_space(bytes[pos - 1]) {
                            comment = Some(pos..content_end);
                            break;
                        }
                        return opaque(Diagnostic::AmbiguousComment, line_end);
                    }
                    b'\'' | b'"' => {
                        // Unsupported concatenation may start a multiline quote.
                        // No subsequent assignment boundary is established safely.
                        return opaque(Diagnostic::UnsupportedSyntax, bytes.len());
                    }
                    b'`' | b';' | b'|' | b'&' | b'<' | b'>' | b'(' | b')' => {
                        return unsupported_line(bytes, assignment_start, content_end, line_end);
                    }
                    b'$' if bytes.get(pos + 1) == Some(&b'(') => {
                        return unsupported_line(bytes, assignment_start, content_end, line_end);
                    }
                    _ if unsupported_control(bytes, pos) => {
                        return unsupported_line(bytes, assignment_start, content_end, line_end);
                    }
                    _ => pos += 1,
                }
            }
            let mut value_end = comment.as_ref().map_or(content_end, |range| range.start);
            while value_end > value_start && is_space(bytes[value_end - 1]) {
                value_end -= 1;
            }
            if value_end > value_start && bytes[value_end - 1] == b'\\' {
                // A trailing backslash might continue the next line in shell dotenv.
                // Keep the uncertain tail together rather than making apparent
                // assignments in its continuation available for structured edits.
                let end = if comment.is_none() {
                    bytes.len()
                } else {
                    line_end
                };
                return opaque(Diagnostic::UnsupportedSyntax, end);
            }
            (value_end, comment, line_end)
        };
    (
        BlockKind::Assignment(Assignment {
            id: EntryId(id),
            commented: false,
            name,
            value: value_start..value_end,
            inline_comment,
            uncommented_value: None,
        }),
        end,
    )
}
