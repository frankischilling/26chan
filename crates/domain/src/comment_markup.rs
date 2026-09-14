//! Whole-comment source BBCode passes. Text is never interpreted as HTML.
//! This stage precedes linkification, word wrapping and quote formatting.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MarkupPolicy {
    pub spoilers: bool,
    pub code: bool,
    pub sjis: bool,
}

impl MarkupPolicy {
    /// Version zero belongs to the historical formatter. Unknown versions do
    /// not grant markup authority. Storage constrains the currently known set.
    pub fn from_post_format(value: i16) -> Option<Self> {
        (8..=15).contains(&value).then_some(Self {
            spoilers: value & 1 != 0,
            code: value & 2 != 0,
            sjis: value & 4 != 0,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tag {
    Spoiler,
    Code,
    Sjis,
}

impl Tag {
    fn marker(self, opening: bool) -> &'static str {
        match (self, opening) {
            (Self::Spoiler, true) => "[spoiler]",
            (Self::Spoiler, false) => "[/spoiler]",
            (Self::Code, true) => "[code]",
            (Self::Code, false) => "[/code]",
            (Self::Sjis, true) => "[sjis]",
            (Self::Sjis, false) => "[/sjis]",
        }
    }

    fn source_html_bytes(self, opening: bool) -> usize {
        match (self, opening) {
            (Self::Spoiler, true) => "<s>".len(),
            (Self::Spoiler, false) => "</s>".len(),
            (Self::Code, true) => "<pre class=\"prettyprint\">".len(),
            (Self::Code, false) => "</pre>".len(),
            (Self::Sjis, true) => "<span class=\"sjis\">".len(),
            (Self::Sjis, false) => "</span>".len(),
        }
    }
}

/// Flat boundaries preserve the source's ordered passes, including crossing
/// tag kinds. Renderers must map only these finite kinds to literal elements
/// and escape Text. Do not turn Text into template-safe HTML.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkupToken {
    Text(String),
    Break,
    Open(Tag),
    Close(Tag),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Atom {
    Char(char),
    Break,
    Open(Tag),
    Close(Tag),
}

impl Atom {
    fn source_html_bytes(self) -> usize {
        match self {
            Self::Char('&') => 5,
            Self::Char('<' | '>') => 4,
            Self::Char('"' | '\'') => 6,
            Self::Char(ch) => ch.len_utf8(),
            Self::Break => 4,
            Self::Open(tag) => tag.source_html_bytes(true),
            Self::Close(tag) => tag.source_html_bytes(false),
        }
    }
}

/// Input is prepared stored text with LF line separators. Independently cap at
/// the global scalar limit. The parser makes a fixed number of linear passes,
/// never recurses and never accepts caller-supplied tag names or HTML.
pub fn parse_markup(input: &str, policy: MarkupPolicy) -> Vec<MarkupToken> {
    let mut atoms: Vec<_> = input
        .chars()
        .take(crate::MAX_COMMENT_CHARS)
        .map(|ch| {
            if ch == '\n' {
                Atom::Break
            } else {
                Atom::Char(ch)
            }
        })
        .collect();
    if policy.sjis {
        let skip = contains_marker(&atoms, "[spoiler]");
        atoms = parse_one(atoms, Tag::Sjis, 1, skip);
    }
    if policy.spoilers {
        atoms = parse_one(atoms, Tag::Spoiler, 2, false);
        atoms = remove_empty_spoilers(atoms);
    }
    if policy.code {
        atoms = unwrap_short_code(atoms);
        atoms = parse_one(atoms, Tag::Code, 2, false);
        atoms = code_breaks(atoms);
    }
    coalesce(atoms)
}

fn marker_at(input: &[Atom], index: usize, marker: &str) -> bool {
    input.get(index..index + marker.len()).is_some_and(|slice| {
        slice
            .iter()
            .zip(marker.bytes())
            .all(|(atom, byte)| *atom == Atom::Char(char::from(byte)))
    })
}

fn contains_marker(input: &[Atom], marker: &str) -> bool {
    (0..input.len()).any(|index| marker_at(input, index, marker))
}

fn contains_spoiler(input: &[Atom]) -> bool {
    contains_marker(input, "[spoiler]") || contains_marker(input, "[/spoiler]")
}

fn parse_one(input: Vec<Atom>, tag: Tag, limit: usize, skip: bool) -> Vec<Atom> {
    let opening = tag.marker(true);
    let closing = tag.marker(false);
    let Some(first) = (0..input.len()).find(|&index| marker_at(&input, index, opening)) else {
        return input;
    };
    let mut out = input[..first].to_vec();
    out.push(Atom::Open(tag));
    let mut depth = 1;
    let mut cursor = first + opening.len();
    let mut index = cursor;
    while index < input.len() {
        let is_open = marker_at(&input, index, opening);
        if !is_open && !marker_at(&input, index, closing) {
            index += 1;
            continue;
        }
        let text = &input[cursor..index];
        if !skip || is_open {
            out.extend_from_slice(text);
        } else if contains_spoiler(text) {
            return input;
        }
        // With skip enabled the source omits text before a closing marker,
        // even when the spoiler which enabled skip is outside this block.
        if is_open {
            if depth < limit {
                out.push(Atom::Open(tag));
            }
            depth += 1;
        } else if depth > 0 {
            if depth <= limit {
                out.push(Atom::Close(tag));
            }
            depth -= 1;
        }
        index += if is_open {
            opening.len()
        } else {
            closing.len()
        };
        cursor = index;
    }
    let tail = &input[cursor..];
    if depth > 0 && skip && contains_spoiler(tail) {
        return input;
    }
    out.extend_from_slice(tail);
    out.extend(std::iter::repeat_n(Atom::Close(tag), depth.min(limit)));
    out
}

fn remove_empty_spoilers(input: Vec<Atom>) -> Vec<Atom> {
    let mut out = Vec::with_capacity(input.len());
    let mut stack: Vec<(usize, bool)> = Vec::new();
    for atom in input {
        match atom {
            Atom::Open(Tag::Spoiler) => {
                stack.push((out.len(), true));
                out.push(atom);
            }
            Atom::Close(Tag::Spoiler) => {
                let (start, empty) = stack
                    .pop()
                    .expect("source spoiler pass balances its own tags");
                if empty {
                    out.truncate(start);
                } else {
                    out.push(atom);
                    if let Some((_, parent_empty)) = stack.last_mut() {
                        *parent_empty = false;
                    }
                }
            }
            _ => {
                if !matches!(
                    atom,
                    Atom::Break | Atom::Char(' ' | '\t' | '\r' | '\u{b}' | '\u{c}')
                ) && let Some((_, empty)) = stack.last_mut()
                {
                    *empty = false;
                }
                out.push(atom);
            }
        }
    }
    out
}

fn unwrap_short_code(input: Vec<Atom>) -> Vec<Atom> {
    let mut out = Vec::with_capacity(input.len());
    let mut cursor = 0;
    while cursor < input.len() {
        if marker_at(&input, cursor, "[code]") {
            let start = cursor + "[code]".len();
            let mut end = start;
            let mut bytes = 0;
            while bytes <= 6 {
                if marker_at(&input, end, "[/code]") {
                    out.extend_from_slice(&input[start..end]);
                    cursor = end + "[/code]".len();
                    break;
                }
                let Some(atom) = input.get(end) else { break };
                // Raw CR can occur only in independently supplied historical
                // input; PCRE dot excludes LF, already represented as <br>.
                bytes += atom.source_html_bytes();
                end += 1;
            }
            if cursor > start {
                continue;
            }
        }
        out.push(input[cursor]);
        cursor += 1;
    }
    out
}

fn code_breaks(input: Vec<Atom>) -> Vec<Atom> {
    let mut out = Vec::with_capacity(input.len());
    let mut after_open = false;
    let mut breaks = 0;
    for atom in input {
        if after_open && atom == Atom::Break {
            after_open = false;
            continue;
        }
        after_open = atom == Atom::Open(Tag::Code);
        if atom == Atom::Break {
            breaks += 1;
            if breaks > 3 {
                continue;
            }
        } else {
            breaks = 0;
        }
        out.push(atom);
    }
    out
}

fn coalesce(input: Vec<Atom>) -> Vec<MarkupToken> {
    let mut out = Vec::new();
    let mut text = String::new();
    for atom in input {
        if let Atom::Char(ch) = atom {
            text.push(ch);
            continue;
        }
        if !text.is_empty() {
            out.push(MarkupToken::Text(std::mem::take(&mut text)));
        }
        out.push(match atom {
            Atom::Break => MarkupToken::Break,
            Atom::Open(tag) => MarkupToken::Open(tag),
            Atom::Close(tag) => MarkupToken::Close(tag),
            Atom::Char(_) => unreachable!(),
        });
    }
    if !text.is_empty() {
        out.push(MarkupToken::Text(text));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // Independent string/byte-offset transcription of imgboard.php:497-550.
    // The production implementation scans typed atoms, not escaped HTML.
    fn reference(input: &str, name: &str, limit: usize, skip: bool) -> String {
        let start = format!("[{name}]");
        let end = format!("[/{name}]");
        let Some(first) = input.find(&start) else {
            return input.into();
        };
        let mut result = format!("{}<OPEN>", &input[..first]);
        let mut level = 1;
        let mut offset = first + start.len();
        let mut matches: Vec<_> = input
            .match_indices(&start)
            .map(|(i, _)| (i, true))
            .chain(input.match_indices(&end).map(|(i, _)| (i, false)))
            .filter(|(i, _)| *i >= offset)
            .collect();
        matches.sort_unstable();
        for (index, opening) in matches {
            let text = &input[offset..index];
            if !skip || opening {
                result.push_str(text);
            } else if text.contains("[spoiler]") || text.contains("[/spoiler]") {
                return input.into();
            }
            offset = index + if opening { start.len() } else { end.len() };
            if opening {
                if level < limit {
                    result.push_str("<OPEN>");
                }
                level += 1;
            } else if level != 0 {
                if level <= limit {
                    result.push_str("<CLOSE>");
                }
                level -= 1;
            }
        }
        let tail = &input[offset..];
        result.push_str(tail);
        if level > 0 && skip && (tail.contains("[spoiler]") || tail.contains("[/spoiler]")) {
            return input.into();
        }
        result.push_str(&"<CLOSE>".repeat(level.min(limit)));
        result
    }

    fn projection(input: &[Atom]) -> String {
        input
            .iter()
            .map(|atom| match atom {
                Atom::Char(ch) => crate::source_html_entities(&ch.to_string()),
                Atom::Break => "<br>".into(),
                Atom::Open(_) => "<OPEN>".into(),
                Atom::Close(_) => "<CLOSE>".into(),
            })
            .collect()
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]
        #[test]
        fn single_pass_matches_source_byte_offset_reference(
            pieces in prop::collection::vec(prop_oneof![
                ".{0,16}", Just("[spoiler]".into()), Just("[/spoiler]".into()),
                Just("[sjis]".into()), Just("[/sjis]".into()),
                Just("[code]".into()), Just("[/code]".into()), Just("\n".into())
            ], 0..120), skip in any::<bool>(),
        ) {
            let raw = pieces.concat();
            let input: Vec<_> = raw.chars().map(|ch| if ch == '\n' { Atom::Break } else { Atom::Char(ch) }).collect();
            let escaped = projection(&input);
            for (tag, name, limit) in [(Tag::Spoiler, "spoiler", 2), (Tag::Code, "code", 2), (Tag::Sjis, "sjis", 1)] {
                let actual = projection(&parse_one(input.clone(), tag, limit, skip));
                prop_assert_eq!(actual, reference(&escaped, name, limit, skip));
            }
        }
    }
}
