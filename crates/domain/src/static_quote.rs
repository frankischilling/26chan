//! Static board/catalog references. Destinations are constructed from a board
//! identifier and a finite lexical search term, never from an arbitrary URL.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaticQuote {
    board: String,
    term: String,
}

impl StaticQuote {
    pub(crate) fn parse(input: &str) -> Option<(Self, usize)> {
        let rest = input.strip_prefix(">>>/")?;
        // Static rules links accept any nonempty lowercase alphanumeric
        // board spelling, independently of the application's routing slugs.
        let end = rest
            .bytes()
            .take_while(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
            .count();
        if end == 0 || rest.as_bytes().get(end) != Some(&b'/') {
            return None;
        }
        let board = &rest[..end];
        let rest = &rest[end + 1..];
        let len = rest
            .bytes()
            .take_while(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"+/,-".contains(byte)
            })
            .count();
        let term = &rest[..len];
        // PHP also recognizes signed numbers and exponents. Leave those for
        // post resolution rather than converting them into catalog searches.
        if numeric_term(term)
            || (!term.starts_with("rules") && !reference_board(board))
            || (board == "f" && term == "catalog")
        {
            return None;
        }
        Some((
            Self {
                board: board.into(),
                term: term.into(),
            },
            4 + end + 1 + len,
        ))
    }

    pub fn label(&self) -> String {
        format!(">>>/{}/{}", self.board, self.term)
    }

    pub fn href(&self) -> String {
        if self.opens_new_tab() {
            let rule = self.term.split_once('/').map_or("", |(_, rule)| rule);
            format!("/rules#{}{rule}", self.board)
        } else if self.term.is_empty() {
            format!("/{}/", self.board)
        } else if self.term == "catalog" {
            format!("/{}/catalog", self.board)
        } else {
            // The source decodes plus as space before encoding its fragment.
            let decoded = self.term.replace('+', " ");
            let encoded: String =
                url::form_urlencoded::byte_serialize(decoded.as_bytes()).collect();
            format!("/{}/catalog#s={encoded}", self.board)
        }
    }

    pub fn opens_new_tab(&self) -> bool {
        self.term.starts_with("rules")
    }

    pub fn source_href(&self) -> String {
        format!(
            "//{}.4chan.org{}",
            if self.opens_new_tab() {
                "www"
            } else {
                "boards"
            },
            self.href()
        )
    }
}

/// Dynamic source references use their whole post-wrap term. Unsupported or
/// incomplete numeric text must never resolve to a leading-digit post ID.
pub(crate) fn unresolved_post_reference_prefix(input: &str) -> Option<usize> {
    let rest = input.strip_prefix(">>>/")?;
    let board_end = rest
        .bytes()
        .take_while(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        .count();
    if board_end == 0 || rest.as_bytes().get(board_end) != Some(&b'/') {
        return None;
    }
    let rest = &rest[board_end + 1..];
    // This is the later auto_link_cb alphabet, not the earlier static-link
    // alphabet. Punctuation and generated soft breaks end the captured term.
    let term_len = rest
        .bytes()
        .take_while(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"+/-".contains(byte)
        })
        .count();
    let term = &rest[..term_len];
    (!term.is_empty() && !term.bytes().all(|byte| byte.is_ascii_digit()))
        .then_some(4 + board_end + 1 + term_len)
}

/// PHP is_numeric() over the source static-link alphabet [a-z0-9+/,l-]*.
/// Check syntax rather than converting to a bounded integer or float: an
/// arbitrarily large exponent is still numeric to the source callback.
fn numeric_term(term: &str) -> bool {
    fn digits(value: &str) -> bool {
        !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
    }
    let unsigned = term.strip_prefix(['+', '-']).unwrap_or(term);
    match unsigned.split_once('e') {
        Some((mantissa, exponent)) => {
            digits(mantissa) && digits(exponent.strip_prefix(['+', '-']).unwrap_or(exponent))
        }
        None => digits(unsigned),
    }
}

pub(crate) fn reference_board(board: &str) -> bool {
    // The source's explicit static-link allowlist, not the board directory.
    const BOARDS: &[&str] = &[
        "3", "a", "aco", "adv", "an", "b", "bant", "biz", "c", "cgl", "ck", "cm", "co", "d", "diy",
        "e", "f", "fa", "fit", "g", "gd", "gif", "h", "hc", "his", "hm", "hr", "i", "ic", "int",
        "jp", "k", "lgbt", "lit", "m", "mlp", "mu", "n", "news", "o", "out", "p", "po", "pol",
        "pw", "qa", "qst", "r", "r9k", "s", "s4s", "sci", "soc", "sp", "t", "tg", "toy", "trash",
        "trv", "tv", "u", "v", "vg", "vip", "vm", "vmg", "vp", "vr", "vrpg", "vst", "vt", "w",
        "wg", "wsg", "wsr", "x", "xs", "y",
    ];
    BOARDS.binary_search(&board).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn board_catalog_and_search_references_have_finite_local_destinations() {
        for (input, label, href) in [
            (">>>/po/", ">>>/po/", "/po/"),
            (">>>/po/catalog", ">>>/po/catalog", "/po/catalog"),
            (">>>/po/lft", ">>>/po/lft", "/po/catalog#s=lft"),
            (
                ">>>/g/a+b/c,d-e!",
                ">>>/g/a+b/c,d-e",
                "/g/catalog#s=a+b%2Fc%2Cd-e",
            ),
            (">>>/g/123abc", ">>>/g/123abc", "/g/catalog#s=123abc"),
            (">>>/g/rules/3", ">>>/g/rules/3", "/rules#g3"),
            (">>>/unknown/rules", ">>>/unknown/rules", "/rules#unknown"),
        ] {
            let (quote, consumed) = StaticQuote::parse(input).unwrap();
            assert_eq!(quote.label(), label);
            assert_eq!(&input[..consumed], label);
            assert_eq!(quote.href(), href);
        }
        for input in [
            ">>>/g/42",
            ">>>/unknown/catalog",
            ">>>/g/+42",
            ">>>/g/-42",
            ">>>/j/catalog",
            ">>>/test/",
            ">>>/f/catalog",
            ">>>/G/catalog",
            ">>>/../catalog",
            ">>>/\"x/catalog",
        ] {
            assert!(StaticQuote::parse(input).is_none(), "{input}");
        }
    }

    #[test]
    fn numeric_exponents_and_long_rules_boards_match_the_source_callback() {
        let reference: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/source-link-boundary-reference.json"
        ))
        .unwrap();
        for case in reference["static_cases"].as_array().unwrap() {
            let input = case["input"].as_str().unwrap();
            let links = case["links"].as_array().unwrap();
            let parsed = StaticQuote::parse(input);
            if links.is_empty() {
                assert!(parsed.is_none(), "{input}");
            } else {
                assert_eq!(links.len(), 1, "single source static prefix");
                let (quote, consumed) = parsed.unwrap_or_else(|| panic!("{input}"));
                assert_eq!(
                    quote.label(),
                    links[0]["label"].as_str().unwrap(),
                    "{input}"
                );
                assert_eq!(quote.href(), links[0]["href"].as_str().unwrap(), "{input}");
                assert_eq!(
                    quote.opens_new_tab(),
                    links[0]["new_tab"].as_bool().unwrap(),
                    "{input}"
                );
                assert_eq!(&input[..consumed], quote.label(), "{input}");
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]
        #[test]
        fn arbitrarily_long_rules_boards_keep_a_fixed_local_path(
            board in "[a-z0-9]{1,128}", rule in "[a-z0-9+/,\\-]{0,128}"
        ) {
            let input = format!(">>>/{board}/rules/{rule}");
            let (quote, consumed) = StaticQuote::parse(&input).unwrap();
            prop_assert_eq!(consumed, input.len());
            prop_assert!(quote.opens_new_tab());
            let base = url::Url::parse("https://board.example/").unwrap();
            let destination = base.join(&quote.href()).unwrap();
            prop_assert_eq!(destination.origin(), base.origin());
            prop_assert_eq!(destination.path(), "/rules");
            prop_assert!(destination.query().is_none());
            prop_assert!(destination.username().is_empty());
        }

        #[test]
        fn arbitrary_terms_cannot_change_the_destination_origin(input in ".{0,512}") {
            if let Some((quote, consumed)) = StaticQuote::parse(&format!(">>>/{input}")) {
                prop_assert!(consumed <= input.len() + 4);
                let base = url::Url::parse("https://board.example/").unwrap();
                let destination = base.join(&quote.href()).unwrap();
                prop_assert_eq!(destination.origin(), base.origin());
                prop_assert!(destination.query().is_none());
                prop_assert!(destination.username().is_empty());
            }
        }
    }
}
