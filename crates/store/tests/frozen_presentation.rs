//! Read-side tests deliberately use excerpts different from the sealed source.
use board_domain::saved_presentation::{FrozenPresentation, SavedCommentRef};
use board_domain::{Line, PostLimits, Token};

fn frozen(text: &str, staff: bool) -> FrozenPresentation {
    FrozenPresentation::from_saved(SavedCommentRef {
        comment: text,
        format: 127,
        source_board: "g",
        wordfilter_payload: None,
        saved_presentation: None,
        limits: if staff {
            PostLimits::authorized(board_domain::MAX_AUTHORIZED_COMMENT_CHARS).unwrap()
        } else {
            PostLimits::ordinary(board_domain::MAX_COMMENT_CHARS)
        },
    })
    .unwrap()
}

fn literal(lines: &[Line]) -> &str {
    assert_eq!(lines.len(), 1);
    assert!(!lines[0].green);
    assert_eq!(lines[0].tokens.len(), 1);
    match &lines[0].tokens[0] {
        Token::Text(text) => text,
        _ => panic!("corrupt saved presentation must stay literal"),
    }
}

#[test]
fn frozen_render_uses_typed_payload_with_excerpt_and_destination_board() {
    let saved = frozen("[b]Historical[/b] >>42 >>>/g/43 <script>", false);
    let bytes = saved.encode().unwrap();
    let lines = board_store::formatted_saved_lines("excerpt", 127, "co", None, Some(&bytes), false);
    assert_eq!(
        board_domain::formatting::plain_text(&lines),
        saved.search_text()
    );
    assert_eq!(
        board_domain::filtered_formatting::source_projection(&lines),
        saved.source_projection()
    );
    let quotes: Vec<_> = lines
        .iter()
        .flat_map(|line| &line.tokens)
        .filter_map(|token| {
            if let Token::PostQuote(quote) = token {
                Some(quote.label())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(quotes, [">>42", ">>>/g/43"]);
}

#[test]
fn corrupt_dual_or_wrong_authority_payload_never_reparses_projection() {
    let projection = "<script>alert(1)</script>[b]raw[/b] >>42 &gt;&gt;43";
    let ordinary = frozen("safe", false).encode().unwrap();
    let staff = frozen("safe", true).encode().unwrap();
    for (bytes, wf, authorized) in [
        (b"invalid".as_slice(), None, false),
        (ordinary.as_slice(), Some(b"WF01".as_slice()), false),
        (staff.as_slice(), None, false),
        (ordinary.as_slice(), None, true),
    ] {
        let lines =
            board_store::formatted_saved_lines(projection, 127, "g", wf, Some(bytes), authorized);
        assert_eq!(literal(&lines), projection);
    }
    let lines = board_store::formatted_saved_lines("excerpt", 127, "g", None, Some(&staff), true);
    assert_eq!(board_domain::formatting::plain_text(&lines), "safe");
}

#[test]
fn corrupt_fallback_is_bounded_before_allocation_at_unicode_boundaries() {
    for (authorized, maximum) in [
        (false, board_domain::MAX_COMMENT_CHARS),
        (true, board_domain::MAX_AUTHORIZED_COMMENT_CHARS),
    ] {
        let comment = "🦀".repeat(maximum + 100);
        let lines =
            board_store::formatted_saved_lines(&comment, 127, "g", None, Some(b"bad"), authorized);
        assert_eq!(literal(&lines).chars().count(), maximum);
        assert_eq!(literal(&lines).len(), maximum * 4);
    }
}
