//! A post reference retains its lexical label independently of its numeric
//! destination. Only validated board identifiers can become route segments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PostQuote {
    pub(crate) board: Option<String>,
    pub(crate) id: u64,
    pub(crate) label: String,
    pub(crate) presentation: QuotePresentation,
}

impl PostQuote {
    /// The source's post-wrap cross-board alphabet accepts signed decimal and
    /// lowercase exponent spellings. Parse the complete token before applying
    /// PHP's string-to-integer conversion at its SQL `%d` boundary.
    pub(crate) fn parse_source_cross(input: &str) -> Option<(Self, usize)> {
        let rest = input.strip_prefix(">>>/")?;
        let end = rest
            .bytes()
            .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            .count();
        if rest.as_bytes().get(end) != Some(&b'/') {
            return None;
        }
        let board = crate::BoardSlug::parse(&rest[..end]).ok()?;
        let rest = &rest[end + 1..];
        let len = rest
            .bytes()
            .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"+/-".contains(c))
            .count();
        let spelling = &rest[..len];
        if !crate::static_quote::numeric_term(spelling) {
            return None;
        }
        let id = crate::php_scalar::integer(spelling).max(0) as u64;
        let consumed = 4 + end + 1 + len;
        Some((
            Self {
                board: Some(board.as_str().into()),
                id,
                label: input[..consumed].into(),
                presentation: if id == 0 {
                    QuotePresentation::Plain
                } else {
                    QuotePresentation::Unresolved
                },
            },
            consumed,
        ))
    }

    pub fn href(&self, current_board: &str) -> String {
        format!(
            "/{}/post/{}",
            self.board.as_deref().unwrap_or(current_board),
            self.id
        )
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn board(&self) -> Option<&str> {
        self.board.as_deref()
    }

    pub fn board_label(&self) -> &str {
        self.board.as_deref().unwrap_or("")
    }

    pub fn digits(&self) -> &str {
        if self.board.is_some() {
            self.label
                .rsplit('/')
                .next()
                .expect("validated post reference")
        } else {
            &self.label[2..]
        }
    }
}

/// A validated identity, never a coerced spelling or an arbitrary route segment.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct QuoteTargetKey {
    board: String,
    post_id: i64,
}

impl QuoteTargetKey {
    pub fn new(board: &str, post_id: i64) -> Option<Self> {
        crate::BoardSlug::parse(board).ok()?;
        (post_id > 0).then(|| Self {
            board: board.into(),
            post_id,
        })
    }
    pub fn board(&self) -> &str {
        &self.board
    }
    pub fn post_id(&self) -> i64 {
        self.post_id
    }
}

/// Unqualified spellings deliberately retain the pre-resolution presentation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QuoteClassification {
    Unqualified,
    Plain,
    Dead,
    Lookup(QuoteTargetKey),
}

/// Request-local presentation only; it is never persisted or used for search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QuotePresentation {
    Unresolved,
    Plain,
    Dead,
    LocalFragment,
    Thread { thread_id: i64 },
}

impl PostQuote {
    pub fn presentation(&self) -> &QuotePresentation {
        &self.presentation
    }

    /// Local syntax lexes `>>1e3` as `>>1` plus `e3`; cross-board syntax
    /// validates its entire numeric term before the source `%d` conversion.
    pub fn classify(&self, current_board: &str) -> QuoteClassification {
        let digits = self.digits();
        if !crate::static_quote::numeric_term(digits) {
            return QuoteClassification::Unqualified;
        }
        let id = crate::php_scalar::integer(digits);
        let board = self.board.as_deref().unwrap_or(current_board);
        if let Some(board) = self.board.as_deref() {
            if !crate::static_quote::reference_board(board) {
                return QuoteClassification::Plain;
            }
            if current_board == "mlp" && matches!(board, "b" | "co") {
                return QuoteClassification::Dead;
            }
        }
        // Post zero cannot exist in the Rust store. Its source lookup is an
        // absence, after the cross-board allowlist and forced-dead branches.
        match QuoteTargetKey::new(board, id) {
            Some(key) => QuoteClassification::Lookup(key),
            None => QuoteClassification::Dead,
        }
    }

    /// Only a visible identity obtained in the caller's snapshot may be passed.
    /// Cross-board syntax always retains a full thread destination.
    pub fn resolve(
        &mut self,
        current_board: &str,
        current_thread: Option<i64>,
        target_thread: Option<i64>,
    ) {
        self.presentation = match self.classify(current_board) {
            QuoteClassification::Unqualified => QuotePresentation::Unresolved,
            QuoteClassification::Plain => QuotePresentation::Plain,
            QuoteClassification::Dead => QuotePresentation::Dead,
            QuoteClassification::Lookup(_) => match target_thread.filter(|id| *id > 0) {
                None => QuotePresentation::Dead,
                Some(thread_id)
                    if self.board.is_none()
                        && current_thread.is_some_and(|current| {
                            (thread_id as u64 != self.id && current == thread_id)
                                || self.source_equals_thread(current)
                        }) =>
                {
                    QuotePresentation::LocalFragment
                }
                Some(thread_id) => QuotePresentation::Thread { thread_id },
            },
        };
    }

    // PHP compares an integer to a numeric string before constructing a local
    // fragment. An overflowing decimal string compares as a double, while its
    // separate SQL `%d` conversion saturates. Those are different operations.
    fn source_equals_thread(&self, thread_id: i64) -> bool {
        self.digits().parse::<i64>().map_or_else(
            |_| {
                self.digits()
                    .parse::<f64>()
                    .is_ok_and(|value| value == thread_id as f64)
            },
            |value| value == thread_id,
        )
    }

    pub fn resolved_href(&self, current_board: &str) -> Option<String> {
        match self.presentation {
            QuotePresentation::LocalFragment => Some(format!("#p{}", self.digits())),
            QuotePresentation::Thread { thread_id } => Some(format!(
                "/{}/thread/{}#p{}",
                self.board.as_deref().unwrap_or(current_board),
                if thread_id as u64 == self.id {
                    self.digits().to_owned()
                } else {
                    thread_id.to_string()
                },
                self.digits()
            )),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Line, Token};

    fn quotes(input: &str, profile: i16, board: &str) -> Vec<PostQuote> {
        crate::parse_post_comment_on_board(input, profile, board)
            .into_iter()
            .flat_map(|line| line.tokens)
            .filter_map(|token| match token {
                Token::PostQuote(q) => Some(q),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn source_profiles_resolve_without_changing_lexical_search_projection() {
        for profile in (104..=111)
            .chain(120..=127)
            .chain(232..=239)
            .chain(248..=255)
        {
            let mut quote = quotes(">>42", profile, "g").remove(0);
            assert_eq!(
                quote.classify("g"),
                QuoteClassification::Lookup(QuoteTargetKey::new("g", 42).unwrap())
            );
            quote.resolve("g", Some(40), Some(40));
            assert_eq!(quote.presentation(), &QuotePresentation::LocalFragment);
            assert_eq!(quote.resolved_href("g").as_deref(), Some("#p42"));
            assert_eq!(
                crate::formatting::plain_text(&[Line {
                    green: false,
                    tokens: vec![Token::PostQuote(quote.clone())]
                }]),
                ">>42"
            );
            quote.resolve("g", None, Some(40));
            assert_eq!(
                quote.resolved_href("g").as_deref(),
                Some("/g/thread/40#p42")
            );
            quote.resolve("g", Some(41), Some(40));
            assert_eq!(
                quote.presentation(),
                &QuotePresentation::Thread { thread_id: 40 }
            );
            quote.resolve("g", Some(40), None);
            assert_eq!(quote.presentation(), &QuotePresentation::Dead);
        }
    }

    #[test]
    fn allowlist_and_forced_dead_links_never_become_lookup_keys() {
        for (board, target, expected) in [
            ("g", "unknown", QuoteClassification::Plain),
            ("g", "j", QuoteClassification::Plain),
            ("mlp", "b", QuoteClassification::Dead),
            ("mlp", "co", QuoteClassification::Dead),
        ] {
            let mut quote = quotes(&format!(">>>/{target}/42"), 104, board).remove(0);
            assert_eq!(quote.classify(board), expected);
            quote.resolve(board, Some(40), Some(40));
            assert!(quote.resolved_href(board).is_none());
        }
        let mut quote = quotes(">>>/co/42", 104, "g").remove(0);
        quote.resolve("g", Some(40), Some(40));
        assert_eq!(
            quote.resolved_href("g").as_deref(),
            Some("/co/thread/40#p42")
        );
    }

    #[test]
    fn leading_zero_labels_resolve_numerically_and_keep_raw_destinations() {
        for input in [">>00042", ">>>/co/00042"] {
            let mut quote = quotes(input, 104, "g").remove(0);
            assert_eq!(
                quote.classify("g"),
                QuoteClassification::Lookup(
                    QuoteTargetKey::new(quote.board().unwrap_or("g"), 42).unwrap()
                )
            );
            quote.resolve("g", Some(40), Some(40));
            assert_eq!(
                quote.resolved_href("g").as_deref(),
                Some(if quote.board().is_none() {
                    "#p00042"
                } else {
                    "/co/thread/40#p00042"
                })
            );
            assert_eq!(quote.label(), input);
            quote.resolve("g", None, Some(42));
            assert_eq!(
                quote.resolved_href("g").as_deref(),
                Some(if quote.board().is_none() {
                    "/g/thread/00042#p00042"
                } else {
                    "/co/thread/00042#p00042"
                })
            );
            quote.resolve("g", Some(40), None);
            assert_eq!(quote.presentation(), &QuotePresentation::Dead);
        }
        assert!(quotes(">>>/co/123abc", 104, "g").is_empty());
        assert!(QuoteTargetKey::new("../g", 1).is_none());
        assert!(QuoteTargetKey::new("g", 0).is_none());
        assert!(QuoteTargetKey::new("g", -1).is_none());
        assert_eq!(
            quotes(">>9223372036854775807", 104, "g")[0].classify("g"),
            QuoteClassification::Lookup(QuoteTargetKey::new("g", i64::MAX).unwrap())
        );
    }

    #[test]
    fn zero_is_inert_and_long_zero_prefixes_follow_source_word_breaks() {
        for profile in (104..=111)
            .chain(120..=127)
            .chain(232..=239)
            .chain(248..=255)
        {
            for input in [">>0", ">>0000", ">>>/co/0", ">>>/co/0000"] {
                let mut quote = quotes(input, profile, "g").remove(0);
                assert_eq!(quote.classify("g"), QuoteClassification::Dead);
                assert_eq!(quote.presentation(), &QuotePresentation::Plain);
                quote.resolve("g", Some(40), Some(40));
                assert_eq!(quote.presentation(), &QuotePresentation::Dead);
                assert_eq!(quote.label(), input);
            }
            let label = format!(">>{}42", "0".repeat(30));
            let parsed = quotes(&label, profile, "g");
            assert_eq!(parsed[0].label(), label);
            assert_eq!(parsed[0].id(), 42);
            let wrapped = quotes(&format!(">>{}42", "0".repeat(33)), profile, "g");
            assert_eq!(wrapped.len(), 1);
            assert_eq!(wrapped[0].id(), 0);
            assert_eq!(wrapped[0].label(), format!(">>{}", "0".repeat(33)));
        }
    }

    #[test]
    fn same_board_digit_prefix_is_lexical_matching_not_numeric_coercion() {
        for (input, expected) in [(">>1e3", 1), (">>123abc", 123)] {
            let parsed = quotes(input, 104, "g");
            assert_eq!(parsed.len(), 1);
            assert_eq!(
                parsed[0].classify("g"),
                QuoteClassification::Lookup(QuoteTargetKey::new("g", expected).unwrap())
            );
        }
    }

    #[test]
    fn historical_tokens_do_not_opt_into_resolution() {
        for profile in [0, 1, 8, 40, 72, 88, 96] {
            assert!(
                quotes(">>42 >>>/co/43", profile, "g").is_empty(),
                "{profile}"
            );
        }
    }
    #[test]
    fn pinned_source_fixture_drives_classification_and_presentation() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/quote-resolution-reference.json"
        ))
        .unwrap();
        assert_eq!(
            fixture["source_revision"],
            "545b7812d1849f7958d914950c91fdbbe38f6b22"
        );
        let cases = fixture["cases"].as_array().unwrap();
        let numeric: serde_json::Value =
            serde_json::from_str(include_str!("../../../fixtures/source-quote-numeric.json"))
                .unwrap();
        assert!(cases.len() >= 108);
        assert!(numeric["cases"].as_array().unwrap().len() >= 53);
        for case in cases.iter().chain(numeric["cases"].as_array().unwrap()) {
            let name = case["id"].as_str().unwrap();
            let board = case["source_board"].as_str().unwrap();
            let current_thread = case["source_thread_id"]
                .as_str()
                .map(|v| v.parse::<i64>().unwrap());
            let target_board = case["target_board"].as_str().unwrap();
            let target_id = case["lookup_post_id"]
                .as_str()
                .unwrap_or_else(|| case["target_post_id"].as_str().unwrap())
                .parse::<i64>()
                .unwrap();
            let label = case["label_html"].as_str().unwrap().replace("&gt;", ">");
            // The source represents OP identities as resto=0; the Rust store
            // returns their actual thread_id. This only adapts the input stub.
            let thread_id = case["lookup_resto"].as_str().map(|value| {
                let resto = value.parse::<i64>().unwrap();
                if resto == 0 { target_id } else { resto }
            });
            let expected = &case["expected"];
            let expected_href = expected["href"]
                .as_str()
                .map(|href| href.strip_prefix("//boards.example.test").unwrap_or(href));
            for profile in (104..=111)
                .chain(120..=127)
                .chain(232..=239)
                .chain(248..=255)
            {
                let mut parsed = quotes(&label, profile, board);
                assert_eq!(parsed.len(), 1, "{name}, profile {profile}");
                let mut quote = parsed.remove(0);
                let classification = if expected["kind"] == "plain" {
                    QuoteClassification::Plain
                } else if target_id <= 0 || (board == "mlp" && matches!(target_board, "b" | "co")) {
                    QuoteClassification::Dead
                } else {
                    QuoteClassification::Lookup(
                        QuoteTargetKey::new(target_board, target_id).unwrap(),
                    )
                };
                assert_eq!(
                    quote.classify(board),
                    classification,
                    "{name}, profile {profile}"
                );
                quote.resolve(board, current_thread, thread_id);
                let presentation = match expected["kind"].as_str().unwrap() {
                    "local" => QuotePresentation::LocalFragment,
                    "thread" => QuotePresentation::Thread {
                        thread_id: expected["thread_id"].as_str().unwrap().parse().unwrap(),
                    },
                    "dead" => QuotePresentation::Dead,
                    "plain" => QuotePresentation::Plain,
                    other => panic!("unrecognized fixture state {other}"),
                };
                assert_eq!(
                    quote.presentation(),
                    &presentation,
                    "{name}, profile {profile}"
                );
                assert_eq!(
                    quote.resolved_href(board).as_deref(),
                    expected_href,
                    "{name}, profile {profile}"
                );
                assert_eq!(quote.label(), label, "{name}, profile {profile}");
                assert_eq!(
                    crate::formatting::visible_text(&Token::PostQuote(quote)),
                    label,
                    "{name}, profile {profile}"
                );
            }
        }
    }
}
