use board_domain::{Token, parse_post_comment};

#[test]
fn static_quotes_are_versioned_and_numeric_quotes_retain_post_resolution() {
    let input = ">>>/po/ >>>/g/catalog >>>/g/a+b/c,d-e >>>/po/42";
    for format in [104, 111, 120, 127] {
        let lines = parse_post_comment(input, format);
        let tokens = &lines[0].tokens;
        let links: Vec<_> = tokens
            .iter()
            .filter_map(|token| match token {
                Token::StaticQuote(quote, _) => Some((quote.label(), quote.href())),
                _ => None,
            })
            .collect();
        assert_eq!(
            links,
            vec![
                (">>>/po/".into(), "/po/".into()),
                (">>>/g/catalog".into(), "/g/catalog".into()),
                (
                    ">>>/g/a+b/c,d-e".into(),
                    "/g/catalog#s=a+b%2Fc%2Cd-e".into()
                ),
            ]
        );
        assert!(tokens.iter().any(|token| matches!(token, Token::PostQuote(quote) if quote.board() == Some("po") && quote.id() == 42)));
    }
    for format in [0, 8, 15, 24, 31, 40, 47, 56, 63, 64, 103, 112, 128] {
        assert!(
            parse_post_comment(input, format)
                .iter()
                .flat_map(|line| &line.tokens)
                .all(|token| !matches!(token, Token::StaticQuote(_, _))),
            "{format}"
        );
    }
}

#[test]
fn post_references_retain_leading_zero_labels_and_bounded_targets() {
    for (input, label, board, id) in [
        (">>00042", ">>00042", None, 42),
        (">>>/po/00043", ">>>/po/00043", Some("po"), 43),
        (
            "https://boards.4chan.org/g/thread/00042#p00043",
            ">>00043",
            None,
            43,
        ),
    ] {
        let lines = board_domain::parse_post_comment_on_board(input, 104, "g");
        let Token::PostQuote(quote) = &lines[0].tokens[0] else {
            panic!("{input}");
        };
        assert_eq!(quote.label(), label);
        assert_eq!(quote.board(), board);
        assert_eq!(quote.id(), id);
        assert_eq!(
            quote.href("g"),
            format!("/{}/post/{id}", board.unwrap_or("g"))
        );
    }
}
