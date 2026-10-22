use super::*;
use crate::comment_markup::MarkupPolicy;
use crate::wordfilter::{LeetRolls, Profile};

fn ordinary() -> PostLimits {
    PostLimits::ordinary(crate::MAX_COMMENT_CHARS)
}
fn authorized() -> PostLimits {
    PostLimits::authorized(crate::MAX_AUTHORIZED_COMMENT_CHARS).unwrap()
}
fn freeze(text: &str) -> FrozenPresentation {
    FrozenPresentation::from_saved(SavedCommentRef {
        comment: text,
        format: 127,
        source_board: "g",
        wordfilter_payload: None,
        saved_presentation: None,
        limits: ordinary(),
    })
    .unwrap()
}
fn rewrite(value: &mut FrozenPresentation, replacements: &[(i64, i64)]) {
    let replacements: Vec<_> = replacements
        .iter()
        .map(|&(old, new)| PriorReplacement::new(old, new).unwrap())
        .collect();
    value
        .rewrite_prior(&replacements, &mut RewriteBudget::new(16 * 1024 * 1024))
        .unwrap();
}
fn filtered(text: &str, profile: Profile, rolls: Option<LeetRolls>) -> (String, Vec<u8>) {
    let mut value = crate::wordfiltered_comment::prepare(
        text,
        MarkupPolicy {
            spoilers: true,
            code: true,
            sjis: true,
            op: true,
        },
        profile,
        rolls,
    )
    .unwrap();
    value.freeze_format("g");
    (
        crate::filtered_formatting::source_projection(&crate::filtered_formatting::lines(
            &value, "g",
        )),
        value.encode().unwrap(),
    )
}
fn freeze_filtered(comment: &str, payload: &[u8]) -> Result<FrozenPresentation> {
    FrozenPresentation::from_saved(SavedCommentRef {
        comment,
        format: 127,
        source_board: "g",
        wordfilter_payload: Some(payload),
        saved_presentation: None,
        limits: ordinary(),
    })
}

#[test]
fn literal_replacement_is_ordered_prefix_sensitive_and_nonrecursive() {
    for (text, mappings, expected) in [
        (">>1 >>2 >>12", vec![(1, 2), (2, 9)], ">>9 >>9 >>92"),
        (">>123 >>012 >>12", vec![(12, 7)], ">>73 >>012 >>7"),
        (">>1", vec![(1, 11)], ">>11"),
        (">>1 >>2", vec![], ">>1 >>2"),
        (">>>/g/12 >>12", vec![(12, 7)], ">>>/g/12 >>7"),
        (">>2 >>3", vec![(1, 2)], ">>2 >>3"),
        (">>1 >>2", vec![(2, 9), (1, 2)], ">>2 >>9"),
    ] {
        let mut value = freeze(text);
        rewrite(&mut value, &mappings);
        assert_eq!(value.search_text(), expected, "{text}");
        assert_eq!(
            value.source_projection(),
            crate::source_html_entities(expected)
        );
    }
}

#[test]
fn byte_literal_boundary_is_not_numeric_quote_identity() {
    let mut value = freeze("&gt;&gt;12 >>012 >>12abc >>>12");
    rewrite(&mut value, &[(12, 7)]);
    assert_eq!(value.search_text(), "&gt;&gt;12 >>012 >>7abc >>>7");
    assert!(value.source_projection().contains("&amp;gt;&amp;gt;12"));
    let labels: Vec<_> = value
        .unresolved_lines()
        .into_iter()
        .flat_map(|line| line.tokens)
        .filter_map(|token| {
            if let Token::PostQuote(q) = token {
                Some(q.label().to_owned())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(labels, [">>012", ">>7", ">>7"]);
}

#[test]
fn preserves_wrapping_instead_of_reflowing_changed_digit_lengths() {
    let mut value = freeze(&format!("{}>>12", "x".repeat(31)));
    let before = value.source_projection();
    assert!(before.ends_with("&gt;&gt;12<wbr>"));
    rewrite(&mut value, &[(12, 1)]);
    assert_eq!(
        value.source_projection(),
        before.replace("&gt;&gt;12", "&gt;&gt;1")
    );
    let mut growing = freeze(&format!("{}>>1", "x".repeat(31)));
    assert!(!growing.source_projection().contains("<wbr>"));
    rewrite(&mut growing, &[(1, 1000000)]);
    assert!(!growing.source_projection().contains("<wbr>"));
}

#[test]
fn fixed_break_and_markup_boundaries_prevent_cross_atom_matching() {
    let mut value = FrozenPresentation::freeze(
        vec![Line {
            green: false,
            tokens: vec![
                Token::Text(">>1".into()),
                Token::WordBreak,
                Token::Text("2 ".into()),
                Token::OpenMarkup(Tag::Spoiler),
                Token::Text(">>12".into()),
                Token::CloseMarkup(Tag::Spoiler),
                Token::Text(" >".into()),
                Token::OpenMarkup(Tag::Bold),
                Token::Text(">12".into()),
                Token::CloseMarkup(Tag::Bold),
            ],
        }],
        WordfilterLimits::Ordinary,
        false,
    )
    .unwrap();
    let before = value.source_projection();
    rewrite(&mut value, &[(12, 7)]);
    assert_eq!(
        value.source_projection(),
        before.replace("&gt;&gt;12", "&gt;&gt;7")
    );
    assert!(value.source_projection().starts_with("&gt;&gt;1<wbr>2"));
    rewrite(&mut value, &[(1, 8)]);
    assert!(value.source_projection().starts_with("&gt;&gt;8<wbr>2"));
}

#[test]
fn code_spoilers_crossing_markup_and_greentext_are_frozen() {
    for raw in [
        "[code]>>12[/code]",
        "[spoiler]>>12[/spoiler]",
        "[b][i]>>12[/b][/i]",
        "> note >>12\n>>12",
    ] {
        let mut value = freeze(raw);
        let before = value.source_projection();
        rewrite(&mut value, &[(12, 7000)]);
        assert_eq!(
            value.source_projection(),
            before.replace("&gt;&gt;12", "&gt;&gt;7000")
        );
        assert!(
            value
                .unresolved_lines()
                .iter()
                .flat_map(|line| &line.tokens)
                .any(|token| matches!(token, Token::PostQuote(q) if q.id()==7000))
        );
        let encoded = value.encode().unwrap();
        assert_eq!(
            FrozenPresentation::decode(&encoded, ordinary()).unwrap(),
            value
        );
    }
}

#[test]
fn source_board_normalization_does_not_repeat_on_next_move() {
    let mut value =
        freeze("https://boards.4chan.org/g/thread/12 https://www.4chan.org/faq >>>/g/catalog");
    let before = value.source_projection();
    assert!(before.contains("&gt;&gt;12"));
    rewrite(&mut value, &[(12, 90)]);
    let expected = before.replace("&gt;&gt;12", "&gt;&gt;90");
    assert_eq!(value.source_projection(), expected);
    let encoded = value.encode().unwrap();
    let moved = FrozenPresentation::from_saved(SavedCommentRef {
        comment: &expected,
        format: 127,
        source_board: "a",
        wordfilter_payload: None,
        saved_presentation: Some(&encoded),
        limits: ordinary(),
    })
    .unwrap();
    assert_eq!(moved.source_projection(), expected);
    assert!(
        moved
            .unresolved_lines()
            .iter()
            .flat_map(|line| &line.tokens)
            .any(|token| matches!(token, Token::PostQuote(q) if q.board().is_none() && q.id()==90))
    );
}

#[test]
fn filtered_saved_output_is_frozen_without_running_a_filter_again() {
    for profile in [
        Profile::Global,
        Profile::Basic,
        Profile::Asp,
        Profile::Video,
        Profile::Test,
    ] {
        for rolls in [
            LeetRolls::from_choices(0, 1).unwrap(),
            LeetRolls::from_choices(4, 5).unwrap(),
        ] {
            let (comment, payload) = filtered(
                "[code]>>12 & \"owned\"[/code]\n[spoiler]>>12[/spoiler]",
                profile,
                matches!(profile, Profile::Test).then_some(rolls),
            );
            let mut frozen = freeze_filtered(&comment, &payload).unwrap();
            assert!(frozen.was_wordfiltered());
            assert_eq!(frozen.source_projection(), comment);
            rewrite(&mut frozen, &[(12, 9000)]);
            assert_eq!(
                frozen.source_projection(),
                comment.replace("&gt;&gt;12", "&gt;&gt;9000")
            );
            assert_eq!(
                FrozenPresentation::decode(&frozen.encode().unwrap(), ordinary()).unwrap(),
                frozen
            );
        }
    }
}

#[test]
fn frozen_and_wordfilter_authority_are_never_interchangeable() {
    let value = freeze(">>12");
    let bytes = value.encode().unwrap();
    assert_eq!(&bytes[..5], b"FP01\0");
    assert!(FrozenPresentation::decode(&bytes, authorized()).is_err());
    let elevated = FrozenPresentation::from_saved(SavedCommentRef {
        comment: ">>12",
        format: 127,
        source_board: "g",
        wordfilter_payload: None,
        saved_presentation: None,
        limits: authorized(),
    })
    .unwrap()
    .encode()
    .unwrap();
    assert_eq!(&elevated[..4], b"FP02");
    assert!(FrozenPresentation::decode(&elevated, ordinary()).is_err());
    let (comment, payload) = filtered(">>12", Profile::Global, None);
    assert!(FrozenPresentation::decode(&payload, ordinary()).is_err());
    assert!(PreparedComment::decode(&bytes).is_err());
    assert!(
        FrozenPresentation::from_saved(SavedCommentRef {
            comment: &comment,
            format: 127,
            source_board: "g",
            wordfilter_payload: Some(&payload),
            saved_presentation: Some(&bytes),
            limits: ordinary(),
        })
        .is_err()
    );
    assert!(
        FrozenPresentation::from_saved(SavedCommentRef {
            comment: "different",
            format: 127,
            source_board: "g",
            wordfilter_payload: None,
            saved_presentation: Some(&bytes),
            limits: ordinary(),
        })
        .is_err()
    );
    assert!(freeze_filtered("different", &payload).is_err());
    let mut bad_wf = payload;
    bad_wf[..4].copy_from_slice(b"WF02");
    assert!(freeze_filtered(&comment, &bad_wf).is_err());
}

#[test]
fn rejects_historical_unknown_profiles_and_oversize_instead_of_clipping() {
    for format in [0, 8, 15, 24, 31, 40, 47, 56, 63, 103, 112, 119, 128, -1] {
        assert!(matches!(
            FrozenPresentation::from_saved(SavedCommentRef {
                comment: ">>1",
                format,
                source_board: "g",
                wordfilter_payload: None,
                saved_presentation: None,
                limits: ordinary(),
            }),
            Err(PresentationError::UnsupportedFormat)
        ));
    }
    for format in (104..=111)
        .chain(120..=127)
        .chain(232..=239)
        .chain(248..=255)
    {
        assert!(
            FrozenPresentation::from_saved(SavedCommentRef {
                comment: ">>1",
                format,
                source_board: "g",
                wordfilter_payload: None,
                saved_presentation: None,
                limits: ordinary(),
            })
            .is_ok()
        );
    }
    assert!(
        FrozenPresentation::from_saved(SavedCommentRef {
            comment: &"x".repeat(16_001),
            format: 127,
            source_board: "g",
            wordfilter_payload: None,
            saved_presentation: None,
            limits: ordinary(),
        })
        .is_err()
    );
    assert!(PriorReplacement::new(0, 1).is_err());
    assert!(PriorReplacement::new(1, -1).is_err());
}

#[test]
fn work_budget_charges_input_and_output_and_errors_are_atomic() {
    let original = freeze(">>1");
    let replacement = PriorReplacement::new(1, 123).unwrap();
    let mut exact = original.clone();
    // Two setup passes: 8; structural loops: 2; first scan: 3;
    // second scan + output: 8; final validation reservation: 6.
    let mut budget = RewriteBudget::new(27);
    exact
        .rewrite_prior(std::slice::from_ref(&replacement), &mut budget)
        .unwrap();
    assert_eq!(budget.remaining(), 0);
    let mut failed = original.clone();
    let mut exhausted = RewriteBudget::new(26);
    assert_eq!(
        failed.rewrite_prior(&[replacement], &mut exhausted),
        Err(PresentationError::WorkLimitExceeded)
    );
    assert_eq!(exhausted.remaining(), 0);
    assert_eq!(failed, original);
    let mut budget = RewriteBudget::new(20);
    assert!(
        failed
            .rewrite_prior(
                &[
                    PriorReplacement::new(1, 2).unwrap(),
                    PriorReplacement::new(2, 999).unwrap()
                ],
                &mut budget
            )
            .is_err()
    );
    assert_eq!(failed, original);
    assert_eq!(budget.remaining(), 0);
    // An unsuccessful match still reads the entire run and preserves output.
    assert!(
        failed
            .rewrite_prior(
                &[PriorReplacement::new(9, 8).unwrap()],
                &mut RewriteBudget::new(5)
            )
            .is_err()
    );
}

#[test]
fn intermediate_expansion_is_rejected_before_mutating_the_original() {
    let mut value = FrozenPresentation {
        atoms: vec![Atom::Text(">>1 ".repeat(30_000))],
        bounds: WordfilterLimits::Ordinary,
        wordfiltered: false,
    };
    value.validate().unwrap();
    let original = value.clone();
    let mut budget = RewriteBudget::new(1_000_000);
    assert_eq!(
        value.rewrite_prior(&[PriorReplacement::new(1, i64::MAX).unwrap()], &mut budget),
        Err(PresentationError::LimitExceeded)
    );
    assert!(budget.remaining() < 1_000_000);
    // Retrying the same rejected expansion cannot scan/clone indefinitely.
    for _ in 0..3 {
        let before = budget.remaining();
        let error = value
            .rewrite_prior(&[PriorReplacement::new(1, i64::MAX).unwrap()], &mut budget)
            .unwrap_err();
        if before > 0 {
            assert!(budget.remaining() < before);
        }
        assert!(matches!(
            error,
            PresentationError::LimitExceeded | PresentationError::WorkLimitExceeded
        ));
    }
    assert_eq!(budget.remaining(), 0);
    assert_eq!(value, original);
}

#[test]
fn structural_only_work_and_failed_reservations_are_bounded() {
    let mut value = FrozenPresentation::freeze(
        vec![Line {
            green: false,
            tokens: vec![Token::OpenMarkup(Tag::Code), Token::CloseMarkup(Tag::Code)],
        }],
        WordfilterLimits::Ordinary,
        false,
    )
    .unwrap();
    let before = value.clone();
    let mappings = vec![PriorReplacement::new(1, 2).unwrap(); 3];
    let mut budget = RewriteBudget::new(17);
    assert_eq!(
        value.rewrite_prior(&mappings, &mut budget),
        Err(PresentationError::WorkLimitExceeded)
    );
    assert_eq!(budget.remaining(), 0);
    assert_eq!(value, before);
    value
        .rewrite_prior(&mappings, &mut RewriteBudget::new(18))
        .unwrap();
    let mut link = freeze("https://www.4chan.org/faq");
    let mut small = RewriteBudget::new(1);
    assert!(link.rewrite_prior(&mappings, &mut small).is_err());
    assert_eq!(small.remaining(), 0);
    let mut overflowing = RewriteBudget::new(usize::MAX);
    assert!(overflowing.charge(usize::MAX, 1).is_err());
    assert_eq!(overflowing.remaining(), 0);
    assert!(overflowing.charge(1, 0).is_err());
    assert_eq!(overflowing.remaining(), 0);
}

#[test]
fn finalized_marker_cleanup_links_must_be_decodable_before_freeze_succeeds() {
    let (comment, payload) = filtered("https://4chan.org/~?rep?~", Profile::Global, None);
    assert!(comment.contains("href=\"https://4chan.org/\""), "{comment}");
    // Cleanup removed the last parseable path component. Do not broaden the
    // finite link grammar or silently normalize to a different saved href.
    assert_eq!(
        freeze_filtered(&comment, &payload),
        Err(PresentationError::InvalidEncoding)
    );
}

#[test]
fn all_truncations_trailing_bytes_and_invalid_discriminants_fail_closed() {
    let encoded = freeze("[code]>>12[/code] https://www.4chan.org/faq >>>/g/catalog")
        .encode()
        .unwrap();
    for end in 0..encoded.len() {
        assert!(
            FrozenPresentation::decode(&encoded[..end], ordinary()).is_err(),
            "prefix {end}"
        );
    }
    let mut trailing = encoded.clone();
    trailing.push(0);
    assert!(FrozenPresentation::decode(&trailing, ordinary()).is_err());
    for (offset, byte) in [(0, b'X'), (4, 2), (5, 255), (9, 255)] {
        let mut bad = encoded.clone();
        bad[offset] = byte;
        assert!(
            FrozenPresentation::decode(&bad, ordinary()).is_err(),
            "offset {offset}"
        );
    }
    let mut unicode = freeze("x").encode().unwrap();
    *unicode.last_mut().unwrap() = 255;
    assert!(FrozenPresentation::decode(&unicode, ordinary()).is_err());
    assert!(FrozenPresentation::decode(&vec![0; 131_073], ordinary()).is_err());
}

fn wire_atom(opcode: u8, text: &str) -> Vec<u8> {
    let mut bytes = b"FP01\0\0\0\0\x01".to_vec();
    bytes.push(opcode);
    bytes.extend_from_slice(&(text.len() as u32).to_be_bytes());
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

fn wire_link(opcode: u8, text: &str) -> Vec<u8> {
    let mut bytes = wire_atom(opcode, text);
    bytes.extend_from_slice(&1u32.to_be_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&(text.len() as u32).to_be_bytes());
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

#[test]
fn arbitrary_html_is_text_and_forged_link_authority_is_rejected() {
    let text = "<script>owned()</script><img src=x onerror=owned()>";
    let value = FrozenPresentation::decode(&wire_atom(0, text), ordinary()).unwrap();
    assert_eq!(value.source_projection(), crate::source_html_entities(text));
    assert!(
        value
            .unresolved_lines()
            .iter()
            .flat_map(|line| &line.tokens)
            .all(|token| matches!(token, Token::Text(_)))
    );
    for href in [
        "javascript:owned()",
        "https://evil.test/",
        "https://www.4chan.org.evil.test/",
        "https://user:secret@www.4chan.org/",
    ] {
        let bytes = wire_link(8, href);
        assert!(
            FrozenPresentation::decode(&bytes, ordinary()).is_err(),
            "{href}"
        );
    }
    let static_link = wire_link(7, ">>>/g/catalog\" onclick=owned()");
    assert!(FrozenPresentation::decode(&static_link, ordinary()).is_err());
    for (opcode, spelling) in [(8, "https://www.4chan.org/faq"), (7, ">>>/g/catalog")] {
        let valid = wire_link(opcode, spelling);
        assert_eq!(
            FrozenPresentation::decode(&valid, ordinary())
                .unwrap()
                .encode()
                .unwrap(),
            valid
        );
    }
}

#[test]
fn malformed_finite_components_and_noncanonical_text_runs_are_rejected() {
    let header = b"FP01\0\0\0\0\x01";
    for atom in [
        vec![3, 99, 1],
        vec![3, 0, 0],
        vec![4, 2],
        vec![4, 1],
        vec![6, 0],
        vec![5, 0, 1, 9, 1],
    ] {
        let mut bytes = header.to_vec();
        bytes.extend(atom);
        assert!(FrozenPresentation::decode(&bytes, ordinary()).is_err());
    }
    let mut adjacent = b"FP01\0\0\0\0\x02".to_vec();
    adjacent.extend_from_slice(&wire_atom(0, ">>")[9..]);
    adjacent.extend_from_slice(&wire_atom(0, "12")[9..]);
    assert!(FrozenPresentation::decode(&adjacent, ordinary()).is_err());
    assert!(FrozenPresentation::decode(&wire_atom(0, ""), ordinary()).is_err());
}

#[test]
fn freezing_coalesces_post_quote_and_neighbor_text_before_prefix_replacement() {
    let mut value = freeze(">>1abc >>12");
    assert!(matches!(&value.atoms[..], [Atom::Text(text)] if text==">>1abc >>12"));
    rewrite(&mut value, &[(1, 7)]);
    assert_eq!(value.search_text(), ">>7abc >>72");
}

#[test]
fn self_and_forward_references_use_only_previous_insertions() {
    let mut op = freeze(">>1 >>2 >>3");
    rewrite(&mut op, &[]);
    assert_eq!(op.search_text(), ">>1 >>2 >>3");
    let mut first = freeze(">>1 >>2 >>3");
    rewrite(&mut first, &[(1, 2)]);
    assert_eq!(first.search_text(), ">>2 >>2 >>3");
    let mut second = freeze(">>1 >>2 >>3");
    rewrite(&mut second, &[(1, 2), (2, 9)]);
    assert_eq!(second.search_text(), ">>9 >>9 >>3");
}

#[test]
fn empty_comments_still_have_a_finite_mapping_count() {
    let mut value = freeze("");
    assert_eq!(value.encode().unwrap(), b"FP01\0\0\0\0\0");
    let mappings = vec![PriorReplacement::new(1, 2).unwrap(); MAX_PRIOR_REPLACEMENTS + 1];
    assert_eq!(
        value.rewrite_prior(&mappings, &mut RewriteBudget::new(usize::MAX)),
        Err(PresentationError::LimitExceeded)
    );
}

mod properties {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]
        #[test]
        fn accepted_wire_data_is_canonical(bytes in prop::collection::vec(any::<u8>(), 0..1024)) {
            if let Ok(value) = FrozenPresentation::decode(&bytes, ordinary()) {
                prop_assert_eq!(value.encode().unwrap(), bytes);
            }
        }

        #[test]
        fn targeted_mutations_of_valid_payloads_are_rejected(text in "[a-zA-Z0-9 >]{0,64}") {
            let value = freeze(&format!("[code]{text} >>1[/code]"));
            let encoded = value.encode().unwrap();
            prop_assert_eq!(FrozenPresentation::decode(&encoded, ordinary()).unwrap().encode().unwrap(), encoded.clone());
            for (offset, replacement) in [(0, b'W'), (4, 255), (5, 255), (9, 255)] {
                let mut mutated = encoded.clone();
                mutated[offset] = replacement;
                prop_assert!(FrozenPresentation::decode(&mutated, ordinary()).is_err());
            }
            let mut trailing = encoded.clone(); trailing.push(0);
            prop_assert!(FrozenPresentation::decode(&trailing, ordinary()).is_err());
            prop_assert!(FrozenPresentation::decode(&encoded[..encoded.len()-1], ordinary()).is_err());
        }

        #[test]
        fn typed_replacement_matches_literal_saved_source_replacement(left in ".{0,128}", right in ".{0,128}") {
            let mut frozen = freeze(&format!("[code]{left} >>1[/code]\n> {right} >>12"));
            let expected = frozen.source_projection().replace("&gt;&gt;1", "&gt;&gt;2").replace("&gt;&gt;2", "&gt;&gt;9");
            rewrite(&mut frozen, &[(1, 2), (2, 9)]);
            prop_assert_eq!(frozen.source_projection(), expected);
            let encoded = frozen.encode().unwrap();
            prop_assert_eq!(FrozenPresentation::decode(&encoded, ordinary()).unwrap(), frozen);
        }
    }
}

#[test]
fn qualified_atoms_match_pinned_independent_source_replacement_vectors() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../fixtures/thread-transfer-reference.json"
    ))
    .unwrap();
    let mut checked = 0;
    let mut excluded = Vec::new();
    for case in fixture["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        // These four reference vectors qualify PHP string replacement, but
        // their HTML cannot be produced by this codec's finite body grammar.
        // No arbitrary HTML/entity decoder is introduced to make them pass.
        if matches!(
            id,
            "markup-stops-long-match"
                | "entity-stops-long-match"
                | "entity-keeps-short-prefix"
                | "existing-server-link-stays-frozen"
        ) {
            excluded.push(id);
            continue;
        }
        let mut value = match id {
            "op-has-no-prior-mappings" => freeze(">>1 >>2"),
            "sequential-cascade-and-prefix" | "different-order" => freeze(">>1 >>2 >>12"),
            "decimal-prefix-not-exact-token" => freeze(">>123 >>012 >>12"),
            "one-pass-not-recursive" => freeze(">>1 >>11"),
            "identity-pass" | "maximum-destination-id" => freeze(">>1"),
            "self-reference-unavailable" => freeze(">>2 >>1"),
            "forward-reference-prefix-effect" => freeze(">>123"),
            "word-break-stops-long-match" | "word-break-keeps-short-prefix" => {
                FrozenPresentation::freeze(
                    vec![Line {
                        green: false,
                        tokens: vec![
                            Token::Text(">>1".into()),
                            Token::WordBreak,
                            Token::Text("2".into()),
                        ],
                    }],
                    WordfilterLimits::Ordinary,
                    false,
                )
                .unwrap()
            }
            "line-break-stops-long-match" => freeze(">>1\n2"),
            "code-and-spoiler-remain-eligible" => freeze("[code]>>1[/code][spoiler]>>1[/spoiler]"),
            "fixed-greentext-span" => FrozenPresentation::freeze(
                vec![Line {
                    green: false,
                    tokens: vec![
                        Token::OpenQuote,
                        Token::Text(">>1 > unchanged".into()),
                        Token::CloseQuote,
                    ],
                }],
                WordfilterLimits::Ordinary,
                false,
            )
            .unwrap(),
            "cross-board-is-not-same-board-prefix" => freeze(">>>/g/12 >>12"),
            "double-escaped-user-entity-is-literal" => freeze("&gt;&gt;12 >>12"),
            "unicode-and-punctuation" => freeze("猫 (>>12), >>12."),
            "expanded-lexical-value-is-not-clipped" => freeze(">>19"),
            _ => panic!("new source vector needs explicit qualification: {id}"),
        };
        assert_eq!(
            value.source_projection(),
            case["input_html"].as_str().unwrap(),
            "input {id}"
        );
        let mappings: Vec<_> = case["replacements"]
            .as_array()
            .unwrap()
            .iter()
            .map(|mapping| {
                PriorReplacement::new(
                    mapping["old"].as_str().unwrap().parse().unwrap(),
                    mapping["new"].as_str().unwrap().parse().unwrap(),
                )
                .unwrap()
            })
            .collect();
        value
            .rewrite_prior(&mappings, &mut RewriteBudget::new(1024 * 1024))
            .unwrap();
        assert_eq!(
            value.source_projection(),
            case["expected_html"].as_str().unwrap(),
            "output {id}"
        );
        checked += 1;
    }
    assert_eq!(checked, 18);
    assert_eq!(
        excluded,
        [
            "markup-stops-long-match",
            "entity-stops-long-match",
            "entity-keeps-short-prefix",
            "existing-server-link-stays-frozen"
        ]
    );
}

#[test]
fn greentext_profile_is_frozen_for_plain_and_wordfiltered_transfers() {
    let raw = ">>not-a-post";
    for format in [104, 232] {
        for wordfiltered in [false, true] {
            let mut prepared = crate::wordfiltered_comment::prepare(
                raw,
                MarkupPolicy::default(),
                Profile::Global,
                None,
            )
            .unwrap();
            prepared.freeze_format("g");
            let payload = prepared.encode().unwrap();
            let projection = crate::filtered_formatting::source_projection(
                &crate::filtered_formatting::lines_for_post(&prepared, "g", format),
            );
            let frozen = FrozenPresentation::from_saved(SavedCommentRef {
                comment: if wordfiltered { &projection } else { raw },
                format,
                source_board: "g",
                wordfilter_payload: wordfiltered.then_some(payload.as_slice()),
                saved_presentation: None,
                limits: ordinary(),
            })
            .unwrap();
            let expected = if format == 232 {
                "<span class=\"quote\">&gt;&gt;not-a-post</span>"
            } else {
                "&gt;&gt;not-a-post"
            };
            assert_eq!(frozen.source_projection(), expected);
            let bytes = frozen.encode().unwrap();
            let moved = FrozenPresentation::from_saved(SavedCommentRef {
                comment: expected,
                format,
                source_board: "co",
                wordfilter_payload: None,
                saved_presentation: Some(&bytes),
                limits: ordinary(),
            })
            .unwrap();
            assert_eq!(moved.source_projection(), expected);
            assert_eq!(moved.encode().unwrap(), bytes);
        }
    }
}

#[test]
fn frozen_comparison_matches_admitted_source_bytes_before_and_after_quote_rewrite() {
    use crate::comment_admission_projection::{self as admission, Filter, SourcePath};
    for raw in [
        "ordinary comment",
        ">>123",
        ">quote",
        "[spoiler]hidden[/spoiler]",
        "https://example.com/",
        "https://boards.4chan.org/g/thread/123",
        "some &amp; text",
        "[code]one\ntwo[/code]",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ] {
        let content = crate::prepare_post_content_input_with_limits(
            "",
            "",
            raw,
            ordinary(),
            false,
            crate::CommentSpacing::for_board("g", true, true),
            crate::PostKind::Reply,
        )
        .unwrap();
        let projection = admission::project(
            admission::Input {
                prepared_comment: (&content).into(),
                board: "g",
                markup: MarkupPolicy {
                    spoilers: true,
                    code: true,
                    sjis: true,
                    op: false,
                },
                filter: Filter::Off,
                randomizer: None,
                source_path: SourcePath::OrdinaryWithoutExif,
            },
            ordinary(),
        )
        .unwrap();
        let mut frozen = freeze(content.comment());
        assert_eq!(
            frozen.source_comparison_hash(),
            projection.comparison_hash(),
            "{raw}"
        );
        let expected = projection
            .as_comparison_data()
            .replace("&gt;&gt;123", "&gt;&gt;456");
        rewrite(&mut frozen, &[(123, 456)]);
        assert_eq!(frozen.source_projection(), expected);
        let mut bytes = admission::HASH_PREFIX.to_vec();
        bytes.extend_from_slice(expected.as_bytes());
        assert_eq!(
            frozen.source_comparison_hash().as_slice(),
            ring::digest::digest(&ring::digest::SHA256, &bytes).as_ref()
        );
        let restored = FrozenPresentation::decode(&frozen.encode().unwrap(), ordinary()).unwrap();
        assert_eq!(
            restored.source_comparison_hash(),
            frozen.source_comparison_hash()
        );
    }
}

#[test]
fn frozen_wordfiltered_comparisons_retain_saved_choices() {
    use crate::comment_admission_projection::{self as admission, Filter, SourcePath};
    for profile in [
        Profile::Global,
        Profile::Basic,
        Profile::Asp,
        Profile::Video,
        Profile::Test,
    ] {
        let rolls = (profile == Profile::Test).then(|| LeetRolls::from_choices(1, 4).unwrap());
        for raw in [
            "fam soy CUCK >>123",
            "[spoiler]fam[/spoiler]",
            ">fam",
            "https://example.com/ >>123",
        ] {
            let content = crate::prepare_post_content_input_with_limits(
                "",
                "",
                raw,
                ordinary(),
                false,
                crate::CommentSpacing::for_board("g", true, true),
                crate::PostKind::Thread {
                    staff_image_only: false,
                    subject_required: false,
                    text_only: false,
                },
            )
            .unwrap();
            let projection = admission::project(
                admission::Input {
                    prepared_comment: (&content).into(),
                    board: "g",
                    markup: MarkupPolicy {
                        spoilers: true,
                        code: true,
                        sjis: true,
                        op: true,
                    },
                    filter: Filter::On { profile, rolls },
                    randomizer: None,
                    source_path: SourcePath::OrdinaryWithoutExif,
                },
                ordinary(),
            )
            .unwrap();
            let (comment, payload) = filtered(content.comment(), profile, rolls);
            let frozen = freeze_filtered(&comment, &payload).unwrap();
            assert_eq!(
                frozen.source_comparison_hash(),
                projection.comparison_hash(),
                "{profile:?}: {raw}"
            );
        }
    }
}
