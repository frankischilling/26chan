use crate::{
    MAX_COMMENT_BYTES, MAX_COMMENT_CHARS, ValidationError, normalize_comment,
    validate_post_with_attachment,
};

/// Source comment sanitation policy, independent of markup rendering support.
#[derive(Clone, Copy)]
pub struct CommentSpacing {
    pub code: bool,
    pub sjis: bool,
    pub preserve_wide_spaces: bool,
    pub strip_zero_width: bool,
}

impl CommentSpacing {
    pub fn for_board(board: &str, code: bool, sjis: bool) -> Self {
        Self {
            code,
            sjis,
            preserve_wide_spaces: sjis || matches!(board, "a" | "b" | "jp"),
            strip_zero_width: !sjis && !matches!(board, "a" | "jp"),
        }
    }
}

/// Check the source's pre-cleanup character budget, then prepare stored text.
/// HTML remains text here; only the escaped rendering layer produces markup.
pub fn prepare_post_comment(
    name: &str,
    subject: &str,
    comment: &str,
    max_chars: usize,
    has_attachment: bool,
    spacing: CommentSpacing,
) -> Result<String, ValidationError> {
    validate_post_with_attachment(name, subject, comment, max_chars, has_attachment)?;
    let normalized = normalize_comment(comment)?;
    let normalized = crate::comment_unicode::before_spacing(&normalized, spacing);
    let preserve = spacing.code || spacing.sjis;
    let mut text = String::with_capacity(normalized.len());
    let mut previous_space = false;
    for mut ch in normalized.chars() {
        if !spacing.preserve_wide_spaces && ch == '\u{3000}' {
            ch = ' ';
        }
        if preserve {
            if ch == '\t' {
                text.push_str("    ");
            } else {
                text.push(ch);
            }
        } else if matches!(ch, ' ' | '\t' | '\u{000c}' | '\u{200b}' | '\u{2029}') {
            if !previous_space {
                text.push(' ');
            }
            previous_space = true;
        } else {
            text.push(ch);
            previous_space = false;
        }
    }
    // PHP trim's default ASCII set, not Unicode-wide Rust str::trim.
    let mut text = text
        .trim_matches([' ', '\t', '\n', '\r', '\0', '\u{000b}'])
        .to_owned();
    // Source strip_private_unicode runs after trim, so removal can expose
    // spaces at the edges. Do not trim those a second time.
    text.retain(|ch| ch as u32 <= 0x3134f);
    let text = if preserve {
        text
    } else {
        collapse_blank_lines(&text)
    };
    // Tab expansion can exceed the independent database storage ceiling even
    // though the pre-cleanup board budget passed. Reject before any mutation.
    if (!has_attachment && text.trim().is_empty())
        || text.len() > MAX_COMMENT_BYTES
        || text.chars().count() > MAX_COMMENT_CHARS
    {
        return Err(ValidationError(
            "Enter a comment within this board's character limit.",
        ));
    }
    Ok(text)
}

pub(crate) fn zero_width(ch: char) -> bool {
    matches!(ch as u32,
        0x0702 | 0x1d176 | 0x008d | 0x00a0 | 0x205f | 0xfeff | 0x11a6 |
        0x00ad | 0x3164 | 0x2800 | 0x180b..=0x180e | 0x115f | 0x1160 |
        0xffa0 | 0x034f | 0x17b4 | 0x17b5 | 0x0001..=0x0008 |
        0x000e..=0x001f | 0x007f..=0x009f | 0x2000..=0x200f |
        0x2028..=0x202f | 0x2060..=0x206f | 0xfe00..=0xfe0f |
        0xfff0..=0xfffb | 0xe0100..=0xe01ef | 0xe0001..=0xe007f)
}

fn collapse_blank_lines(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut start = 0;
    while let Some(offset) = text[start..].find('\n') {
        let newline = start + offset;
        result.push_str(&text[start..=newline]);
        let mut end = newline + 1;
        let mut count = 1;
        while let Some(offset) = text[end..].find('\n') {
            if !text[end..end + offset]
                .chars()
                .all(|ch| matches!(ch, ' ' | '\u{3000}'))
            {
                break;
            }
            count += 1;
            end += offset + 1;
        }
        start = if count >= 4 { end } else { newline + 1 };
    }
    result.push_str(&text[start..]);
    result
}
