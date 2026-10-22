use crate::comment_markup::{MarkupToken, parse_markup_with_limits};
use crate::{
    CommentSpacing, PostLimits, ValidationError, prepare_post_comment_with_limits,
    prepare_post_subject_with_limits,
};

/// Server-selected post position and operator-owned OP subject rule.
#[derive(Clone, Copy)]
pub enum PostKind {
    Thread {
        /// Server-selected moderator Options exception; attachment authority is checked separately.
        staff_image_only: bool,
        subject_required: bool,
        text_only: bool,
    },
    Reply,
}

pub struct PreparedPostContent {
    pub subject: String,
    pub comment: String,
}

/// Sanitized fields before configured rules and final markup admission.
/// The policy and fields stay paired until final admission completes.
pub struct PostContentInput {
    subject: String,
    comment: String,
    has_attachment: bool,
    markup: crate::comment_markup::MarkupPolicy,
    kind: PostKind,
    limits: PostLimits,
}

impl PostContentInput {
    pub fn subject(&self) -> &str {
        &self.subject
    }

    pub fn comment(&self) -> &str {
        &self.comment
    }

    /// The content hook sees generated wrappers before user markup is expanded.
    /// This comparison/log value must never be treated as trusted display HTML.
    pub fn admission_comment(
        &self,
        randomizer: Option<&crate::posting_randomizers::Outcome>,
    ) -> Result<String, ValidationError> {
        let (prefix, suffix) = crate::posting_randomizers::source_wrappers(randomizer)?;
        Ok(format!(
            "{prefix}{}{suffix}",
            crate::source_html_entities(&self.comment)
        ))
    }

    pub fn finish(self) -> Result<PreparedPostContent, ValidationError> {
        self.finish_with_randomizer(None)
    }

    /// Reuse the already generated, board-authorized result. Generated markup
    /// counts as nonblank source content; required-subject/raw checks ran earlier.
    pub fn finish_with_randomizer(
        self,
        randomizer: Option<&crate::posting_randomizers::Outcome>,
    ) -> Result<PreparedPostContent, ValidationError> {
        self.finish_with_metadata(randomizer, None)
    }

    pub fn finish_with_metadata(
        self,
        randomizer: Option<&crate::posting_randomizers::Outcome>,
        exif: Option<&crate::exif_comment::ExtractedExif>,
    ) -> Result<PreparedPostContent, ValidationError> {
        crate::posting_randomizers::source_wrappers(randomizer)?;
        let Self {
            subject,
            comment,
            has_attachment,
            markup,
            kind,
            limits,
        } = self;
        final_admission(
            subject,
            comment,
            has_attachment,
            markup,
            kind,
            limits,
            randomizer.is_some() || exif.is_some_and(crate::exif_comment::ExtractedExif::has_table),
        )
    }
}

/// Convenience path for callers without intervening admission hooks.
pub fn prepare_post_content(
    name: &str,
    subject: &str,
    comment: &str,
    max_chars: usize,
    has_attachment: bool,
    spacing: CommentSpacing<'_>,
    kind: PostKind,
) -> Result<PreparedPostContent, ValidationError> {
    prepare_post_content_input(
        name,
        subject,
        comment,
        max_chars,
        has_attachment,
        spacing,
        kind,
    )?
    .finish()
}

/// Check raw bounds, required subject, sanitation and line rules before hooks.
pub fn prepare_post_content_input(
    name: &str,
    subject: &str,
    comment: &str,
    max_chars: usize,
    has_attachment: bool,
    spacing: CommentSpacing<'_>,
    kind: PostKind,
) -> Result<PostContentInput, ValidationError> {
    prepare_post_content_input_with_limits(
        name,
        subject,
        comment,
        PostLimits::ordinary(max_chars),
        has_attachment,
        spacing,
        kind,
    )
}

pub fn prepare_post_content_input_with_limits(
    name: &str,
    subject: &str,
    comment: &str,
    limits: PostLimits,
    has_attachment: bool,
    spacing: CommentSpacing<'_>,
    kind: PostKind,
) -> Result<PostContentInput, ValidationError> {
    // Defer empty-comment admission, not raw limits or control rejection.
    // This does not assert attachment authority; final admission follows below.
    crate::validate_post_with_limits(name, subject, comment, limits, true)?;
    let subject = prepare_post_subject_with_limits(subject, spacing, limits)?;
    if matches!(
        kind,
        PostKind::Thread {
            subject_required: true,
            ..
        }
    ) && subject.is_empty()
    {
        return Err(ValidationError("Error: New threads require a subject."));
    }
    // Defer blank admission through sanitation as well. Bounds and controls
    // precede final markup admission; ordinary posts also get spam/line checks.
    let comment = prepare_post_comment_with_limits(name, "", comment, limits, true, spacing)?;
    // The caller removes existing internal markers once before its admission
    // filters and generated markup. A newly joined marker is left for the
    // later formatter, matching the source's two separate passes.
    let comment = crate::filtered_formatting::remove_source_markers(&comment);
    Ok(PostContentInput {
        subject,
        comment,
        has_attachment,
        markup: spacing.markup_policy(),
        kind,
        limits,
    })
}

fn final_admission(
    subject: String,
    comment: String,
    has_attachment: bool,
    markup: crate::comment_markup::MarkupPolicy,
    kind: PostKind,
    limits: PostLimits,
    generated_content: bool,
) -> Result<PreparedPostContent, ValidationError> {
    let blank = !generated_content
        && parse_markup_with_limits(&comment, markup, limits)
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
            PostKind::Thread {
                staff_image_only, ..
            } if subject.is_empty() && !(staff_image_only && has_attachment) => {
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
    if matches!(
        kind,
        PostKind::Thread {
            text_only: true,
            ..
        }
    ) && subject.is_empty()
        && !blank
    {
        return Err(ValidationError("Error: New threads require a subject."));
    }
    Ok(PreparedPostContent { subject, comment })
}
