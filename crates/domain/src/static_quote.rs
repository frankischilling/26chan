//! Static board/catalog references. Destinations are constructed from a board
//! identifier and a finite lexical search term, never from an arbitrary URL.
use crate::BoardSlug;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaticQuote {
    board: String,
    term: String,
}

impl StaticQuote {
    pub(crate) fn parse(input: &str) -> Option<(Self, usize)> {
        let rest = input.strip_prefix(">>>/")?;
        let end = rest.bytes().take(11).position(|byte| byte == b'/')?;
        let board = BoardSlug::parse(&rest[..end]).ok()?;
        let rest = &rest[end + 1..];
        let len = rest
            .bytes()
            .take_while(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"+/,-".contains(byte)
            })
            .count();
        let term = &rest[..len];
        // PHP's numeric check also accepts a leading sign. Post resolution
        // retains that input rather than converting it into a catalog search.
        let unsigned = term.strip_prefix(['+', '-']).unwrap_or(term);
        if (!unsigned.is_empty() && unsigned.bytes().all(|byte| byte.is_ascii_digit()))
            || (!term.starts_with("rules") && !reference_board(board.as_str()))
            || (board.as_str() == "f" && term == "catalog")
        {
            return None;
        }
        Some((
            Self {
                board: board.as_str().into(),
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

fn reference_board(board: &str) -> bool {
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

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]
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
