//! Operator-owned content admission. Public fields never carry rule or actor
//! authority. Decisions contain effects for a transactional caller to apply.
use crate::admission_normalization::{NormalizationError, Normalizer};
use pcre2::bytes::{Regex, RegexBuilder};

pub const MAX_RULES: usize = 128;
pub const MAX_PATTERN_BYTES: usize = 2_048;
pub const MAX_MATCH_CALLS: usize = 1_024;
const REGEX_LIMITS: &str = "(*LIMIT_MATCH=50000)(*LIMIT_DEPTH=64)(*LIMIT_HEAP=1024)";

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AdmissionError {
    #[error("Content admission policy is invalid.")]
    InvalidPolicy,
    #[error("Content admission work limit exceeded.")]
    WorkLimit,
    #[error("Content admission is unavailable.")]
    Unavailable,
    #[error(transparent)]
    Normalization(#[from] NormalizationError),
}

pub struct Rule {
    pub id: i64,
    /// Empty is global. A validated board slug restricts the rule to that board.
    pub board: String,
    pub pattern: String,
    pub regex: bool,
    pub autosage: bool,
    pub log: bool,
    pub quiet: bool,
    pub lenient: bool,
    pub ops_only: bool,
    pub min_count: i32,
    pub ban_days: i32,
}

pub struct Post<'a> {
    pub board: &'a str,
    pub reply: bool,
    pub name: &'a str,
    pub subject: &'a str,
    pub comment: &'a str,
    pub filename: &'a str,
}

/// The store constructs this from locked private activity, never request fields.
#[derive(Default, Clone, Copy)]
pub struct Actor {
    pub session_present: bool,
    pub known_or_verified: bool,
    pub posts: u16,
}

impl Actor {
    pub fn from_state(state: &crate::anonymous_session::State, now: u64) -> Self {
        Self {
            session_present: true,
            known_or_verified: state.is_known_or_verified(now, 1_440, 0),
            posts: state.post_count(),
        }
    }

    fn lenient(self) -> bool {
        self.session_present && self.known_or_verified && self.posts > 10
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Autosage {
        rule: i64,
    },
    Log {
        rule: i64,
        comment: String,
    },
    Reject {
        rule: i64,
        ban_days: i32,
        quiet: bool,
    },
    FilenameProxy,
    InvalidSubject {
        logged_rule: Option<i64>,
        comment: String,
    },
}

enum Pattern {
    Literal(String),
    Regex(BoundedRegex),
}

struct CompiledRule {
    rule: Rule,
    pattern: Pattern,
    minimum: usize,
}

/// Rules retain the caller's explicit ordering. The database policy must define
/// that order; the supplied source query has no ORDER BY guarantee.
pub struct Policy {
    rules: Vec<CompiledRule>,
}

impl Policy {
    pub fn compile(rules: Vec<Rule>) -> Result<Self, AdmissionError> {
        if rules.len() > MAX_RULES {
            return Err(AdmissionError::InvalidPolicy);
        }
        let mut compiled = Vec::with_capacity(rules.len());
        let mut ids = std::collections::HashSet::new();
        for rule in rules {
            let minimum = rule.min_count.max(1) as usize;
            if rule.id < 1
                || !ids.insert(rule.id)
                || rule.pattern.len() > MAX_PATTERN_BYTES
                || minimum > MAX_MATCH_CALLS
                || !(-1..=3_650).contains(&rule.ban_days)
                || (!rule.board.is_empty() && crate::BoardSlug::parse(&rule.board).is_err())
                || (!rule.regex && rule.pattern.is_empty() && minimum > 1)
            {
                return Err(AdmissionError::InvalidPolicy);
            }
            let pattern = if rule.regex {
                Pattern::Regex(BoundedRegex::compile(&rule.pattern)?)
            } else {
                Pattern::Literal(rule.pattern.clone())
            };
            compiled.push(CompiledRule {
                rule,
                pattern,
                minimum,
            });
        }
        Ok(Self { rules: compiled })
    }

    pub fn evaluate(&self, post: Post<'_>, actor: Actor) -> Result<Decision, AdmissionError> {
        if crate::BoardSlug::parse(post.board).is_err() {
            return Err(AdmissionError::InvalidPolicy);
        }
        let fields = [post.name, post.subject, post.filename, post.comment];
        let total = fields
            .iter()
            .try_fold(0usize, |sum, value| sum.checked_add(value.len()))
            .ok_or(AdmissionError::WorkLimit)?;
        if total > crate::admission_normalization::MAX_INPUT_BYTES.saturating_sub(3) {
            return Err(AdmissionError::WorkLimit);
        }
        // PHP's non-UTF regexp is byte-wise: the two bytes following "php"
        // must not be LF. This check precedes rules, sessions and subject checks.
        let filename = post.filename.as_bytes();
        if filename.starts_with(b"php")
            && filename.len() >= 5
            && filename[3] != b'\n'
            && filename[4] != b'\n'
            && !filename.contains(&b'.')
        {
            return Ok(Decision::FilenameProxy);
        }
        let comment = strip_matching_markup(post.comment);
        if self.rules.is_empty() {
            let lowered = post.subject.to_ascii_lowercase();
            return Ok(if lowered.contains("moot") || lowered.contains("admin") {
                Decision::InvalidSubject {
                    logged_rule: None,
                    comment,
                }
            } else {
                Decision::Allow
            });
        }
        let normalizer = Normalizer::new()?;
        let literal = normalizer.text(&format!(
            "{}{}{}{}",
            post.name, post.subject, post.filename, comment
        ))?;
        let expanded = format!("{} {} {}", post.name, post.subject, comment);
        let autosage = if post.reply {
            None
        } else {
            Some(normalizer.ascii(
                &title_words(&format!("{} {} {}", post.subject, comment, post.name)),
                true,
            )?)
        };
        let mut remaining = MAX_MATCH_CALLS;
        let mut logged_rule = None;
        for compiled in &self.rules {
            let rule = &compiled.rule;
            if (!rule.board.is_empty() && rule.board != post.board)
                || (rule.lenient && actor.lenient())
                || (post.reply && (rule.ops_only || rule.autosage))
            {
                continue;
            }
            let matches = if rule.autosage {
                match &compiled.pattern {
                    Pattern::Literal(pattern) => literal_matches(
                        autosage.as_deref().expect("OP projection"),
                        pattern,
                        compiled.minimum,
                    ),
                    Pattern::Regex(regex) => {
                        regex.matches(&expanded, compiled.minimum, &mut remaining)?
                    }
                }
            } else {
                false
            };
            // An autosage literal that misses its title-cased projection still
            // falls through to the ordinary normalized projection in the source.
            let matches = matches
                || match &compiled.pattern {
                    Pattern::Literal(pattern) => {
                        literal_matches(&literal, pattern, compiled.minimum)
                    }
                    Pattern::Regex(regex) => {
                        regex.matches(&expanded, compiled.minimum, &mut remaining)?
                    }
                };
            if !matches {
                continue;
            }
            if rule.autosage {
                return Ok(Decision::Autosage { rule: rule.id });
            }
            if rule.log {
                logged_rule = Some(rule.id);
                break;
            }
            return Ok(Decision::Reject {
                rule: rule.id,
                ban_days: rule.ban_days,
                quiet: rule.quiet,
            });
        }
        // The source's normalized-subject local is unused; its actual checks
        // use byte-wise case-insensitive searches on the original subject.
        let lowered = post.subject.to_ascii_lowercase();
        if lowered.contains("moot") || lowered.contains("admin") {
            return Ok(Decision::InvalidSubject {
                logged_rule,
                comment,
            });
        }
        Ok(match logged_rule {
            Some(rule) => Decision::Log { rule, comment },
            None => Decision::Allow,
        })
    }
}

fn literal_matches(text: &str, pattern: &str, minimum: usize) -> bool {
    if minimum == 1 {
        text.contains(pattern)
    } else {
        text.match_indices(pattern).take(minimum).count() >= minimum
    }
}

fn strip_matching_markup(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(offset) = rest.find('[') {
        result.push_str(&rest[..offset]);
        rest = &rest[offset..];
        let matching = [
            "[spoiler]",
            "[/spoiler]",
            "[code]",
            "[/code]",
            "[sjis]",
            "[/sjis]",
        ]
        .into_iter()
        .find(|tag| rest.starts_with(tag));
        if let Some(tag) = matching {
            rest = &rest[tag.len()..];
        } else {
            result.push('[');
            rest = &rest[1..];
        }
    }
    result.push_str(rest);
    result
}

fn title_words(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut rest = input;
    let mut capitalize = true;
    while !rest.is_empty() {
        if rest.starts_with("&gt;") {
            result.push(' ');
            capitalize = true;
            rest = &rest[4..];
            continue;
        }
        let ch = rest.chars().next().expect("remaining scalar");
        rest = &rest[ch.len_utf8()..];
        if matches!(ch, '.' | ',' | '!' | ':' | '>' | '/') {
            while rest
                .as_bytes()
                .first()
                .is_some_and(|ch| b".,!:>/".contains(ch))
            {
                rest = &rest[1..];
            }
            result.push(' ');
            capitalize = true;
        } else {
            result.push(if capitalize {
                ch.to_ascii_uppercase()
            } else {
                ch.to_ascii_lowercase()
            });
            capitalize = matches!(ch, ' ' | '\t' | '\n' | '\r' | '\u{b}' | '\u{c}');
        }
    }
    result
}

struct BoundedRegex {
    regular: Regex,
    nonempty: Regex,
    utf: bool,
    anchored: bool,
}

impl BoundedRegex {
    fn compile(pattern: &str) -> Result<Self, AdmissionError> {
        let (body, modifiers) = delimited(pattern)?;
        let mut builder = RegexBuilder::new();
        let mut utf = false;
        let mut anchored = false;
        let mut inline = String::new();
        for modifier in modifiers.bytes() {
            match modifier {
                b'i' => {
                    builder.caseless(true);
                }
                b'm' => {
                    builder.multi_line(true);
                }
                b's' => {
                    builder.dotall(true);
                }
                b'x' => {
                    builder.extended(true);
                }
                b'u' => {
                    utf = true;
                    builder.utf(true);
                }
                b'A' => {
                    anchored = true;
                }
                b'U' | b'J' | b'n' => inline.push(char::from(modifier)),
                // Study is only an optimization hint, not matching semantics.
                b'S' | b' ' | b'\n' | b'\r' | b'\t' => {}
                _ => return Err(AdmissionError::InvalidPolicy),
            }
        }
        // JIT ignores depth/heap ceilings. Never enable it for these rules.
        builder.jit(false);
        let directives_end = start_directives(body);
        let options = if inline.is_empty() {
            String::new()
        } else {
            format!("(?{inline})")
        };
        // The binding's ucp builder also enables MATCH_INVALID_UTF. Validated
        // UTF-8 and PHP /u semantics require strict checking; use the native
        // UCP directive without that additional permissive option.
        let unicode = if utf { "(*UCP)" } else { "" };
        let ordinary = format!(
            "{REGEX_LIMITS}{unicode}{}{}{}",
            &body[..directives_end],
            options,
            &body[directives_end..]
        );
        let retry = format!(
            "{REGEX_LIMITS}{unicode}(*NOTEMPTY_ATSTART){}{}{}",
            &body[..directives_end],
            options,
            &body[directives_end..]
        );
        Ok(Self {
            regular: builder
                .build(&ordinary)
                .map_err(|_| AdmissionError::InvalidPolicy)?,
            nonempty: builder
                .build(&retry)
                .map_err(|_| AdmissionError::InvalidPolicy)?,
            utf: utf || body[..directives_end].contains("(*UTF)"),
            anchored,
        })
    }

    fn matches(
        &self,
        text: &str,
        minimum: usize,
        remaining: &mut usize,
    ) -> Result<bool, AdmissionError> {
        let mut count = 0usize;
        let mut offset = 0usize;
        let mut retry = false;
        while offset <= text.len() {
            *remaining = remaining.checked_sub(1).ok_or(AdmissionError::WorkLimit)?;
            let regex = if retry { &self.nonempty } else { &self.regular };
            let found = regex
                .find_at(text.as_bytes(), offset)
                .map_err(|_| AdmissionError::WorkLimit)?;
            let found = found.filter(|found| !(self.anchored || retry) || found.start() == offset);
            if let Some(found) = found {
                count += 1;
                if count >= minimum {
                    return Ok(true);
                }
                offset = found.end();
                retry = found.start() == found.end();
            } else if retry {
                if offset == text.len() {
                    break;
                }
                offset += if self.utf {
                    text[offset..]
                        .chars()
                        .next()
                        .ok_or(AdmissionError::Unavailable)?
                        .len_utf8()
                } else {
                    1
                };
                retry = false;
            } else {
                break;
            }
        }
        Ok(false)
    }
}

fn start_directives(body: &str) -> usize {
    let mut offset = 0;
    while body[offset..].starts_with("(*") {
        let Some(end) = body[offset..].find(')') else {
            break;
        };
        // These are passed through to PCRE2 before inline compile options.
        let item = &body[offset + 2..offset + end];
        if !matches!(
            item,
            "UTF"
                | "UCP"
                | "NO_AUTO_POSSESS"
                | "NO_START_OPT"
                | "NO_DOTSTAR_ANCHOR"
                | "NO_JIT"
                | "NOTEMPTY"
                | "NOTEMPTY_ATSTART"
                | "CR"
                | "LF"
                | "CRLF"
                | "ANYCRLF"
                | "ANY"
                | "NUL"
                | "BSR_ANYCRLF"
                | "BSR_UNICODE"
        ) && !item.starts_with("LIMIT_")
        {
            break;
        }
        offset += end + 1;
    }
    offset
}

fn delimited(pattern: &str) -> Result<(&str, &str), AdmissionError> {
    let bytes = pattern.as_bytes();
    let Some(&open) = bytes.first() else {
        return Err(AdmissionError::InvalidPolicy);
    };
    if !open.is_ascii()
        || open.is_ascii_alphanumeric()
        || open.is_ascii_whitespace()
        || open == b'\\'
    {
        return Err(AdmissionError::InvalidPolicy);
    }
    let close = match open {
        b'(' => b')',
        b'[' => b']',
        b'{' => b'}',
        b'<' => b'>',
        _ => open,
    };
    let mut depth = 1;
    let mut escaped = false;
    for (offset, &ch) in bytes.iter().enumerate().skip(1) {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == b'\\' {
            escaped = true;
            continue;
        }
        if open != close && ch == open {
            depth += 1;
        }
        if ch == close {
            depth -= 1;
            if depth == 0 {
                return Ok((&pattern[1..offset], &pattern[offset + 1..]));
            }
        }
    }
    Err(AdmissionError::InvalidPolicy)
}

#[cfg(test)]
mod tests;
