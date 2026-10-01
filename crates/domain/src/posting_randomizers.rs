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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiceRequest {
    pub count: u8,
    pub sides: u32,
    pub modifier: Option<i64>,
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
    let count = parse_clamped_count(&input[count_start..cursor])?;
    cursor += 1;
    let side_start = cursor;
    while input.get(cursor).is_some_and(u8::is_ascii_digit) {
        cursor += 1;
    }
    if cursor == side_start {
        return Ok(None);
    }
    let sides = parse_positive_u32(&input[side_start..cursor], MAX_DICE_SIDES)?;
    let modifier = parse_modifier(input, cursor)?;
    Ok(Some(DiceRequest {
        count,
        sides,
        modifier,
    }))
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
        let magnitude = parse_positive_i64_allow_zero(&input[number_start..number_end])?;
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
            value = value
                .checked_neg()
                .ok_or(ValidationError("Dice modifier is too large."))?;
        }
        return Ok(Some(value));
    }
    Ok(None)
}

fn parse_positive_i64_allow_zero(digits: &[u8]) -> Result<i64, ValidationError> {
    let mut value = 0i64;
    for digit in digits {
        value = value
            .checked_mul(10)
            .and_then(|value| value.checked_add(i64::from(*digit - b'0')))
            .ok_or(ValidationError("Dice modifier is too large."))?;
    }
    Ok(value)
}

pub fn generate(request: &Request) -> Result<Outcome, ()> {
    let random = SystemRandom::new();
    generate_with(request, |upper| random_below(&random, upper))
}

fn random_below(random: &dyn SecureRandom, upper: u32) -> Result<u32, ()> {
    let zone = u32::MAX - (u32::MAX % upper);
    loop {
        let mut bytes = [0u8; 4];
        random.fill(&mut bytes).map_err(|_| ())?;
        let value = u32::from_le_bytes(bytes);
        if value < zone {
            return Ok(value % upper);
        }
    }
}

fn generate_with(
    request: &Request,
    mut below: impl FnMut(u32) -> Result<u32, ()>,
) -> Result<Outcome, ()> {
    match request {
        Request::Fortune => {
            let index = below(FORTUNES.len() as u32)? as usize;
            Ok(Outcome::Fortune {
                text: FORTUNES[index],
                color: fortune_color(index),
            })
        }
        Request::Dice(dice) => {
            let mut values = Vec::with_capacity(usize::from(dice.count));
            let mut total = 0i64;
            for _ in 0..dice.count {
                let value = i64::from(below(dice.sides)? + 1);
                values.push(value);
                total += value;
            }
            let modifier = dice.modifier.map(|value| {
                total += value;
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
                    .map(i64::to_string)
                    .collect::<Vec<_>>()
                    .join(", "),
                modifier.as_deref().unwrap_or("")
            );
            if dice.count > 1 {
                text.push_str(&format!(" = {total}"));
            }
            text.push_str(&format!(
                " ({}d{}{})",
                dice.count,
                dice.sides,
                modifier.as_deref().unwrap_or("")
            ));
            Ok(Outcome::Dice(text))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_option_grammar_and_board_guards_are_preserved() {
        assert_eq!(
            request("fortune", false, true).unwrap(),
            Some(Request::Fortune)
        );
        assert_eq!(request("Fortune", false, true).unwrap(), None);
        assert_eq!(request("fortune", false, false).unwrap(), None);
        assert_eq!(
            request("xxdice+2d6+3yy", true, false).unwrap(),
            Some(Request::Dice(DiceRequest {
                count: 2,
                sides: 6,
                modifier: Some(3)
            }))
        );
        assert_eq!(
            request("dice+2d6-3", true, false).unwrap(),
            Some(Request::Dice(DiceRequest {
                count: 2,
                sides: 6,
                modifier: Some(3)
            }))
        );
        assert_eq!(
            request("dice+2d6 -3", true, false).unwrap(),
            Some(Request::Dice(DiceRequest {
                count: 2,
                sides: 6,
                modifier: Some(-3)
            }))
        );
        assert_eq!(
            request("dice+99d20", true, false).unwrap(),
            Some(Request::Dice(DiceRequest {
                count: 25,
                sides: 20,
                modifier: None
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
    }

    #[test]
    fn outcomes_keep_source_text_shape_and_stable_standard_fortunes() {
        let dice = Request::Dice(DiceRequest {
            count: 3,
            sides: 6,
            modifier: Some(-2),
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
        });
        assert_eq!(
            generate_with(&one, |_| Ok(9)).unwrap(),
            Outcome::Dice("Rolled 10 + 4 (1d20 + 4)".into())
        );
        assert_eq!(fortune_color(0), "#7fec11");
        assert_eq!(fortune_color(12), "#43fd3b");
        assert_eq!(
            generate_with(&Request::Fortune, |_| Ok(10)).unwrap(),
            Outcome::Fortune {
                text: "ｷﾀ━━━━━━(ﾟ∀ﾟ)━━━━━━ !!!!",
                color: "#00cbb0".into()
            }
        );
    }
}
