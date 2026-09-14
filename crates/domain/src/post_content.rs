use crate::{CommentSpacing, ValidationError, prepare_post_comment, prepare_post_subject};

pub struct PreparedPostContent {
    pub subject: String,
    pub comment: String,
}

/// `subject_required` is operator policy for an OP, never a form field.
/// Check raw bounds before cleanup and S_NOSUB before comment line admission.
pub fn prepare_post_content(
    name: &str,
    subject: &str,
    comment: &str,
    max_chars: usize,
    has_attachment: bool,
    spacing: CommentSpacing<'_>,
    subject_required: bool,
) -> Result<PreparedPostContent, ValidationError> {
    // Defer empty-comment admission, not raw limits or control rejection.
    // This does not assert attachment authority or permit an empty stored post.
    crate::validate_post_with_attachment(name, subject, comment, max_chars, true)?;
    let subject = prepare_post_subject(subject, spacing)?;
    if subject_required && subject.is_empty() {
        return Err(ValidationError("Error: New threads require a subject."));
    }
    let comment = prepare_post_comment(name, "", comment, max_chars, has_attachment, spacing)?;
    Ok(PreparedPostContent { subject, comment })
}
