//! Bounded projections used by the supplied content admission rules.
//! Native handles stay private, local, and exclusively owned. Only the fixed
//! factory, text transformation, and destructor are used; no custom ICU rules.

pub const MAX_INPUT_BYTES: usize = 131_072;
pub const MAX_OUTPUT_BYTES: usize = 524_288;
/// Security work budget, independent of the source's matching projection.
pub const MAX_COMPATIBILITY_SCALARS: usize = 65_536;
pub const MAX_NATIVE_INPUT_SCALARS: usize = 18_000;
const TRANSFORM: &str = "Any-Latin; nfd; [:nonspacing mark:] remove; nfkc; Latin-ASCII";

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum NormalizationError {
    #[error("Admission normalization input is too large.")]
    InputTooLarge,
    #[error("Admission normalization output is too large.")]
    OutputTooLarge,
    #[error("Admission normalization work limit exceeded.")]
    WorkTooLarge,
    #[error("Admission normalization returned invalid Unicode.")]
    InvalidOutput,
    #[error("Admission normalization is unavailable.")]
    Unavailable,
}

/// Construct, use, and drop within one synchronous operation before awaiting.
/// This type has no cloning or native-handle access API.
pub struct Normalizer {
    transform: rust_icu_utrans::UTransliterator,
}

impl Normalizer {
    pub fn new() -> Result<Self, NormalizationError> {
        let transform = rust_icu_utrans::UTransliterator::new(
            TRANSFORM,
            None,
            rust_icu_sys::UTransDirection::UTRANS_FORWARD,
        )
        .map_err(|_| NormalizationError::Unavailable)?;
        Ok(Self { transform })
    }

    pub fn ascii(&self, input: &str, preserve_case: bool) -> Result<String, NormalizationError> {
        check_input(input)?;
        let replaced = replace_dots(input);
        // ASCII is unchanged by this fixed ICU chain. Besides avoiding native
        // work, this preserves byte-wise PHP casing and embedded NUL characters.
        let mut output = if replaced.is_ascii() {
            replaced
        } else {
            // Compatibility ligatures can expand into many characters during
            // the native in-place operation. Count a bounded streaming NFKC
            // projection before entering it; this is only a work guard. Matching
            // still uses the complete original ICU transform and its ICU 74 data.
            if replaced.chars().take(MAX_NATIVE_INPUT_SCALARS + 1).count()
                > MAX_NATIVE_INPUT_SCALARS
                || icu_normalizer::ComposingNormalizer::new_nfkc()
                    .normalize_iter(replaced.chars())
                    .take(MAX_COMPATIBILITY_SCALARS + 1)
                    .count()
                    > MAX_COMPATIBILITY_SCALARS
            {
                return Err(NormalizationError::WorkTooLarge);
            }
            self.transform.transliterate(&replaced).map_err(|error| {
                if error.is_code(rust_icu_sys::UErrorCode::U_INVALID_CHAR_FOUND) {
                    NormalizationError::InvalidOutput
                } else {
                    NormalizationError::Unavailable
                }
            })?
        };
        if output.len() > MAX_OUTPUT_BYTES {
            return Err(NormalizationError::OutputTooLarge);
        }
        // PHP strtolower changes ASCII bytes, not Unicode case mappings.
        if !preserve_case {
            output.make_ascii_lowercase();
        }
        Ok(output)
    }

    pub fn text(&self, input: &str) -> Result<String, NormalizationError> {
        let mut output = self.ascii(input, false)?;
        retain_text(&mut output);
        Ok(output)
    }
}

/// ASCII needs no native handle; non-ASCII retains the fixed ICU projection.
pub fn text(input: &str) -> Result<String, NormalizationError> {
    check_input(input)?;
    if input.is_ascii() {
        let mut output = replace_dots(input).to_ascii_lowercase();
        retain_text(&mut output);
        Ok(output)
    } else {
        Normalizer::new()?.text(input)
    }
}

fn retain_text(output: &mut String) {
    output.retain(|ch| {
        !crate::comment_spacing::zero_width(ch)
            && (ch.is_ascii_alphanumeric()
                || matches!(
                    ch,
                    '.' | ',' | '/' | '&' | ':' | ';' | '?' | '=' | '~' | '_' | '-'
                ))
    });
}

pub fn strip_zero_width(input: &str) -> Result<String, NormalizationError> {
    check_input(input)?;
    Ok(input
        .chars()
        .filter(|&ch| !crate::comment_spacing::zero_width(ch))
        .collect())
}

fn check_input(input: &str) -> Result<(), NormalizationError> {
    if input.len() > MAX_INPUT_BYTES {
        Err(NormalizationError::InputTooLarge)
    } else {
        Ok(())
    }
}

fn replace_dots(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = String::with_capacity(input.len());
    let mut offset = 0;
    while offset < bytes.len() {
        if bytes.len() - offset >= 5
            && b"([={".contains(&bytes[offset])
            && bytes[offset + 1..offset + 4].eq_ignore_ascii_case(b"dot")
            && b")]=}".contains(&bytes[offset + 4])
        {
            output.push('.');
            offset += 5;
        } else {
            // offset advances only over complete UTF-8 scalars or ASCII matches.
            let ch = input[offset..].chars().next().expect("remaining scalar");
            output.push(ch);
            offset += ch.len_utf8();
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn all_recorded_php_icu_projections_match() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/admission-normalization.json"
        ))
        .unwrap();
        assert_eq!(fixture["extractor_icu"], "74.2");
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 1862);
        let normalizer = Normalizer::new().unwrap();
        let mut invalid_outputs = 0;
        for (index, case) in cases.iter().enumerate() {
            let input = case["input"].as_str().unwrap();
            if case["ascii_preserving_case"] == false {
                invalid_outputs += 1;
                assert_eq!(case["icu_error_code"], 10);
                assert_eq!(case["ascii"], "");
                assert_eq!(case["text"], "");
                // PHP coerces its failed native output to empty matching text.
                // Admission must instead reject all projections explicitly.
                assert_eq!(
                    normalizer.ascii(input, false),
                    Err(NormalizationError::InvalidOutput)
                );
                assert_eq!(
                    normalizer.ascii(input, true),
                    Err(NormalizationError::InvalidOutput)
                );
                assert_eq!(
                    normalizer.text(input),
                    Err(NormalizationError::InvalidOutput)
                );
                assert_eq!(text(input), Err(NormalizationError::InvalidOutput));
                assert_eq!(strip_zero_width(input).unwrap(), case["zero_width_removed"]);
                continue;
            }
            assert_eq!(
                normalizer.ascii(input, false).unwrap(),
                case["ascii"],
                "lowercase {index}"
            );
            assert_eq!(
                normalizer.ascii(input, true).unwrap(),
                case["ascii_preserving_case"],
                "case {index}"
            );
            assert_eq!(
                normalizer.text(input).unwrap(),
                case["text"],
                "text {index}"
            );
            assert_eq!(
                text(input).unwrap(),
                case["text"],
                "standalone projection {index}"
            );
            assert_eq!(
                strip_zero_width(input).unwrap(),
                case["zero_width_removed"],
                "exclusions {index}"
            );
        }
        assert_eq!(invalid_outputs, 10);
    }

    #[test]
    fn compatibility_expansion_is_rejected_before_native_work() {
        let normalizer = Normalizer::new().unwrap();
        let maximum = "A".repeat(MAX_INPUT_BYTES);
        assert_eq!(
            normalizer.ascii(&maximum, false).unwrap(),
            "a".repeat(MAX_INPUT_BYTES)
        );
        let oversized = "A".repeat(MAX_INPUT_BYTES + 1);
        assert_eq!(
            normalizer.ascii(&oversized, true),
            Err(NormalizationError::InputTooLarge)
        );
        assert_eq!(
            normalizer.text(&oversized),
            Err(NormalizationError::InputTooLarge)
        );
        assert_eq!(
            strip_zero_width(&oversized),
            Err(NormalizationError::InputTooLarge)
        );
        // One source scalar expands into many compatibility characters. Both
        // projections must reject before the expensive native in-place pass.
        let expanding = "ﷺ".repeat(4096);
        assert!(matches!(
            normalizer.ascii(&expanding, true),
            Err(NormalizationError::WorkTooLarge)
        ));
        assert!(matches!(
            normalizer.text(&expanding),
            Err(NormalizationError::WorkTooLarge)
        ));
        let too_many_native_scalars = "é".repeat(MAX_NATIVE_INPUT_SCALARS + 1);
        assert!(matches!(
            normalizer.ascii(&too_many_native_scalars, true),
            Err(NormalizationError::WorkTooLarge)
        ));
        let admitted = normalizer
            .ascii(&"é".repeat(MAX_NATIVE_INPUT_SCALARS), true)
            .unwrap();
        assert_eq!(admitted.len(), MAX_NATIVE_INPUT_SCALARS);
        assert!(admitted.bytes().all(|ch| ch == b'e'));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]
        #[test]
        fn arbitrary_unicode_is_bounded_and_text_has_only_source_ascii(
            input in prop::collection::vec(any::<char>(), 0..256)
        ) {
            let input: String = input.into_iter().collect();
            let normalizer = Normalizer::new().unwrap();
            let ascii = normalizer.ascii(&input, true);
            let lower = normalizer.ascii(&input, false);
            let matching = normalizer.text(&input);
            match ascii {
                Ok(ascii) => {
                    prop_assert!(ascii.len() <= MAX_OUTPUT_BYTES);
                    prop_assert_eq!(lower.unwrap(), ascii.to_ascii_lowercase());
                    let matching = matching.unwrap();
                    prop_assert!(matching.bytes().all(|ch| ch.is_ascii_alphanumeric() || b".,/&:;?=~_-".contains(&ch)));
                    prop_assert!(matching.len() <= MAX_OUTPUT_BYTES);
                    prop_assert_eq!(text(&input).unwrap(), matching);
                }
                Err(error) => {
                    // Valid input can produce invalid UTF-16 inside ICU 74.
                    // No generated input is discarded: only that specific
                    // native failure is accepted, and every API must reject it.
                    prop_assert_eq!(error, NormalizationError::InvalidOutput);
                    prop_assert_eq!(lower, Err(NormalizationError::InvalidOutput));
                    prop_assert_eq!(matching, Err(NormalizationError::InvalidOutput));
                    prop_assert_eq!(text(&input), Err(NormalizationError::InvalidOutput));
                }
            }
            let stripped = strip_zero_width(&input).unwrap();
            prop_assert!(stripped.len() <= input.len());
            prop_assert_eq!(strip_zero_width(&stripped).unwrap(), stripped);
        }
    }
}
