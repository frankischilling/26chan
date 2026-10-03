use board_domain::{CommentSpacing, PostKind, PostLimits, prepare_post_content_input_with_limits};
use serde_json::Value;

#[test]
fn larger_staff_comments_retain_the_tail_through_admission_markup_and_saved_formatting() {
    let limits = PostLimits::authorized(50_000).unwrap();
    let spacing = CommentSpacing::for_board("j", true, false);
    let raw = format!("A{}Z", "\t".repeat(49_998));
    let prepared = prepare_post_content_input_with_limits(
        "Anonymous",
        "Owned",
        &raw,
        limits,
        false,
        spacing,
        PostKind::Reply,
    )
    .unwrap()
    .finish()
    .unwrap();
    assert_eq!(prepared.comment, format!("A{}Z", " ".repeat(199_992)));
    let rendered = board_domain::formatting::parse_post_comment_on_board_with_limits(
        &prepared.comment,
        1,
        "j",
        limits,
    );
    assert_eq!(
        board_domain::formatting::plain_text(&rendered),
        prepared.comment
    );
    let ordinary = board_domain::formatting::parse_post_comment_on_board(&prepared.comment, 1, "j");
    assert_eq!(
        board_domain::formatting::plain_text(&ordinary),
        format!("A{}", " ".repeat(15_999))
    );
    let text = "\u{20000}".repeat(50_000);
    let mut filtered = board_domain::wordfiltered_comment::prepare_with_limits(
        &text,
        board_domain::comment_markup::MarkupPolicy::default(),
        board_domain::wordfilter::Profile::Global,
        None,
        limits,
    )
    .unwrap();
    filtered.freeze_format("j");
    let decoded =
        board_domain::wordfiltered_comment::PreparedComment::decode(&filtered.encode().unwrap())
            .unwrap();
    assert_eq!(
        board_domain::formatting::plain_text(&board_domain::filtered_formatting::lines(
            &decoded, "j"
        )),
        text
    );
}

#[test]
fn pinned_source_rank_budgets_precede_cleanup_and_preserve_line_exemptions() {
    let reference: Value =
        serde_json::from_str(include_str!("fixtures/authorized-post.json")).unwrap();
    assert_eq!(reference["boards"].as_array().unwrap().len(), 82);
    let mut tested = 0;
    for group in reference["groups"].as_array().unwrap() {
        let limits = if group["role"] == "janitor" {
            PostLimits::ordinary(group["public_chars"].as_u64().unwrap() as usize)
        } else {
            PostLimits::authorized(group["authorized_chars"].as_u64().unwrap() as usize).unwrap()
        };
        let spacing = CommentSpacing::for_board(
            group["board"].as_str().unwrap(),
            group["code"].as_bool().unwrap(),
            group["sjis"].as_bool().unwrap(),
        )
        .with_line_rules(
            group["max_lines"].as_u64().unwrap() as usize,
            group["spoilers"].as_bool().unwrap(),
        );
        for case in group["cases"].as_array().unwrap() {
            let value = if let Some(lines) = case["lines"].as_u64() {
                (0..=lines)
                    .map(|line| format!("line{line}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                case["char"]
                    .as_str()
                    .unwrap()
                    .repeat(case["repeat"].as_u64().unwrap() as usize)
            };
            let (mut name, mut subject, mut comment) = ("Owned", "Owned", "Owned");
            match case["field"].as_str().unwrap() {
                "name" => name = &value,
                "sub" => subject = &value,
                "com" => comment = &value,
                "email" => {}
                _ => panic!("Unknown source field"),
            }
            let result = if case["field"] == "email" {
                limits.validate_field(&value)
            } else {
                prepare_post_content_input_with_limits(
                    name,
                    subject,
                    comment,
                    limits,
                    true,
                    spacing,
                    PostKind::Reply,
                )
                .and_then(|prepared| prepared.finish())
                .map(|_| ())
            };
            assert_eq!(
                result.is_ok(),
                case["accepted"].as_bool().unwrap(),
                "board={} role={} recipe={case} error={result:?}",
                group["board"],
                group["role"]
            );
            tested += 1;
        }
    }
    assert_eq!(tested, 312);
}

#[test]
fn authorized_expansion_keeps_raw_limits_and_ordinary_entry_points() {
    let limits = PostLimits::authorized(50_000).unwrap();
    let spacing = CommentSpacing::for_board("j", true, false);
    let subject = format!("A{}B", "\t".repeat(253));
    let prepared = prepare_post_content_input_with_limits(
        "Owned",
        &subject,
        &"\t".repeat(50_000),
        limits,
        true,
        spacing,
        PostKind::Reply,
    )
    .unwrap()
    .finish()
    .unwrap();
    assert_eq!(prepared.subject, format!("A{}B", " ".repeat(1012)));
    assert!(prepared.comment.is_empty());
    let comment = format!("A{}B", "\t".repeat(49_998));
    let prepared = prepare_post_content_input_with_limits(
        "Owned",
        "Owned",
        &comment,
        limits,
        false,
        spacing,
        PostKind::Reply,
    )
    .unwrap()
    .finish()
    .unwrap();
    assert_eq!(prepared.comment, format!("A{}B", " ".repeat(199_992)));
    assert!(
        board_domain::prepare_post_content_input(
            "Owned",
            &subject,
            "Owned",
            16_000,
            false,
            spacing,
            PostKind::Reply
        )
        .is_err()
    );
    assert!(
        prepare_post_content_input_with_limits(
            "Owned",
            "Owned",
            &format!("{comment}x"),
            limits,
            false,
            spacing,
            PostKind::Reply
        )
        .is_err()
    );
    for maximum in [0, 50_001, usize::MAX] {
        assert!(PostLimits::authorized(maximum).is_err());
    }
}

#[test]
fn larger_authorized_wordfilters_use_a_bounded_distinct_saved_format() {
    use board_domain::{
        comment_markup::MarkupPolicy,
        wordfilter::{LeetRolls, Profile},
        wordfiltered_comment::{PreparedComment, prepare, prepare_with_limits},
    };
    let limits = PostLimits::authorized(50_000).unwrap();
    let policy = MarkupPolicy {
        spoilers: true,
        code: true,
        sjis: false,
        op: false,
    };
    for (raw, profile, rolls) in [
        ("𠀀".repeat(50_000), Profile::Global, None),
        (format!("A{}B", "\t".repeat(49_998)), Profile::Global, None),
        (format!("{}Z", "x\n".repeat(24_999)), Profile::Global, None),
        (
            "\"".repeat(50_000),
            Profile::Test,
            Some(LeetRolls::from_choices(0, 1).unwrap()),
        ),
    ] {
        let content = prepare_post_content_input_with_limits(
            "Owned",
            "Owned",
            &raw,
            limits,
            false,
            CommentSpacing::for_board("j", true, false),
            PostKind::Reply,
        )
        .unwrap()
        .finish()
        .unwrap();
        assert!(prepare(&content.comment, policy, profile, rolls).is_err());
        let mut prepared =
            prepare_with_limits(&content.comment, policy, profile, rolls, limits).unwrap();
        prepared.freeze_format("j");
        let encoded = prepared.encode().unwrap();
        assert!(encoded.starts_with(b"WF02"));
        assert_eq!(PreparedComment::decode(&encoded).unwrap(), prepared);
        if encoded.len() > board_domain::wordfiltered_comment::MAX_STORED_BYTES {
            let mut ordinary = encoded.clone();
            ordinary[..4].copy_from_slice(b"WF01");
            assert!(PreparedComment::decode(&ordinary).is_err());
        }
        let projection = prepared.source_projection();
        assert!(projection.len() <= board_domain::WordfilterLimits::Authorized.output_bytes());
    }
    let mut ordinary = prepare("Owned", policy, Profile::Global, None).unwrap();
    ordinary.freeze_format("g");
    let encoded = ordinary.encode().unwrap();
    assert!(encoded.starts_with(b"WF01"));
    assert_eq!(PreparedComment::decode(&encoded).unwrap(), ordinary);
    assert!(
        PreparedComment::decode(&vec![
            0;
            board_domain::WordfilterLimits::Authorized
                .stored_bytes()
                + 1
        ])
        .is_err()
    );
}
