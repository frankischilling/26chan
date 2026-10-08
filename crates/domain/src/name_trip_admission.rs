//! Fixed source checks after configured content/IP/file policy, before markup.
//! Callers supply escaped display text and an already-derived legacy trip.

use crate::admission_normalization::{NormalizationError, text};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejection {
    Upload,
    BannedText,
}

impl Rejection {
    pub fn message(self) -> &'static str {
        match self {
            Self::Upload => "Error: Upload failed.",
            Self::BannedText => "Error: Your post contained banned text.",
        }
    }
}

/// Moderator authority is server-owned. Secure trip displays have no legacy
/// trip value in the source. No private trip password belongs in either field.
pub fn evaluate(
    display_name: &str,
    legacy_trip: &str,
    moderator: bool,
) -> Result<Option<Rejection>, NormalizationError> {
    if legacy_trip.len() > 64 {
        return Err(NormalizationError::InputTooLarge);
    }
    let normalized = text(display_name)?;
    if normalized.contains("moot")
        && (legacy_trip == "Ep8pui8Vw2" || normalized.contains("ep8pui8vw2"))
        && !moderator
    {
        return Ok(Some(Rejection::Upload));
    }
    // Preserve the source PCRE's literal pipe members and ASCII /i behavior.
    // This fixed seven-byte pattern needs neither public regex nor native work.
    let banned = legacy_trip.as_bytes().windows(7).any(|value| {
        let value: [u8; 7] = std::array::from_fn(|index| value[index].to_ascii_lowercase());
        b"l|i1".contains(&value[0])
            && b"o|0".contains(&value[1])
            && b"l|i1".contains(&value[2])
            && b"l|i1".contains(&value[3])
            && value[4] == b'c'
            && b"o|0".contains(&value[5])
            && b"n|m".contains(&value[6])
    });
    Ok(banned.then_some(Rejection::BannedText))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_name_trip_checks_match_the_selected_original_body() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/name-trip-admission.json"))
                .unwrap();
        assert_eq!(fixture["extractor_icu"], "74.2");
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 352);
        for case in cases {
            let name = case["name"].as_str().unwrap();
            let trip = case["trip"].as_str().unwrap();
            let actual = evaluate(name, trip, case["moderator"].as_bool().unwrap()).unwrap();
            let expected = match case["outcome"].as_str().unwrap() {
                "allow" => None,
                "upload" => Some(Rejection::Upload),
                "banned" => Some(Rejection::BannedText),
                value => panic!("Unexpected source outcome: {value}"),
            };
            assert_eq!(actual, expected, "name={name:?} trip={trip:?}");
        }
    }

    #[test]
    fn fixed_checks_retain_normalization_and_derived_trip_work_bounds() {
        assert_eq!(
            evaluate("moot", &"x".repeat(65), false),
            Err(NormalizationError::InputTooLarge)
        );
        assert_eq!(
            evaluate(
                &"x".repeat(crate::admission_normalization::MAX_INPUT_BYTES + 1),
                "",
                false
            ),
            Err(NormalizationError::InputTooLarge)
        );
        assert_eq!(
            evaluate(&"ﷺ".repeat(4096), "", false),
            Err(NormalizationError::WorkTooLarge)
        );
        for moderator in [false, true] {
            assert_eq!(
                evaluate("\u{10000}\u{309d}", "", moderator),
                Err(NormalizationError::InvalidOutput)
            );
        }
    }
}
