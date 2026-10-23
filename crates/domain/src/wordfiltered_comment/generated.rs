//! Finite generated wrappers. Neither tag names nor attributes come from input.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeneratedDelimiter {
    // Zero is bold; 1..=13 selects one exact source fortune palette entry.
    number: u8,
    tim: Option<i64>,
    opening: bool,
    rolls: Option<LeetRolls>,
}

impl GeneratedDelimiter {
    pub(crate) fn from_saved(number: u8, opening: bool, rolls: Option<LeetRolls>) -> Option<Self> {
        (number <= 13).then_some(Self {
            number,
            tim: None,
            opening,
            rolls,
        })
    }

    pub(crate) fn from_exif(
        number: u8,
        opening: bool,
        tim: i64,
        rolls: Option<LeetRolls>,
    ) -> Option<Self> {
        ((14..=19).contains(&number) && (1..=crate::exif_comment::MAX_TIM).contains(&tim))
            .then_some(Self {
                number,
                tim: Some(tim),
                opening,
                rolls,
            })
    }
    pub fn is_exif(self) -> bool {
        self.tim.is_some()
    }
    pub(super) fn tim(self) -> Option<i64> {
        self.tim
    }
    pub fn is_exif_toggle(self) -> bool {
        self.number == 15
    }
    pub fn is_exif_table(self) -> bool {
        self.number == 16
    }
    pub fn exif_colspan_attribute(self) -> String {
        self.filtered("colspan")
    }
    pub fn exif_colspan(self) -> bool {
        self.number == 19
    }
    pub fn exif_id(self) -> String {
        self.filtered(&format!("exif{}", self.tim.unwrap_or_default()))
    }
    pub fn exif_toggle_active(self) -> bool {
        self.number == 15
            && self.filtered("onclick") == "onclick"
            && self.filtered("toggle") == "toggle"
    }
    pub fn exif_element(self) -> String {
        self.filtered(match self.number {
            14 => "span",
            15 => "a",
            16 => "table",
            17 => "tr",
            18 | 19 => "td",
            _ => "b",
        })
    }
    pub fn exif_valid_element(self) -> bool {
        self.exif_element()
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic)
    }
    pub(super) fn number(self) -> u8 {
        self.number
    }
    pub fn closing(self) -> Self {
        Self {
            opening: false,
            ..self
        }
    }
    pub fn closes_span(self) -> bool {
        !self.opening && (self.number == 14 || (!self.is_exif() && self.element_name() == "span"))
    }

    pub fn opening(self) -> bool {
        self.opening
    }
    pub fn valid_element(self) -> bool {
        self.element_name()
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic)
    }
    pub fn has_class(self) -> bool {
        self.opening && matches!(self.number, 1..=14 | 16)
    }
    pub fn element_name(self) -> String {
        if self.is_exif() {
            return self.exif_element();
        }
        if self.number == 0 {
            return "b".into();
        }
        let selected = |choice| {
            self.rolls
                .is_some_and(|rolls| rolls.choices().contains(&choice))
        };
        match (selected(0), selected(4)) {
            (false, false) => "span",
            (true, false) => "sp4n",
            (false, true) => "5pan",
            (true, true) => "5p4n",
        }
        .into()
    }
    pub fn class_attribute(self) -> String {
        self.filtered("class")
    }
    pub fn class_value(self) -> String {
        self.filtered(match self.number {
            14 => "abbr",
            16 => "exif",
            _ => "fortune",
        })
    }
    pub fn style_attribute(self) -> String {
        self.filtered("style")
    }
    pub fn style_value(self) -> String {
        if self.number == 0 || self.is_exif() {
            return String::new();
        }
        self.filtered(&format!(
            "color:{}",
            crate::posting_randomizers::fortune_palette_color(self.number - 1)
                .expect("finite palette")
        ))
    }
    /// Preserve only the color effect of valid source CSS through finite classes.
    /// Mutated style attributes/properties have no color effect in the source.
    pub fn color_class(self) -> Option<String> {
        if self.number == 0
            || self.is_exif()
            || !self.valid_element()
            || self.style_attribute() != "style"
            || !self.style_value().starts_with("color:")
        {
            return None;
        }
        let index = self.number - 1;
        let changed_a = self.rolls.is_some_and(|rolls| rolls.choices().contains(&0));
        Some(if changed_a && matches!(index, 1 | 4 | 5 | 6 | 8) {
            format!("fortune-a-{index}")
        } else {
            format!("fortune-{index}")
        })
    }
    pub fn html_class_value(self) -> String {
        let value = self.class_value();
        match self.color_class() {
            Some(color) if self.class_attribute() == "class" => format!("{value} {color}"),
            _ => value,
        }
    }
    pub fn extra_color_class(self) -> Option<String> {
        if self.class_attribute() == "class" {
            None
        } else {
            self.color_class()
        }
    }

    pub fn source_projection(self) -> String {
        if self.is_exif() {
            let tim = self.tim.expect("validated EXIF timestamp");
            let tag = match self.number {
                14 => "span",
                15 => "a",
                16 => "table",
                17 => "tr",
                18 | 19 => "td",
                _ => unreachable!("finite EXIF tag"),
            };
            let literal = if !self.opening {
                format!("</{tag}>")
            } else {
                match self.number {
                    14 => "<span class=\"abbr\">".into(),
                    15 => {
                        format!("<a href=\"javascript:void(0)\" onclick=\"toggle('exif{tim}')\">")
                    }
                    16 => format!("<table class=\"exif\" id=\"exif{tim}\">"),
                    17 => "<tr>".into(),
                    18 => "<td>".into(),
                    19 => "<td colspan=\"2\">".into(),
                    _ => unreachable!("finite EXIF tag"),
                }
            };
            return self.filtered(&literal);
        }
        let literal = match (self.number, self.opening) {
            (0, true) => "<b>".into(),
            (0, false) => "</b>".into(),
            (_, false) => "</span>".into(),
            (_, true) => format!(
                "<span class=\"fortune\" style=\"color:{}\">",
                crate::posting_randomizers::fortune_palette_color(self.number - 1)
                    .expect("finite palette")
            ),
        };
        self.filtered(&literal)
    }
    fn filtered(self, text: &str) -> String {
        self.rolls.map_or_else(
            || text.to_owned(),
            |rolls| wordfilter::leet(text, rolls).expect("finite generated wrapper"),
        )
    }
}

pub(super) fn prepare(
    comment: &str,
    policy: MarkupPolicy,
    profile: Option<Profile>,
    rolls: Option<LeetRolls>,
    post_limits: PostLimits,
    outcome: &crate::posting_randomizers::Outcome,
) -> Result<PreparedComment, ValidationError> {
    prepare_metadata(
        comment,
        policy,
        profile,
        rolls,
        post_limits,
        Some(outcome),
        None,
    )
}

pub(super) fn prepare_metadata(
    comment: &str,
    policy: MarkupPolicy,
    profile: Option<Profile>,
    rolls: Option<LeetRolls>,
    post_limits: PostLimits,
    outcome: Option<&crate::posting_randomizers::Outcome>,
    exif: Option<(&crate::exif_comment::ExtractedExif, i64)>,
) -> Result<PreparedComment, ValidationError> {
    const ERROR: ValidationError = ValidationError("Invalid generated comment projection.");
    if comment.len() > post_limits.prepared_bytes()
        || comment.chars().count() > post_limits.prepared_chars()
    {
        return Err(ValidationError("Wordfilter input is too large."));
    }
    if (profile == Some(Profile::Test)) != rolls.is_some() {
        return Err(ERROR);
    }
    let (prefix, mut suffix) = crate::posting_randomizers::source_wrappers(outcome)?;
    if let Some((exif, tim)) = exif {
        suffix.push_str(&exif.comparison_markup(tim).map_err(|_| ERROR)?);
    }
    let source = crate::comment_markup::admission_source(comment, policy, &prefix, &suffix);
    let limits = WordfilterLimits::for_post(post_limits);
    let source = match profile {
        Some(profile) => {
            wordfilter::apply_with_limits(&source, Field::Comment, profile, rolls, limits)?
        }
        None => source,
    };
    if source.len() > limits.output_bytes() {
        return Err(ValidationError("Wordfilter output is too large."));
    }
    let mut parts = Vec::new();
    let mut stack: Vec<Part> = Vec::new();
    let mut remaining = source.as_str();
    while !remaining.is_empty() {
        if let Some(tail) = remaining.strip_prefix("<br>") {
            parts.push(Part::Break);
            remaining = tail;
        } else if remaining.starts_with("</") {
            // Several source markup kinds share the same span spelling. The
            // nearest matching finite element gives identical HTML boundaries.
            let found = stack
                .iter()
                .rposition(|part| {
                    let close = closing(part);
                    remaining.starts_with(&projection(&close))
                })
                .ok_or(ERROR)?;
            let close = closing(&stack.remove(found));
            remaining = &remaining[projection(&close).len()..];
            parts.push(close);
        } else if remaining.starts_with('<') {
            let mut matched = None;
            for number in 0..8 {
                let delimiter = Delimiter {
                    tag: number_tag(number).expect("finite markup"),
                    opening: true,
                    rolls,
                };
                let part = Part::Delimiter(delimiter);
                if remaining.starts_with(&projection(&part)) {
                    matched = Some(part);
                    break;
                }
            }
            if matched.is_none() {
                for number in 0..=13 {
                    let part = Part::Generated(
                        GeneratedDelimiter::from_saved(number, true, rolls)
                            .expect("finite generated wrapper"),
                    );
                    if remaining.starts_with(&projection(&part)) {
                        matched = Some(part);
                        break;
                    }
                }
            }
            if matched.is_none()
                && let Some((_, tim)) = exif
            {
                for number in 14..=19 {
                    let part = Part::Generated(
                        GeneratedDelimiter::from_exif(number, true, tim, rolls).ok_or(ERROR)?,
                    );
                    if remaining.starts_with(&projection(&part)) {
                        matched = Some(part);
                        break;
                    }
                }
            }
            let part = matched.ok_or(ERROR)?;
            remaining = &remaining[projection(&part).len()..];
            stack.push(part.clone());
            parts.push(part);
        } else {
            let end = remaining.find('<').unwrap_or(remaining.len());
            append_filtered_text(&remaining[..end], &mut parts)?;
            remaining = &remaining[end..];
        }
        if parts.len() > limits.max_parts() {
            return Err(ValidationError("Wordfilter output is too large."));
        }
    }
    if !stack.is_empty() {
        return Err(ERROR);
    }
    Ok(PreparedComment {
        parts,
        rolls,
        wrap: None,
        limits,
        generated: true,
    })
}

fn closing(part: &Part) -> Part {
    match part {
        Part::Delimiter(delimiter) => Part::Delimiter(delimiter.closing()),
        Part::Generated(delimiter) => Part::Generated(GeneratedDelimiter {
            opening: false,
            ..*delimiter
        }),
        _ => unreachable!("finite open delimiter"),
    }
}
fn projection(part: &Part) -> String {
    match part {
        Part::Delimiter(delimiter) => delimiter.source_projection(),
        Part::Generated(delimiter) => delimiter.source_projection(),
        _ => unreachable!("finite delimiter"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::posting_randomizers::Outcome;

    #[test]
    fn exif_codec_rejects_forged_identifiers_kinds_and_unbalanced_tables() {
        let exif = crate::exif_comment::ExtractedExif::parse("Header\nCamera: Owned\n").unwrap();
        let mut prepared = prepare_metadata(
            "",
            MarkupPolicy::default(),
            None,
            None,
            PostLimits::ordinary(MAX_COMMENT_CHARS),
            None,
            Some((&exif, 1730000000000)),
        )
        .unwrap();
        prepared.freeze_format("p");
        let bytes = prepared.encode().unwrap();
        let mut offsets = Vec::new();
        let mut offset = 11;
        while offset < bytes.len() {
            match bytes[offset] {
                0 => {
                    let count =
                        u32::from_be_bytes(bytes[offset + 1..offset + 5].try_into().unwrap())
                            as usize;
                    offset += 5 + count;
                }
                1 => offset += 2,
                2 => offset += 1,
                3 | 4 => offset += 3,
                5 => {
                    offsets.push(offset);
                    offset += 11;
                }
                _ => panic!("unexpected owned encoding"),
            }
        }
        assert!(offsets.len() > 8);
        let mut unknown = bytes.clone();
        unknown[offsets[0] + 1] = 20;
        assert!(PreparedComment::decode(&unknown).is_err());
        let mut zero = bytes.clone();
        zero[offsets[0] + 3..offsets[0] + 11].fill(0);
        assert!(PreparedComment::decode(&zero).is_err());
        let mut changed = bytes.clone();
        changed[offsets[1] + 10] ^= 1;
        assert!(PreparedComment::decode(&changed).is_err());
        let mut unclosed = bytes.clone();
        unclosed[*offsets.last().unwrap() + 2] = 1;
        assert!(PreparedComment::decode(&unclosed).is_err());
        let mut old = bytes.clone();
        old[..4].copy_from_slice(b"WF03");
        assert!(PreparedComment::decode(&old).is_err());
        let exif =
            crate::exif_comment::ExtractedExif::parse(&vec!["Header"; 256].join("\n")).unwrap();
        let mut full = prepare_metadata(
            "",
            MarkupPolicy::default(),
            None,
            None,
            PostLimits::ordinary(MAX_COMMENT_CHARS),
            None,
            Some((&exif, 1)),
        )
        .unwrap();
        full.freeze_format("p");
        assert_eq!(
            PreparedComment::decode(&full.encode().unwrap()).unwrap(),
            full
        );
    }

    #[test]
    fn exif_payloads_match_independent_source_composition() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../fixtures/wordfilter-posting-reference.json"
        ))
        .unwrap();
        let mut count = 0;
        for (name, profile) in [
            ("global", Profile::Global),
            ("ck", Profile::Basic),
            ("asp", Profile::Asp),
            ("v", Profile::Video),
            ("test", Profile::Test),
        ] {
            for case in fixture["exif_projection"][name].as_array().unwrap() {
                let p = &case["policy"];
                let markup = MarkupPolicy {
                    spoilers: p["spoilers"].as_bool().unwrap(),
                    code: p["code"].as_bool().unwrap(),
                    sjis: p["sjis"].as_bool().unwrap(),
                    op: p["op"].as_bool().unwrap(),
                };
                let enabled = case["filter_enabled"].as_bool().unwrap();
                let rolls = if enabled && profile == Profile::Test {
                    Some(
                        LeetRolls::from_choices(
                            case["rolls"][0].as_u64().unwrap() as u8,
                            case["rolls"][1].as_u64().unwrap() as u8,
                        )
                        .unwrap(),
                    )
                } else {
                    None
                };
                let outcome = case["randomizer"].is_object().then(|| Outcome::Fortune {
                    text: "You will meet a dark handsome stranger",
                    color: "#0893e1".into(),
                });
                let exif = crate::exif_comment::ExtractedExif::parse(
                    case["exif"]["raw"].as_str().unwrap(),
                )
                .unwrap();
                let mut prepared = super::prepare_metadata(
                    case["prepared_comment"].as_str().unwrap(),
                    markup,
                    enabled.then_some(profile),
                    rolls,
                    PostLimits::ordinary(MAX_COMMENT_CHARS),
                    outcome.as_ref(),
                    Some((&exif, case["exif"]["tim"].as_i64().unwrap())),
                )
                .unwrap();
                assert_eq!(
                    prepared.source_projection(),
                    case["filtered"].as_str().unwrap(),
                    "{name} {case}"
                );
                prepared.freeze_format("g");
                let bytes = prepared.encode().unwrap();
                assert_eq!(&bytes[..4], b"WF05");
                let decoded = PreparedComment::decode(&bytes).unwrap();
                assert_eq!(decoded, prepared);
                let lines = crate::filtered_formatting::lines_for_post(&decoded, "g", 488);
                assert_eq!(
                    crate::filtered_formatting::source_projection(&lines),
                    case["final"].as_str().unwrap(),
                    "{name} {case}"
                );
                count += 1;
            }
        }
        assert_eq!(count, 180);
    }

    #[test]
    fn generated_payloads_match_independent_php_composition_vectors() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../fixtures/wordfilter-posting-reference.json"
        ))
        .unwrap();
        let mut checked = 0;
        for (name, profile) in [
            ("global", Profile::Global),
            ("ck", Profile::Basic),
            ("asp", Profile::Asp),
            ("v", Profile::Video),
            ("test", Profile::Test),
        ] {
            for case in fixture["admission_projection"][name].as_array().unwrap() {
                let outcome = match case["randomizer"]["kind"].as_str() {
                    Some("dice") => {
                        Outcome::Dice(case["randomizer"]["text"].as_str().unwrap().into())
                    }
                    Some("fortune") => {
                        assert_eq!(
                            case["randomizer"]["text"],
                            "You will meet a dark handsome stranger"
                        );
                        Outcome::Fortune {
                            text: "You will meet a dark handsome stranger",
                            color: case["randomizer"]["color"].as_str().unwrap().into(),
                        }
                    }
                    None => continue,
                    _ => panic!("unknown fixture result"),
                };
                let p = &case["policy"];
                let policy = MarkupPolicy {
                    spoilers: p["spoilers"].as_bool().unwrap(),
                    code: p["code"].as_bool().unwrap(),
                    sjis: p["sjis"].as_bool().unwrap(),
                    op: p["op"].as_bool().unwrap(),
                };
                let enabled = case["filter_enabled"].as_bool().unwrap();
                let rolls = if enabled && profile == Profile::Test {
                    Some(
                        LeetRolls::from_choices(
                            case["rolls"][0].as_u64().unwrap() as u8,
                            case["rolls"][1].as_u64().unwrap() as u8,
                        )
                        .unwrap(),
                    )
                } else {
                    None
                };
                let board = case["board"].as_str().unwrap();
                let mut prepared = prepare(
                    case["prepared_comment"].as_str().unwrap(),
                    policy,
                    enabled.then_some(profile),
                    rolls,
                    PostLimits::ordinary(MAX_COMMENT_CHARS),
                    &outcome,
                )
                .unwrap_or_else(|error| panic!("{name} {case}: {error}"));
                assert!(prepared.has_generated_content());
                prepared.freeze_format(board);
                let encoded = prepared.encode().unwrap();
                assert_eq!(&encoded[..4], b"WF03");
                let decoded = PreparedComment::decode(&encoded)
                    .unwrap_or_else(|error| panic!("{name} {case}: {error}"));
                assert_eq!(decoded, prepared);
                let lines = crate::filtered_formatting::lines_for_post(&decoded, board, 488);
                assert_eq!(
                    crate::filtered_formatting::source_projection(&lines),
                    case["final"].as_str().unwrap(),
                    "{name} {case}"
                );
                checked += 1;
            }
        }
        assert_eq!(checked, 706);
    }

    #[test]
    fn finite_wrapper_attributes_preserve_every_leet_choice() {
        for first in 0..6 {
            for second in 0..6 {
                let rolls = LeetRolls::from_choices(first, second).unwrap();
                for number in 0..=13 {
                    for opening in [false, true] {
                        let delimiter =
                            GeneratedDelimiter::from_saved(number, opening, Some(rolls)).unwrap();
                        let plain = GeneratedDelimiter::from_saved(number, opening, None).unwrap();
                        assert_eq!(
                            delimiter.source_projection(),
                            wordfilter::leet(&plain.source_projection(), rolls).unwrap()
                        );
                        let fields = if delimiter.has_class() {
                            format!(
                                " {}=\"{}\" {}=\"{}\"",
                                delimiter.class_attribute(),
                                delimiter.class_value(),
                                delimiter.style_attribute(),
                                delimiter.style_value()
                            )
                        } else {
                            String::new()
                        };
                        let reconstructed = if opening {
                            format!("<{}{fields}>", delimiter.element_name())
                        } else {
                            format!("</{}>", delimiter.element_name())
                        };
                        assert_eq!(reconstructed, delimiter.source_projection());
                    }
                }
            }
        }
        assert!(GeneratedDelimiter::from_saved(14, true, None).is_none());
    }

    #[test]
    fn generated_codec_requires_its_version_and_balanced_finite_wrappers() {
        let outcome = Outcome::Dice("Rolled 1 (1d1)".into());
        let mut prepared = prepare(
            "<b>ordinary</b>",
            MarkupPolicy::default(),
            None,
            None,
            PostLimits::ordinary(1000),
            &outcome,
        )
        .unwrap();
        prepared.freeze_format("b");
        let encoded = prepared.encode().unwrap();
        for length in 0..encoded.len() {
            assert!(PreparedComment::decode(&encoded[..length]).is_err());
        }
        let mut legacy = encoded.clone();
        legacy[..4].copy_from_slice(b"WF01");
        assert!(PreparedComment::decode(&legacy).is_err());
        let mut unknown = encoded.clone();
        assert_eq!(unknown[11], 4);
        unknown[12] = 14;
        assert!(PreparedComment::decode(&unknown).is_err());
        assert!(
            prepared
                .source_projection()
                .contains("&lt;b&gt;ordinary&lt;/b&gt;")
        );
        let mut authorized = prepare(
            "",
            MarkupPolicy::default(),
            None,
            None,
            PostLimits::authorized(50000).unwrap(),
            &outcome,
        )
        .unwrap();
        authorized.freeze_format("b");
        let encoded = authorized.encode().unwrap();
        assert_eq!(&encoded[..4], b"WF04");
        assert_eq!(PreparedComment::decode(&encoded).unwrap(), authorized);
    }
}
