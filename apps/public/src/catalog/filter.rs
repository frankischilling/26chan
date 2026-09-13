//! The pinned client's escaped regex language: literals, ^, $, and | only.
pub(super) struct Filter {
    branches: Vec<Branch>,
}

struct Branch {
    literal: Vec<u16>,
    start: bool,
    end: bool,
}

impl Filter {
    pub(super) fn new(query: &str) -> Self {
        if query.chars().count() > 128 || query.chars().any(char::is_control) {
            return Self {
                branches: Vec::new(),
            };
        }
        let branches = query
            .split('|')
            .filter_map(|part| {
                let mut literal = String::new();
                let mut start = false;
                let mut end = false;
                for ch in part.chars() {
                    match ch {
                        '^' if literal.is_empty() => start = true,
                        '^' => return None,
                        '$' => end = true,
                        _ if end => return None,
                        _ => literal.push(ch),
                    }
                }
                Some(Branch {
                    literal: literal.encode_utf16().map(canonicalize).collect(),
                    start,
                    end,
                })
            })
            .collect();
        Self { branches }
    }

    pub(super) fn matches(&self, text: &str) -> bool {
        let text: Vec<_> = text.encode_utf16().map(canonicalize).collect();
        self.branches
            .iter()
            .any(|branch| match (branch.start, branch.end) {
                (true, true) => text == branch.literal,
                (true, false) => text.starts_with(&branch.literal),
                (false, true) => text.ends_with(&branch.literal),
                (false, false) => {
                    branch.literal.is_empty()
                        || text
                            .windows(branch.literal.len())
                            .any(|window| window == branch.literal)
                }
            })
    }
}

// ECMAScript Canonicalize's non-Unicode ignore-case branch operates on UTF-16
// code units, rejects multi-unit uppercase mappings, and does not fold a
// non-ASCII code unit to ASCII. Surrogate units remain unchanged.
fn canonicalize(unit: u16) -> u16 {
    let Some(ch) = char::from_u32(u32::from(unit)) else {
        return unit;
    };
    let mut upper = ch.to_uppercase();
    let Some(first) = upper.next() else {
        return unit;
    };
    if upper.next().is_some() || first.len_utf16() != 1 || (unit >= 128 && u32::from(first) < 128) {
        return unit;
    }
    first as u16
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn shared_reference_cases_match() {
        let cases: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/catalog-search-cases.json"
        ))
        .unwrap();
        for case in cases["cases"].as_array().unwrap() {
            let query = case["query"].as_str().unwrap();
            let text = case["text"].as_str().unwrap();
            assert_eq!(
                Filter::new(query).matches(text),
                case["matches"].as_bool().unwrap(),
                "{case}"
            );
        }
    }

    #[test]
    fn compilation_retains_the_public_query_bound() {
        for query in ["x".repeat(129), "x\ny".into(), "\0".into()] {
            assert!(!Filter::new(&query).matches(&query));
        }
        let filter = Filter::new(&"|".repeat(128));
        assert_eq!(filter.branches.len(), 129);
        assert!(filter.matches("anything"));
        assert!(Filter::new(&"x".repeat(128)).matches(&"x".repeat(128)));
    }

    proptest! {
        #[test]
        fn bounded_literals_and_anchors_preserve_their_own_text(
            chars in prop::collection::vec(any::<char>().prop_filter("literal, non-control", |ch| !ch.is_control() && !matches!(ch, '^' | '$' | '|')), 0..65)
        ) {
            let literal: String = chars.into_iter().collect();
            let surrounded = format!("prefix{literal}suffix");
            prop_assert!(Filter::new(&literal).matches(&surrounded));
            let anchored = Filter::new(&format!("^{literal}$"));
            prop_assert!(anchored.matches(&literal));
            prop_assert!(!anchored.matches(&surrounded));
        }

        #[test]
        fn code_unit_canonicalization_is_idempotent(unit in any::<u16>()) {
            prop_assert_eq!(canonicalize(canonicalize(unit)), canonicalize(unit));
        }
    }
}
