//! Source upload-name cleanup and basename rules, independent of storage paths.

/// Remove the four directional controls removed before source upload admission.
/// HTML escaping belongs to the output boundary, not the stored plain text.
pub fn strip_upload_direction_controls(filename: &str) -> String {
    filename
        .chars()
        .filter(|character| !matches!(character, '\u{202e}' | '\u{202d}' | '\u{200f}' | '\u{200e}'))
        .collect()
}

/// Source sanitize_text rules for admitted upload names. Storage keeps plain
/// text; each HTML boundary escapes it once. Intake rejects control characters
/// and names exceeding its independent byte limit before publication.
pub fn normalize_upload(filename: &str, board: &str) -> String {
    let sjis = matches!(board, "jp" | "vip");
    let spaced = sjis || matches!(board, "g" | "test" | "j");
    let wide = sjis || matches!(board, "jp" | "a" | "b");
    let mut result = String::with_capacity(filename.len());
    let mut previous_space = false;
    for mut ch in strip_upload_direction_controls(filename).chars() {
        if !wide && matches!(ch, '\u{00a0}' | '\u{3000}') {
            ch = ' ';
        }
        if !spaced && matches!(ch, ' ' | '\t' | '\u{000c}' | '\u{200b}' | '\u{2029}') {
            if !previous_space {
                result.push(' ');
            }
            previous_space = true;
        } else {
            if spaced && ch == '\t' {
                result.push_str("    ");
            } else {
                result.push(ch);
            }
            previous_space = false;
        }
    }
    let mut result = result
        .trim_matches([' ', '\t', '\n', '\r', '\0', '\u{000b}'])
        .to_owned();
    // RLO was removed above; the source balances retained RLE characters.
    let embeddings = result.chars().filter(|ch| *ch == '\u{202b}').count();
    result.extend(std::iter::repeat_n('\u{202c}', embeddings));
    result
}

/// Source preg_replace('/\.[a-z0-9]+$/i', '', name): remove only the final
/// nonempty ASCII-alphanumeric suffix. A leading dot may leave an empty stem.
pub fn basename(filename: &str) -> &str {
    filename
        .rsplit_once('.')
        .filter(|(_, extension)| {
            !extension.is_empty() && extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
        .map_or(filename, |(stem, _)| stem)
}

pub fn display(filename: &str, retained_extension: &str) -> String {
    format!("{}.{}", basename(filename), retained_extension)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_the_source_direction_controls_are_removed() {
        assert_eq!(
            strip_upload_direction_controls("a\u{202e}\u{202d}\u{200f}\u{200e}b.jpg"),
            "ab.jpg"
        );
        assert_eq!(
            strip_upload_direction_controls("a\u{202b}\u{2066}猫.jpg"),
            "a\u{202b}\u{2066}猫.jpg"
        );
        assert_eq!(strip_upload_direction_controls("\u{202e}"), "");
    }
    #[test]
    fn source_upload_spacing_and_direction_balance() {
        assert_eq!(
            normalize_upload("  a  \u{200b}b\u{3000}c.JPG  ", "co"),
            "a b c.JPG"
        );
        assert_eq!(
            normalize_upload("a\u{00a0}b\u{3000}c.png", "a"),
            "a\u{00a0}b\u{3000}c.png"
        );
        assert_eq!(normalize_upload("a  b\u{3000}c.png", "g"), "a  b c.png");
        assert_eq!(
            normalize_upload("a  b\u{3000}c.png", "jp"),
            "a  b\u{3000}c.png"
        );
        assert_eq!(normalize_upload("a\tb.png", "test"), "a    b.png");
        assert_eq!(
            normalize_upload("a\u{202e}b\u{202b}.png", "p"),
            "ab\u{202b}.png\u{202c}"
        );
        assert_eq!(normalize_upload("<cat>&.jpg", "co"), "<cat>&.jpg");
        assert_eq!(normalize_upload(" \u{200b} ", "co"), "");
    }

    #[test]
    fn upload_names_match_executed_source_sanitizer_cases() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/source-upload-filenames.json"
        ))
        .unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            assert_eq!(
                normalize_upload(
                    case["input"].as_str().unwrap(),
                    case["board"].as_str().unwrap()
                ),
                case["expected"].as_str().unwrap()
            );
        }
    }

    #[test]
    fn upload_spacing_board_flags_match_the_source_fixture() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../../fixtures/board-reference.json")).unwrap();
        for board in fixture["boards"].as_array().unwrap() {
            let slug = board["slug"].as_str().unwrap();
            assert_eq!(
                board["comment_code_spacing"],
                matches!(slug, "g" | "test" | "j")
            );
            assert_eq!(board["comment_sjis_spacing"], matches!(slug, "jp" | "vip"));
        }
    }

    #[test]
    fn final_extension_is_ascii_alphanumeric_and_independent_of_retained_format() {
        for (name, expected) in [
            ("cat.JPG", "cat"),
            ("a.tar.gz", "a.tar"),
            (".png", ""),
            (".123", ""),
            ("cat.", "cat."),
            ("cat.foo-bar", "cat.foo-bar"),
            ("cat.猫", "cat.猫"),
            ("cat", "cat"),
        ] {
            assert_eq!(basename(name), expected);
        }
        assert_eq!(display("cat.JPG", "png"), "cat.png");
        assert_eq!(display("cat.foo-bar", "jpg"), "cat.foo-bar.jpg");
    }
}
