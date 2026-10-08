//! Source comment data at imgboard.php's duplicate-admission call site.
//! This is neither template-safe HTML nor a database equality key.
use crate::wordfilter::{LeetRolls, Profile};
use crate::{
    PostLimits, WordfilterLimits, comment_markup::MarkupPolicy, posting_randomizers::Outcome,
};
use pcre2::bytes::{Captures, Regex};

pub const VERSION: u16 = 1;

#[derive(Clone, Copy, Debug)]
pub enum Filter {
    Off,
    On {
        profile: Profile,
        rolls: Option<LeetRolls>,
    },
}

/// Deliberately has no default: missing media provenance is not absence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourcePath {
    OrdinaryWithoutExif,
    UnknownExif,
    ExifPresent,
    PrivilegedHtml,
    Oekaki,
}

/// The posting preparer's output after its one initial source-marker cleanup.
/// Newly joined markers are deliberately retained until the final formatter.
#[derive(Clone, Copy, Debug)]
pub struct PreparedComment<'a> {
    text: &'a str,
}

impl<'a> From<&'a crate::PostContentInput> for PreparedComment<'a> {
    fn from(content: &'a crate::PostContentInput) -> Self {
        Self {
            text: content.comment(),
        }
    }
}

pub struct Input<'a> {
    /// Prepared stage, before markup or word filtering; never raw form input.
    pub prepared_comment: PreparedComment<'a>,
    pub board: &'a str,
    pub markup: MarkupPolicy,
    pub filter: Filter,
    pub randomizer: Option<&'a Outcome>,
    pub source_path: SourcePath,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdmissionProjection {
    data: String,
}

impl AdmissionProjection {
    pub fn version(&self) -> u16 {
        VERSION
    }
    pub fn as_comparison_data(&self) -> &str {
        &self.data
    }
    /// PHP's `if ($com)` gate. This says nothing about SQL collation.
    pub fn source_check_applies(&self) -> bool {
        !self.data.is_empty() && self.data != "0"
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProjectionError {
    #[error("This source comment path is not supported: {0:?}.")]
    Unsupported(SourcePath),
    #[error("Invalid comment projection input.")]
    InvalidInput,
    #[error("Comment projection exceeds its bounded output size.")]
    TooLarge,
    #[error("Comment projection formatting failed.")]
    Formatting,
}

pub fn project(
    input: Input<'_>,
    limits: PostLimits,
) -> Result<AdmissionProjection, ProjectionError> {
    if input.source_path != SourcePath::OrdinaryWithoutExif {
        return Err(ProjectionError::Unsupported(input.source_path));
    }
    if crate::BoardSlug::parse(input.board).is_err()
        || input.prepared_comment.text.len() > limits.prepared_bytes()
        || input.prepared_comment.text.chars().count() > limits.prepared_chars()
        || input.prepared_comment.text.contains('\r')
    {
        return Err(ProjectionError::InvalidInput);
    }
    let budget = WordfilterLimits::for_post(limits);
    let (prefix, suffix) = match input.randomizer {
        None => (String::new(), String::new()),
        Some(outcome) => {
            if !crate::posting_randomizers::valid_outcome(outcome) {
                return Err(ProjectionError::InvalidInput);
            }
            match outcome {
                Outcome::Dice(text) => (format!("<b>{text}<br><br></b>"), String::new()),
                Outcome::Fortune { text, color } => (
                    String::new(),
                    format!(
                        "<span class=\"fortune\" style=\"color:{color}\"><br><br><b>Your fortune: {text}</b></span>"
                    ),
                ),
            }
        }
    };
    // Randomizer decorations precede markup, so an unclosed tag can contain
    // the fortune suffix. They also participate in the global filter/wrap pass.
    // Posting preparation already performed the initial marker cleanup. Doing
    // it again changes wrap positions when that pass joined a new marker.
    let mut data = crate::comment_markup::admission_source(
        input.prepared_comment.text,
        input.markup,
        &prefix,
        &suffix,
    );
    bounded(&data, budget.output_bytes())?;
    if let Filter::On { profile, rolls } = input.filter {
        if (profile == Profile::Test) != rolls.is_some() {
            return Err(ProjectionError::InvalidInput);
        }
        data = crate::wordfilter::apply_with_limits(
            &data,
            crate::wordfilter::Field::Comment,
            profile,
            rolls,
            budget,
        )
        .map_err(|_| ProjectionError::Formatting)?;
    }
    data = linkify(&data, input.board, budget.output_bytes())?;
    data = wrap_source(&data);
    bounded(&data, budget.output_bytes())?;
    data = replace(
        &data,
        r"(&gt;&gt;&gt;/[a-z0-9]+/[^ <$]*|&gt;&gt;[0-9]+)",
        budget.output_bytes(),
        |c| format!("~?rep?~{}~?erep?~", capture(c, 1)),
    )?;
    data = replace(
        &data,
        r"(^|r>|r> )(&gt;[^<]*)",
        budget.output_bytes(),
        |c| {
            format!(
                "{}<span class=\"quote\">{}</span>",
                capture(c, 1),
                capture(c, 2)
            )
        },
    )?;
    data = replace(
        &data,
        r#"~?rep?~<span class="quote">(.+?)</span>~?erep?~"#,
        budget.output_bytes(),
        |c| capture(c, 1).into(),
    )?;
    data = crate::filtered_formatting::remove_source_markers(&data).replace("{{w_br}}", "<wbr>");
    bounded(&data, budget.output_bytes())?;
    Ok(AdmissionProjection { data })
}

fn bounded(data: &str, limit: usize) -> Result<(), ProjectionError> {
    if data.len() > limit {
        Err(ProjectionError::TooLarge)
    } else {
        Ok(())
    }
}
fn capture<'a>(captures: &'a Captures<'a>, index: usize) -> &'a str {
    std::str::from_utf8(
        captures
            .get(index)
            .expect("fixed source capture")
            .as_bytes(),
    )
    .expect("source capture boundaries")
}
fn replace(
    input: &str,
    pattern: &str,
    limit: usize,
    mut replacement: impl FnMut(&Captures<'_>) -> String,
) -> Result<String, ProjectionError> {
    let regex = Regex::new(pattern).map_err(|_| ProjectionError::Formatting)?;
    let mut result = String::new();
    let mut end = 0;
    for captures in regex.captures_iter(input.as_bytes()) {
        let captures = captures.map_err(|_| ProjectionError::Formatting)?;
        let matched = captures.get(0).ok_or(ProjectionError::Formatting)?;
        result.push_str(&input[end..matched.start()]);
        result.push_str(&replacement(&captures));
        bounded(&result, limit)?;
        end = matched.end();
    }
    result.push_str(&input[end..]);
    bounded(&result, limit)?;
    Ok(result)
}

fn linkify(input: &str, board: &str, limit: usize) -> Result<String, ProjectionError> {
    let normalize = crate::server_link::source_probe(input);
    let mut linked = if normalize {
        normalize_source_links(input, board, limit)?
    } else {
        input.into()
    };
    if normalize && crate::server_link::link_probe(&linked) {
        linked = replace(
            &linked,
            r"(?i)(https?://(?:[A-Za-z]*\.)?)(4chan|4channel|4cdn)(\.org)(/[\w\-\.,@?^=%&;:/~+#()]*[\w\-@?^=%&;/~+#])?",
            limit,
            |c| {
                let link = capture(c, 0)
                    .replace("&gt;&gt;&gt;", "")
                    .replace("&gt;&gt;", "");
                format!("<a href=\"{link}\" target=\"_blank\">{link}</a>")
            },
        )?;
    }
    replace(
        &linked,
        r"&gt;&gt;&gt;/([a-z0-9]+)/([a-z0-9+/,l\-]*)",
        limit,
        |c| {
            let original = capture(c, 0);
            // The source calls is_numeric() before choosing a catalog URL.
            // The display parser applies the same lexical rule independently.
            if source_numeric_static_term(capture(c, 2)) {
                return original.into();
            }
            let target = capture(c, 1);
            let term = capture(c, 2);
            let (href, new_tab) = if term.starts_with("rules") {
                let rule = term.split_once('/').map_or("", |(_, rule)| rule);
                (format!("//www.4chan.org/rules#{target}{rule}"), true)
            } else if crate::static_quote::reference_board(target) {
                if target == "f" && term == "catalog" {
                    return original.into();
                }
                let base = format!("//boards.4chan.org/{target}/");
                let href = if term.is_empty() {
                    base
                } else if term == "catalog" {
                    format!("{base}catalog")
                } else {
                    let decoded = term.replace('+', " ");
                    let encoded: String =
                        url::form_urlencoded::byte_serialize(decoded.as_bytes()).collect();
                    format!("{base}catalog#s={encoded}")
                };
                (href, false)
            } else {
                return original.into();
            };
            format!(
                "<a href=\"{href}\" class=\"quotelink\"{}>{original}</a>",
                if new_tab { " target=\"_blank\"" } else { "" }
            )
        },
    )
}

fn normalize_source_links(
    input: &str,
    board: &str,
    limit: usize,
) -> Result<String, ProjectionError> {
    // Exact byte-mode imgboard.php:3880 pattern. The dot before php is
    // intentionally unescaped; Unicode whitespace is not PHP's byte-mode \s.
    replace(
        input,
        r"(?i)https?://([a-z]*)[.](?:4chan|4channel)[.]org/(\w+)/(?:(res|thread)/(\d+)(?:/[-a-z0-9]+)?(?:#[qp]?(\d*))?|(catalog(?:#s=[a-z0-9+]+)?)|\w+.php[?]res=(\d+)(?:#[qp]?(\d*))?|)(?=[\s.<!?,]|$)",
        limit,
        |c| {
            if capture(c, 1) != "boards" {
                return capture(c, 0).into();
            }
            let target = capture(c, 2).to_ascii_lowercase();
            // PHP scans backwards for a truthy capture, so even a captured "0"
            // must be skipped. A root URL leaves its number unset/empty.
            let number = (3..=8)
                .rev()
                .filter_map(|index| c.get(index))
                .map(|matched| {
                    std::str::from_utf8(matched.as_bytes()).expect("ASCII source capture")
                })
                .find(|value| !value.is_empty() && *value != "0")
                .unwrap_or("");
            if number
                .get(..7)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("catalog"))
            {
                let term = number
                    .to_ascii_lowercase()
                    .find("#s=")
                    .map_or("catalog", |offset| &number[offset + 3..]);
                format!("&gt;&gt;&gt;/{target}/{term}")
            } else if target == board && !number.is_empty() && number != "catalog" {
                format!("&gt;&gt;{number}")
            } else {
                format!("&gt;&gt;&gt;/{target}/{number}")
            }
        },
    )
}

/// PHP numeric syntax restricted to auto_link_static_cb's captured alphabet:
/// [a-z0-9+/,l-]*. Dots, whitespace and uppercase E cannot reach this callback.
/// Exponent magnitude is irrelevant; neither integer nor float conversion is
/// appropriate for this lexical decision.
fn source_numeric_static_term(term: &str) -> bool {
    fn unsigned_digits(value: &str) -> bool {
        !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
    }
    let term = term.strip_prefix(['+', '-']).unwrap_or(term);
    match term.split_once('e') {
        Some((mantissa, exponent)) => {
            unsigned_digits(mantissa)
                && unsigned_digits(exponent.strip_prefix(['+', '-']).unwrap_or(exponent))
        }
        None => unsigned_digits(term),
    }
}

fn wrap_source(input: &str) -> String {
    if input.chars().count() < 35
        || !input
            .as_bytes()
            .split(|byte| b" <>".contains(byte))
            .any(|run| run.len() >= 35)
    {
        return input.into();
    }
    let mut result = String::new();
    for (index, part) in input.split(['<', '>']).enumerate() {
        if index % 2 == 1 {
            result.push('<');
            result.push_str(part);
            result.push('>');
            continue;
        }
        for (word_index, word) in part.split(' ').enumerate() {
            if word_index > 0 {
                result.push(' ');
            }
            let decoded = crate::semantic_context::decode_special_entities(word);
            let mut wrapped = String::new();
            let mut count = 0;
            for ch in decoded.chars() {
                wrapped.push(ch);
                if ch == '\n' {
                    count = 0;
                } else {
                    count += 1;
                    if count == 35 {
                        wrapped.push_str("{{w_br}}");
                        count = 0;
                    }
                }
            }
            result.push_str(&crate::source_html_entities(&wrapped));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn ordinary<'a>(comment: &'a str) -> Input<'a> {
        Input {
            prepared_comment: PreparedComment { text: comment },
            board: "g",
            markup: MarkupPolicy::default(),
            filter: Filter::Off,
            randomizer: None,
            source_path: SourcePath::OrdinaryWithoutExif,
        }
    }
    fn limits() -> PostLimits {
        PostLimits::ordinary(crate::MAX_COMMENT_CHARS)
    }

    #[test]
    fn independent_php_composition_and_filter_off_vectors() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/wordfilter-posting-reference.json"
        ))
        .unwrap();
        let mut checked = 0;
        let mut filter_off = 0;
        for (name, profile) in [
            ("global", Profile::Global),
            ("ck", Profile::Basic),
            ("asp", Profile::Asp),
            ("v", Profile::Video),
            ("test", Profile::Test),
        ] {
            for case in fixture["admission_projection"][name]
                .as_array()
                .expect("independent projection fixture group")
            {
                let p = &case["policy"];
                let randomizer = match case["randomizer"]["kind"].as_str() {
                    Some("dice") => Some(Outcome::Dice(
                        case["randomizer"]["text"].as_str().unwrap().into(),
                    )),
                    Some("fortune") => {
                        assert_eq!(
                            case["randomizer"]["text"],
                            "You will meet a dark handsome stranger"
                        );
                        Some(Outcome::Fortune {
                            text: "You will meet a dark handsome stranger",
                            color: case["randomizer"]["color"].as_str().unwrap().into(),
                        })
                    }
                    None => None,
                    _ => panic!("unknown fixture outcome"),
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
                let projection = project(
                    Input {
                        prepared_comment: PreparedComment {
                            text: case["prepared_comment"].as_str().unwrap(),
                        },
                        board: case["board"].as_str().unwrap(),
                        markup: MarkupPolicy {
                            spoilers: p["spoilers"].as_bool().unwrap(),
                            code: p["code"].as_bool().unwrap(),
                            sjis: p["sjis"].as_bool().unwrap(),
                            op: p["op"].as_bool().unwrap(),
                        },
                        filter: if enabled {
                            Filter::On { profile, rolls }
                        } else {
                            Filter::Off
                        },
                        randomizer: randomizer.as_ref(),
                        source_path: SourcePath::OrdinaryWithoutExif,
                    },
                    limits(),
                )
                .unwrap();
                assert_eq!(
                    projection.as_comparison_data(),
                    case["final"].as_str().unwrap(),
                    "profile={name}, case={case}"
                );
                assert_eq!(
                    projection.source_check_applies(),
                    case["source_check_applies"].as_bool().unwrap()
                );
                assert_eq!(projection.version(), VERSION);
                checked += 1;
                filter_off += usize::from(!enabled);
            }
        }
        assert!(checked >= 300);
        assert!(filter_off >= 50);
    }

    #[test]
    fn existing_filtered_source_vectors_also_remain_compatible() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/wordfilter-posting-reference.json"
        ))
        .unwrap();
        for (name, profile) in [
            ("global", Profile::Global),
            ("ck", Profile::Basic),
            ("asp", Profile::Asp),
            ("v", Profile::Video),
            ("test", Profile::Test),
        ] {
            for case in fixture["profiles"][name].as_array().unwrap() {
                let rolls = if profile == Profile::Test {
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
                let mut input = ordinary(case["admission_input"].as_str().unwrap());
                input.markup = MarkupPolicy {
                    spoilers: true,
                    code: true,
                    sjis: true,
                    op: true,
                };
                input.filter = Filter::On { profile, rolls };
                let actual = project(input, limits()).unwrap();
                assert_eq!(
                    actual.as_comparison_data(),
                    case["final"].as_str().unwrap(),
                    "{name} {case}"
                );
            }
        }
    }

    #[test]
    fn prepared_stage_retains_newly_joined_markers_until_after_wrapping() {
        let prepared = crate::prepare_post_content_input(
            "",
            "",
            "https://www.4chan.org/faq/~?re~?rep?~p?~owned",
            crate::MAX_COMMENT_CHARS,
            false,
            crate::CommentSpacing::for_board("g", true, true),
            crate::PostKind::Reply,
        )
        .unwrap();
        assert_eq!(prepared.comment(), "https://www.4chan.org/faq/~?rep?~owned");
        let mut input = ordinary("");
        input.prepared_comment = PreparedComment::from(&prepared);
        assert_eq!(
            project(input, limits()).unwrap().as_comparison_data(),
            "<a href=\"https://www.4chan.org/faq/owned\" target=\"_blank\">https://www.4chan.org/faq/ow<wbr>ned</a>"
        );
    }

    #[test]
    fn static_numeric_terms_are_lexical_even_when_floats_would_overflow() {
        for term in [
            "1e2",
            "1e+2",
            "1e-2",
            "+001e+02",
            "-0e-000",
            "1e999999999999999999999999999999999999",
            "-1e-999999999999999999999999999999999999",
        ] {
            assert!(source_numeric_static_term(term), "{term}");
        }
        for term in [
            "", "+", "e2", "1e", "1e+", "1e-", "1e--2", "1e+-2", "1e2e3", "1e2/", "1e2,", "1e2l",
            "0x1", "nan", "inf",
        ] {
            assert!(!source_numeric_static_term(term), "{term}");
        }
    }

    #[test]
    fn truthiness_bounds_and_unknown_provenance_are_explicit() {
        for (comment, applies) in [("", false), ("0", false), ("00", true), ("0\n", true)] {
            assert_eq!(
                project(ordinary(comment), limits())
                    .unwrap()
                    .source_check_applies(),
                applies
            );
        }
        for source_path in [
            SourcePath::UnknownExif,
            SourcePath::ExifPresent,
            SourcePath::PrivilegedHtml,
            SourcePath::Oekaki,
        ] {
            let mut input = ordinary("text");
            input.source_path = source_path;
            assert_eq!(
                project(input, limits()),
                Err(ProjectionError::Unsupported(source_path))
            );
        }
        assert_eq!(
            project(
                ordinary(&"x".repeat(limits().prepared_chars() + 1)),
                limits()
            ),
            Err(ProjectionError::InvalidInput)
        );
        assert_eq!(
            project(ordinary("raw\rtext"), limits()),
            Err(ProjectionError::InvalidInput)
        );
        let mut input = ordinary("text");
        input.filter = Filter::On {
            profile: Profile::Test,
            rolls: None,
        };
        assert_eq!(project(input, limits()), Err(ProjectionError::InvalidInput));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]
        // Bounds and panic regression only. The PHP fixtures above, not this
        // generator, establish source equivalence.
        #[test]
        fn prepared_unicode_compositions_remain_bounded(
            scalars in proptest::collection::vec(any::<char>(), 0..2048),
            prefix in 0usize..7,
            policy_bits in 0u8..16,
            filter_choice in 0u8..6,
            first_roll in 0u8..6,
            second_roll in 0u8..6,
            randomizer_choice in 0u8..3,
        ) {
            let prefixes = [
                "", "0", "~?re~?rep?~p?~", "[spoiler][code]",
                "[sjis][b]", ">>>/g/catalog\n>",
                "https://www.4chan.org/faq/~?re~?rep?~p?~",
            ];
            let raw = format!("{}{}", prefixes[prefix], scalars.into_iter().collect::<String>());
            let policy = MarkupPolicy {
                spoilers: policy_bits & 1 != 0,
                code: policy_bits & 2 != 0,
                sjis: policy_bits & 4 != 0,
                op: policy_bits & 8 != 0,
            };
            // Also try a control-free companion, so long arbitrary samples
            // cannot make every projection path vacuous through rejection.
            let supported: String = raw.chars().filter(|ch| !ch.is_control() || matches!(ch, '\n' | '\r' | '\t')).collect();
            for raw in [raw, supported] {
            let prepared = crate::prepare_post_content_input(
                "", "", &raw, crate::MAX_COMMENT_CHARS, true,
                crate::CommentSpacing::for_board("g", policy.code, policy.sjis)
                    .with_line_rules(100, policy.spoilers)
                    .with_op_markup(policy.op),
                crate::PostKind::Reply,
            );
            // Invalid controls, excessive lines and other preparation failures
            // are ordinary rejections, not discarded proptest samples.
            if let Ok(prepared) = prepared {
                let filter = match filter_choice {
                    0 => Filter::Off,
                    choice => {
                        let profile = [Profile::Global, Profile::Basic, Profile::Asp, Profile::Video, Profile::Test][usize::from(choice - 1)];
                        Filter::On {
                            profile,
                            rolls: if profile == Profile::Test {
                                Some(LeetRolls::from_choices(first_roll, second_roll).unwrap())
                            } else { None },
                        }
                    }
                };
                let randomizer = match randomizer_choice {
                    0 => None,
                    1 => Some(Outcome::Dice("Rolled 1, 1 + 3 = 5 (2d1 + 3)".into())),
                    _ => Some(Outcome::Fortune {
                        text: "You will meet a dark handsome stranger",
                        color: "#0893e1".into(),
                    }),
                };
                let result = project(Input {
                    prepared_comment: PreparedComment::from(&prepared),
                    board: "g",
                    markup: policy,
                    filter,
                    randomizer: randomizer.as_ref(),
                    source_path: SourcePath::OrdinaryWithoutExif,
                }, limits());
                match result {
                    Ok(projection) => {
                        let data = projection.as_comparison_data();
                        prop_assert!(data.len() <= WordfilterLimits::for_post(limits()).output_bytes());
                        prop_assert_eq!(projection.version(), VERSION);
                        prop_assert_eq!(projection.source_check_applies(), !data.is_empty() && data != "0");
                    }
                    Err(ProjectionError::TooLarge | ProjectionError::Formatting) => {}
                    Err(error) => prop_assert!(false, "valid prepared context rejected: {error}"),
                }
            }
            }
        }
    }

    #[test]
    fn constructible_randomizer_variants_do_not_grant_html_authority() {
        for randomizer in [
            Outcome::Dice("<script>bad</script>".into()),
            Outcome::Dice("Rolled 1, 1 + 3 = 6 (2d1 + 3)".into()),
            Outcome::Fortune {
                text: "Bad Luck",
                color: "#0893e1".into(),
            },
            Outcome::Fortune {
                text: "<b>invented</b>",
                color: "#0893e1".into(),
            },
        ] {
            let mut input = ordinary("text");
            input.randomizer = Some(&randomizer);
            assert_eq!(project(input, limits()), Err(ProjectionError::InvalidInput));
        }
    }
}
