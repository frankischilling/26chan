//! Posting-time filtering after source markup, with no trusted user HTML.
//! Saved data grants only finite generated components; ordinary text is escaped.

use crate::comment_markup::{MarkupPolicy, MarkupToken, Tag, parse_markup_with_limits};
use crate::wordfilter::{self, Field, LeetRolls, Profile};
use crate::{
    MAX_COMMENT_CHARS, PostLimits, ValidationError, WordfilterLimits, source_html_entities,
};

pub const MAX_PARTS: usize = 32_768;
pub const MAX_STORED_BYTES: usize = 131_072;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangedEntity {
    Amp,
    QuoteZero,
    QuoteSeven,
    QuoteZeroSeven,
}

impl ChangedEntity {
    /// Only fixed source spellings. Render this visible spelling as escaped
    /// text in HTML; the source JSON projection can retain the fixed entity.
    pub fn spelling(self) -> &'static str {
        match self {
            Self::Amp => "&4mp;",
            Self::QuoteZero => "&qu0t;",
            Self::QuoteSeven => "&quo7;",
            Self::QuoteZeroSeven => "&qu07;",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Delimiter {
    tag: Tag,
    opening: bool,
    rolls: Option<LeetRolls>,
}

impl Delimiter {
    pub fn tag(self) -> Tag {
        self.tag
    }
    pub fn opening(self) -> bool {
        self.opening
    }
    pub fn rolls(self) -> Option<LeetRolls> {
        self.rolls
    }

    /// Names come only from the finite generated source tags. A leading digit
    /// is text in the HTML parser, so its opening must be escaped and its
    /// closing omitted by the template.
    pub fn valid_element(self) -> bool {
        !self.element_name().starts_with('5')
    }

    pub fn element_name(self) -> &'static str {
        let selected = |choice| {
            self.rolls
                .is_some_and(|rolls| rolls.choices().contains(&choice))
        };
        match self.tag {
            Tag::Spoiler if selected(4) => "5",
            Tag::Spoiler => "s",
            Tag::Code if selected(1) => "pr3",
            Tag::Code => "pre",
            _ => match (selected(0), selected(4)) {
                (false, false) => "span",
                (true, false) => "sp4n",
                (false, true) => "5pan",
                (true, true) => "5p4n",
            },
        }
    }

    pub fn has_class(self) -> bool {
        self.opening && self.tag != Tag::Spoiler
    }

    pub fn class_attribute(self) -> &'static str {
        let selected = |choice| {
            self.rolls
                .is_some_and(|rolls| rolls.choices().contains(&choice))
        };
        match (selected(0), selected(4)) {
            (false, false) => "class",
            (true, false) => "cl4ss",
            (false, true) => "cla55",
            (true, true) => "cl455",
        }
    }

    pub fn class_value(self) -> String {
        let value = match self.tag {
            Tag::Spoiler => "",
            Tag::Code => "prettyprint",
            Tag::Sjis => "sjis",
            Tag::Bold => "mu-s",
            Tag::Italic => "mu-i",
            Tag::Red => "mu-r",
            Tag::Green => "mu-g",
            Tag::Blue => "mu-b",
        };
        self.rolls.map_or_else(
            || value.to_owned(),
            |rolls| wordfilter::leet(value, rolls).expect("finite validated class"),
        )
    }

    pub fn is_sjis(self) -> bool {
        self.opening
            && self.element_name() == "span"
            && self.class_attribute() == "class"
            && self.class_value() == "sjis"
    }

    pub fn closes_span(self) -> bool {
        !self.opening && self.element_name() == "span"
    }

    pub fn closing(self) -> Self {
        Self {
            opening: false,
            ..self
        }
    }

    /// A finite generated delimiter, not an HTML string supplied by a caller.
    /// Templates must choose its approved components instead of applying a
    /// template-safe filter to this source-projection string.
    pub fn source_projection(self) -> String {
        let literal = match (self.tag, self.opening) {
            (Tag::Spoiler, true) => "<s>",
            (Tag::Spoiler, false) => "</s>",
            (Tag::Code, true) => "<pre class=\"prettyprint\">",
            (Tag::Code, false) => "</pre>",
            (Tag::Sjis, true) => "<span class=\"sjis\">",
            (Tag::Bold, true) => "<span class=\"mu-s\">",
            (Tag::Italic, true) => "<span class=\"mu-i\">",
            (Tag::Red, true) => "<span class=\"mu-r\">",
            (Tag::Green, true) => "<span class=\"mu-g\">",
            (Tag::Blue, true) => "<span class=\"mu-b\">",
            (_, false) => "</span>",
        };
        self.rolls.map_or_else(
            || literal.to_owned(),
            |rolls| wordfilter::leet(literal, rolls).expect("finite validated delimiter"),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Part {
    Text(String),
    ChangedEntity(ChangedEntity),
    Break,
    Delimiter(Delimiter),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedComment {
    parts: Vec<Part>,
    rolls: Option<LeetRolls>,
    wrap: Option<bool>,
    limits: WordfilterLimits,
}

impl PreparedComment {
    pub fn parts(&self) -> &[Part] {
        &self.parts
    }

    pub fn rolls(&self) -> Option<LeetRolls> {
        self.rolls
    }

    pub fn wrap_enabled(&self) -> Option<bool> {
        self.wrap
    }

    /// Retain the whole-comment wrapping decision before making excerpts.
    pub fn freeze_format(&mut self, board: &str) {
        self.wrap = Some(crate::filtered_formatting::wrap_required(self, board));
    }

    /// Versioned, bounded data; neither HTML nor an executable filter program.
    pub fn encode(&self) -> Result<Vec<u8>, ValidationError> {
        let mut output = Vec::new();
        output.extend_from_slice(self.limits.version());
        output.extend_from_slice(&(self.parts.len() as u32).to_be_bytes());
        output.extend_from_slice(&self.rolls.map_or([255, 255], LeetRolls::choices));
        output
            .push(u8::from(self.wrap.ok_or(ValidationError(
                "Wordfilter formatting is not frozen.",
            ))?));
        for part in &self.parts {
            match part {
                Part::Text(text) => {
                    output.push(0);
                    output.extend_from_slice(&(text.len() as u32).to_be_bytes());
                    output.extend_from_slice(text.as_bytes());
                }
                Part::ChangedEntity(entity) => output.extend_from_slice(&[
                    1,
                    match entity {
                        ChangedEntity::Amp => 0,
                        ChangedEntity::QuoteZero => 1,
                        ChangedEntity::QuoteSeven => 2,
                        ChangedEntity::QuoteZeroSeven => 3,
                    },
                ]),
                Part::Break => output.push(2),
                Part::Delimiter(delimiter) => output.extend_from_slice(&[
                    3,
                    tag_number(delimiter.tag),
                    u8::from(delimiter.opening),
                ]),
            }
            if output.len() > self.limits.stored_bytes() {
                return Err(ValidationError("Wordfilter output is too large."));
            }
        }
        Ok(output)
    }

    /// Validate every field before constructing any rendering authority.
    pub fn decode(input: &[u8]) -> Result<Self, ValidationError> {
        const ERROR: ValidationError = ValidationError("Invalid saved wordfilter comment.");
        let limits = if input.starts_with(b"WF01") {
            WordfilterLimits::Ordinary
        } else if input.starts_with(b"WF02") {
            WordfilterLimits::Authorized
        } else {
            return Err(ERROR);
        };
        if input.len() > limits.stored_bytes() {
            return Err(ERROR);
        }
        let mut cursor = Cursor { input, offset: 4 };
        let count = u32::from_be_bytes(cursor.take(4)?.try_into().map_err(|_| ERROR)?) as usize;
        if count > limits.max_parts() {
            return Err(ERROR);
        }
        let choices = cursor.take(2)?;
        let rolls = if choices == [255, 255] {
            None
        } else {
            Some(LeetRolls::from_choices(choices[0], choices[1])?)
        };
        let wrap = match cursor.take(1)?[0] {
            0 => false,
            1 => true,
            _ => return Err(ERROR),
        };
        let mut parts = Vec::with_capacity(count);
        let mut source_bytes = 0;
        let mut depth = [0u8; 8];
        for _ in 0..count {
            let part = match cursor.take(1)?[0] {
                0 => {
                    let len =
                        u32::from_be_bytes(cursor.take(4)?.try_into().map_err(|_| ERROR)?) as usize;
                    let text = std::str::from_utf8(cursor.take(len)?).map_err(|_| ERROR)?;
                    source_bytes += text
                        .chars()
                        .map(|ch| match ch {
                            '&' => 5,
                            '<' | '>' => 4,
                            '"' | '\'' => 6,
                            _ => ch.len_utf8(),
                        })
                        .sum::<usize>();
                    Part::Text(text.to_owned())
                }
                1 => {
                    let entity = match cursor.take(1)?[0] {
                        0 => ChangedEntity::Amp,
                        1 => ChangedEntity::QuoteZero,
                        2 => ChangedEntity::QuoteSeven,
                        3 => ChangedEntity::QuoteZeroSeven,
                        _ => return Err(ERROR),
                    };
                    if rolls.is_none() {
                        return Err(ERROR);
                    }
                    source_bytes += entity.spelling().len();
                    Part::ChangedEntity(entity)
                }
                2 => {
                    source_bytes += 4;
                    Part::Break
                }
                3 => {
                    let number = cursor.take(1)?[0];
                    let tag = number_tag(number).ok_or(ERROR)?;
                    let opening = match cursor.take(1)?[0] {
                        0 => false,
                        1 => true,
                        _ => return Err(ERROR),
                    };
                    let count = &mut depth[number as usize];
                    if opening {
                        if *count
                            >= if matches!(tag, Tag::Spoiler | Tag::Code) {
                                2
                            } else {
                                1
                            }
                        {
                            return Err(ERROR);
                        }
                        *count += 1;
                    } else {
                        if *count == 0 {
                            return Err(ERROR);
                        }
                        *count -= 1;
                    }
                    let delimiter = Delimiter {
                        tag,
                        opening,
                        rolls,
                    };
                    source_bytes += delimiter.source_projection().len();
                    Part::Delimiter(delimiter)
                }
                _ => return Err(ERROR),
            };
            if source_bytes > limits.output_bytes() {
                return Err(ERROR);
            }
            parts.push(part);
        }
        if cursor.offset != input.len() || depth.iter().any(|depth| *depth != 0) {
            return Err(ERROR);
        }
        Ok(Self {
            parts,
            rolls,
            wrap: Some(wrap),
            limits,
        })
    }

    /// Source/API comparison only. User text is escaped here, while entities
    /// and delimiters come from closed generated variants. Never use this as
    /// template-safe HTML or as a browser innerHTML value.
    pub fn source_projection(&self) -> String {
        let mut result = String::new();
        for part in &self.parts {
            match part {
                Part::Text(text) => result.push_str(&source_html_entities(text)),
                Part::ChangedEntity(entity) => result.push_str(entity.spelling()),
                Part::Break => result.push_str("<br>"),
                Part::Delimiter(delimiter) => result.push_str(&delimiter.source_projection()),
            }
        }
        result
    }
}

/// The caller supplies the already admitted, sanitized comment and locked
/// board/OP markup policy. The output freezes transformations, not raw HTML.
pub fn prepare(
    comment: &str,
    policy: MarkupPolicy,
    profile: Profile,
    rolls: Option<LeetRolls>,
) -> Result<PreparedComment, ValidationError> {
    prepare_with_limits(
        comment,
        policy,
        profile,
        rolls,
        PostLimits::ordinary(MAX_COMMENT_CHARS),
    )
}

pub fn prepare_with_limits(
    comment: &str,
    policy: MarkupPolicy,
    profile: Profile,
    rolls: Option<LeetRolls>,
    post_limits: PostLimits,
) -> Result<PreparedComment, ValidationError> {
    if comment.len() > post_limits.prepared_bytes()
        || comment.chars().count() > post_limits.prepared_chars()
    {
        return Err(ValidationError("Wordfilter input is too large."));
    }
    let limits = WordfilterLimits::for_post(post_limits);
    if profile == Profile::Test && rolls.is_none() {
        return Err(ValidationError("Wordfilter randomness is unavailable."));
    }
    let mut parts = Vec::new();
    let mut after_generated = false;
    let mut source_bytes = 0;
    for token in parse_markup_with_limits(comment, policy, post_limits) {
        match token {
            MarkupToken::Text(text) => {
                let escaped = source_html_entities(&text);
                // A generated delimiter ends in '>'. The source's bytewise,
                // non-overlapping t rule can consume that byte and a leading
                // t/T from this text. Splitting without context changes it.
                let input = if after_generated {
                    format!(">{escaped}")
                } else {
                    escaped
                };
                let filtered =
                    wordfilter::apply_with_limits(&input, Field::Comment, profile, rolls, limits)?;
                let filtered = if after_generated {
                    filtered
                        .strip_prefix('>')
                        .ok_or(ValidationError("Invalid wordfilter boundary."))?
                } else {
                    &filtered
                };
                source_bytes += filtered.len();
                append_filtered_text(filtered, &mut parts)?;
                after_generated = false;
            }
            MarkupToken::Break => {
                parts.push(Part::Break);
                source_bytes += 4;
                after_generated = true;
            }
            MarkupToken::Open(tag) | MarkupToken::Close(tag) => {
                let delimiter = Delimiter {
                    tag,
                    opening: matches!(token, MarkupToken::Open(_)),
                    rolls: if profile == Profile::Test {
                        rolls
                    } else {
                        None
                    },
                };
                source_bytes += delimiter.source_projection().len();
                parts.push(Part::Delimiter(delimiter));
                after_generated = true;
            }
        }
        if parts.len() > limits.max_parts() || source_bytes > limits.output_bytes() {
            return Err(ValidationError("Wordfilter output is too large."));
        }
    }
    Ok(PreparedComment {
        parts,
        rolls: if profile == Profile::Test {
            rolls
        } else {
            None
        },
        wrap: None,
        limits,
    })
}

struct Cursor<'a> {
    input: &'a [u8],
    offset: usize,
}
impl<'a> Cursor<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], ValidationError> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or(ValidationError("Invalid saved wordfilter comment."))?;
        let value = self
            .input
            .get(self.offset..end)
            .ok_or(ValidationError("Invalid saved wordfilter comment."))?;
        self.offset = end;
        Ok(value)
    }
}

fn tag_number(tag: Tag) -> u8 {
    match tag {
        Tag::Spoiler => 0,
        Tag::Code => 1,
        Tag::Sjis => 2,
        Tag::Bold => 3,
        Tag::Italic => 4,
        Tag::Red => 5,
        Tag::Green => 6,
        Tag::Blue => 7,
    }
}
fn number_tag(number: u8) -> Option<Tag> {
    Some(match number {
        0 => Tag::Spoiler,
        1 => Tag::Code,
        2 => Tag::Sjis,
        3 => Tag::Bold,
        4 => Tag::Italic,
        5 => Tag::Red,
        6 => Tag::Green,
        7 => Tag::Blue,
        _ => return None,
    })
}

fn append_filtered_text(input: &str, parts: &mut Vec<Part>) -> Result<(), ValidationError> {
    let mut text = String::new();
    let mut remaining = input;
    while !remaining.is_empty() {
        if remaining.starts_with('&') {
            let ordinary = [
                ("&amp;", '&'),
                ("&quot;", '"'),
                ("&#039;", '\''),
                ("&lt;", '<'),
                ("&gt;", '>'),
            ];
            if let Some((entity, value)) = ordinary
                .into_iter()
                .find(|(entity, _)| remaining.starts_with(entity))
            {
                text.push(value);
                remaining = &remaining[entity.len()..];
                continue;
            }
            let changed = [
                ChangedEntity::Amp,
                ChangedEntity::QuoteZero,
                ChangedEntity::QuoteSeven,
                ChangedEntity::QuoteZeroSeven,
            ];
            if let Some(entity) = changed
                .into_iter()
                .find(|entity| remaining.starts_with(entity.spelling()))
            {
                if !text.is_empty() {
                    parts.push(Part::Text(std::mem::take(&mut text)));
                }
                parts.push(Part::ChangedEntity(entity));
                remaining = &remaining[entity.spelling().len()..];
                continue;
            }
            return Err(ValidationError("Invalid filtered text entity."));
        }
        let ch = remaining.chars().next().expect("remaining filtered text");
        if matches!(ch, '<' | '>') {
            return Err(ValidationError("Invalid filtered text delimiter."));
        }
        text.push(ch);
        remaining = &remaining[ch.len_utf8()..];
    }
    if !text.is_empty() {
        parts.push(Part::Text(text));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]
        #[test]
        fn arbitrary_saved_data_never_creates_unknown_rendering_components(bytes in prop::collection::vec(any::<u8>(), 0..1024)) {
            if let Ok(prepared) = PreparedComment::decode(&bytes) {
                prop_assert_eq!(prepared.encode().unwrap(), bytes);
                for part in prepared.parts() {
                    if let Part::Delimiter(delimiter) = part {
                        prop_assert!(["s","5","pre","pr3","span","sp4n","5pan","5p4n"].contains(&delimiter.element_name()));
                        prop_assert!(["class","cl4ss","cla55","cl455"].contains(&delimiter.class_attribute()));
                    }
                }
            }
        }

        #[test]
        fn unicode_results_survive_validated_storage(
            text in ".{0,512}", first in 0u8..6, second in 0u8..6,
        ) {
            let rolls = Some(LeetRolls::from_choices(first, second).unwrap());
            let policy = MarkupPolicy { spoilers: true, code: true, sjis: true, op: true };
            let mut prepared = prepare(&text, policy, Profile::Test, rolls).unwrap();
            prepared.freeze_format("test");
            let encoded = prepared.encode().unwrap();
            prop_assert!(encoded.len()<=MAX_STORED_BYTES);
            let decoded = PreparedComment::decode(&encoded).unwrap();
            prop_assert_eq!(&decoded, &prepared);
        }
    }

    #[test]
    fn invalid_versions_lengths_discriminators_unicode_and_nesting_fail_closed() {
        let mut prepared =
            prepare("owned", MarkupPolicy::default(), Profile::Global, None).unwrap();
        assert!(prepared.encode().is_err());
        prepared.freeze_format("g");
        let valid = prepared.encode().unwrap();
        for length in 0..valid.len() {
            assert!(PreparedComment::decode(&valid[..length]).is_err());
        }
        let mut trailing = valid.clone();
        trailing.push(0);
        assert!(PreparedComment::decode(&trailing).is_err());
        let mut version = valid.clone();
        version[3] = b'3';
        assert!(PreparedComment::decode(&version).is_err());
        let mut count = valid.clone();
        count[4..8].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(PreparedComment::decode(&count).is_err());
        let mut choice = valid.clone();
        choice[8] = 6;
        assert!(PreparedComment::decode(&choice).is_err());
        let mut wrap = valid.clone();
        wrap[10] = 2;
        assert!(PreparedComment::decode(&wrap).is_err());
        let mut token = valid.clone();
        token[11] = 4;
        assert!(PreparedComment::decode(&token).is_err());
        let mut unicode = valid.clone();
        unicode[16] = 255;
        assert!(PreparedComment::decode(&unicode).is_err());
        let mut declared = valid.clone();
        declared[12..16].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(PreparedComment::decode(&declared).is_err());
        assert!(PreparedComment::decode(&vec![0; MAX_STORED_BYTES + 1]).is_err());
        let mut nesting = b"WF01".to_vec();
        nesting.extend_from_slice(&3u32.to_be_bytes());
        nesting.extend_from_slice(&[255, 255, 0, 3, 0, 1, 3, 0, 1, 3, 0, 1]);
        assert!(PreparedComment::decode(&nesting).is_err());
        assert_eq!(
            crate::formatting::parse_saved_comment(
                "<script>literal</script>",
                104,
                "g",
                Some(&token)
            )[0]
            .tokens,
            vec![crate::Token::Text("[Comment unavailable]".into())]
        );
    }

    #[test]
    fn markup_projection_matches_independent_source_filter_vectors() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../../fixtures/wordfilter-reference.json"))
                .unwrap();
        let source = "<pre class=\"prettyprint\">soy fam CUCK</pre>";
        let policy = MarkupPolicy {
            code: true,
            ..MarkupPolicy::default()
        };
        for (name, profile) in [
            ("global", Profile::Global),
            ("ck", Profile::Basic),
            ("asp", Profile::Asp),
            ("v", Profile::Video),
            ("test", Profile::Test),
        ] {
            let expected = fixture["profiles"][name]["cases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|case| case["input"] == source && case["field"] == "com")
                .unwrap();
            let prepared = prepare(
                "[code]soy fam CUCK[/code]",
                policy,
                profile,
                Some(LeetRolls::from_choices(0, 0).unwrap()),
            )
            .unwrap();
            assert_eq!(
                prepared.source_projection(),
                expected["output"].as_str().unwrap()
            );
        }
    }

    #[test]
    fn escaped_text_and_changed_entities_are_separate_from_generated_tags() {
        let prepared = prepare(
            "<pre> & \" &#x3C; [code]ordinary text[/code]",
            MarkupPolicy {
                code: true,
                ..MarkupPolicy::default()
            },
            Profile::Test,
            Some(LeetRolls::from_choices(0, 3).unwrap()),
        )
        .unwrap();
        assert!(
            prepared
                .source_projection()
                .starts_with("&lt;pre&gt; &4mp; &qu0t; &4mp;#x3C; ")
        );
        assert!(
            prepared
                .parts()
                .iter()
                .any(|part| matches!(part, Part::Text(text) if text.contains("<pre>")))
        );
        assert_eq!(
            prepared
                .parts()
                .iter()
                .filter(|part| matches!(part, Part::Delimiter(_)))
                .count(),
            2
        );
        assert_eq!(
            prepared
                .parts()
                .iter()
                .filter(|part| matches!(part, Part::ChangedEntity(_)))
                .count(),
            3
        );
    }

    #[test]
    fn generated_boundaries_preserve_the_source_special_t_context() {
        let rolls = Some(LeetRolls::from_choices(5, 5).unwrap());
        let prepared = prepare(
            "t\ntext [code]textttt[/code]",
            MarkupPolicy {
                code: true,
                ..MarkupPolicy::default()
            },
            Profile::Test,
            rolls,
        )
        .unwrap();
        assert_eq!(
            prepared.source_projection(),
            "t<br>7ex7 <pre class=\"pre7typrin7\">7ex7t7t</pre>"
        );
        assert_eq!(
            prepare(
                "t\ntext [code]texttt[/code]",
                MarkupPolicy {
                    code: true,
                    ..MarkupPolicy::default()
                },
                Profile::Test,
                rolls
            )
            .unwrap()
            .source_projection(),
            "t<br>7ex7 7ex7t7"
        );
    }

    #[test]
    fn missing_choices_and_raw_overflow_cannot_silently_truncate() {
        assert!(prepare("", MarkupPolicy::default(), Profile::Test, None).is_err());
        assert!(
            prepare(
                &"x".repeat(MAX_COMMENT_CHARS + 1),
                MarkupPolicy::default(),
                Profile::Global,
                None
            )
            .is_err()
        );
        assert!(append_filtered_text("<script>", &mut Vec::new()).is_err());
        assert!(append_filtered_text("&unknown;", &mut Vec::new()).is_err());
    }
}
