use env_core::{Assignment, BlockKind, Diagnostic, Document};

const FIXTURES: &[&[u8]] = &[
    include_bytes!("fixtures/basic.fixture"),
    include_bytes!("fixtures/multiline.fixture"),
    include_bytes!("fixtures/opaque.fixture"),
];

fn entries(document: &Document) -> Vec<&Assignment> {
    document
        .blocks()
        .iter()
        .filter_map(|block| match &block.kind {
            BlockKind::Assignment(entry) => Some(entry),
            _ => None,
        })
        .collect()
}

fn entry<'a>(document: &'a Document, name: &str) -> &'a Assignment {
    entries(document)
        .into_iter()
        .find(|entry| document.text(entry.name.clone()) == Some(name))
        .expect("fixture entry exists")
}

fn assert_partition(document: &Document) {
    let mut offset = if document.has_bom() && !document.is_read_only() {
        3
    } else {
        0
    };
    for block in document.blocks() {
        assert_eq!(block.range.start, offset);
        assert!(block.range.end > block.range.start);
        assert!(block.range.end <= document.bytes().len());
        offset = block.range.end;
        if let BlockKind::Assignment(entry) = &block.kind {
            assert!(block.range.start <= entry.name.start);
            assert!(entry.name.end < entry.value.start);
            assert!(entry.value.start <= entry.value.end);
            assert!(entry.value.end <= block.range.end);
            assert!(document.text(entry.name.clone()).is_some());
            assert!(document.text(entry.value.clone()).is_some());
            if let Some(comment) = &entry.inline_comment {
                assert!(entry.value.end <= comment.start);
                assert!(comment.end <= block.range.end);
                assert!(document.text(comment.clone()).unwrap().starts_with('#'));
            }
        }
    }
    assert_eq!(offset, document.bytes().len());
}

#[test]
fn fixtures_round_trip_and_cover_source_in_order() {
    for &source in FIXTURES {
        let document = Document::from_bytes(source.to_vec());
        assert_eq!(document.bytes(), source);
        assert!(!document.is_read_only());
        assert_partition(&document);
    }
}

#[test]
fn byte_edges_round_trip_without_normalization() {
    let sources: &[&[u8]] = &[
        b"",
        b"\xef\xbb\xbf",
        b"NO_NEWLINE=token",
        b"A=1\r\nB=2\r\n",
        b"\xef\xbb\xbf# BOM\r\nA=1\n\t\r\nB=2",
        b"\t \n# final comment",
        "UNICODE=mañana ☀ 日本語\r\n".as_bytes(),
    ];
    for &source in sources {
        let document = Document::from_bytes(source.to_vec());
        assert_eq!(document.bytes(), source);
        assert_eq!(document.has_bom(), source.starts_with(b"\xef\xbb\xbf"));
        assert_partition(&document);
    }
}

#[test]
fn assignments_expose_only_the_name_and_literal_value_ranges() {
    let source = b" \texport NAME \t= \t\"synthetic = # value\"  \t# tail\r\n";
    let document = Document::from_bytes(source.to_vec());
    let assignment = entry(&document, "NAME");
    assert_eq!(
        document.text(assignment.value.clone()),
        Some("\"synthetic = # value\"")
    );
    assert_eq!(
        document.text(assignment.inline_comment.clone().unwrap()),
        Some("# tail")
    );
    assert_eq!(document.text(0..assignment.name.start), Some(" \texport "));
    assert_eq!(
        document.text(assignment.name.end..assignment.value.start),
        Some(" \t= \t")
    );
    assert_eq!(
        document.text(assignment.value.end..source.len()),
        Some("  \t# tail\r\n")
    );
    assert_partition(&document);
}

#[test]
fn empty_values_and_empty_inline_comment_values_have_valid_ranges() {
    let document = Document::from_bytes(b"EMPTY=\nSPACED=  \t\r\nCOMMENT=  # final\n".to_vec());
    assert_eq!(entries(&document).len(), 3);
    for assignment in entries(&document) {
        assert_eq!(document.text(assignment.value.clone()), Some(""));
    }
    assert_eq!(
        document.text(entry(&document, "COMMENT").inline_comment.clone().unwrap()),
        Some("# final")
    );
    assert_partition(&document);
}

#[test]
fn comments_and_blanks_are_distinct_blocks_with_original_terminators() {
    let document = Document::from_bytes(b" \t# before\r\n\t \nA=token\r\n# after".to_vec());
    assert!(matches!(document.blocks()[0].kind, BlockKind::Comment));
    assert!(matches!(document.blocks()[1].kind, BlockKind::Blank));
    assert!(matches!(
        document.blocks()[2].kind,
        BlockKind::Assignment(_)
    ));
    assert!(matches!(document.blocks()[3].kind, BlockKind::Comment));
    assert_eq!(
        document.text(document.blocks()[0].range.clone()),
        Some(" \t# before\r\n")
    );
    assert_partition(&document);
}

#[test]
fn quoted_multiline_is_one_entry_including_embedded_assignment_text() {
    let source =
        b"FIRST=one\r\nMULTI=\"line 1\r\n\r\nA=part of value\nlast\" \t# tail\r\nAFTER=two";
    let document = Document::from_bytes(source.to_vec());
    assert_eq!(entries(&document).len(), 3);
    assert_eq!(
        document.text(entry(&document, "MULTI").value.clone()),
        Some("\"line 1\r\n\r\nA=part of value\nlast\"")
    );
    assert_eq!(
        document.text(entry(&document, "AFTER").value.clone()),
        Some("two")
    );
    assert_partition(&document);
}

#[test]
fn escaped_double_quotes_and_single_quoted_backslashes_remain_literal() {
    let document = Document::from_bytes(include_bytes!("fixtures/multiline.fixture").to_vec());
    assert_eq!(entries(&document).len(), 4);
    assert_eq!(
        document.text(entry(&document, "DOUBLE").value.clone()),
        Some("\"a # inside = token, escaped \\\"quote\\\" and \\\\ slash\"")
    );
    assert_eq!(
        document.text(entry(&document, "SINGLE").value.clone()),
        Some("'literal # ${NAME} backslash\\'")
    );
    assert_partition(&document);
}

#[test]
fn duplicate_names_have_separate_identities_and_order() {
    let document = Document::from_bytes(b"SAME=first\nSAME=second\nSAME=third\n".to_vec());
    let assignments = entries(&document);
    assert_eq!(assignments.len(), 3);
    assert_ne!(assignments[0].id, assignments[1].id);
    assert_ne!(assignments[1].id, assignments[2].id);
    assert_ne!(assignments[0].id, assignments[2].id);
    assert_eq!(
        assignments
            .iter()
            .map(|assignment| document.text(assignment.value.clone()).unwrap())
            .collect::<Vec<_>>(),
        ["first", "second", "third"]
    );
}

#[test]
fn references_and_escapes_are_never_interpreted() {
    let source = br"A=${SYNTHETIC}/$NAME\n\t
EQUALS=left=right
";
    let document = Document::from_bytes(source.to_vec());
    assert_eq!(
        document.text(entry(&document, "A").value.clone()),
        Some(r"${SYNTHETIC}/$NAME\n\t")
    );
    assert_eq!(
        document.text(entry(&document, "EQUALS").value.clone()),
        Some("left=right")
    );
    assert_eq!(document.bytes(), source);
}

#[test]
fn unsupported_shell_and_ambiguous_hash_are_preserved_as_opaque() {
    let source = b"A=abc#def\nB=$(nothing)\nC=`nothing`\nD=x; nothing\nNOT-KEY=1\nGOOD=safe\n";
    let document = Document::from_bytes(source.to_vec());
    assert!(matches!(
        document.blocks()[0].kind,
        BlockKind::Opaque(Diagnostic::AmbiguousComment)
    ));
    assert!(
        document.blocks()[1..5]
            .iter()
            .all(|block| matches!(block.kind, BlockKind::Opaque(Diagnostic::UnsupportedSyntax)))
    );
    assert_eq!(entries(&document).len(), 1);
    assert_eq!(
        document.text(entry(&document, "GOOD").value.clone()),
        Some("safe")
    );
    assert_eq!(document.bytes(), source);
    assert_partition(&document);
}

#[test]
fn unclosed_quote_preserves_the_entire_uncertain_tail() {
    let source = b"GOOD=before\nBROKEN=\"unfinished\nFALSE_ENTRY=still quoted\n";
    let document = Document::from_bytes(source.to_vec());
    assert_eq!(entries(&document).len(), 1);
    assert_eq!(document.blocks().len(), 2);
    assert!(matches!(
        document.blocks()[1].kind,
        BlockKind::Opaque(Diagnostic::UnclosedQuote)
    ));
    assert_eq!(document.blocks()[1].range.end, source.len());
    assert_partition(&document);
}

#[test]
fn quote_concatenation_and_line_continuations_are_not_structured() {
    for source in [
        b"A=\"a\"'b'\n".as_slice(),
        b"A=value\\\n",
        b"A=un\"quoted\n",
    ] {
        let document = Document::from_bytes(source.to_vec());
        assert!(matches!(
            document.blocks()[0].kind,
            BlockKind::Opaque(Diagnostic::UnsupportedSyntax)
        ));
        assert_eq!(document.bytes(), source);
    }
}

#[test]
fn quoted_tail_keeps_open_second_token_opaque() {
    let prefix = b"BEFORE=safe\n";
    let source = b"BEFORE=safe\nBAD='first'\"second\nLOOKS_LIKE_ENTRY=still quoted";
    assert_opaque_quoted_tail(source, prefix);
}

#[test]
fn quoted_tail_hash_inside_an_open_suffix_keeps_the_remainder_opaque() {
    let prefix = b"BEFORE=safe\n";
    for opaque_line in [
        b"BAD='first'\"second # still quoted\n".as_slice(),
        b"BAD=\"first\"'second\t# still quoted\n",
        b"BAD='first'\"second\"#'third\n",
        b"BAD='first'\"second\"\\ #'third\n",
    ] {
        let source = [
            prefix.as_slice(),
            opaque_line,
            b"LOOKS_LIKE_ENTRY=still quoted",
        ]
        .concat();
        assert_opaque_quoted_tail(&source, prefix);
    }
}

#[test]
fn quoted_tail_stays_opaque_when_second_token_closes_on_a_later_line() {
    let prefix = b"BEFORE=safe\r\n";
    let source = b"BEFORE=safe\r\nBAD=\"first\r\nline\"'second\r\n\
                   LOOKS_LIKE_ENTRY=still quoted\r\n'\r\nAFTER=uncertain\r\n";
    assert_opaque_quoted_tail(source, prefix);
}

fn assert_opaque_quoted_tail(source: &[u8], prefix: &[u8]) -> Document {
    let document = Document::from_bytes(source.to_vec());
    assert_eq!(document.bytes(), source);
    assert_partition(&document);
    assert_eq!(document.blocks()[0].range, 0..prefix.len());
    assert_eq!(document.blocks()[1].range, prefix.len()..source.len());
    assert!(matches!(
        document.blocks()[1].kind,
        BlockKind::Opaque(Diagnostic::UnsupportedSyntax)
    ));
    assert_eq!(document.blocks().len(), 2);
    assert_eq!(entries(&document).len(), 1);
    assert_eq!(
        document.text(entry(&document, "BEFORE").value.clone()),
        Some("safe")
    );
    document
}

#[test]
fn quoted_tail_balanced_concatenation_keeps_following_entry_editable() {
    let opaque_line = b"BAD=\"first\"'second'\n";
    let source = b"BAD=\"first\"'second'\nAFTER=safe";
    let mut document = Document::from_bytes(source.to_vec());
    assert_eq!(document.bytes(), source);
    assert_partition(&document);
    assert_eq!(document.blocks()[0].range, 0..opaque_line.len());
    assert!(matches!(
        document.blocks()[0].kind,
        BlockKind::Opaque(Diagnostic::UnsupportedSyntax)
    ));
    assert_eq!(entries(&document).len(), 1);
    let after = entry(&document, "AFTER").id;
    assert!(
        document
            .set_value(after, "changed", document.revision())
            .unwrap()
    );
    assert_eq!(document.bytes(), b"BAD=\"first\"'second'\nAFTER=changed");
    assert_partition(&document);
}

#[test]
fn quoted_tail_balanced_concatenation_ignores_delimiters_in_trailing_comments() {
    for opaque_line in [
        b"BAD=\"first\"'second' # don't split\n".as_slice(),
        b"BAD='first'\"second # literal\"\t# unmatched (`\" \\\r\n",
    ] {
        let source = [opaque_line, b"AFTER=safe"].concat();
        let document = Document::from_bytes(source.clone());
        assert_eq!(document.bytes(), source);
        assert_partition(&document);
        assert_eq!(document.blocks()[0].range, 0..opaque_line.len());
        assert!(matches!(
            document.blocks()[0].kind,
            BlockKind::Opaque(Diagnostic::UnsupportedSyntax)
        ));
        assert_eq!(entries(&document).len(), 1);
        assert_eq!(
            document.entry_value(entry(&document, "AFTER").id),
            Some("safe")
        );
    }
}

#[test]
fn quoted_tail_ignores_unmatched_quote_in_trailing_comment() {
    let source = b"GOOD='first' \t# unmatched \"\r\nAFTER=safe\r\n";
    let document = Document::from_bytes(source.to_vec());
    assert_eq!(document.bytes(), source);
    assert_partition(&document);
    assert_eq!(document.blocks().len(), 2);
    assert_eq!(entries(&document).len(), 2);
    let good = entry(&document, "GOOD");
    assert_eq!(document.text(good.value.clone()), Some("'first'"));
    assert_eq!(
        document.text(good.inline_comment.clone().unwrap()),
        Some("# unmatched \"")
    );
    assert_eq!(
        document.entry_value(entry(&document, "AFTER").id),
        Some("safe")
    );
}

#[test]
fn quoted_tail_control_with_a_trailing_comment_stays_line_bounded() {
    let opaque_line = b"BAD=\"control\x1b\" # unmatched '\n";
    let source = b"BAD=\"control\x1b\" # unmatched '\nAFTER=safe";
    let document = Document::from_bytes(source.to_vec());
    assert_eq!(document.bytes(), source);
    assert_partition(&document);
    assert_eq!(document.blocks()[0].range, 0..opaque_line.len());
    assert!(matches!(
        document.blocks()[0].kind,
        BlockKind::Opaque(Diagnostic::UnsupportedSyntax)
    ));
    assert_eq!(entries(&document).len(), 1);
    assert_eq!(
        document.entry_value(entry(&document, "AFTER").id),
        Some("safe")
    );
}

#[test]
fn quoted_tail_control_with_an_open_second_token_survives_prefix_edit() {
    let prefix = b"BEFORE=safe\r\n";
    let source = b"BEFORE=safe\r\nBAD=\"control\x1b\"'second\r\nLOOKS_LIKE_ENTRY=still quoted";
    let mut document = assert_opaque_quoted_tail(source, prefix);
    let before = entry(&document, "BEFORE").id;
    assert!(
        document
            .set_value(before, "changed", document.revision())
            .unwrap()
    );

    let edited_prefix = b"BEFORE=changed\r\n";
    let opaque_tail = &source[prefix.len()..];
    assert_eq!(
        document.bytes(),
        [edited_prefix.as_slice(), opaque_tail].concat()
    );
    assert_partition(&document);
    assert_eq!(document.blocks().len(), 2);
    assert_eq!(entries(&document).len(), 1);
    assert_eq!(
        document.blocks()[1].range,
        edited_prefix.len()..document.bytes().len()
    );
    assert!(matches!(
        document.blocks()[1].kind,
        BlockKind::Opaque(Diagnostic::UnsupportedSyntax)
    ));
}

#[test]
fn quoted_tail_adjacent_hash_preserves_line_bounded_ambiguity() {
    let source = b"BAD='first'#ambiguous \"\nAFTER=safe\n";
    let document = Document::from_bytes(source.to_vec());
    assert_eq!(document.bytes(), source);
    assert_partition(&document);
    assert!(matches!(
        document.blocks()[0].kind,
        BlockKind::Opaque(Diagnostic::AmbiguousComment)
    ));
    assert_eq!(entries(&document).len(), 1);
    assert_eq!(
        document.entry_value(entry(&document, "AFTER").id),
        Some("safe")
    );
}

#[test]
fn quoted_tail_in_a_standalone_comment_stays_within_that_line() {
    let comment = b"# BAD='first'\"second\n";
    let source = b"# BAD='first'\"second\nAFTER=safe\n";
    let document = Document::from_bytes(source.to_vec());
    assert_eq!(document.bytes(), source);
    assert_partition(&document);
    assert_eq!(document.blocks()[0].range, 0..comment.len());
    assert!(matches!(document.blocks()[0].kind, BlockKind::Comment));
    assert_eq!(entries(&document).len(), 1);
    assert_eq!(
        document.entry_value(entry(&document, "AFTER").id),
        Some("safe")
    );
}

#[test]
fn controls_inside_quoted_values_are_preserved_as_opaque() {
    for source in [
        b"BEFORE=safe\nBAD=\"nul\0inside\"\nAFTER=safe\n".as_slice(),
        b"BEFORE=safe\nBAD='control\x1bsequence'\nAFTER=safe\n",
        b"BEFORE=safe\nBAD=\"escaped\\\0control\"\nAFTER=safe\n",
        b"BEFORE=safe\nBAD=\"bare\rcarriage\"\nAFTER=safe\n",
    ] {
        let document = Document::from_bytes(source.to_vec());
        assert_eq!(entries(&document).len(), 2);
        assert!(matches!(
            document.blocks()[1].kind,
            BlockKind::Opaque(Diagnostic::UnsupportedSyntax)
        ));
        assert_eq!(
            document.text(entry(&document, "AFTER").value.clone()),
            Some("safe")
        );
        assert_eq!(document.bytes(), source);
        assert_partition(&document);
    }
}

#[test]
fn supported_whitespace_in_quoted_values_remains_literal() {
    let source = b"VALUE=\"tab\there\r\nand newline\nend\"\r\n";
    let document = Document::from_bytes(source.to_vec());
    assert_eq!(entries(&document).len(), 1);
    assert_eq!(
        document.text(entry(&document, "VALUE").value.clone()),
        Some("\"tab\there\r\nand newline\nend\"")
    );
    assert_partition(&document);
}

#[test]
fn continuation_cannot_expose_the_following_line_as_an_assignment() {
    for source in [
        b"BEFORE=safe\nCONTINUED=prefix\\\nLOOKS_LIKE_ENTRY=part of continuation\n".as_slice(),
        b"BEFORE=safe\r\nCONTINUED=prefix\\\r\nLOOKS_LIKE_ENTRY=\"unterminated\r\nSTILL_INSIDE=tail\r\n",
        b"BEFORE=safe\nCONCATENATED=prefix\"unterminated\nLOOKS_LIKE_ENTRY=still quoted\n",
    ] {
        let document = Document::from_bytes(source.to_vec());
        assert_eq!(entries(&document).len(), 1);
        assert_eq!(document.blocks().len(), 2);
        assert!(matches!(document.blocks()[1].kind, BlockKind::Opaque(Diagnostic::UnsupportedSyntax)));
        assert_eq!(document.blocks()[1].range.end, source.len());
        assert_eq!(document.bytes(), source);
        assert_partition(&document);
    }
}

#[test]
fn hash_without_space_after_closing_quote_is_ambiguous() {
    for source in [
        b"A=\"value\"#ambiguous\nAFTER=safe\n".as_slice(),
        b"A='value'#ambiguous\nAFTER=safe\n",
    ] {
        let document = Document::from_bytes(source.to_vec());
        assert!(matches!(
            document.blocks()[0].kind,
            BlockKind::Opaque(Diagnostic::AmbiguousComment)
        ));
        assert_eq!(entries(&document).len(), 1);
        assert_eq!(document.bytes(), source);
        assert_partition(&document);
    }
}

#[test]
fn invalid_utf8_is_read_only_and_never_lossily_decoded() {
    for source in [
        b"NAME=caf\xe9\n".as_slice(),
        b"\xff\xfeA\0=\0x\0",
        b"\xef\xbb\xbfA=\xff\n",
    ] {
        let document = Document::from_bytes(source.to_vec());
        assert!(document.is_read_only());
        assert_eq!(document.bytes(), source);
        assert_eq!(document.text(0..source.len()), None);
        assert!(matches!(
            document.blocks()[0].kind,
            BlockKind::Opaque(Diagnostic::UnsupportedEncoding)
        ));
        assert_partition(&document);
    }
}

#[test]
fn text_refuses_invalid_ranges_and_mid_unicode_boundaries() {
    let document = Document::from_bytes("A=ñ".as_bytes().to_vec());
    assert_eq!(document.text(0..999), None);
    assert_eq!(document.text(3..4), None);
    assert_eq!(document.text(2..4), Some("ñ"));
}

#[test]
fn bounded_spacing_combinations_keep_ranges_and_exact_bytes() {
    for indent in ["", " ", "\t"] {
        for gap in ["", " ", "\t "] {
            for terminator in ["", "\n", "\r\n"] {
                for token in ["", "value", "\"a # b\"", "'mañana'", "${NAME}"] {
                    let source = format!("{indent}export KEY{gap}={gap}{token}{gap}{terminator}");
                    let document = Document::from_bytes(source.as_bytes().to_vec());
                    assert_eq!(document.bytes(), source.as_bytes());
                    assert_eq!(entries(&document).len(), 1);
                    assert_eq!(
                        document.text(entry(&document, "KEY").value.clone()),
                        Some(token)
                    );
                    assert_partition(&document);
                }
            }
        }
    }
}

#[test]
fn unsupported_multiline_delimiters_do_not_expose_apparent_assignments() {
    for source in [
        b"BEFORE=safe\nBAD-KEY=\"unclosed\nLOOKS_LIKE_ENTRY=still quoted\n".as_slice(),
        b"BEFORE=safe\nSHELL=$(command\nLOOKS_LIKE_ENTRY=inside command\n)\n",
        b"BEFORE=safe\nSHELL=`command\nLOOKS_LIKE_ENTRY=inside command\n`\n",
        b"BEFORE=safe\nSHELL=prefix; command \\\nLOOKS_LIKE_ENTRY=continued command\n",
        b"BEFORE=safe\nSHELL=prefix <<END\nLOOKS_LIKE_ENTRY=inside heredoc\nEND\n",
    ] {
        let document = Document::from_bytes(source.to_vec());
        assert_eq!(entries(&document).len(), 1);
        assert_eq!(document.blocks().len(), 2);
        assert!(matches!(
            document.blocks()[1].kind,
            BlockKind::Opaque(Diagnostic::UnsupportedSyntax)
        ));
        assert_eq!(document.blocks()[1].range.end, source.len());
        assert_eq!(document.bytes(), source);
        assert_partition(&document);
    }
}
