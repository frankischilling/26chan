use crate::{CommentSpacing, comment_ascii::similar_to_ascii, comment_spacing::zero_width};

// Source imgboard.php:5309-5341. Call only after raw-input validation.
pub(super) fn before_spacing(text: &str, spacing: CommentSpacing<'_>) -> String {
    let mut result = String::with_capacity(text.len());
    for mut ch in text.chars() {
        if spacing.strip_zero_width {
            if matches!(ch as u32, 0x2600..=0x26ff) {
                continue;
            }
            ch = similar_to_ascii(ch);
            if zero_width(ch) {
                continue;
            }
        }
        if matches!(ch, '\u{a0}' | '\u{ad}') || emoticon(ch, spacing.sjis) {
            continue;
        }
        result.push(ch);
    }
    // The source's character class includes a literal pipe, not alternation.
    if result
        .chars()
        .all(|ch| matches!(ch, ' ' | '|' | '\u{3000}' | '\t'))
    {
        result.clear();
    }
    result
}

// Fixed supplied-source exclusions, not a changing Unicode emoji property.
fn emoticon(ch: char, sjis: bool) -> bool {
    matches!(ch as u32,
        0x2300..=0x2311 | 0x2313..=0x23ff | 0x3200..=0x32ff |
        0x2190..=0x21ff | 0x2580..=0x259f | 0x2600..=0x26ff |
        0x2b00..=0x2bfe | 0x1f700..=0x1f77f | 0x1f780..=0x1f7ff |
        0x1f800..=0x1f8ff | 0x1f900..=0x1f9ff | 0x1f200..=0x1f2ff |
        0x2460..=0x24ff | 0x1f100..=0x1f1ff | 0x1f600..=0x1f64f |
        0x1f300..=0x1f5ff | 0x1f680..=0x1f6ff | 0x2700..=0x27bf |
        0x1f000..=0x1f02f | 0x1f0a0..=0x1f0ff | 0x2139 | 0x0365 |
        0xfdfd | 0x0488 | 0x0489 | 0x1abe | 0x20dd..=0x20e0 |
        0x20e2..=0x20e4 | 0xa670..=0xa672 | 0x061c | 0x070f |
        0x0332 | 0x0305 | 0x202a..=0x202e | 0x2060..=0x206f |
        0x200e | 0x200f | 0x180e | 0x1fa70..=0x1faff |
        0x1d173..=0x1d17a | 0x13000..=0x1342f | 0xfe00..=0xfe0f)
        || (!sjis && matches!(ch as u32, 0x2502..=0x257f))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn finite_mapping_and_emoticon_predicate_match_source_fixtures_for_every_scalar() {
        let mut ascii = BTreeMap::new();
        for line in include_str!("../tests/fixtures/comment-ascii.txt").lines() {
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            let mut points = line
                .split_whitespace()
                .map(|s| u32::from_str_radix(s, 16).unwrap());
            let output = char::from_u32(points.next().unwrap()).unwrap();
            for point in points {
                assert!(ascii.insert(point, output).is_none());
            }
        }
        assert_eq!(ascii.len(), 879);
        let ranges: Vec<(u32, u32)> = include_str!("../tests/fixtures/comment-emoticons.txt")
            .lines()
            .filter(|line| !line.starts_with('#') && !line.is_empty())
            .map(|line| {
                let (start, end) = line.split_once(' ').unwrap();
                (
                    u32::from_str_radix(start, 16).unwrap(),
                    u32::from_str_radix(end, 16).unwrap(),
                )
            })
            .collect();
        for point in 0..=0x10ffff {
            let Some(ch) = char::from_u32(point) else {
                continue;
            };
            assert_eq!(
                similar_to_ascii(ch),
                ascii.get(&point).copied().unwrap_or(ch),
                "{point:x}"
            );
            let base = ranges
                .iter()
                .any(|&(start, end)| (start..=end).contains(&point));
            assert_eq!(emoticon(ch, true), base, "SJIS {point:x}");
            assert_eq!(
                emoticon(ch, false),
                base || (0x2502..=0x257f).contains(&point),
                "plain {point:x}"
            );
        }
    }
}
