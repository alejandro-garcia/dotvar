use env_core::{
    BlockKind, Document, EditError, EntryId, HISTORY_MAX_BYTES, HISTORY_MAX_OPERATIONS,
};

fn ids(document: &Document) -> Vec<EntryId> {
    document
        .blocks()
        .iter()
        .filter_map(|block| match &block.kind {
            BlockKind::Assignment(entry) => Some(entry.id),
            _ => None,
        })
        .collect()
}

fn value(document: &Document, id: EntryId) -> &str {
    document.entry_value(id).unwrap()
}

#[test]
fn value_patch_preserves_bom_export_spacing_comments_other_entries_and_terminators() {
    let source = b"\xef\xbb\xbf# before\r\n \texport KEY \t= \t'old token'  \t# inline\r\n\nOTHER=unchanged\n# eof";
    let mut document = Document::from_bytes(source.to_vec());
    let entries = ids(&document);
    let before = document.entry(entries[0]).unwrap().value.clone();
    assert!(
        document
            .set_value(entries[0], "\"new # literal = token\"", 0)
            .unwrap()
    );
    let mut expected = source[..before.start].to_vec();
    expected.extend_from_slice(b"\"new # literal = token\"");
    expected.extend_from_slice(&source[before.end..]);
    assert_eq!(document.bytes(), expected);
    assert_eq!(ids(&document), entries);
    assert_eq!(value(&document, entries[1]), "unchanged");
    assert_eq!(document.revision(), 1);
    assert!(document.undo());
    assert_eq!(document.bytes(), source);
    assert_eq!(ids(&document), entries);
    assert!(document.redo());
    assert_eq!(document.bytes(), expected);
    assert_eq!(ids(&document), entries);
}

#[test]
fn all_spacing_and_quote_combinations_are_reversible_exact_patches() {
    for indent in ["", " ", "\t"] {
        for gap in ["", " ", "\t "] {
            for ending in ["", "\n", "\r\n"] {
                for token in ["", "literal", "\"a # b\"", "'mañana'", "${NAME}"] {
                    let source =
                        format!("{indent}export NAME{gap}={gap}'before'{gap} # kept{ending}");
                    let mut document = Document::from_bytes(source.as_bytes().to_vec());
                    let id = ids(&document)[0];
                    let range = document.entry(id).unwrap().value.clone();
                    let mut expected = source.as_bytes()[..range.start].to_vec();
                    expected.extend_from_slice(token.as_bytes());
                    expected.extend_from_slice(&source.as_bytes()[range.end..]);
                    document.set_value(id, token, 0).unwrap();
                    assert_eq!(document.bytes(), expected);
                    assert_eq!(value(&document, id), token);
                    assert!(document.undo());
                    assert_eq!(document.bytes(), source.as_bytes());
                }
            }
        }
    }
}

#[test]
fn toggling_a_variable_is_a_reversible_patch_to_the_selected_occurrence() {
    let source = b"\xef\xbb\xbfKEY=old\n \texport KEY \t= \t'new # token'  # inline\r\n# eof";
    let commented = b"\xef\xbb\xbfKEY=old\n \t# export KEY \t= \t'new # token'  # inline\r\n# eof";
    let mut doc = Document::from_bytes(source.to_vec());
    let entries = ids(&doc);
    let id = entries[1];
    assert!(doc.set_commented(id, true, 0).unwrap());
    assert_eq!(doc.bytes(), commented);
    assert_eq!(ids(&doc), entries);
    assert!(doc.entry(id).unwrap().commented);
    assert_eq!(value(&doc, id), "'new # token'");
    assert!(!doc.set_commented(id, true, 1).unwrap());
    assert!(doc.undo());
    assert_eq!(doc.bytes(), source);
    assert!(doc.redo());
    assert_eq!(doc.bytes(), commented);
    assert!(doc.set_commented(id, false, 3).unwrap());
    assert_eq!(doc.bytes(), source);
    assert_eq!(ids(&doc), entries);
}

#[test]
fn multiline_comment_round_trip_restores_every_byte_without_a_final_newline() {
    let source = b"NOTE='one\r\n\nOTHER=literal\n#| still literal\r\nlast' # kept";
    let commented = b"# NOTE='one\r\n#| \n#| OTHER=literal\n#| #| still literal\r\n#| last' # kept";
    let mut doc = Document::from_bytes(source.to_vec());
    let id = ids(&doc)[0];
    assert!(doc.set_commented(id, true, 0).unwrap());
    assert_eq!(doc.bytes(), commented);

    let mut reopened = Document::from_bytes(doc.bytes().to_vec());
    assert_eq!(reopened.blocks().len(), 1);
    assert!(reopened.entry(id).unwrap().commented);
    assert_eq!(
        value(&reopened, id),
        value(&Document::from_bytes(source.to_vec()), id)
    );
    assert!(reopened.set_commented(id, false, 0).unwrap());
    assert_eq!(reopened.bytes(), source);
}

#[test]
fn existing_commented_variables_can_be_edited_without_activating_them() {
    let source = b"# before\n#\tNAME=old # keep\n# PARTIAL=\"unclosed\n# OTHER=yes\nACTIVE=last\n";
    let mut doc = Document::from_bytes(source.to_vec());
    let entries = ids(&doc);
    assert_eq!(entries.len(), 3);
    assert!(matches!(doc.blocks()[2].kind, BlockKind::Comment));
    let id = entries[0];
    assert!(doc.entry(id).unwrap().commented);
    assert!(doc.entry(entries[1]).unwrap().commented);
    doc.rename_entry(id, "RENAMED", 0).unwrap();
    doc.set_value(id, "\"one\nsecond\"", 1).unwrap();
    doc.set_text_field(env_core::TextField::InlineComment(id), " edited", 2)
        .unwrap();
    assert_eq!(value(&doc, id), "\"one\nsecond\"");
    assert!(doc.entry(id).unwrap().commented);
    assert!(doc.set_commented(id, false, 3).unwrap());
    assert_eq!(
        doc.bytes(),
        b"# before\nRENAMED=\"one\nsecond\" # edited\n# PARTIAL=\"unclosed\n# OTHER=yes\nACTIVE=last\n"
    );
    assert_eq!(ids(&doc), entries);
}

#[test]
fn duplicate_entries_are_edited_by_id_and_survive_length_changes_before_them() {
    let mut document = Document::from_bytes(b"SAME=first\nSAME=second\nSAME=third\n".to_vec());
    let entries = ids(&document);
    document
        .set_value(entries[0], "a much longer token", 0)
        .unwrap();
    document.set_value(entries[1], "middle", 1).unwrap();
    document.rename_entry(entries[2], "RENAMED", 2).unwrap();
    assert_eq!(
        document.bytes(),
        b"SAME=a much longer token\nSAME=middle\nRENAMED=third\n"
    );
    assert_eq!(ids(&document), entries);
    for _ in 0..3 {
        assert!(document.undo());
    }
    assert_eq!(document.bytes(), b"SAME=first\nSAME=second\nSAME=third\n");
    assert_eq!(ids(&document), entries);
    for _ in 0..3 {
        assert!(document.redo());
    }
    assert_eq!(ids(&document), entries);
}

#[test]
fn multiline_edits_are_one_entry_and_do_not_normalize_other_terminators() {
    let mut document =
        Document::from_bytes(b"A=one\r\nMULTI=\"old\r\nline\" # tail\nZ=last".to_vec());
    let entries = ids(&document);
    let token = "'primera línea\n\nLOOKS_LIKE_KEY=part of token\r\núltima'";
    document.set_value(entries[1], token, 0).unwrap();
    assert_eq!(
        document.bytes(),
        format!("A=one\r\nMULTI={token} # tail\nZ=last").as_bytes()
    );
    assert_eq!(ids(&document), entries);
    assert_eq!(value(&document, entries[1]), token);
    document.set_value(entries[1], "short", 1).unwrap();
    assert_eq!(document.bytes(), b"A=one\r\nMULTI=short # tail\nZ=last");
    assert_eq!(ids(&document), entries);
}

#[test]
fn invalid_drafts_cannot_inject_assignments_comments_or_unclosed_quotes() {
    let source = b"KEY=before # retained\nAFTER=safe\n";
    let invalid = [
        "literal\nINJECTED=value",
        "\"unterminated",
        "one # injected",
        "one#ambiguous",
        "'one'\n",
        "'one' ignored",
        "\"value\0\"",
        "leading ",
        " leading",
        "$(command)",
        "trailing\\",
        "\"quoted\"#comment",
        "value\rINJECTED=token",
    ];
    for token in invalid {
        let mut document = Document::from_bytes(source.to_vec());
        let id = ids(&document)[0];
        assert!(document.validate_value(id, token).is_err(), "{token:?}");
        assert!(document.set_value(id, token, 0).is_err(), "{token:?}");
        assert_eq!(document.bytes(), source);
        assert_eq!(document.revision(), 0);
        assert!(!document.can_undo());
    }
}

#[test]
fn validation_is_side_effect_free_and_noop_keeps_redo_and_revision() {
    let mut document = Document::from_bytes(b"A=original\n".to_vec());
    let id = ids(&document)[0];
    document.validate_value(id, "'valid draft'").unwrap();
    assert_eq!(document.bytes(), b"A=original\n");
    assert_eq!(document.revision(), 0);
    assert!(!document.can_undo());
    assert!(!document.set_value(id, "original", 0).unwrap());
    assert!(!document.rename_entry(id, "A", 0).unwrap());
    document.set_value(id, "new", 0).unwrap();
    assert!(document.undo());
    let revision = document.revision();
    assert!(!document.set_value(id, "original", revision).unwrap());
    assert!(!document.rename_entry(id, "A", revision).unwrap());
    assert_eq!(document.revision(), revision);
    assert!(document.can_redo());
    assert!(document.redo());
    assert_eq!(document.bytes(), b"A=new\n");
    assert_eq!(document.revision(), revision + 1);
}

#[test]
fn empty_values_with_inline_comments_gain_only_the_required_separator() {
    let mut document = Document::from_bytes(b"A=  # kept\r\nB=x # also kept\n".to_vec());
    let entries = ids(&document);
    document.set_value(entries[0], "one", 0).unwrap();
    assert_eq!(document.bytes(), b"A=  one # kept\r\nB=x # also kept\n");
    assert_eq!(value(&document, entries[0]), "one");
    document.set_value(entries[1], "", 1).unwrap();
    assert_eq!(document.bytes(), b"A=  one # kept\r\nB= # also kept\n");
    document.set_value(entries[1], "'two'", 2).unwrap();
    assert_eq!(
        document.bytes(),
        b"A=  one # kept\r\nB= 'two' # also kept\n"
    );
    for _ in 0..3 {
        document.undo();
    }
    assert_eq!(document.bytes(), b"A=  # kept\r\nB=x # also kept\n");
}

#[test]
fn stale_revision_and_missing_entry_fail_without_mutation() {
    let mut document = Document::from_bytes(b"A=x\n".to_vec());
    let id = ids(&document)[0];
    document.set_value(id, "changed", 0).unwrap();
    let expected = document.bytes().to_vec();
    assert_eq!(
        document.set_value(id, "bad", 0),
        Err(EditError::StaleRevision {
            expected: 0,
            actual: 1
        })
    );
    assert!(matches!(
        document.rename_entry(id, "B", 0),
        Err(EditError::StaleRevision { .. })
    ));
    assert!(matches!(
        document.remove_entry(id, 0),
        Err(EditError::StaleRevision { .. })
    ));
    assert!(matches!(
        document.add_entry("B", "new", false, 0),
        Err(EditError::StaleRevision { .. })
    ));
    assert_eq!(
        document.set_value(EntryId(999), "safe", 1),
        Err(EditError::MissingEntry(EntryId(999)))
    );
    assert_eq!(document.bytes(), expected);
    assert_eq!(document.revision(), 1);
    document.undo();
    assert!(matches!(
        document.set_value(id, "old revision", 0),
        Err(EditError::StaleRevision { .. })
    ));
}

#[test]
fn renaming_only_changes_the_ascii_name_range() {
    let source = b"\xef\xbb\xbf  export OLD \t= \"literal\\n\" \t# tail\r\nOTHER=unchanged";
    let mut document = Document::from_bytes(source.to_vec());
    let entries = ids(&document);
    document
        .rename_entry(entries[0], "new_NAME_123", 0)
        .unwrap();
    assert_eq!(
        document.bytes(),
        b"\xef\xbb\xbf  export new_NAME_123 \t= \"literal\\n\" \t# tail\r\nOTHER=unchanged"
    );
    assert_eq!(ids(&document), entries);
    assert_eq!(value(&document, entries[0]), "\"literal\\n\"");
    for invalid in ["", "1NAME", "NAME-OTHER", "ñame", "A\nB", "A=B", "A B"] {
        assert_eq!(
            document.rename_entry(entries[0], invalid, 1),
            Err(EditError::InvalidName)
        );
    }
    assert_eq!(document.revision(), 1);
    document.undo();
    assert_eq!(document.bytes(), source);
}

#[test]
fn appending_pasted_variables_preserves_tokens_comments_and_one_undo_step() {
    let before = b"\xef\xbb\xbf# local\r\nORIGINAL=one";
    let mut document = Document::from_bytes(before.to_vec());
    let fragment = Document::from_bytes(b"\xef\xbb\xbf# Development defaults\nexport URL=https://app.local?a=b\nUSER='demo@example.invalid' # contact\nCERT=\"first\nsecond\"\nEMPTY=\n\n".to_vec());
    let ids = document.append_entries(&fragment, false, 0).unwrap();
    assert_eq!(ids.len(), 4);
    assert_eq!(document.bytes(), b"\xef\xbb\xbf# local\r\nORIGINAL=one\r\n# Development defaults\r\nexport URL=https://app.local?a=b\r\nUSER='demo@example.invalid' # contact\r\nCERT=\"first\nsecond\"\r\nEMPTY=");
    assert!(document.undo());
    assert_eq!(document.bytes(), before);
    assert!(document.redo());
    assert_eq!(value(&document, ids[2]), "\"first\nsecond\"");
}

#[test]
fn appending_pasted_variables_rejects_invalid_blocks_and_duplicates_atomically() {
    let before = b"A=one\n";
    let mut document = Document::from_bytes(before.to_vec());
    let parse = |text: &str| Document::from_bytes(text.as_bytes().to_vec());
    let invalid = parse("B=two\nC='unclosed\nD=three");
    assert_eq!(
        document.append_entries(&invalid, false, 0),
        Err(EditError::InvalidFragment)
    );
    let collision = parse("B=two\nA=new");
    assert!(matches!(
        document.append_entries(&collision, false, 0),
        Err(EditError::DuplicateName { .. })
    ));
    let repeated = parse("B=two\nB=three");
    assert_eq!(
        document.append_entries(&repeated, false, 0),
        Err(EditError::DuplicateFragmentName)
    );
    assert_eq!(document.bytes(), before);
    assert!(!document.can_undo());

    let opaque_tail = b"A='unclosed";
    let mut uncertain = Document::from_bytes(opaque_tail.to_vec());
    assert_eq!(
        uncertain.append_entries(&collision, true, 0),
        Err(EditError::UnsafeBoundary)
    );
    assert_eq!(uncertain.bytes(), opaque_tail);

    document.append_entries(&repeated, true, 0).unwrap();
    assert_eq!(document.bytes(), b"A=one\nB=two\nB=three\n");
}

#[test]
fn adding_respects_eof_bom_and_predominant_line_ending() {
    let cases: &[(&[u8], &[u8])] = &[
        (b"", b"NEW=value\n"),
        (b"\xef\xbb\xbf", b"\xef\xbb\xbfNEW=value\n"),
        (b"A=one", b"A=one\nNEW=value"),
        (b"A=one\n", b"A=one\nNEW=value\n"),
        (
            b"# before\r\nA=one\r\nB=two\n# eof",
            b"# before\r\nA=one\r\nB=two\n# eof\r\nNEW=value",
        ),
        (
            b"# before\r\nA=one\r\n",
            b"# before\r\nA=one\r\nNEW=value\r\n",
        ),
        (b"# trailing comment", b"# trailing comment\nNEW=value"),
        (b"  ", b"  \nNEW=value"),
    ];
    for &(source, expected) in cases {
        let mut document = Document::from_bytes(source.to_vec());
        let previous_ids = ids(&document);
        let id = document.add_entry("NEW", "value", false, 0).unwrap();
        assert_eq!(document.bytes(), expected);
        assert_eq!(value(&document, id), "value");
        assert_eq!(&ids(&document)[..previous_ids.len()], previous_ids);
        assert!(document.undo());
        assert_eq!(document.bytes(), source);
        assert_eq!(ids(&document), previous_ids);
        assert!(document.redo());
        assert_eq!(document.bytes(), expected);
        assert_eq!(value(&document, id), "value");
    }
}

#[test]
fn adding_duplicate_requires_explicit_override_and_returns_first_occurrence() {
    let mut document = Document::from_bytes(b"A=first\nA=second\n".to_vec());
    let entries = ids(&document);
    assert_eq!(
        document.add_entry("A", "third", false, 0),
        Err(EditError::DuplicateName {
            existing: entries[0]
        })
    );
    assert_eq!(document.revision(), 0);
    let new = document.add_entry("A", "third", true, 0).unwrap();
    assert!(!entries.contains(&new));
    assert_eq!(document.bytes(), b"A=first\nA=second\nA=third\n");
    assert_eq!(ids(&document), [entries[0], entries[1], new]);
    document.undo();
    let another = document
        .add_entry("B", "different", false, document.revision())
        .unwrap();
    assert_ne!(
        new, another,
        "undo must not recycle identity for another variable"
    );
    assert!(!document.can_redo());
}

#[test]
fn adding_rejects_invalid_tokens_names_and_uncertain_eof() {
    let mut document = Document::from_bytes(b"A=one\n".to_vec());
    for token in [
        "new\nB=injected",
        "new # comment",
        "\"unterminated",
        "new ",
        " new",
        "new\\",
    ] {
        assert!(document.add_entry("B", token, false, 0).is_err());
    }
    assert_eq!(
        document.add_entry("B=B", "safe", false, 0),
        Err(EditError::InvalidName)
    );
    assert_eq!(document.bytes(), b"A=one\n");
    assert!(!document.can_undo());
    for source in [
        b"A=\"unclosed".as_slice(),
        b"A=continuation\\\n",
        b"A=prefix\"unclosed\n",
    ] {
        let mut document = Document::from_bytes(source.to_vec());
        assert_eq!(
            document.add_entry("SAFE", "safe", false, 0),
            Err(EditError::UnsafeBoundary)
        );
        assert_eq!(document.bytes(), source);
    }
}

#[test]
fn adding_multiline_token_keeps_existing_opaque_content_and_identity() {
    let mut document = Document::from_bytes(b"not a dotenv assignment\r\nA=one\r\n".to_vec());
    let previous = ids(&document)[0];
    let added = document
        .add_entry("NEW", "\"line1\nA=part of token\nline3\"", false, 0)
        .unwrap();
    assert_eq!(
        document.bytes(),
        b"not a dotenv assignment\r\nA=one\r\nNEW=\"line1\nA=part of token\nline3\"\r\n"
    );
    assert_eq!(ids(&document), [previous, added]);
    assert!(matches!(document.blocks()[0].kind, BlockKind::Opaque(_)));
}

#[test]
fn remove_keeps_previous_comments_and_inline_comment_as_independent_line() {
    let source = b"\xef\xbb\xbf# previous\r\n \texport DUP = \"line1\nline2\"  # inline kept\r\nDUP=second\n# eof";
    let mut document = Document::from_bytes(source.to_vec());
    let entries = ids(&document);
    document.remove_entry(entries[0], 0).unwrap();
    assert_eq!(
        document.bytes(),
        b"\xef\xbb\xbf# previous\r\n \t# inline kept\r\nDUP=second\n# eof"
    );
    assert_eq!(ids(&document), [entries[1]]);
    assert!(document.entry(entries[0]).is_none());
    assert_eq!(value(&document, entries[1]), "second");
    assert!(document.undo());
    assert_eq!(document.bytes(), source);
    assert_eq!(ids(&document), entries);
    assert!(document.redo());
    document
        .remove_entry(entries[1], document.revision())
        .unwrap();
    assert_eq!(
        document.bytes(),
        b"\xef\xbb\xbf# previous\r\n \t# inline kept\r\n# eof"
    );
}

#[test]
fn remove_final_assignment_preserves_bom_and_its_final_comment_without_newline() {
    let mut document = Document::from_bytes(b"\xef\xbb\xbf A=token # final".to_vec());
    let id = ids(&document)[0];
    document.remove_entry(id, 0).unwrap();
    assert_eq!(document.bytes(), b"\xef\xbb\xbf # final");
    document.undo();
    assert_eq!(document.bytes(), b"\xef\xbb\xbf A=token # final");
    document.set_value(id, "", document.revision()).unwrap();
    document.remove_entry(id, document.revision()).unwrap();
    assert_eq!(document.bytes(), b"\xef\xbb\xbf # final");
    let mut document = Document::from_bytes(b"\xef\xbb\xbfA=token".to_vec());
    document.remove_entry(ids(&document)[0], 0).unwrap();
    assert_eq!(document.bytes(), b"\xef\xbb\xbf");
}

#[test]
fn sequence_undo_redo_restores_exact_versions_and_stable_ids() {
    let mut document = Document::from_bytes(b"# before\r\nA=one\r\nB=two # tail\n".to_vec());
    let original_ids = ids(&document);
    let mut versions = vec![(document.bytes().to_vec(), ids(&document))];
    document
        .set_value(original_ids[0], "'mañana'", document.revision())
        .unwrap();
    versions.push((document.bytes().to_vec(), ids(&document)));
    let added = document
        .add_entry("ADDED", "\"multi\nline\"", false, document.revision())
        .unwrap();
    versions.push((document.bytes().to_vec(), ids(&document)));
    document
        .rename_entry(original_ids[1], "NEW_B", document.revision())
        .unwrap();
    versions.push((document.bytes().to_vec(), ids(&document)));
    document
        .remove_entry(original_ids[0], document.revision())
        .unwrap();
    versions.push((document.bytes().to_vec(), ids(&document)));
    document
        .set_value(added, "last", document.revision())
        .unwrap();
    versions.push((document.bytes().to_vec(), ids(&document)));
    let mut last_revision = document.revision();
    for (bytes, entries) in versions[..versions.len() - 1].iter().rev() {
        assert!(document.undo());
        assert_eq!(document.bytes(), bytes);
        assert_eq!(ids(&document), *entries);
        assert!(document.revision() > last_revision);
        last_revision = document.revision();
    }
    assert!(!document.undo());
    for (bytes, entries) in &versions[1..] {
        assert!(document.redo());
        assert_eq!(document.bytes(), bytes);
        assert_eq!(ids(&document), *entries);
        assert!(document.revision() > last_revision);
        last_revision = document.revision();
    }
    assert!(!document.redo());
}

#[test]
fn unsupported_encoding_is_read_only_for_every_operation() {
    let mut document = Document::from_bytes(b"A=\xff\n".to_vec());
    let id = EntryId(1);
    assert_eq!(
        document.validate_value(id, "safe"),
        Err(EditError::ReadOnly)
    );
    assert_eq!(document.set_value(id, "safe", 0), Err(EditError::ReadOnly));
    assert_eq!(document.rename_entry(id, "B", 0), Err(EditError::ReadOnly));
    assert_eq!(
        document.add_entry("B", "safe", false, 0),
        Err(EditError::ReadOnly)
    );
    assert_eq!(document.remove_entry(id, 0), Err(EditError::ReadOnly));
    assert!(!document.undo());
    assert!(!document.redo());
    assert_eq!(document.bytes(), b"A=\xff\n");
}

#[test]
fn history_keeps_the_last_hundred_operations_and_rejects_an_oversized_one_atomically() {
    let mut document = Document::from_bytes(b"A=initial\n".to_vec());
    let id = ids(&document)[0];
    for index in 0..HISTORY_MAX_OPERATIONS + 10 {
        document
            .set_value(id, &format!("value-{index}"), document.revision())
            .unwrap();
    }
    for _ in 0..HISTORY_MAX_OPERATIONS {
        assert!(document.undo());
    }
    assert!(!document.undo());
    assert_eq!(value(&document, id), "value-9");
    let before = document.bytes().to_vec();
    let revision = document.revision();
    let too_large = "x".repeat(HISTORY_MAX_BYTES + 1);
    assert_eq!(
        document.set_value(id, &too_large, revision),
        Err(EditError::HistoryLimit)
    );
    assert_eq!(document.bytes(), before);
    assert_eq!(document.revision(), revision);
    assert!(document.can_redo(), "failed edits must not erase redo");
}

#[test]
fn diagnostics_do_not_include_source_values_or_names() {
    let mut document = Document::from_bytes(b"PRIVATE_IDENTIFIER=synthetic\n".to_vec());
    let id = ids(&document)[0];
    let secret = "unquoted\nSYNTHETIC_PRIVATE_VALUE=synthetic";
    let error = document.set_value(id, secret, 0).unwrap_err();
    assert!(!error.to_string().contains("SYNTHETIC_PRIVATE_VALUE"));
    let duplicate = document
        .add_entry("PRIVATE_IDENTIFIER", "unused", false, 0)
        .unwrap_err();
    assert!(!duplicate.to_string().contains("PRIVATE_IDENTIFIER"));
}

#[test]
fn history_byte_budget_evicts_old_patches_while_recent_edits_stay_reversible() {
    let mut document = Document::from_bytes(b"A=initial\n".to_vec());
    let id = ids(&document)[0];
    for index in 0..8 {
        let token = format!("{index}{}", "x".repeat(1024 * 1024));
        document.set_value(id, &token, document.revision()).unwrap();
    }
    let final_bytes = document.bytes().to_vec();
    let mut undos = 0;
    while document.undo() {
        undos += 1;
    }
    assert!(
        undos > 0 && undos < 8,
        "large patches must be bounded by bytes, not just operation count"
    );
    for _ in 0..undos {
        assert!(document.redo());
    }
    assert_eq!(document.bytes(), final_bytes);
    assert_eq!(ids(&document), [id]);
}

#[test]
fn cloning_document_keeps_history_accounting_and_new_edits_reversible() {
    let original = b"A= # kept\n";
    let mut document = Document::from_bytes(original.to_vec());
    let id = ids(&document)[0];
    // The inserted separator causes this record's Vec capacity to exceed len.
    document
        .set_value(id, &"x".repeat(2 * 1024 * 1024), 0)
        .unwrap();
    let mut cloned = document.clone();
    assert!(cloned.undo());
    assert_eq!(cloned.bytes(), original);
    cloned
        .rename_entry(id, &"N".repeat(6 * 1024 * 1024), cloned.revision())
        .unwrap();
    assert!(
        cloned.can_undo(),
        "replacing redo in a clone must not retain stale byte accounting"
    );
    assert!(cloned.undo());
    assert_eq!(cloned.bytes(), original);
    assert_eq!(value(&document, id).len(), 2 * 1024 * 1024);
}

#[test]
fn comment_edits_preserve_syntax_ids_bom_and_mixed_terminators() {
    use env_core::TextField;
    let source = b"\xef\xbb\xbf \t# before\r\nexport A = 'literal # value' \t# tail\n# eof";
    let mut d = Document::from_bytes(source.to_vec());
    let a = ids(&d)[0];
    for (field, text) in [
        (TextField::Comment(0), " encabezado á # ="),
        (TextField::InlineComment(a), " changed ${A}"),
        (TextField::Comment(2), ""),
    ] {
        d.validate_text_field(field, text).unwrap();
        d.set_text_field(field, text, d.revision()).unwrap();
    }
    let expected =
        "\u{feff} \t# encabezado á # =\r\nexport A = 'literal # value' \t# changed ${A}\n#";
    assert_eq!(d.bytes(), expected.as_bytes());
    assert_eq!(ids(&d), vec![a]);
    for _ in 0..3 {
        assert!(d.undo());
    }
    assert_eq!(d.bytes(), source);
    for _ in 0..3 {
        assert!(d.redo());
    }
    assert_eq!(d.bytes(), expected.as_bytes());
}

#[test]
fn text_edits_reject_injection_stale_revisions_and_read_only_source() {
    use env_core::TextField;
    let source = b"# header\nA=old # tail\nB=keep\n";
    let mut d = Document::from_bytes(source.to_vec());
    let a = ids(&d)[0];
    for field in [TextField::Comment(0), TextField::InlineComment(a)] {
        for text in ["bad\nINJECTED=value", "bad\rtext", "bad\0text"] {
            assert_eq!(
                d.validate_text_field(field, text),
                Err(EditError::InvalidComment)
            );
            assert_eq!(
                d.set_text_field(field, text, 0),
                Err(EditError::InvalidComment)
            );
            assert_eq!(d.bytes(), source);
        }
    }
    assert_eq!(
        d.validate_text_field(TextField::Name(a), "INVALID NAME"),
        Err(EditError::InvalidName)
    );
    d.set_text_field(TextField::Name(a), "RENAMED", 0).unwrap();
    assert!(matches!(
        d.set_text_field(TextField::Comment(0), " changed", 0),
        Err(EditError::StaleRevision { .. })
    ));
    let mut read_only = Document::from_bytes(vec![0xff]);
    assert_eq!(
        read_only.set_text_field(TextField::Comment(0), "new", 0),
        Err(EditError::ReadOnly)
    );
    assert!(d.text_field_range(TextField::Comment(1)).is_none());
    assert!(
        d.text_field_range(TextField::InlineComment(EntryId(99)))
            .is_none()
    );
}

#[test]
fn moving_blocks_keeps_duplicates_multiline_comments_and_exact_history() {
    let source =
        b"\xef\xbb\xbf# heading\r\nSAME='line one\r\nline two' # inline\n\nSAME=second\r\n# last";
    let mut d = Document::from_bytes(source.to_vec());
    let original_ids = ids(&d);
    d.move_block(1, 4, 0).unwrap();
    assert_eq!(
        d.bytes(),
        b"\xef\xbb\xbf# heading\r\n\nSAME=second\r\n# last\nSAME='line one\r\nline two' # inline"
    );
    assert_eq!(ids(&d), vec![original_ids[1], original_ids[0]]);
    assert!(d.undo());
    assert_eq!(d.bytes(), source);
    assert_eq!(ids(&d), original_ids);
    assert!(!d.can_undo());
    assert!(d.redo());
    d.move_block(3, 0, d.revision()).unwrap();
    assert!(d.bytes().starts_with(b"\xef\xbb\xbf# last\n# heading\r\n"));
    assert_eq!(ids(&d), vec![original_ids[1], original_ids[0]]);
}

#[test]
fn every_supported_move_preserves_bodies_final_newline_and_reversibility() {
    for ending in ["", "\n", "\r\n"] {
        let source =
            format!("# first\r\nA=one # inline\n# middle\r\nB='two\\nline'\n# last{ending}");
        for from in 0..5 {
            for to in 0..5 {
                let mut d = Document::from_bytes(source.as_bytes().to_vec());
                let before_ids = ids(&d);
                let bodies: Vec<_> = d
                    .blocks()
                    .iter()
                    .map(|b| {
                        d.text(b.range.clone())
                            .unwrap()
                            .trim_end_matches(['\r', '\n'])
                            .to_owned()
                    })
                    .collect();
                assert_eq!(d.move_block(from, to, 0).unwrap(), from != to);
                let mut expected = bodies.clone();
                let moved = expected.remove(from);
                expected.insert(to, moved);
                let actual: Vec<_> = d
                    .blocks()
                    .iter()
                    .map(|b| {
                        d.text(b.range.clone())
                            .unwrap()
                            .trim_end_matches(['\r', '\n'])
                            .to_owned()
                    })
                    .collect();
                assert_eq!(actual, expected);
                assert_eq!(d.bytes().ends_with(b"\n"), !ending.is_empty());
                if from != to {
                    assert!(d.undo());
                }
                assert_eq!(d.bytes(), source.as_bytes());
                assert_eq!(ids(&d), before_ids);
            }
        }
    }
}

#[test]
fn reordering_rejects_stale_read_only_opaque_and_unsafe_boundaries_without_changes() {
    let mut d = Document::from_bytes(b"# first\nA=one\nUNFINISHED='unknown".to_vec());
    let before = d.bytes().to_vec();
    assert!(d.move_block(0, 2, 0).is_err());
    assert!(d.move_block(2, 0, 0).is_err());
    assert!(d.move_block(0, 99, 0).is_err());
    assert!(matches!(
        d.move_block(0, 1, 99),
        Err(EditError::StaleRevision { .. })
    ));
    assert_eq!(d.bytes(), before);
    assert!(!d.can_undo());
    let mut read_only = Document::from_bytes(vec![0xff]);
    assert_eq!(read_only.move_block(0, 0, 0), Err(EditError::ReadOnly));
}

#[test]
fn moving_byte_identical_duplicates_preserves_their_separate_identities() {
    let mut d = Document::from_bytes(b"SAME=value\nSAME=value\n".to_vec());
    let before_ids = ids(&d);
    assert!(d.move_block(0, 1, 0).unwrap());
    assert_eq!(ids(&d), vec![before_ids[1], before_ids[0]]);
    assert_eq!(d.bytes(), b"SAME=value\nSAME=value\n");
    assert!(d.undo());
    assert_eq!(ids(&d), before_ids);
    assert!(d.redo());
    assert_eq!(ids(&d), vec![before_ids[1], before_ids[0]]);
}
