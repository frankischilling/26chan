//! Finish the saved, typed wordfilter result without applying filters again.
use crate::formatting::{Line, Token, tokenize_spanned};
use crate::wordfiltered_comment::{ChangedEntity, Part, PreparedComment};

#[derive(Default)]
struct Text {
    value: String,
    entities: Vec<(std::ops::Range<usize>, ChangedEntity)>,
}

impl Text {
    fn append(&mut self, part: &Part, board: &str, normalize: bool) {
        match part {
            Part::Text(text) => self
                .value
                .push_str(&crate::server_link::normalize_with_probe(
                    text, board, normalize,
                )),
            Part::ChangedEntity(entity) => {
                let start = self.value.len();
                self.value.push_str(entity.spelling());
                self.entities.push((start..self.value.len(), *entity));
            }
            _ => unreachable!("text run"),
        }
    }

    fn source_slice(&self, range: std::ops::Range<usize>, output: &mut String) {
        let mut cursor = range.start;
        let first = self
            .entities
            .partition_point(|(entity_range, _)| entity_range.start < range.start);
        for (entity_range, entity) in &self.entities[first..] {
            if entity_range.start >= range.end {
                break;
            }
            if entity_range.start >= range.start && entity_range.end <= range.end {
                output.push_str(&crate::source_html_entities(
                    &self.value[cursor..entity_range.start],
                ));
                output.push_str(entity.spelling());
                cursor = entity_range.end;
            }
        }
        output.push_str(&crate::source_html_entities(&self.value[cursor..range.end]));
    }

    fn append_tokens(&self, range: std::ops::Range<usize>, output: &mut Vec<Token>) {
        let mut cursor = range.start;
        let first = self
            .entities
            .partition_point(|(entity_range, _)| entity_range.start < range.start);
        for (entity_range, entity) in &self.entities[first..] {
            if entity_range.start >= range.end {
                break;
            }
            if entity_range.start >= range.start && entity_range.end <= range.end {
                if cursor < entity_range.start {
                    output.push(Token::Text(self.value[cursor..entity_range.start].into()));
                }
                output.push(Token::ChangedEntity(*entity));
                cursor = entity_range.end;
            }
        }
        if cursor < range.end {
            output.push(Token::Text(self.value[cursor..range.end].into()));
        }
    }
}

enum Section {
    Text(Text),
    Break,
    Delimiter(crate::wordfiltered_comment::Delimiter),
}

pub fn lines(prepared: &PreparedComment, board: &str) -> Vec<Line> {
    lines_and_wrap(prepared, board).0
}

pub fn wrap_required(prepared: &PreparedComment, board: &str) -> bool {
    lines_and_wrap(prepared, board).1
}

fn lines_and_wrap(prepared: &PreparedComment, board: &str) -> (Vec<Line>, bool) {
    let normalize = crate::server_link::source_probe(&prepared.source_projection());
    let mut sections = Vec::new();
    let mut text = Text::default();
    for part in prepared.parts() {
        match part {
            Part::Text(_) | Part::ChangedEntity(_) => text.append(part, board, normalize),
            _ => {
                if !text.value.is_empty() {
                    sections.push(Section::Text(std::mem::take(&mut text)));
                }
                sections.push(match part {
                    Part::Break => Section::Break,
                    Part::Delimiter(delimiter) => Section::Delimiter(*delimiter),
                    _ => unreachable!("nontext part"),
                });
            }
        }
    }
    if !text.value.is_empty() {
        sections.push(Section::Text(text));
    }
    let normalized: String = sections
        .iter()
        .map(|section| match section {
            Section::Text(text) => text.value.clone(),
            Section::Break => "<br>".into(),
            Section::Delimiter(delimiter) => delimiter.source_projection(),
        })
        .collect();
    let internal_links = crate::server_link::link_probe(&normalized);
    // Probe the linked source representation, including attribute bytes. The
    // source enables its decode/wrap/re-escape pass for the whole comment.
    let mut linked = String::new();
    for section in &sections {
        match section {
            Section::Break => linked.push_str("<br>"),
            Section::Delimiter(delimiter) => linked.push_str(&delimiter.source_projection()),
            Section::Text(text) => {
                for (token, range) in
                    tokenize_spanned(&text.value, false, false, internal_links, true, false)
                {
                    match token {
                        Token::Text(_) => text.source_slice(range, &mut linked),
                        Token::ServerLink(_, _) => {
                            linked.push_str("<a href=\"");
                            text.source_slice(range.clone(), &mut linked);
                            linked.push_str("\" target=\"_blank\">");
                            text.source_slice(range, &mut linked);
                            linked.push_str("</a>");
                        }
                        Token::StaticQuote(quote, _) => {
                            linked.push_str("<a href=\"");
                            linked.push_str(&crate::source_html_entities(&quote.source_href()));
                            linked.push_str("\" class=\"quotelink\"");
                            if quote.opens_new_tab() {
                                linked.push_str(" target=\"_blank\"");
                            }
                            linked.push('>');
                            text.source_slice(range, &mut linked);
                            linked.push_str("</a>");
                        }
                        _ => unreachable!("source link-only tokens"),
                    }
                }
            }
        }
    }
    let wrap = prepared.wrap_enabled().unwrap_or_else(|| {
        linked.chars().count() >= 35
            && linked
                .as_bytes()
                .split(|byte| b" <>".contains(byte))
                .any(|run| run.len() >= 35)
    });
    let mut lines = vec![Line {
        green: false,
        tokens: Vec::new(),
    }];
    let mut line_start = true;
    for section in sections {
        if matches!(section, Section::Break) {
            lines.push(Line {
                green: false,
                tokens: Vec::new(),
            });
            line_start = true;
            continue;
        }
        let after_break = lines.len() > 1;
        let output = &mut lines.last_mut().expect("initial line").tokens;
        match section {
            Section::Delimiter(delimiter) => {
                if delimiter.rolls().is_some() {
                    output.push(Token::FilteredDelimiter(delimiter));
                } else if delimiter.opening() {
                    output.push(Token::OpenMarkup(delimiter.tag()));
                } else {
                    output.push(Token::CloseMarkup(delimiter.tag()));
                }
            }
            Section::Text(text) => {
                let prefix = usize::from(after_break && text.value.starts_with(' '));
                let start = &text.value[prefix..];
                let mut green = line_start && start.starts_with('>') && !start.starts_with(">>");
                let offset = if green { prefix } else { 0 };
                if green {
                    if prefix > 0 {
                        output.push(Token::Text(" ".into()));
                    }
                    output.push(Token::OpenQuote);
                }
                let input = &text.value[offset..];
                let tokens = if wrap {
                    crate::word_break::tokenize_source(input, true, internal_links)
                } else {
                    let mut tokens = Vec::new();
                    for (token, range) in
                        tokenize_spanned(input, false, true, internal_links, true, true)
                    {
                        if matches!(token, Token::Text(_)) {
                            text.append_tokens(
                                range.start + offset..range.end + offset,
                                &mut tokens,
                            );
                        } else {
                            // The no-op wordwrap still supplies a typed label.
                            tokens.push(match token {
                                Token::ServerLink(link, _) => Token::ServerLink(
                                    link.clone(),
                                    vec![crate::word_break::WordPart::Text(link.href().into())],
                                ),
                                Token::StaticQuote(quote, _) => Token::StaticQuote(
                                    quote.clone(),
                                    vec![crate::word_break::WordPart::Text(quote.label())],
                                ),
                                other => other,
                            });
                        }
                    }
                    tokens
                };
                for token in tokens {
                    if green && matches!(token, Token::ServerLink(_, _) | Token::StaticQuote(_, _))
                    {
                        output.push(Token::CloseQuote);
                        green = false;
                    }
                    output.push(token);
                }
                if green {
                    output.push(Token::CloseQuote);
                }
            }
            Section::Break => unreachable!("handled above"),
        }
        line_start = false;
    }
    for line in &mut lines {
        let mut final_tokens = Vec::new();
        for token in std::mem::take(&mut line.tokens) {
            match token {
                Token::Text(text) => {
                    let text = remove_source_markers(&text);
                    for (index, part) in text.split("{{w_br}}").enumerate() {
                        if index > 0 {
                            final_tokens.push(Token::WordBreak);
                        }
                        if !part.is_empty() {
                            final_tokens.push(Token::Text(part.into()));
                        }
                    }
                }
                Token::ServerLink(link, parts) => final_tokens.push(Token::ServerLink(
                    link.without_source_markers(),
                    parts
                        .into_iter()
                        .map(|part| match part {
                            crate::word_break::WordPart::Text(text) => {
                                crate::word_break::WordPart::Text(remove_source_markers(&text))
                            }
                            other => other,
                        })
                        .collect(),
                )),
                other => final_tokens.push(other),
            }
        }
        line.tokens = final_tokens;
    }
    (lines, wrap)
}

pub(crate) fn remove_source_markers(text: &str) -> String {
    text.replace("~?rep?~", "").replace("~?erep?~", "")
}

/// Synthetic comparison and stored search representation. Every user-text
/// byte is escaped. Do not use this projection as template-safe HTML.
pub fn source_projection(lines: &[Line]) -> String {
    let mut result = String::new();
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            result.push_str("<br>");
        }
        for token in &line.tokens {
            match token {
                Token::Text(text) => result.push_str(&crate::source_html_entities(text)),
                Token::ChangedEntity(entity) => result.push_str(entity.spelling()),
                Token::FilteredDelimiter(delimiter) => {
                    result.push_str(&delimiter.source_projection())
                }
                Token::OpenMarkup(tag) | Token::CloseMarkup(tag) => {
                    let open = matches!(token, Token::OpenMarkup(_));
                    result.push_str(crate::robot9000::markup(*tag, open));
                }
                Token::OpenQuote => result.push_str("<span class=\"quote\">"),
                Token::CloseQuote => result.push_str("</span>"),
                Token::WordBreak => result.push_str("<wbr>"),
                Token::PostQuote(quote) => {
                    result.push_str(&crate::source_html_entities(quote.label()))
                }
                Token::StaticQuote(quote, parts) => {
                    result.push_str("<a href=\"");
                    result.push_str(&crate::source_html_entities(&quote.source_href()));
                    result.push_str("\" class=\"quotelink\"");
                    if quote.opens_new_tab() {
                        result.push_str(" target=\"_blank\"");
                    }
                    result.push('>');
                    append_parts(parts, &mut result);
                    result.push_str("</a>");
                }
                Token::ServerLink(link, parts) => {
                    result.push_str("<a href=\"");
                    result.push_str(&crate::source_html_entities(link.href()));
                    result.push_str("\" target=\"_blank\">");
                    append_parts(parts, &mut result);
                    result.push_str("</a>");
                }
                _ => unreachable!("saved source format tokens"),
            }
        }
    }
    result
}

fn append_parts(parts: &[crate::word_break::WordPart], result: &mut String) {
    for part in parts {
        match part {
            crate::word_break::WordPart::Text(text) => {
                result.push_str(&crate::source_html_entities(text))
            }
            crate::word_break::WordPart::Break => result.push_str("<wbr>"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wordfilter::{LeetRolls, Profile};
    #[test]
    fn stored_parts_and_finished_projection_match_independent_source_vectors() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/wordfilter-posting-reference.json"
        ))
        .unwrap();
        let policy = crate::comment_markup::MarkupPolicy {
            spoilers: true,
            code: true,
            sjis: true,
            op: true,
        };
        for (name, profile) in [
            ("global", Profile::Global),
            ("ck", Profile::Basic),
            ("asp", Profile::Asp),
            ("v", Profile::Video),
            ("test", Profile::Test),
        ] {
            for case in fixture["profiles"][name].as_array().unwrap() {
                let input = case["admission_input"].as_str().unwrap();
                let admitted = crate::prepare_post_content(
                    "",
                    "",
                    case["input"].as_str().unwrap(),
                    16000,
                    false,
                    crate::CommentSpacing::for_board("g", true, true)
                        .with_line_rules(100, true)
                        .with_op_markup(true),
                    crate::PostKind::Thread {
                        subject_required: false,
                        text_only: false,
                    },
                )
                .unwrap();
                assert_eq!(
                    admitted.comment, input,
                    "caller input {name} {}",
                    case["input"]
                );
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
                let mut prepared =
                    crate::wordfiltered_comment::prepare(input, policy, profile, rolls).unwrap();
                assert_eq!(
                    prepared.source_projection(),
                    case["filtered"].as_str().unwrap(),
                    "prepared {name} {rolls:?} {input}"
                );
                prepared.freeze_format("g");
                let decoded = PreparedComment::decode(&prepared.encode().unwrap()).unwrap();
                assert_eq!(decoded, prepared);
                assert_eq!(
                    source_projection(&lines(&decoded, "g")),
                    case["final"].as_str().unwrap(),
                    "finished {name} {rolls:?} {input}"
                );
            }
        }
    }
}
