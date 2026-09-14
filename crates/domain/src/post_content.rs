use crate::comment_markup::{MarkupToken, parse_markup};
use crate::{CommentSpacing, ValidationError, prepare_post_comment, prepare_post_subject};

/// Server-selected post position and operator-owned OP subject rule.
#[derive(Clone, Copy)]
pub enum PostKind {
    Thread { subject_required: bool },
    Reply,
}

pub struct PreparedPostContent {
    pub subject: String,
    pub comment: String,
}

/// Check raw bounds before cleanup and S_NOSUB before comment line admission.
pub fn prepare_post_content(
    name: &str,
    subject: &str,
    comment: &str,
    max_chars: usize,
    has_attachment: bool,
    spacing: CommentSpacing<'_>,
    kind: PostKind,
) -> Result<PreparedPostContent, ValidationError> {
    // Defer empty-comment admission, not raw limits or control rejection.
    // This does not assert attachment authority; final admission follows below.
    crate::validate_post_with_attachment(name, subject, comment, max_chars, true)?;
    let subject = prepare_post_subject(subject, spacing)?;
    if matches!(
        kind,
        PostKind::Thread {
            subject_required: true
        }
    ) && subject.is_empty()
    {
        return Err(ValidationError("Error: New threads require a subject."));
    }
    // Defer blank admission through sanitation as well. Bounds, controls,
    // spam and line checks still run before the source's final markup check.
    let comment = prepare_post_comment(name, "", comment, max_chars, true, spacing)?;
    let blank = parse_markup(&comment, spacing.markup_policy())
        .iter()
        .all(|token| match token {
            MarkupToken::Break => true,
            MarkupToken::Text(text) => text
                .chars()
                .all(|ch| matches!(ch, ' ' | '\t' | '\n' | '\r' | '\u{b}' | '\u{c}')),
            // The source blank regexp ignores only ASCII whitespace and <br>.
            // Even an empty generated code/SJIS element counts as content.
            MarkupToken::Open(_) | MarkupToken::Close(_) => false,
        });
    if blank {
        match kind {
            PostKind::Thread { .. } if subject.is_empty() => {
                return Err(ValidationError(
                    "Error: New threads require a subject or comment.",
                ));
            }
            PostKind::Reply if !has_attachment => {
                return Err(ValidationError("Error: No text entered."));
            }
            _ => {}
        }
    }
    Ok(PreparedPostContent { subject, comment })
}
