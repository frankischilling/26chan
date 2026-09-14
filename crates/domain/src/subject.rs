use crate::{CommentSpacing, MAX_PUBLIC_FIELD_BYTES, ValidationError};

// A raw public subject has at most 100 bytes; source tabs expand fourfold.
pub const MAX_SUBJECT_BYTES: usize = MAX_PUBLIC_FIELD_BYTES * 4;

/// Prepare subject text, not trusted markup. Validate before any shortening.
pub fn prepare_post_subject(
    raw: &str,
    spacing: CommentSpacing<'_>,
) -> Result<String, ValidationError> {
    if raw.len() > MAX_PUBLIC_FIELD_BYTES {
        return Err(ValidationError("Name or subject is too long."));
    }
    if raw
        .chars()
        .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
    {
        return Err(ValidationError("Unsupported control character."));
    }
    // Unlike comments, subjects always normalize and strip zero-width points.
    let mut text = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if matches!(ch as u32, 0x2600..=0x26ff) {
            continue;
        }
        let ch = crate::comment_ascii::similar_to_ascii(ch);
        if crate::comment_spacing::zero_width(ch)
            || crate::comment_unicode::emoticon(ch, spacing.sjis)
        {
            continue;
        }
        text.push(ch);
    }
    // strip_fake_capcodes removes runs of two or more #/fullwidth #, not singles.
    let mut filtered = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if matches!(ch, '#' | '\u{ff03}') {
            let mut count = 1;
            while chars.next_if(|ch| matches!(ch, '#' | '\u{ff03}')).is_some() {
                count += 1;
            }
            if count == 1 {
                filtered.push(ch);
            }
        } else if ch != '\u{2318}' {
            filtered.push(ch);
        }
    }
    if filtered
        .chars()
        .all(|ch| matches!(ch, ' ' | '|' | '\u{3000}'))
    {
        filtered.clear();
    }
    let mut result = crate::comment_spacing::sanitize_spacing(&filtered, spacing);
    // Source removes CR/LF after trim, then private codepoints; do not trim again.
    result.retain(|ch| !matches!(ch, '\n' | '\r') && ch as u32 <= 0x3134f);
    if result.len() > MAX_SUBJECT_BYTES {
        return Err(ValidationError("Name or subject is too long."));
    }
    Ok(result)
}

/// Source ENT_QUOTES representation for JSON text and admission comparisons.
/// This return value does not grant permission to bypass escaped templates.
pub fn source_html_entities(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => result.push_str("&amp;"),
            '<' => result.push_str("&lt;"),
            '>' => result.push_str("&gt;"),
            '"' => result.push_str("&quot;"),
            '\'' => result.push_str("&#039;"),
            _ => result.push(ch),
        }
    }
    result
}
