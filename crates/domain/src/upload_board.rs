//! Upload-board category identifiers and source subject prefixes.

pub const FILE_ONLY_ERROR: &str =
    "It doesn't make any sense to do a file-only delete on a file board!";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Category {
    pub id: u8,
    pub name: &'static str,
    pub short: &'static str,
}

/// Form order is distinct from the stored numeric identifiers.
pub const CATEGORIES: [Category; 7] = [
    Category {
        id: 0,
        name: "Hentai",
        short: "[H]",
    },
    Category {
        id: 6,
        name: "Porn",
        short: "[P]",
    },
    Category {
        id: 1,
        name: "Japanese",
        short: "[J]",
    },
    Category {
        id: 2,
        name: "Anime",
        short: "[A]",
    },
    Category {
        id: 3,
        name: "Game",
        short: "[G]",
    },
    Category {
        id: 5,
        name: "Loop",
        short: "[L]",
    },
    Category {
        id: 4,
        name: "Other",
        short: "[?]",
    },
];

pub fn category(id: i64) -> Option<Category> {
    CATEGORIES
        .iter()
        .copied()
        .find(|category| i64::from(category.id) == id)
}

/// Source checks both numeric syntax and the integer-coerced table key.
/// Callers must bound the request field before calling this pure function.
pub fn submitted_category(value: &str) -> Option<Category> {
    let raw = value.trim_matches(|ch: char| ch.is_ascii_whitespace());
    let unsigned = raw.strip_prefix(['+', '-']).unwrap_or(raw);
    let (mantissa, exponent) = unsigned
        .split_once(['e', 'E'])
        .map_or((unsigned, None), |(a, b)| (a, Some(b)));
    if let Some(exponent) = exponent {
        let exponent = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
        if exponent.is_empty() || !exponent.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
    }
    let mut digits = 0;
    let mut point = false;
    for byte in mantissa.bytes() {
        if byte.is_ascii_digit() {
            digits += 1;
        } else if byte == b'.' && !point {
            point = true;
        } else {
            return None;
        }
    }
    if digits == 0 {
        return None;
    }
    category(crate::php_scalar::integer(raw))
}

/// No prefix selects Other in HTML. Unknown numeric prefixes remain unknown;
/// their prefix is still stripped, as in the source renderers.
pub fn subject(value: &str) -> (Option<Category>, &str, bool) {
    if let Some((prefix, title)) = value.split_once('|')
        && !prefix.is_empty()
        && prefix.bytes().all(|byte| byte.is_ascii_digit())
    {
        return (prefix.parse::<i64>().ok().and_then(category), title, true);
    }
    (category(4), value, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_and_admission_match_the_original_php() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/upload-board-reference.json"
        ))
        .unwrap();
        for category in CATEGORIES {
            let row = &fixture["tags"][category.id.to_string()];
            assert_eq!(row["long"], category.name);
            assert_eq!(row["short"], category.short);
        }
        assert_eq!(
            CATEGORIES.map(|category| category.id),
            [0, 6, 1, 2, 3, 5, 4]
        );
        for case in fixture["cases"].as_array().unwrap() {
            let actual = submitted_category(case["input"].as_str().unwrap());
            assert_eq!(
                actual.is_some(),
                case["expected"]["accepted"].as_bool().unwrap(),
                "{case}"
            );
            if let Some(category) = actual {
                assert_eq!(
                    format!("{}|owned subject", category.id),
                    case["expected"]["subject"].as_str().unwrap()
                );
            }
        }
    }

    #[test]
    fn subject_prefixes_keep_missing_and_unknown_tags_distinct() {
        assert_eq!(subject("plain"), (category(4), "plain", false));
        assert_eq!(subject("0006|title"), (category(6), "title", true));
        assert_eq!(subject("9|title"), (None, "title", true));
        assert_eq!(subject("+1|title"), (category(4), "+1|title", false));
        assert_eq!(subject("0|"), (category(0), "", true));
    }
}
