use crate::{MAX_PUBLIC_FIELD_BYTES, ValidationError};

#[derive(Debug, PartialEq, Eq)]
pub struct PostingOptions {
    pub sage: bool,
    pub return_to_board: bool,
    pub anonymous: bool,
}

/// Public options are bounded raw text. The source removes all ASCII
/// case-insensitive "sage" occurrences before its exact nonoko/capcode checks.
pub fn parse(value: &str) -> Result<PostingOptions, ValidationError> {
    if value.len() > MAX_PUBLIC_FIELD_BYTES {
        return Err(ValidationError("Options must contain at most 100 bytes."));
    }
    let lowered = value.to_ascii_lowercase();
    let mut remainder = String::with_capacity(value.len());
    let mut end = 0;
    for (index, _) in lowered.match_indices("sage") {
        remainder.push_str(&value[end..index]);
        end = index + 4;
    }
    remainder.push_str(&value[end..]);
    Ok(PostingOptions {
        sage: end != 0,
        return_to_board: remainder.eq_ignore_ascii_case("nonoko"),
        // This parser is for unauthenticated public posting only. The source
        // clears the name on a capcode attempt and grants no capcode authority.
        anonymous: remainder.starts_with("capcode_"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn source_options_remove_sage_without_trimming_or_interpreting_other_text() {
        for (value, sage, board, anonymous) in [
            ("", false, false, false),
            ("sage", true, false, false),
            ("NONOKO", false, true, false),
            ("sageNONOKOSaGe", true, true, false),
            ("nonokosageSAGE", true, true, false),
            ("nonoko sage", true, false, false),
            ("nonokononokosage", true, false, false),
            ("message", true, false, false),
            ("sage sage", true, false, false),
            (" sage", true, false, false),
            ("📄SAGE", true, false, false),
            ("ＳＡＧＥ", false, false, false),
            ("<script>fold</script>", false, false, false),
            ("capcode_admin", false, false, true),
            ("sagecapcode_mod", true, false, true),
            ("CAPCODE_admin", false, false, false),
        ] {
            assert_eq!(
                parse(value).unwrap(),
                PostingOptions {
                    sage,
                    return_to_board: board,
                    anonymous
                },
                "{value}"
            );
        }
        assert!(parse(&"😀".repeat(25)).is_ok());
        assert!(parse(&format!("{}a", "😀".repeat(25))).is_err());
        assert!(parse(&"a".repeat(101)).is_err());
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]
        #[test]
        fn utf8_byte_bounds_and_ascii_matching_hold_for_arbitrary_text(value in ".{0,130}") {
            let result = parse(&value);
            prop_assert_eq!(result.is_ok(), value.len() <= MAX_PUBLIC_FIELD_BYTES);
            if let Ok(result) = result {
                let lower = value.to_ascii_lowercase();
                prop_assert_eq!(result.sage, lower.contains("sage"));
                prop_assert_eq!(result.return_to_board, lower.replace("sage", "") == "nonoko");
            }
        }
    }
}
