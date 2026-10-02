//! Audited built-in word transformations. These are not admission classifiers.

use ring::rand::{SecureRandom, SystemRandom};

use crate::ValidationError;

#[path = "wordfilter_unicode.rs"]
mod unicode;

pub const MAX_INPUT_BYTES: usize = 131_072;
pub const MAX_OUTPUT_BYTES: usize = 524_288;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    Global,
    Basic,
    Asp,
    Video,
    Test,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Comment,
    Subject,
    Name,
}

/// Two server-owned choices, sampled once and retained with the saved result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeetRolls([u8; 2]);

impl LeetRolls {
    pub fn from_choices(first: u8, second: u8) -> Result<Self, ValidationError> {
        if first > 5 || second > 5 {
            return Err(ValidationError("Invalid wordfilter random choices."));
        }
        Ok(Self([first, second]))
    }

    pub fn choices(self) -> [u8; 2] {
        self.0
    }

    pub fn generate() -> Result<Self, ValidationError> {
        let random = SystemRandom::new();
        Self::generate_with(|bytes| random.fill(bytes).map_err(|_| ()))
    }

    fn generate_with(
        mut fill: impl FnMut(&mut [u8]) -> Result<(), ()>,
    ) -> Result<Self, ValidationError> {
        let mut choices = [0; 2];
        for choice in &mut choices {
            let mut selected = false;
            for _ in 0..64 {
                let mut byte = [0];
                fill(&mut byte)
                    .map_err(|_| ValidationError("Wordfilter randomness is unavailable."))?;
                if byte[0] < 252 {
                    *choice = byte[0] % 6;
                    selected = true;
                    break;
                }
            }
            if !selected {
                return Err(ValidationError("Wordfilter randomness is unavailable."));
            }
        }
        Ok(Self(choices))
    }
}

/// Input is the bounded source field at the audited call site. Do not apply
/// transformations when rendering an existing post or reading its JSON.
pub fn apply(
    input: &str,
    field: Field,
    profile: Profile,
    rolls: Option<LeetRolls>,
) -> Result<String, ValidationError> {
    check_input(input)?;
    if field != Field::Comment {
        return Ok(input.to_owned());
    }
    let mut text = input.replace("CUCK", "KEK");
    if profile == Profile::Asp {
        text = text
            .replace("finna", "ding-dong diddly")
            .replace("Finna ", "Ding-Dong Diddly")
            .replace("FINNA", "DING-DONG DIDDLY");
    }
    text = common(&text);
    if profile != Profile::Basic {
        text = soy(&text);
    }
    if profile == Profile::Video {
        for (from, to) in CONSOLES {
            text = text.replace(from, to);
        }
    }
    if profile == Profile::Test {
        text = leet(
            &text,
            rolls.ok_or(ValidationError("Wordfilter randomness is unavailable."))?,
        )?;
    }
    if text.len() > MAX_OUTPUT_BYTES {
        return Err(ValidationError("Wordfilter output is too large."));
    }
    Ok(text)
}

fn check_input(input: &str) -> Result<(), ValidationError> {
    if input.len() > MAX_INPUT_BYTES {
        return Err(ValidationError("Wordfilter input is too large."));
    }
    Ok(())
}

fn common(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut position = 0;
    while position < input.len() {
        let ch = input[position..].chars().next().expect("remaining input");
        if ch.is_ascii_alphanumeric() || ch == '_' {
            let start = position;
            while input
                .as_bytes()
                .get(position)
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
            {
                position += 1;
            }
            let word = &input[start..position];
            output.push_str(match word {
                "smh" => "baka",
                "SMH" => "BAKA",
                "tbh" => "desu",
                "TBH" => "DESU",
                "fam" => "senpai",
                "FAM" => "SENPAI",
                "Fam" => "Senpai",
                "fams" => "senpaitachi",
                "FAMS" | "FAMs" => "SENPAITACHI",
                "Fams" => "Senpaitachi",
                _ => word,
            });
        } else {
            output.push(ch);
            position += ch.len_utf8();
        }
    }
    output
}

fn char_matches(class: &[(u32, u32)], ch: char) -> bool {
    let point = ch as u32;
    let after = class.partition_point(|&(start, _)| start <= point);
    after > 0 && class[after - 1].1 >= point
}

fn soy(input: &str) -> String {
    let chars: Vec<(usize, char)> = input.char_indices().collect();
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    let mut index = 0;
    while index < chars.len() {
        let (start, first) = chars[index];
        if !char_matches(unicode::S, first)
            || (index > 0 && char_matches(unicode::WORD, chars[index - 1].1))
        {
            index += 1;
            continue;
        }
        let o_start = index + 1;
        let mut y_index = o_start;
        while y_index < chars.len() && char_matches(unicode::O, chars[y_index].1) {
            y_index += 1;
        }
        if y_index == o_start
            || y_index == chars.len()
            || !char_matches(unicode::Y, chars[y_index].1)
        {
            index = y_index.max(index + 1);
            continue;
        }
        let suffix_index = y_index + 1;
        let mut end_index = suffix_index;
        let boundary = chars
            .get(suffix_index)
            .is_none_or(|(_, ch)| !char_matches(unicode::WORD, *ch));
        if !boundary {
            while end_index < chars.len()
                && end_index - suffix_index < 4
                && char_matches(unicode::ALPHA, chars[end_index].1)
            {
                end_index += 1;
            }
            if end_index - suffix_index < 2 {
                index = suffix_index;
                continue;
            }
        }
        let core_end = chars
            .get(suffix_index)
            .map_or(input.len(), |(byte, _)| *byte);
        let end = chars.get(end_index).map_or(input.len(), |(byte, _)| *byte);
        let core = &input[start..core_end];
        let suffix = &input[core_end..end];
        output.push_str(&input[cursor..start]);
        if suffix.eq_ignore_ascii_case("uz") {
            output.push_str(&input[start..end]);
        } else {
            let suffix = if suffix.eq_ignore_ascii_case("im") || suffix.eq_ignore_ascii_case("lent")
            {
                ""
            } else {
                suffix
            };
            let upper = !core.bytes().any(|byte| byte.is_ascii_lowercase());
            if suffix.len() < 2 {
                output.push_str(if upper {
                    "ONIONS"
                } else if first == 's' {
                    "onions"
                } else {
                    "Onions"
                });
            } else {
                output.push(if upper {
                    'B'
                } else if first == 's' {
                    'b'
                } else {
                    'B'
                });
                let count = y_index - o_start;
                for _ in 0..if count < 35 { count } else { 1 } {
                    output.push(if upper { 'A' } else { 'a' });
                }
                output.push_str(if upper { "SED" } else { "sed" });
            }
            output.push_str(suffix);
        }
        cursor = end;
        index = end_index;
    }
    output.push_str(&input[cursor..]);
    output
}

pub fn leet(input: &str, rolls: LeetRolls) -> Result<String, ValidationError> {
    check_input(input)?;
    let mut output = leet_once(input, rolls.0[0]);
    if rolls.0[0] != rolls.0[1] {
        output = leet_once(&output, rolls.0[1]);
    }
    Ok(output)
}

fn leet_once(input: &str, roll: u8) -> String {
    if roll == 5 {
        // Source /([^gl])[tT]/ consumes both characters. The replacement is
        // non-overlapping: 'ttt' becomes 't7t', and a leading t stays unchanged.
        let mut chars = input.chars().peekable();
        let mut output = String::with_capacity(input.len());
        while let Some(ch) = chars.next() {
            output.push(ch);
            if !matches!(ch, 'g' | 'l')
                && chars.peek().is_some_and(|next| matches!(next, 't' | 'T'))
            {
                chars.next();
                output.push('7');
            }
        }
        output
    } else {
        let (from, to) =
            [('a', '4'), ('e', '3'), ('i', '1'), ('o', '0'), ('s', '5')][roll as usize];
        input
            .chars()
            .map(|ch| {
                if ch.eq_ignore_ascii_case(&from) {
                    to
                } else {
                    ch
                }
            })
            .collect()
    }
}

const CONSOLES: &[(&str, &str)] = &[
    ("pcfat", "pcbro"),
    ("pcuck", "pcbro"),
    ("pccuck", "pcbro"),
    ("valvedrone", "pcbro"),
    ("sonynigger", "sonybro"),
    ("sonygger", "sonybro"),
    ("sonydrone", "sonybro"),
    ("sonycuck", "sonybro"),
    ("sonypony", "sonybro"),
    ("nintencuck", "nintenbro"),
    ("nintoddler", "nintenbro"),
    ("nintendotoddler", "nintenbro"),
    ("nintendrone", "nintenbro"),
    ("nintenyearold", "nintenbro"),
    ("nintendroid", "nintenbro"),
    ("nintenshit", "nintenbro"),
    ("Pcfat", "Pcbro"),
    ("Pcuck", "Pcbro"),
    ("PCuck", "PCbro"),
    ("Pccuck", "Pcbro"),
    ("Valvedrone", "Pcbro"),
    ("Sonynigger", "Sonybro"),
    ("Sonygger", "Sonybro"),
    ("Sonydrone", "Sonybro"),
    ("Sonycuck", "Sonybro"),
    ("Sonypony", "Sonybro"),
    ("Nintencuck", "Nintenbro"),
    ("Nintoddler", "Nintenbro"),
    ("Nintendotoddler", "Nintenbro"),
    ("Nintendrone", "Nintenbro"),
    ("Nintenyearold", "Nintenbro"),
    ("Nintendroid", "Nintenbro"),
    ("Nintenshit", "Nintenbro"),
    ("nintendr0ne", "nintenbro"),
    ("Nintendr0ne", "Nintenbro"),
    ("sonybrony", "sonybro"),
    ("Sonybrony", "Sonybro"),
    ("sonybronies", "sonybros"),
    ("Sonybronies", "Sonybros"),
    ("sonypony", "sonybro"),
    ("Sonypony", "Sonybro"),
    ("sonyponies", "sonybros"),
    ("Sonyponies", "Sonybros"),
    ("sonigger", "sonybro"),
    ("Sonigger", "Sonybro"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn fixed_source_classes_match_every_unicode_scalar() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/wordfilter-unicode-reference.json"
        ))
        .unwrap();
        for (name, actual) in [
            ("word", unicode::WORD),
            ("alpha", unicode::ALPHA),
            ("s", unicode::S),
            ("o", unicode::O),
            ("y", unicode::Y),
        ] {
            let expected: Vec<(u32, u32)> = fixture["ranges"][name]
                .as_array()
                .unwrap()
                .iter()
                .map(|range| {
                    (
                        range[0].as_u64().unwrap() as u32,
                        range[1].as_u64().unwrap() as u32,
                    )
                })
                .collect();
            assert!(
                expected
                    .iter()
                    .all(|&(start, end)| start <= end && end < 0x110000)
            );
            assert!(expected.windows(2).all(|pair| pair[0].1 < pair[1].0));
            let (mut range, mut matched, mut scalars) = (0, 0, 0);
            for point in 0..0x110000 {
                let Some(ch) = char::from_u32(point) else {
                    continue;
                };
                scalars += 1;
                while range < expected.len() && expected[range].1 < point {
                    range += 1;
                }
                let reference = expected
                    .get(range)
                    .is_some_and(|&(start, end)| start <= point && point <= end);
                assert_eq!(char_matches(actual, ch), reference, "{name} U+{point:04X}");
                matched += u64::from(reference);
            }
            assert_eq!(matched, fixture["counts"][name].as_u64().unwrap());
            assert_eq!(scalars, fixture["scalars"].as_u64().unwrap());
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 128, max_shrink_iters: 256, ..ProptestConfig::default() })]
        #[test]
        fn arbitrary_unicode_fields_preserve_valid_boundaries(
            chars in prop::collection::vec(any::<char>(), 0..512),
            first in 0u8..6,
            second in 0u8..6,
        ) {
            let input: String = chars.into_iter().collect();
            let rolls = LeetRolls::from_choices(first, second).unwrap();
            for profile in [Profile::Global, Profile::Basic, Profile::Asp, Profile::Video, Profile::Test] {
                let result = apply(&input, Field::Comment, profile, Some(rolls)).unwrap();
                prop_assert!(result.len() <= MAX_OUTPUT_BYTES);
            }
        }
    }

    #[test]
    fn all_extracted_pure_profiles_match() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../../fixtures/wordfilter-reference.json"))
                .unwrap();
        for (name, profile) in [
            ("global", Profile::Global),
            ("ck", Profile::Basic),
            ("int", Profile::Basic),
            ("asp", Profile::Asp),
            ("v", Profile::Video),
            ("vg", Profile::Global),
            ("vp", Profile::Global),
            ("test", Profile::Test),
        ] {
            for case in fixture["profiles"][name]["cases"].as_array().unwrap() {
                let field = match case["field"].as_str().unwrap() {
                    "com" => Field::Comment,
                    "sub" => Field::Subject,
                    "name" => Field::Name,
                    _ => panic!("fixture field"),
                };
                let actual = apply(
                    case["input"].as_str().unwrap(),
                    field,
                    profile,
                    Some(LeetRolls([0, 0])),
                )
                .unwrap();
                assert_eq!(
                    actual,
                    case["output"].as_str().unwrap(),
                    "{name}: {}",
                    case["input"]
                );
            }
        }
    }

    #[test]
    fn every_extracted_leet_choice_pair_matches() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../../fixtures/wordfilter-reference.json"))
                .unwrap();
        for case in fixture["profiles"]["test"]["leet"].as_array().unwrap() {
            let rolls = LeetRolls::from_choices(
                case["rolls"][0].as_u64().unwrap() as u8,
                case["rolls"][1].as_u64().unwrap() as u8,
            )
            .unwrap();
            assert_eq!(
                leet(case["input"].as_str().unwrap(), rolls).unwrap(),
                case["output"].as_str().unwrap(),
                "{case}"
            );
        }
    }

    #[test]
    fn bounds_and_server_choices_fail_closed() {
        assert!(LeetRolls::from_choices(6, 0).is_err());
        assert!(LeetRolls::from_choices(0, 255).is_err());
        assert!(apply("ordinary", Field::Comment, Profile::Test, None).is_err());
        assert_eq!(
            apply("ordinary", Field::Name, Profile::Test, None).unwrap(),
            "ordinary"
        );
        assert!(
            apply(
                &"x".repeat(MAX_INPUT_BYTES + 1),
                Field::Comment,
                Profile::Basic,
                None
            )
            .is_err()
        );
        let unmatched = format!("s{}!", "o".repeat(MAX_INPUT_BYTES - 2));
        assert_eq!(
            apply(&unmatched, Field::Comment, Profile::Global, None).unwrap(),
            unmatched
        );
        assert_eq!(
            apply(
                &"finna".repeat(MAX_INPUT_BYTES / 5),
                Field::Comment,
                Profile::Asp,
                None
            )
            .unwrap()
            .len(),
            (MAX_INPUT_BYTES / 5) * 16
        );
    }

    #[test]
    fn entropy_failure_and_rejection_are_bounded() {
        assert!(LeetRolls::generate_with(|_| Err(())).is_err());
        let mut attempts = 0;
        assert!(
            LeetRolls::generate_with(|bytes| {
                attempts += 1;
                bytes[0] = 255;
                Ok(())
            })
            .is_err()
        );
        assert_eq!(attempts, 64);
        let mut samples = [251, 252, 6].into_iter();
        assert_eq!(
            LeetRolls::generate_with(|bytes| {
                bytes[0] = samples.next().unwrap();
                Ok(())
            })
            .unwrap()
            .choices(),
            [5, 0]
        );
        assert!(samples.next().is_none());
    }
}
