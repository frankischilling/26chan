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

#[test]
fn nondecimal_numeric_quotes_never_resolve_to_a_leading_digit_post() {
    for format in [104, 111, 120, 127] {
        for term in [
            "1e2",
            "1e+2",
            "1e-2",
            "+001e+02",
            "-1e2",
            "1e999999999999999999999999999999999999",
        ] {
            let input = format!(">>>/po/{term}");
            let lines = board_domain::parse_post_comment_on_board(&input, format, "g");
            let mut retained = String::new();
            for token in lines.iter().flat_map(|line| &line.tokens) {
                match token {
                    Token::Text(text) => retained.push_str(text),
                    Token::WordBreak => {}
                    _ => panic!("Unexpected reference token for {input}: {token:?}"),
                }
            }
            assert_eq!(retained, input);
        }
    }
}

#[test]
fn post_wrap_numeric_fragments_do_not_gain_partial_destinations() {
    for (prefix, term) in [(26, "1e2"), (25, "1e+2"), (26, "1e+2"), (26, "1e-2")] {
        let input = format!("{}>>>/po/{term}", "x".repeat(prefix));
        let lines = board_domain::parse_post_comment_on_board(&input, 104, "g");
        assert!(
            lines
                .iter()
                .flat_map(|line| &line.tokens)
                .all(|token| !matches!(token, Token::PostQuote(_))),
            "{input}"
        );
    }
    // The source wraps before resolving posts. When the break lands before
    // the exponent, the complete surviving term really is decimal 1.
    let input = format!("{}>>>/po/1e2", "x".repeat(27));
    let lines = board_domain::parse_post_comment_on_board(&input, 104, "g");
    assert!(lines.iter().flat_map(|line| &line.tokens).any(|token| matches!(token, Token::PostQuote(quote) if quote.board() == Some("po") && quote.id() == 1 && quote.label() == ">>>/po/1")));
}

#[test]
fn source_post_wrap_lookup_gate_never_becomes_a_different_decimal_id() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/source-link-boundary-reference.json"
    ))
    .unwrap();
    for case in fixture["dynamic_cases"].as_array().unwrap() {
        let input = case["input"].as_str().unwrap();
        let lines = board_domain::parse_post_comment_on_board(input, 104, "g");
        let quotes: Vec<_> = lines
            .iter()
            .flat_map(|line| &line.tokens)
            .filter_map(|token| match token {
                Token::PostQuote(quote) => Some(quote),
                _ => None,
            })
            .collect();
        let expected: Vec<_> = case["lookup_terms"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|term| {
                let value = term["number"].as_str().unwrap();
                (value.bytes().all(|byte| byte.is_ascii_digit()))
                    .then(|| value.parse::<u64>().unwrap())
            })
            .collect();
        assert_eq!(
            quotes.iter().map(|quote| quote.id()).collect::<Vec<_>>(),
            expected,
            "{input}"
        );
    }
}
