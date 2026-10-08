use ring::rand::{SecureRandom, SystemRandom};

use crate::{MAX_PUBLIC_FIELD_BYTES, ValidationError};

pub const MAX_DICE_ROLLS: u8 = 25;
pub const MAX_DICE_SIDES: u32 = i32::MAX as u32;

const FORTUNES: [&str; 13] = [
    "Bad Luck",
    "Average Luck",
    "Good Luck",
    "Excellent Luck",
    "Reply hazy, try again",
    "Godly Luck",
    "Very Bad Luck",
    "Outlook good",
    "Better not tell you now",
    "You will meet a dark handsome stranger",
    "ｷﾀ━━━━━━(ﾟ∀ﾟ)━━━━━━ !!!!",
    "（　´_ゝ`）ﾌｰﾝ ",
    "Good news will come to you by mail",
];
const FORTUNE_COLORS: [&str; 13] = [
    "#7fec11", "#bac200", "#e7890c", "#fd4d32", "#f51c6a", "#d302a7", "#9d05da", "#6023f8",
    "#2a56fb", "#0893e1", "#00cbb0", "#16f174", "#43fd3b",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiceRequest {
    pub count: u8,
    pub sides: u32,
    pub modifier: Option<i64>,
    /// Source `min(25, $match[1])` retains captured digits below the cap.
    pub count_text: String,
    /// Source interpolates the captured digits, including leading zeroes.
    pub sides_text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Dice(DiceRequest),
    Fortune,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Dice(String),
    Fortune { text: &'static str, color: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GenerationError {
    InvalidRequest,
    RandomnessUnavailable,
}

/// Parse only features enabled by the locked board row. The source checks the
/// exact lower-case `fortune` option and searches the case-sensitive dice
/// expression without anchoring it to the whole options field.
pub fn request(
    value: &str,
    dice_enabled: bool,
    fortune_enabled: bool,
) -> Result<Option<Request>, ValidationError> {
    if value.len() > MAX_PUBLIC_FIELD_BYTES {
        return Err(ValidationError("Options must contain at most 100 bytes."));
    }
    let normalized = crate::posting_options::without_sage(value);
    let value = normalized.as_str();
    if fortune_enabled && value == "fortune" {
        return Ok(Some(Request::Fortune));
    }
    if dice_enabled {
        for (start, _) in value.match_indices("dice") {
            if let Some(dice) = parse_dice_at(value.as_bytes(), start)? {
                return Ok(Some(Request::Dice(dice)));
            }
        }
    }
    Ok(None)
}

fn parse_dice_at(input: &[u8], start: usize) -> Result<Option<DiceRequest>, ValidationError> {
    let mut cursor = start + 4;
    if !matches!(input.get(cursor), Some(b' ' | b'+')) {
        return Ok(None);
    }
    cursor += 1;
    let count_start = cursor;
    while input.get(cursor).is_some_and(u8::is_ascii_digit) {
        cursor += 1;
    }
    if cursor == count_start || !matches!(input.get(cursor), Some(b' ' | b'd' | b'+')) {
        return Ok(None);
    }
    let count_end = cursor;
    cursor += 1;
    let side_start = cursor;
    while input.get(cursor).is_some_and(u8::is_ascii_digit) {
        cursor += 1;
    }
    if cursor == side_start {
        return Ok(None);
    }
    // Do not reject a count until the complete mandatory expression matches.
    // PCRE skips incomplete candidates and can find a later valid expression.
    let count = parse_clamped_count(&input[count_start..count_end])?;
    let sides = parse_positive_u32(&input[side_start..cursor], MAX_DICE_SIDES)?;
    let modifier = parse_modifier(input, cursor)?;
    Ok(Some(DiceRequest {
        count,
        sides,
        modifier,
        count_text: if count == MAX_DICE_ROLLS {
            count.to_string()
        } else {
            ascii_digits(&input[count_start..count_end])
        },
        sides_text: ascii_digits(&input[side_start..cursor]),
    }))
}

fn ascii_digits(digits: &[u8]) -> String {
    digits.iter().map(|digit| char::from(*digit)).collect()
}

fn valid_dice_request(dice: &DiceRequest) -> bool {
    if !(1..=MAX_DICE_ROLLS).contains(&dice.count)
        || !(1..=MAX_DICE_SIDES).contains(&dice.sides)
        || dice.count_text.is_empty()
        || dice.sides_text.is_empty()
        || dice.count_text.len() > MAX_PUBLIC_FIELD_BYTES
        || dice.sides_text.len() > MAX_PUBLIC_FIELD_BYTES
        || !dice.count_text.bytes().all(|byte| byte.is_ascii_digit())
        || !dice.sides_text.bytes().all(|byte| byte.is_ascii_digit())
        || dice.count_text.parse::<u8>() != Ok(dice.count)
        || dice.sides_text.parse::<u32>() != Ok(dice.sides)
        || (dice.count == MAX_DICE_ROLLS && dice.count_text != MAX_DICE_ROLLS.to_string())
    {
        return false;
    }
    // Require a spelling that can originate in a bounded options field. A
    // negative modifier needs two sign bytes because a lone '-' means '+'.
    let modifier_bytes = dice.modifier.map_or(0, |value| {
        value.unsigned_abs().to_string().len() + if value < 0 { 2 } else { 1 }
    });
    6 + dice.count_text.len() + dice.sides_text.len() + modifier_bytes <= MAX_PUBLIC_FIELD_BYTES
}

fn parse_clamped_count(digits: &[u8]) -> Result<u8, ValidationError> {
    if digits.iter().all(|digit| *digit == b'0') {
        return Err(ValidationError("Dice roll count must be at least one."));
    }
    let mut value = 0u8;
    for digit in digits {
        value = value
            .saturating_mul(10)
            .saturating_add(digit.saturating_sub(b'0'));
        if value >= MAX_DICE_ROLLS {
            return Ok(MAX_DICE_ROLLS);
        }
    }
    Ok(value)
}

fn parse_positive_u32(digits: &[u8], maximum: u32) -> Result<u32, ValidationError> {
    let mut value = 0u32;
    for digit in digits {
        value = value
            .checked_mul(10)
            .and_then(|value| value.checked_add(u32::from(*digit - b'0')))
            .ok_or(ValidationError("Dice side count is too large."))?;
        if value > maximum {
            return Err(ValidationError("Dice side count is too large."));
        }
    }
    if value == 0 {
        return Err(ValidationError("Dice must have at least one side."));
    }
    Ok(value)
}

/// PCRE makes `[ +-]+?` non-greedy and lets the following `-?\d+` consume a
/// leading minus. Preserve that split, including the source's `strpos(...) > 0`
/// quirk where `dice+2d6-3` displays an addition while `dice+2d6 -3` subtracts.
fn parse_modifier(input: &[u8], start: usize) -> Result<Option<i64>, ValidationError> {
    if !matches!(input.get(start), Some(b' ' | b'+' | b'-')) {
        return Ok(None);
    }
    let mut signs_end = start;
    while matches!(input.get(signs_end), Some(b' ' | b'+' | b'-')) {
        signs_end += 1;
    }
    for split in start + 1..=signs_end {
        let mut number_start = split;
        let negative_capture = input.get(number_start) == Some(&b'-');
        if negative_capture {
            number_start += 1;
        }
        let mut number_end = number_start;
        while input.get(number_end).is_some_and(u8::is_ascii_digit) {
            number_end += 1;
        }
        if number_end == number_start {
            continue;
        }
        let magnitude = parse_modifier_magnitude(&input[number_start..number_end])?;
        let mut value = if negative_capture {
            -magnitude
        } else {
            magnitude
        };
        if input[start..split]
            .iter()
            .position(|byte| *byte == b'-')
            .is_some_and(|position| position > 0)
        {
            value = -value;
        }
        return i64::try_from(value)
            .map(Some)
            .map_err(|_| ValidationError("Dice modifier is too large."));
    }
    Ok(None)
}

fn parse_modifier_magnitude(digits: &[u8]) -> Result<i128, ValidationError> {
    let mut value = 0i128;
    for digit in digits {
        value = value
            .checked_mul(10)
            .and_then(|value| value.checked_add(i128::from(*digit - b'0')))
            .ok_or(ValidationError("Dice modifier is too large."))?;
        if value > i128::from(i64::MAX) + 1 {
            return Err(ValidationError("Dice modifier is too large."));
        }
    }
    Ok(value)
}

pub fn generate(request: &Request) -> Result<Outcome, GenerationError> {
    let random = SystemRandom::new();
    generate_with(request, |upper| random_below(&random, upper))
}

fn random_below(random: &dyn SecureRandom, upper: u32) -> Result<u32, GenerationError> {
    if upper == 0 {
        return Err(GenerationError::InvalidRequest);
    }
    let zone = u32::MAX - (u32::MAX % upper);
    for _ in 0..128 {
        let mut bytes = [0u8; 4];
        random
            .fill(&mut bytes)
            .map_err(|_| GenerationError::RandomnessUnavailable)?;
        let value = u32::from_le_bytes(bytes);
        if value < zone {
            return Ok(value % upper);
        }
    }
    Err(GenerationError::RandomnessUnavailable)
}

fn generate_with(
    request: &Request,
    mut below: impl FnMut(u32) -> Result<u32, GenerationError>,
) -> Result<Outcome, GenerationError> {
    match request {
        Request::Fortune => {
            let index = below(FORTUNES.len() as u32)? as usize;
            if index >= FORTUNES.len() {
                return Err(GenerationError::RandomnessUnavailable);
            }
            Ok(Outcome::Fortune {
                text: FORTUNES[index],
                color: fortune_color(index),
            })
        }
        Request::Dice(dice) => {
            if !valid_dice_request(dice) {
                return Err(GenerationError::InvalidRequest);
            }
            let mut values = Vec::with_capacity(usize::from(dice.count));
            let mut total = 0i128;
            for _ in 0..dice.count {
                let draw = below(dice.sides)?;
                if draw >= dice.sides {
                    return Err(GenerationError::RandomnessUnavailable);
                }
                let value = i128::from(draw) + 1;
                values.push(value);
                total += value;
            }
            let modifier = dice.modifier.map(|value| {
                total += i128::from(value);
                if value >= 0 {
                    format!(" + {value}")
                } else {
                    format!(" - {}", value.unsigned_abs())
                }
            });
            let mut text = format!(
                "Rolled {}{}",
                values
                    .iter()
                    .map(i128::to_string)
                    .collect::<Vec<_>>()
                    .join(", "),
                modifier.as_deref().unwrap_or("")
            );
            if dice.count > 1 {
                text.push_str(&format!(" = {total}"));
            }
            text.push_str(&format!(
                " ({}d{}{})",
                dice.count_text,
                dice.sides_text,
                modifier.as_deref().unwrap_or("")
            ));
            Ok(Outcome::Dice(text))
        }
    }
}

/// Public Outcome variants are constructible by callers. Validate the complete
/// generated value before using it in a source comparison projection.
pub(crate) fn valid_outcome(outcome: &Outcome) -> bool {
    match outcome {
        Outcome::Fortune { text, color } => FORTUNES
            .iter()
            .zip(FORTUNE_COLORS)
            .any(|(expected, expected_color)| text == expected && color == expected_color),
        Outcome::Dice(text) => {
            if text.len() > 1024 {
                return false;
            }
            let Some((body, expression)) = text
                .strip_prefix("Rolled ")
                .and_then(|text| text.strip_suffix(')'))
                .and_then(|text| text.rsplit_once(" ("))
            else {
                return false;
            };
            let fields: Vec<_> = expression.split(' ').collect();
            if fields.len() != 1 && fields.len() != 3 {
                return false;
            }
            let Some((count_text, sides_text)) = fields[0].split_once('d') else {
                return false;
            };
            let (Ok(count), Ok(sides)) = (count_text.parse::<u8>(), sides_text.parse::<u32>())
            else {
                return false;
            };
            let modifier = if fields.len() == 3 {
                if !matches!(fields[1], "+" | "-") {
                    return false;
                }
                let Ok(value) = format!("{}{}", fields[1], fields[2]).parse::<i64>() else {
                    return false;
                };
                Some(value)
            } else {
                None
            };
            let mut values = body.split(", ");
            let request = Request::Dice(DiceRequest {
                count,
                sides,
                modifier,
                count_text: count_text.into(),
                sides_text: sides_text.into(),
            });
            let generated = generate_with(&request, |_| {
                let value = values.next().ok_or(GenerationError::InvalidRequest)?;
                let digits: String = value.chars().take_while(char::is_ascii_digit).collect();
                digits
                    .parse::<u32>()
                    .ok()
                    .and_then(|value| value.checked_sub(1))
                    .ok_or(GenerationError::InvalidRequest)
            });
            generated.as_ref() == Ok(outcome)
        }
    }
}

fn fortune_color(index: usize) -> String {
    let angle = 2.0 * std::f64::consts::PI * index as f64 / FORTUNES.len() as f64;
    let component = |offset: f64| (127.0 + 127.0 * (angle + offset).sin()) as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        component(0.0),
        component(2.0 / 3.0 * std::f64::consts::PI),
        component(4.0 / 3.0 * std::f64::consts::PI)
    )
}

pub fn fortune_class(color: &str) -> Option<&'static str> {
    FORTUNE_COLORS
        .iter()
        .position(|candidate| *candidate == color)
        .map(|index| match index {
            0 => "fortune fortune-0",
            1 => "fortune fortune-1",
            2 => "fortune fortune-2",
            3 => "fortune fortune-3",
            4 => "fortune fortune-4",
            5 => "fortune fortune-5",
            6 => "fortune fortune-6",
            7 => "fortune fortune-7",
            8 => "fortune fortune-8",
            9 => "fortune fortune-9",
            10 => "fortune fortune-10",
            11 => "fortune fortune-11",
            _ => "fortune fortune-12",
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn retained_dice_and_fortunes_match_the_extracted_source_vectors() {
        let reference: serde_json::Value =
            serde_json::from_str(include_str!("../../../fixtures/randomizer-reference.json"))
                .unwrap();
        for case in reference["dice_cases"].as_array().unwrap() {
            let parsed = request(case["options"].as_str().unwrap(), true, false).unwrap();
            if case["dice"].is_null() {
                assert_eq!(parsed, None);
            } else {
                let expected = Outcome::Dice(case["dice"].as_str().unwrap().into());
                assert_eq!(generate(&parsed.unwrap()).unwrap(), expected);
                assert!(valid_outcome(&expected), "{case}");
            }
        }
        for case in reference["bounded_rejections"].as_array().unwrap() {
            assert!(
                request(case["options"].as_str().unwrap(), true, false).is_err(),
                "{case}"
            );
        }
        for (index, entry) in reference["fortunes"].as_array().unwrap().iter().enumerate() {
            let expected = Outcome::Fortune {
                text: FORTUNES[index],
                color: fortune_color(index),
            };
            assert_eq!(FORTUNES[index], entry["text"]);
            assert_eq!(fortune_color(index), entry["color"]);
            assert_eq!(
                generate_with(&Request::Fortune, |_| Ok(index as u32)).unwrap(),
                expected
            );
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]
        #[test]
        fn arbitrary_options_never_create_unbounded_requests(value in ".{0,130}") {
            let parsed = request(&value, true, true);
            if value.len() > MAX_PUBLIC_FIELD_BYTES { prop_assert!(parsed.is_err()); }
            if let Ok(Some(Request::Dice(dice))) = parsed {
                prop_assert!((1..=MAX_DICE_ROLLS).contains(&dice.count));
                prop_assert!((1..=MAX_DICE_SIDES).contains(&dice.sides));
                prop_assert!(valid_dice_request(&dice));
            }
        }

        #[test]
        fn bounded_digit_spellings_survive_generation_and_outcome_validation(
            count in 1u8..=60,
            sides in 1u32..=MAX_DICE_SIDES,
            count_zeroes in 0usize..40,
            side_zeroes in 0usize..40,
        ) {
            let count_text = format!("{}{count}", "0".repeat(count_zeroes));
            let sides_text = format!("{}{sides}", "0".repeat(side_zeroes));
            let input = format!("dice+{count_text}d{sides_text}");
            let parsed = request(&input, true, false).unwrap().unwrap();
            let Request::Dice(dice) = &parsed else { unreachable!() };
            prop_assert_eq!(&dice.sides_text, &sides_text);
            prop_assert_eq!(&dice.count_text, &if count >= MAX_DICE_ROLLS {
                MAX_DICE_ROLLS.to_string()
            } else {
                count_text
            });
            let outcome = generate_with(&parsed, |_| Ok(0)).unwrap();
            prop_assert!(valid_outcome(&outcome));
        }
    }

    #[test]
    fn forged_digit_spellings_fail_before_randomness() {
        let Request::Dice(valid) = request("dice+01d001", true, false).unwrap().unwrap() else {
            unreachable!()
        };
        for (count_text, sides_text) in [
            ("", "1"),
            ("1", ""),
            ("+1", "1"),
            ("1", "+1"),
            ("1", "1</b>"),
            ("１", "1"),
            ("1", "１"),
            ("0", "1"),
            ("1", "0"),
            ("2", "1"),
            ("1", "2"),
        ] {
            let mut forged = valid.clone();
            forged.count_text = count_text.into();
            forged.sides_text = sides_text.into();
            assert_eq!(
                generate_with(&Request::Dice(forged), |_| panic!("RNG must not run")),
                Err(GenerationError::InvalidRequest)
            );
        }

        let mut overlong = valid.clone();
        overlong.count_text = format!("{}1", "0".repeat(93));
        assert_eq!(
            generate_with(&Request::Dice(overlong), |_| panic!("RNG must not run")),
            Err(GenerationError::InvalidRequest)
        );
        assert!(!valid_outcome(&Outcome::Dice(format!(
            "Rolled 1 (1d{}1)",
            "0".repeat(93)
        ))));

        let Request::Dice(mut capped) = request("dice+25d01", true, false).unwrap().unwrap() else {
            unreachable!()
        };
        capped.count_text = "025".into();
        assert_eq!(
            generate_with(&Request::Dice(capped), |_| panic!("RNG must not run")),
            Err(GenerationError::InvalidRequest)
        );
    }

    #[test]
    fn generated_outcome_validation_checks_the_complete_text() {
        for text in [
            "Rolled 1 (01d001)",
            "Rolled 1 + 0 (01d001 + 0)",
            "Rolled 1, 1 - 3 = -1 (02d001 - 3)",
        ] {
            assert!(valid_outcome(&Outcome::Dice(text.into())), "{text}");
        }
        for text in [
            "Rolled 1 (d1)",
            "Rolled 1 (1d)",
            "Rolled 1 (+1d1)",
            "Rolled 1 (1d+1)",
            "Rolled 1 (1d1</b>)",
            "Rolled 1 (１d1)",
            "Rolled 1 (1d１)",
            "Rolled 1 (0d1)",
            "Rolled 1 (1d0)",
            "Rolled 01 (01d001)",
            "Rolled 0 (01d001)",
            "Rolled 2 (01d001)",
            "Rolled 1 - 0 (01d001 - 0)",
            "Rolled 1 + 00 (01d001 + 00)",
            "Rolled 1 + 3 (01d001 + 2)",
            "Rolled 1, 1 = 3 (02d001)",
            "Rolled 1, 1 (01d001)",
            "Rolled 1 (01d001) trailing",
            "Rolled 1 (01d001\n)",
        ] {
            assert!(!valid_outcome(&Outcome::Dice(text.into())), "{text}");
        }
    }

    #[test]
    fn source_option_grammar_and_board_guards_are_preserved() {
        assert_eq!(
            request("fortune", false, true).unwrap(),
            Some(Request::Fortune)
        );
        assert_eq!(request("Fortune", false, true).unwrap(), None);
        assert_eq!(
            request("SaGefortunesage", false, true).unwrap(),
            Some(Request::Fortune)
        );
        assert_eq!(request("fortune sage", false, true).unwrap(), None);
        assert_eq!(request("fortune", false, false).unwrap(), None);
        assert_eq!(
            request("xxdice+2d6+3yy", true, false).unwrap(),
            Some(Request::Dice(DiceRequest {
                count: 2,
                sides: 6,
                modifier: Some(3),
                count_text: "2".into(),
                sides_text: "6".into(),
            }))
        );
        assert_eq!(
            request("dice+2d6-3", true, false).unwrap(),
            Some(Request::Dice(DiceRequest {
                count: 2,
                sides: 6,
                modifier: Some(3),
                count_text: "2".into(),
                sides_text: "6".into(),
            }))
        );
        assert_eq!(
            request("dice+2d6 -3", true, false).unwrap(),
            Some(Request::Dice(DiceRequest {
                count: 2,
                sides: 6,
                modifier: Some(-3),
                count_text: "2".into(),
                sides_text: "6".into(),
            }))
        );
        assert_eq!(
            request("dice+99d20", true, false).unwrap(),
            Some(Request::Dice(DiceRequest {
                count: 25,
                sides: 20,
                modifier: None,
                count_text: "25".into(),
                sides_text: "20".into(),
            }))
        );
        assert_eq!(request("dice+2d6", false, false).unwrap(), None);
    }

    #[test]
    fn invalid_zero_and_unbounded_values_fail_without_random_generation() {
        assert!(request("dice+0d6", true, false).is_err());
        assert!(request("dice+2d0", true, false).is_err());
        assert!(request("dice+2d2147483648", true, false).is_err());
        assert!(request("dice+2d6+999999999999999999999", true, false).is_err());
        assert!(request(&"x".repeat(101), true, true).is_err());
        assert!(request("dice+2d1+9223372036854775808", true, false).is_err());
        assert_eq!(
            request("dice+2d1 -9223372036854775808", true, false).unwrap(),
            Some(Request::Dice(DiceRequest {
                count: 2,
                sides: 1,
                modifier: Some(i64::MIN),
                count_text: "2".into(),
                sides_text: "1".into(),
            }))
        );
    }

    #[test]
    fn generator_validates_public_requests_and_handles_numeric_boundaries() {
        for dice in [
            DiceRequest {
                count: 0,
                sides: 6,
                modifier: None,
                count_text: "0".into(),
                sides_text: "6".into(),
            },
            DiceRequest {
                count: MAX_DICE_ROLLS + 1,
                sides: 6,
                modifier: None,
                count_text: (MAX_DICE_ROLLS + 1).to_string(),
                sides_text: "6".into(),
            },
            DiceRequest {
                count: 1,
                sides: 0,
                modifier: None,
                count_text: "1".into(),
                sides_text: "0".into(),
            },
            DiceRequest {
                count: 1,
                sides: MAX_DICE_SIDES + 1,
                modifier: None,
                count_text: "1".into(),
                sides_text: (MAX_DICE_SIDES + 1).to_string(),
            },
        ] {
            assert_eq!(
                generate_with(&Request::Dice(dice), |_| panic!("RNG must not run")),
                Err(GenerationError::InvalidRequest)
            );
        }

        let request = request("dice+2d6+9223372036854775807", true, false)
            .unwrap()
            .unwrap();
        let mut draws = [0, 0].into_iter();
        assert_eq!(
            generate_with(&request, |_| Ok(draws.next().unwrap())).unwrap(),
            Outcome::Dice(
                "Rolled 1, 1 + 9223372036854775807 = 9223372036854775809 (2d6 + 9223372036854775807)"
                    .into()
            )
        );
    }

    #[test]
    fn generator_surfaces_rng_failures_without_partial_results() {
        assert_eq!(
            generate_with(&Request::Fortune, |_| Err(
                GenerationError::RandomnessUnavailable
            )),
            Err(GenerationError::RandomnessUnavailable)
        );
        assert_eq!(
            generate_with(
                &Request::Dice(DiceRequest {
                    count: 1,
                    sides: 6,
                    modifier: None,
                    count_text: "1".into(),
                    sides_text: "6".into(),
                }),
                |_| Err(GenerationError::RandomnessUnavailable)
            ),
            Err(GenerationError::RandomnessUnavailable)
        );
        assert_eq!(
            generate_with(
                &Request::Dice(DiceRequest {
                    count: 1,
                    sides: 6,
                    modifier: None,
                    count_text: "1".into(),
                    sides_text: "6".into(),
                }),
                |_| Ok(6)
            ),
            Err(GenerationError::RandomnessUnavailable)
        );
    }

    #[test]
    fn outcomes_keep_source_text_shape_and_stable_standard_fortunes() {
        let dice = Request::Dice(DiceRequest {
            count: 3,
            sides: 6,
            modifier: Some(-2),
            count_text: "3".into(),
            sides_text: "6".into(),
        });
        let mut draws = [0, 2, 5].into_iter();
        assert_eq!(
            generate_with(&dice, |_| Ok(draws.next().unwrap())).unwrap(),
            Outcome::Dice("Rolled 1, 3, 6 - 2 = 8 (3d6 - 2)".into())
        );
        let one = Request::Dice(DiceRequest {
            count: 1,
            sides: 20,
            modifier: Some(4),
            count_text: "1".into(),
            sides_text: "20".into(),
        });
        assert_eq!(
            generate_with(&one, |_| Ok(9)).unwrap(),
            Outcome::Dice("Rolled 10 + 4 (1d20 + 4)".into())
        );
        assert_eq!(fortune_color(0), "#7fec11");
        assert_eq!(fortune_color(12), "#43fd3b");
        for (index, color) in FORTUNE_COLORS.iter().enumerate() {
            assert_eq!(fortune_color(index), *color);
            assert_eq!(
                fortune_class(color),
                Some(match index {
                    0 => "fortune fortune-0",
                    1 => "fortune fortune-1",
                    2 => "fortune fortune-2",
                    3 => "fortune fortune-3",
                    4 => "fortune fortune-4",
                    5 => "fortune fortune-5",
                    6 => "fortune fortune-6",
                    7 => "fortune fortune-7",
                    8 => "fortune fortune-8",
                    9 => "fortune fortune-9",
                    10 => "fortune fortune-10",
                    11 => "fortune fortune-11",
                    _ => "fortune fortune-12",
                })
            );
        }
        assert_eq!(fortune_class("#ffffff"), None);
        assert_eq!(
            generate_with(&Request::Fortune, |_| Ok(10)).unwrap(),
            Outcome::Fortune {
                text: "ｷﾀ━━━━━━(ﾟ∀ﾟ)━━━━━━ !!!!",
                color: "#00cbb0".into()
            }
        );
    }
}
