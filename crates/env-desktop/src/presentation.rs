//! Pure document navigation. Searching never examines values or comments and
//! never filters or mutates the source used by document copy/save.

use env_core::{BlockKind, Document, EntryId};

/// Ordered occurrences whose ASCII key contains the case-insensitive query.
/// An empty query has no active matches. Duplicate names remain distinct IDs.
pub fn key_matches(document: &Document, query: &str) -> Vec<EntryId> {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    document
        .blocks()
        .iter()
        .filter_map(|block| {
            let BlockKind::Assignment(entry) = &block.kind else {
                return None;
            };
            document
                .text(entry.name.clone())
                .is_some_and(|name| name.to_ascii_lowercase().contains(&query))
                .then_some(entry.id)
        })
        .collect()
}

/// Navigate ordered matches with wraparound. An absent/stale selection starts
/// at the first match (or last when moving backwards).
pub fn adjacent_match(
    matches: &[EntryId],
    current: Option<EntryId>,
    backwards: bool,
) -> Option<EntryId> {
    if matches.is_empty() {
        return None;
    }
    let current = current.and_then(|id| matches.iter().position(|candidate| *candidate == id));
    let index = match (current, backwards) {
        (Some(0), true) | (None, true) => matches.len() - 1,
        (Some(index), true) => index - 1,
        (Some(index), false) => (index + 1) % matches.len(),
        (None, false) => 0,
    };
    Some(matches[index])
}

/// The entry Enter in the search box should edit: the current match, or the
/// first match when nothing (valid) is selected. `None` without matches.
pub fn match_to_edit(
    document: &Document,
    query: &str,
    current: Option<EntryId>,
) -> Option<EntryId> {
    let matches = key_matches(document, query);
    current
        .filter(|id| matches.contains(id))
        .or_else(|| matches.first().copied())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_uses_only_keys_and_keeps_duplicate_occurrences_in_source_order() {
        let document = Document::from_bytes(
            b"# DATABASE comment\nTOKEN=DATABASE_hidden_value\nDATABASE_URL=first\n\nDATABASE_URL=second\nDATABASE_URL_DIRECT=third\nnot supported DATABASE\n".to_vec(),
        );
        let original = document.bytes().to_vec();
        let matches = key_matches(&document, "  dataBASE_url  ");
        assert_eq!(matches, [EntryId(2), EntryId(3), EntryId(4)]);
        assert!(key_matches(&document, "hidden").is_empty());
        assert!(key_matches(&document, "comment").is_empty());
        assert!(key_matches(&document, "not supported").is_empty());
        assert!(key_matches(&document, "  ").is_empty());
        assert_eq!(document.bytes(), original);
        assert_eq!(document.revision(), 0);
    }

    #[test]
    fn enter_edits_the_selected_match_or_the_first_one() {
        let document = Document::from_bytes(
            b"# DATABASE comment\nTOKEN=a\nDATABASE_URL=first\nDATABASE_URL=second\nDATABASE_HOST=third\n".to_vec(),
        );
        let matches = key_matches(&document, "database");
        assert_eq!(matches.len(), 3);
        assert_eq!(match_to_edit(&document, "database", None), Some(matches[0]));
        let next = adjacent_match(&matches, None, false);
        let next = adjacent_match(&matches, next, false);
        assert_eq!(match_to_edit(&document, "database", next), Some(matches[1]));
        assert_eq!(
            match_to_edit(&document, "database", Some(matches[2])),
            Some(matches[2])
        );
        // A stale selection (no longer a match) falls back to the first match.
        assert_eq!(
            match_to_edit(&document, "database", Some(EntryId(9999))),
            Some(matches[0])
        );
        assert_eq!(match_to_edit(&document, "nothing", Some(matches[1])), None);
        assert_eq!(match_to_edit(&document, "  ", None), None);
        assert_eq!(match_to_edit(&document, "", Some(matches[1])), None);
        assert_eq!(document.revision(), 0);
    }

    #[test]
    fn navigation_wraps_and_recovers_from_deleted_or_missing_selection() {
        let matches = [EntryId(4), EntryId(9), EntryId(12)];
        assert_eq!(adjacent_match(&matches, None, false), Some(EntryId(4)));
        assert_eq!(adjacent_match(&matches, None, true), Some(EntryId(12)));
        assert_eq!(
            adjacent_match(&matches, Some(EntryId(12)), false),
            Some(EntryId(4))
        );
        assert_eq!(
            adjacent_match(&matches, Some(EntryId(4)), true),
            Some(EntryId(12))
        );
        assert_eq!(
            adjacent_match(&matches, Some(EntryId(9)), false),
            Some(EntryId(12))
        );
        assert_eq!(
            adjacent_match(&matches, Some(EntryId(9)), true),
            Some(EntryId(4))
        );
        assert_eq!(
            adjacent_match(&matches, Some(EntryId(99)), false),
            Some(EntryId(4))
        );
        assert_eq!(
            adjacent_match(&[EntryId(4)], Some(EntryId(4)), true),
            Some(EntryId(4))
        );
        assert_eq!(adjacent_match(&[], None, false), None);
    }

    #[test]
    fn matches_follow_rename_and_new_duplicate_without_searching_values() {
        let mut document = Document::from_bytes(b"FIRST=secret\nSECOND=secret\n".to_vec());
        let second = EntryId(2);
        document
            .rename_entry(second, "FIRST_OTHER", document.revision())
            .unwrap();
        let duplicate = document
            .add_entry("FIRST", "value", true, document.revision())
            .unwrap();
        assert_eq!(
            key_matches(&document, "first"),
            [EntryId(1), second, duplicate]
        );
        document.remove_entry(second, document.revision()).unwrap();
        assert_eq!(key_matches(&document, "first"), [EntryId(1), duplicate]);
        assert!(key_matches(&document, "secret").is_empty());
    }
}
