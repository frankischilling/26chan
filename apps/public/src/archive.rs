//! Source archive excerpts projected from saved typed formatting, never raw HTML.
use crate::{catalog::teaser, handlers::AppError};
use board_domain::{Line, Token, comment_markup::Tag};

pub struct Prepared {
    pub lines: Vec<Line>,
    /// Source-shaped data for qualification; never an HTML trust boundary.
    pub serialized: String,
}

pub struct Row {
    pub id: i64,
    pub href: String,
    pub lines: Vec<Line>,
}

/// `subject` is source-escaped text (or the source's internal SPOILER<> prefix).
/// PHP's initial serialized-length check intentionally survives tag stripping.
pub struct Input<'a> {
    pub subject: &'a str,
    pub lines: &'a [Line],
    pub board: &'a str,
    pub format: i16,
    pub sjis: bool,
    pub dice: Option<&'a str>,
    pub fortune: Option<(&'a str, &'a str)>,
}

pub fn prepare(input: Input<'_>) -> Prepared {
    let Input {
        subject,
        lines,
        board,
        format,
        sjis,
        dice,
        fortune,
    } = input;
    let subject = subject.strip_prefix("SPOILER<>").unwrap_or(subject);
    let source_links = matches!(format, 104..=111 | 120..=127);
    let mut tokens = Vec::new();
    if let Some(dice) = dice {
        tokens.extend([
            Token::GeneratedBold(true),
            Token::Text(format!("{dice} ")),
            Token::GeneratedBold(false),
        ]);
    }
    // Adjacent source <br> tags collapse, but intervening generated tags do not.
    let mut previous_break = false;
    for (index, line) in lines.iter().enumerate() {
        if index > 0 && !previous_break {
            tokens.push(Token::Text(" ".into()));
            previous_break = true;
        }
        if line.green {
            tokens.push(Token::OpenQuote);
        }
        tokens.extend(line.tokens.iter().cloned().map(|token| {
            if source_links {
                match token {
                    Token::Quote(id) => Token::Text(format!(">>{id}")),
                    Token::CrossQuote(target, id) => Token::Text(format!(">>>/{target}/{id}")),
                    Token::PostQuote(quote) => Token::Text(quote.label().into()),
                    other => other,
                }
            } else {
                token
            }
        }));
        if line.green {
            tokens.push(Token::CloseQuote);
        }
        if line.green || !line.tokens.is_empty() {
            previous_break = false;
        }
    }
    if let Some((text, color)) = fortune {
        tokens.extend([
            Token::GeneratedFortune(true, color.into()),
            Token::Text(" ".into()),
            Token::GeneratedBold(true),
            Token::Text(format!("Your fortune: {text}")),
            Token::GeneratedBold(false),
            Token::GeneratedFortune(false, color.into()),
        ]);
    }
    // Source !empty() also excludes the string "0".
    if !subject.is_empty() && subject != "0" {
        let mut prefix = if tokens.is_empty() {
            vec![Token::Text(decode(subject))]
        } else {
            vec![
                Token::GeneratedBold(true),
                Token::Text(format!("{}:", decode(subject))),
                Token::GeneratedBold(false),
                Token::Text(" ".into()),
            ]
        };
        prefix.append(&mut tokens);
        tokens = prefix;
    }
    if sjis {
        tokens = teaser::replace_sjis(tokens);
    }
    let serialized = teaser::serialize(&tokens, board, source_links).replace("&quot;", "'");
    if serialized.chars().count() > 100 {
        // Archive keep_spoilers defaults to false, unlike catalog's true.
        let stripped: Vec<_> = teaser::strip(tokens)
            .into_iter()
            .filter(|token| {
                !matches!(
                    token,
                    Token::OpenMarkup(Tag::Spoiler) | Token::CloseMarkup(Tag::Spoiler)
                )
            })
            .collect();
        let mut serialized: String = teaser::serialize(&stripped, board, source_links)
            .replace("&quot;", "'")
            .chars()
            .take(100)
            .collect();
        if let Some(amp) = serialized.rfind('&')
            && !serialized[amp..].contains(';')
        {
            serialized.truncate(amp);
        }
        serialized.push('…');
        return Prepared {
            lines: vec![Line {
                green: false,
                tokens: vec![Token::Text(decode(&serialized))],
            }],
            serialized,
        };
    }
    // Only display text changes. A quote substitution must never corrupt a
    // typed URL/attribute into source's unsafe raw-HTML attribute syntax.
    for token in &mut tokens {
        match token {
            Token::Text(text) | Token::Spoiler(text) => *text = text.replace('"', "'"),
            Token::WrappedLink(_, parts)
            | Token::ServerLink(_, parts)
            | Token::StaticQuote(_, parts) => {
                for part in parts {
                    if let board_domain::word_break::WordPart::Text(text) = part {
                        *text = text.replace('"', "'");
                    }
                }
            }
            Token::Link(url) => {
                *token = Token::WrappedLink(
                    url.clone(),
                    vec![board_domain::word_break::WordPart::Text(
                        url.replace('"', "'"),
                    )],
                );
            }
            _ => {}
        }
    }
    Prepared {
        lines: vec![Line {
            green: false,
            tokens,
        }],
        serialized,
    }
}

// Decode only entities generated by source_html_entities, once; saved changed
// entities remain literal visible text. Askama escapes the result again.
fn decode(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#039;", "'")
        .replace("&amp;", "&")
}

pub(crate) fn row(
    entry: board_store::ArchivePageEntry,
    board: &board_store::Board,
) -> Result<Row, AppError> {
    let lines = board_domain::formatting::parse_saved_comment_with_limits(
        &entry.comment,
        entry.comment_format,
        &board.slug,
        entry.wordfilter_payload.as_deref(),
        if entry.staff_authorized_limits {
            board_domain::PostLimits::authorized(board_domain::MAX_AUTHORIZED_COMMENT_CHARS)
                .expect("finite persisted staff bound")
        } else {
            board_domain::PostLimits::ordinary(board_domain::MAX_COMMENT_CHARS)
        },
    );
    let subject = board_domain::source_html_entities(&entry.subject);
    let mut comment = teaser::stored_comment(&lines, &board.slug, entry.comment_format);
    if let Some(dice) = &entry.dice_result {
        comment = format!(
            "<b>{}<br><br></b>{comment}",
            board_domain::source_html_entities(dice)
        );
    }
    let fortune = entry
        .fortune_text
        .as_deref()
        .zip(entry.fortune_color.as_deref());
    if let Some((text, color)) = fortune {
        comment.push_str(&format!(
            "<span class=\"fortune\" style=\"color:{}\"><br><br><b>Your fortune: {}</b></span>",
            board_domain::source_html_entities(color),
            board_domain::source_html_entities(text)
        ));
    }
    let context = board_domain::semantic_context::generate(&subject, &comment, board.staff_only)
        .map_err(|_| {
            AppError(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Could not format thread context.",
            )
        })?;
    let href = crate::semantic_thread::href(&board.slug, entry.id, &context);
    let prepared = prepare(Input {
        subject: &subject,
        lines: &lines,
        board: &board.slug,
        format: entry.comment_format,
        // Source's preview switch is current; saved parser authority is not.
        sjis: board.comment_sjis_spacing,
        dice: entry.dice_result.as_deref(),
        fortune,
    });
    Ok(Row {
        id: entry.id,
        href,
        lines: prepared.lines,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn excerpt(subject: &str, comment: &str, format: i16, sjis: bool) -> String {
        let lines = board_domain::parse_post_comment(comment, format);
        prepare(Input {
            subject,
            lines: &lines,
            board: "g",
            format,
            sjis,
            dice: None,
            fortune: None,
        })
        .serialized
    }

    #[test]
    fn archive_source_subject_and_short_markup() {
        assert_eq!(
            excerpt("SPOILER<>Title", "body", 9, false),
            "<b>Title:</b> body"
        );
        assert_eq!(
            excerpt("SPOILER&lt;&gt;Title", "", 9, false),
            "SPOILER&lt;&gt;Title"
        );
        assert_eq!(excerpt("0", "body", 9, false), "body");
        assert_eq!(excerpt("Title", "", 9, false), "Title");
        assert_eq!(
            excerpt("&quot;title&quot;", "\"body\"\n\nnext", 9, false),
            "<b>'title':</b> 'body' next"
        );
        assert_eq!(
            excerpt("", "[spoiler]hidden[/spoiler]", 9, false),
            "<s>hidden</s>"
        );
        assert_eq!(excerpt("", "[sjis]a\nb[/sjis]", 44, true), "[SJIS]");
    }

    #[test]
    fn archive_long_strips_spoilers_before_scalar_and_entity_cut() {
        for size in [99, 100, 101] {
            let text = "界".repeat(size);
            assert_eq!(
                excerpt("", &text, 9, false),
                if size <= 100 {
                    text
                } else {
                    format!("{}…", "界".repeat(100))
                }
            );
        }
        assert_eq!(
            excerpt(
                "",
                &format!("[spoiler]{}[/spoiler]", "x".repeat(94)),
                9,
                false
            ),
            format!("{}…", "x".repeat(94))
        );
        assert_eq!(
            excerpt("", &format!("{}&tail", "a".repeat(99)), 9, false),
            format!("{}…", "a".repeat(99))
        );
        assert_eq!(
            excerpt("", &format!("{}&tail", "a".repeat(95)), 9, false),
            format!("{}&amp;…", "a".repeat(95))
        );
        assert_eq!(
            excerpt("", &format!("{}\"tail", "a".repeat(99)), 9, false),
            format!("{}'…", "a".repeat(99))
        );
    }

    #[test]
    fn saved_randomizers_participate_in_archive_length_check() {
        let lines = board_domain::parse_post_comment("body", 9);
        let prepared = prepare(Input {
            subject: "",
            lines: &lines,
            board: "g",
            format: 9,
            sjis: false,
            dice: Some("Rolled 3 (1d6)"),
            fortune: None,
        });
        assert_eq!(prepared.serialized, "<b>Rolled 3 (1d6) </b>body");
        let prepared = prepare(Input {
            subject: "",
            lines: &lines,
            board: "g",
            format: 9,
            sjis: false,
            dice: None,
            fortune: Some(("Good Luck", "green")),
        });
        let source = "body<span class=\"fortune\" style=\"color:green\"> <b>Your fortune: Good Luck</b></span>";
        assert_eq!(prepared.serialized, source);
    }
}
