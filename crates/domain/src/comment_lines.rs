use std::collections::VecDeque;

// The source regex lacks /u: \S tests bytes, not Unicode whitespace.
fn source_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

// Equivalent to /(\S)\[spoiler\](.*?)\[\/spoiler\](\S)/ without /s or /i.
// Preindex valid closing markers per line to avoid rescanning malformed nests.
pub(super) fn remove_intra_spoilers(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let closes: Vec<usize> = line
            .match_indices("[/spoiler]")
            .map(|(index, _)| index)
            .filter(|&index| {
                line.as_bytes()
                    .get(index + 10)
                    .is_some_and(|&b| !source_space(b))
            })
            .collect();
        let mut copied = 0;
        let mut consumed = 0;
        for (open, _) in line.match_indices("[spoiler]") {
            if open <= consumed || source_space(line.as_bytes()[open - 1]) {
                continue;
            }
            let index = closes.partition_point(|&close| close < open + 9);
            if let Some(&close) = closes.get(index) {
                result.push_str(&line[copied..open]);
                result.push_str(&line[open + 9..close]);
                copied = close + 10;
                // Global matches consume the right-hand byte. It cannot also
                // be the left-hand byte of the next match, even if retained.
                consumed = copied + 1;
            }
        }
        result.push_str(&line[copied..]);
    }
    result
}

#[derive(Clone, Copy)]
struct Run<'a> {
    text: &'a str,
    newlines: usize,
}

// Source #([^\n]+\n+)\1{5,}#, with its separate strict >6-newline gate.
// A match can start at a suffix of its first text run. Middle runs must be
// equal; the final newline run may be longer than the captured run.
pub(super) fn repeated_lines(text: &str) -> bool {
    if text.bytes().filter(|&b| b == b'\n').count() <= 6 {
        return false;
    }
    // sanitize_text uses htmlspecialchars(..., ENT_QUOTES) before this test.
    // Entity suffixes can match a later literal line ("&" then "amp;").
    // This projection is only for admission, never trusted rendering or storage.
    if text.contains(['&', '<', '>', '"', '\'']) {
        let escaped = crate::source_html_entities(text);
        repeated_line_runs(&escaped)
    } else {
        repeated_line_runs(text)
    }
}

fn repeated_line_runs(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut cursor = 0;
    let mut runs = VecDeque::<Run<'_>>::with_capacity(6);
    while cursor < bytes.len() {
        while bytes.get(cursor) == Some(&b'\n') {
            cursor += 1;
        }
        let start = cursor;
        while cursor < bytes.len() && bytes[cursor] != b'\n' {
            cursor += 1;
        }
        if cursor == bytes.len() {
            break;
        }
        let end = cursor;
        while bytes.get(cursor) == Some(&b'\n') {
            cursor += 1;
        }
        if runs.len() == 6 {
            runs.pop_front();
        }
        runs.push_back(Run {
            text: &text[start..end],
            newlines: cursor - end,
        });
        if runs.len() == 6 {
            let first = runs[0];
            let repeated = runs[1].text;
            if first.text.ends_with(repeated)
                && runs
                    .iter()
                    .skip(1)
                    .take(4)
                    .all(|run| run.text == repeated && run.newlines == first.newlines)
                && runs[5].text == repeated
                && runs[5].newlines >= first.newlines
            {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn brute_repeat(text: &str) -> bool {
        let bytes = text.as_bytes();
        if bytes.iter().filter(|&&b| b == b'\n').count() <= 6 {
            return false;
        }
        for start in 0..bytes.len().saturating_sub(11) {
            if bytes[start] == b'\n' {
                continue;
            }
            let Some(offset) = bytes[start..].iter().position(|&b| b == b'\n') else {
                continue;
            };
            let mut end = start + offset;
            while bytes.get(end) == Some(&b'\n') {
                end += 1;
                let chunk = &bytes[start..end];
                if (1..6).all(|repeat| {
                    bytes.get(start + repeat * chunk.len()..start + (repeat + 1) * chunk.len())
                        == Some(chunk)
                }) {
                    return true;
                }
            }
        }
        false
    }

    // Enumerate the byte-level capture possibilities directly, independently
    // of the production line/closing-marker indexes.
    fn brute_spoilers(text: &str) -> String {
        let bytes = text.as_bytes();
        let mut result = Vec::new();
        let mut copied = 0;
        let mut cursor = 0;
        while cursor < bytes.len() {
            if !source_space(bytes[cursor])
                && bytes.get(cursor + 1..cursor + 10) == Some(b"[spoiler]")
            {
                let capture = cursor + 10;
                let mut close = capture;
                while close < bytes.len() && bytes[close] != b'\n' {
                    if bytes.get(close..close + 10) == Some(b"[/spoiler]")
                        && bytes.get(close + 10).is_some_and(|&b| !source_space(b))
                    {
                        result.extend_from_slice(&bytes[copied..cursor + 1]);
                        result.extend_from_slice(&bytes[capture..close]);
                        result.push(bytes[close + 10]);
                        copied = close + 11;
                        cursor = copied - 1;
                        break;
                    }
                    close += 1;
                }
            }
            cursor += 1;
        }
        result.extend_from_slice(&bytes[copied..]);
        String::from_utf8(result).unwrap()
    }

    #[test]
    fn repetition_matches_exhaustive_small_byte_captures() {
        for len in 0..=16 {
            for mask in 0..(1u32 << len) {
                let text: String = (0..len)
                    .map(|bit| if mask & (1 << bit) == 0 { 'x' } else { '\n' })
                    .collect();
                assert_eq!(repeated_lines(&text), brute_repeat(&text), "{text:?}");
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]
        #[test]
        fn indexed_spoiler_cleanup_matches_byte_capture_order(
            pieces in prop::collection::vec(prop_oneof![Just("x"), Just(" "), Just("\n"), Just("\t"), Just("é"), Just("　"), Just("[spoiler]"), Just("[/spoiler]"), Just("[SPOILER]")], 0..100)
        ) {
            let raw = pieces.concat();
            prop_assert_eq!(remove_intra_spoilers(&raw), brute_spoilers(&raw));
        }

        #[test]
        fn sliding_repetition_matches_byte_captures(
            pieces in prop::collection::vec(prop_oneof![Just("x"), Just("xx"), Just("y"), Just(" "), Just("\n"), Just("\n\n"), Just("é"), Just("&"), Just("<"), Just(">"), Just("\""), Just("'"), Just("amp;")], 0..100)
        ) {
            let raw = pieces.concat();
            let escaped = raw.replace('&', "&amp;").replace('<', "&lt;")
                .replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#039;");
            prop_assert_eq!(repeated_lines(&raw), brute_repeat(&escaped));
        }
    }
}
