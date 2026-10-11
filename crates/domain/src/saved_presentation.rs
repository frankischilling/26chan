//! Frozen final source formatting for bounded text-thread transfers.
//! FP01/FP02 are finite presentation data, not HTML and not wordfilter programs.
//! Dynamic post references remain lexical text until request-local resolution.

mod codec;
#[cfg(test)]
mod tests;

use crate::comment_markup::Tag;
use crate::formatting::{Line, Token};
use crate::server_link::ServerLink;
use crate::static_quote::StaticQuote;
use crate::word_break::WordPart;
use crate::wordfiltered_comment::{ChangedEntity, Delimiter, PreparedComment};
use crate::{PostLimits, WordfilterLimits};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresentationError {
    InvalidEncoding,
    UnsupportedFormat,
    InconsistentSavedComment,
    LimitExceeded,
    WorkLimitExceeded,
    InvalidReplacement,
}

impl std::fmt::Display for PresentationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidEncoding => "Invalid saved presentation.",
            Self::UnsupportedFormat => "Comment format cannot be transferred.",
            Self::InconsistentSavedComment => "Saved comment presentation is inconsistent.",
            Self::LimitExceeded => "Saved presentation exceeds its limits.",
            Self::WorkLimitExceeded => "Comment transfer exceeds its work limit.",
            Self::InvalidReplacement => "Invalid transferred post number.",
        })
    }
}
impl std::error::Error for PresentationError {}

type Result<T> = std::result::Result<T, PresentationError>;

/// Implementation bound aligned with the existing 1,001-row thread snapshot.
/// The PHP reply query itself does not supply an ordering or this limit.
pub const MAX_PRIOR_REPLACEMENTS: usize = 1001;

/// Complete source values, not public/search excerpts. Limits express existing
/// storage authority; constructing this input grants no posting authority.
pub struct SavedCommentRef<'a> {
    pub comment: &'a str,
    pub format: i16,
    pub source_board: &'a str,
    pub wordfilter_payload: Option<&'a [u8]>,
    pub saved_presentation: Option<&'a [u8]>,
    pub limits: PostLimits,
}

#[derive(Clone, Debug)]
pub struct PriorReplacement {
    from: String,
    to: String,
}

impl PriorReplacement {
    pub fn new(old: i64, new: i64) -> Result<Self> {
        if old <= 0 || new <= 0 {
            return Err(PresentationError::InvalidReplacement);
        }
        Ok(Self {
            from: format!(">>{old}"),
            to: format!(">>{new}"),
        })
    }
}

/// Share one caller-selected budget across the entire transaction. Failed work
/// remains charged; retrying a comment does not reset the transaction budget.
#[derive(Debug)]
pub struct RewriteBudget {
    remaining: usize,
}

impl RewriteBudget {
    pub fn new(maximum_work_bytes: usize) -> Self {
        Self {
            remaining: maximum_work_bytes,
        }
    }
    pub fn remaining(&self) -> usize {
        self.remaining
    }
    fn charge(&mut self, input: usize, output: usize) -> Result<()> {
        match input
            .checked_add(output)
            .and_then(|work| self.remaining.checked_sub(work))
        {
            Some(remaining) => {
                self.remaining = remaining;
                Ok(())
            }
            None => {
                self.remaining = 0;
                Err(PresentationError::WorkLimitExceeded)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Atom {
    Text(String),
    LineBreak,
    WordBreak,
    Markup(Tag, bool),
    QuoteSpan(bool),
    FilteredDelimiter(Delimiter),
    ChangedEntity(ChangedEntity),
    StaticLink(StaticQuote, Vec<WordPart>),
    ServerLink(ServerLink, Vec<WordPart>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrozenPresentation {
    atoms: Vec<Atom>,
    bounds: WordfilterLimits,
    wordfiltered: bool,
}

impl FrozenPresentation {
    /// Reconstruct only qualified source profiles, without current board filters.
    /// Repeated moves decode the frozen representation without reformatting it.
    pub fn from_saved(input: SavedCommentRef<'_>) -> Result<Self> {
        if !matches!(input.format, 104..=111 | 120..=127) {
            return Err(PresentationError::UnsupportedFormat);
        }
        let bounds = WordfilterLimits::for_post(input.limits);
        if input.comment.len() > bounds.output_bytes() {
            return Err(PresentationError::LimitExceeded);
        }
        if let Some(bytes) = input.saved_presentation {
            if input.wordfilter_payload.is_some() {
                return Err(PresentationError::InconsistentSavedComment);
            }
            let frozen = Self::decode(bytes, input.limits)?;
            if frozen.source_projection() != input.comment {
                return Err(PresentationError::InconsistentSavedComment);
            }
            return Ok(frozen);
        }
        crate::BoardSlug::parse(input.source_board)
            .map_err(|_| PresentationError::InvalidEncoding)?;
        let lines = if let Some(bytes) = input.wordfilter_payload {
            if !bytes.starts_with(bounds.version()) {
                return Err(PresentationError::InvalidEncoding);
            }
            let prepared =
                PreparedComment::decode(bytes).map_err(|_| PresentationError::InvalidEncoding)?;
            let lines = crate::filtered_formatting::lines(&prepared, input.source_board);
            if crate::filtered_formatting::source_projection(&lines) != input.comment {
                return Err(PresentationError::InconsistentSavedComment);
            }
            lines
        } else {
            // The normal parser deliberately clips oversized input. Transfers
            // reject it first so no source content is silently lost.
            if input.comment.len() > input.limits.prepared_bytes()
                || input.comment.chars().count() > input.limits.prepared_chars()
            {
                return Err(PresentationError::LimitExceeded);
            }
            crate::formatting::parse_post_comment_on_board_with_limits(
                input.comment,
                input.format,
                input.source_board,
                input.limits,
            )
        };
        Self::freeze(lines, bounds, input.wordfilter_payload.is_some())
    }

    fn freeze(lines: Vec<Line>, bounds: WordfilterLimits, wordfiltered: bool) -> Result<Self> {
        let mut result = Self {
            atoms: Vec::new(),
            bounds,
            wordfiltered,
        };
        for (index, line) in lines.into_iter().enumerate() {
            if index > 0 {
                result.atoms.push(Atom::LineBreak);
            }
            if line.green {
                return Err(PresentationError::UnsupportedFormat);
            }
            for token in line.tokens {
                let atom = match token {
                    Token::Text(text) => Atom::Text(text),
                    Token::PostQuote(quote) => Atom::Text(quote.label().into()),
                    Token::WordBreak => Atom::WordBreak,
                    Token::OpenMarkup(tag) => Atom::Markup(tag, true),
                    Token::CloseMarkup(tag) => Atom::Markup(tag, false),
                    Token::OpenQuote => Atom::QuoteSpan(true),
                    Token::CloseQuote => Atom::QuoteSpan(false),
                    Token::FilteredDelimiter(value) => Atom::FilteredDelimiter(value),
                    Token::ChangedEntity(value) => Atom::ChangedEntity(value),
                    Token::StaticQuote(quote, label) => Atom::StaticLink(quote, label),
                    Token::ServerLink(link, label) => Atom::ServerLink(link, label),
                    _ => return Err(PresentationError::UnsupportedFormat),
                };
                if let Atom::Text(text) = atom {
                    if text.is_empty() {
                        continue;
                    }
                    if let Some(Atom::Text(previous)) = result.atoms.last_mut() {
                        previous.push_str(&text);
                    } else {
                        result.atoms.push(Atom::Text(text));
                    }
                } else {
                    result.atoms.push(atom);
                }
            }
        }
        result.validate()?;
        Ok(result)
    }

    pub fn was_wordfiltered(&self) -> bool {
        self.wordfiltered
    }

    /// Sequential PHP str_replace semantics. Only preceding successful inserts
    /// belong in `prior`. The original remains unchanged on every error.
    pub fn rewrite_prior(
        &mut self,
        prior: &[PriorReplacement],
        budget: &mut RewriteBudget,
    ) -> Result<()> {
        if prior.len() > MAX_PRIOR_REPLACEMENTS {
            return Err(PresentationError::LimitExceeded);
        }
        if prior.is_empty() {
            return Ok(());
        }
        // Reserve before any clone or content validation. Metadata walks are
        // themselves charged before traversal; string lengths are O(1).
        self.reserve_snapshot_pass(budget)?;
        self.reserve_snapshot_pass(budget)?;
        let mut candidate = self.clone();
        let mut sizes = candidate.validate()?;
        for replacement in prior {
            // Both the scan loop and the later changed-index loop have fixed
            // traversal cost, even for a comment containing only delimiters.
            budget.charge(candidate.atoms.len(), candidate.atoms.len())?;
            let mut changes = Vec::new();
            for (index, atom) in candidate.atoms.iter().enumerate() {
                let Atom::Text(text) = atom else { continue };
                budget.charge(text.len(), 0)?;
                let count = text.match_indices(&replacement.from).count();
                let removed = count
                    .checked_mul(replacement.from.len())
                    .ok_or(PresentationError::LimitExceeded)?;
                let added = count
                    .checked_mul(replacement.to.len())
                    .ok_or(PresentationError::LimitExceeded)?;
                let length = text
                    .len()
                    .checked_sub(removed)
                    .and_then(|n| n.checked_add(added))
                    .ok_or(PresentationError::LimitExceeded)?;
                // Replacements change only ASCII digits, so escaped/source,
                // encoded and visible-search lengths have the same delta.
                sizes.replace_text(text.len(), length)?;
                sizes.check(candidate.bounds)?;
                // Reserve the second match scan and complete output before
                // allocation. A size refusal above still charged its scan.
                budget.charge(if count > 0 { text.len() } else { 0 }, length)?;
                if count > 0 {
                    changes.push((index, length));
                }
            }
            for (index, length) in changes {
                let Atom::Text(text) = &mut candidate.atoms[index] else {
                    unreachable!("text index")
                };
                let mut output = String::with_capacity(length);
                let mut offset = 0;
                for (start, _) in text.match_indices(&replacement.from) {
                    output.push_str(&text[offset..start]);
                    output.push_str(&replacement.to);
                    offset = start + replacement.from.len();
                }
                output.push_str(&text[offset..]);
                *text = output;
            }
        }
        candidate.reserve_snapshot_pass(budget)?;
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }

    /// One bounded structural/content pass. Nested-label structure is reserved
    /// before looking at its entries. Static links own both a spelling and its
    /// display parts, so their text is charged twice without allocating a label.
    fn reserve_snapshot_pass(&self, budget: &mut RewriteBudget) -> Result<()> {
        budget.charge(self.atoms.len(), 0)?;
        for atom in &self.atoms {
            match atom {
                Atom::Text(text) => budget.charge(text.len(), 0)?,
                Atom::StaticLink(_, parts) => {
                    budget.charge(parts.len(), 0)?;
                    for part in parts {
                        if let WordPart::Text(text) = part {
                            budget.charge(text.len(), text.len())?;
                        }
                    }
                }
                Atom::ServerLink(link, parts) => {
                    budget.charge(link.href().len(), parts.len())?;
                    for part in parts {
                        if let WordPart::Text(text) = part {
                            budget.charge(text.len(), 0)?;
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn lines(&self, dynamic_quotes: bool) -> Vec<Line> {
        let mut lines = vec![Line {
            green: false,
            tokens: Vec::new(),
        }];
        for atom in &self.atoms {
            if matches!(atom, Atom::LineBreak) {
                lines.push(Line {
                    green: false,
                    tokens: Vec::new(),
                });
                continue;
            }
            let output = &mut lines.last_mut().expect("initial line").tokens;
            if let Atom::Text(text) = atom {
                if dynamic_quotes {
                    output.extend(crate::formatting::tokenize_with(
                        text, false, true, false, false, true,
                    ));
                } else {
                    output.push(Token::Text(text.clone()));
                }
            } else {
                output.push(match atom {
                    Atom::WordBreak => Token::WordBreak,
                    Atom::Markup(tag, true) => Token::OpenMarkup(*tag),
                    Atom::Markup(tag, false) => Token::CloseMarkup(*tag),
                    Atom::QuoteSpan(true) => Token::OpenQuote,
                    Atom::QuoteSpan(false) => Token::CloseQuote,
                    Atom::FilteredDelimiter(value) => Token::FilteredDelimiter(*value),
                    Atom::ChangedEntity(value) => Token::ChangedEntity(*value),
                    Atom::StaticLink(quote, label) => {
                        Token::StaticQuote(quote.clone(), label.clone())
                    }
                    Atom::ServerLink(link, label) => Token::ServerLink(link.clone(), label.clone()),
                    _ => unreachable!("handled text and line break"),
                });
            }
        }
        lines
    }

    /// Only late post-number lexing. No markup/filter/normalization/wrap pass.
    pub fn unresolved_lines(&self) -> Vec<Line> {
        self.lines(true)
    }

    /// Comparison/storage data only. Never mark this string as template-safe.
    pub fn source_projection(&self) -> String {
        crate::filtered_formatting::source_projection(&self.lines(false))
    }

    pub fn search_text(&self) -> String {
        crate::formatting::plain_text(&self.lines(false))
    }
}

#[derive(Default)]
struct Sizes {
    encoded: usize,
    source: usize,
    search: usize,
}

impl Sizes {
    fn check(&self, bounds: WordfilterLimits) -> Result<()> {
        if self.encoded > bounds.stored_bytes()
            || self.source > bounds.output_bytes()
            || self.search > bounds.stored_bytes()
        {
            return Err(PresentationError::LimitExceeded);
        }
        Ok(())
    }
    fn replace_text(&mut self, old: usize, new: usize) -> Result<()> {
        for size in [&mut self.encoded, &mut self.source, &mut self.search] {
            *size = size
                .checked_sub(old)
                .and_then(|n| n.checked_add(new))
                .ok_or(PresentationError::LimitExceeded)?;
        }
        Ok(())
    }
}

fn escaped_len(text: &str) -> usize {
    text.chars()
        .map(|ch| match ch {
            '&' => 5,
            '<' | '>' => 4,
            '"' | '\'' => 6,
            _ => ch.len_utf8(),
        })
        .sum()
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

fn tag_from(number: u8) -> Result<Tag> {
    Ok(match number {
        0 => Tag::Spoiler,
        1 => Tag::Code,
        2 => Tag::Sjis,
        3 => Tag::Bold,
        4 => Tag::Italic,
        5 => Tag::Red,
        6 => Tag::Green,
        7 => Tag::Blue,
        _ => return Err(PresentationError::InvalidEncoding),
    })
}
