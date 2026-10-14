//! Source upload-name cleanup and basename rules, independent of storage paths.

/// Remove the four directional controls removed before source upload admission.
/// HTML escaping belongs to the output boundary, not the stored plain text.
pub fn strip_upload_direction_controls(filename: &str) -> String {
    filename
        .chars()
        .filter(|character| !matches!(character, '\u{202e}' | '\u{202d}' | '\u{200f}' | '\u{200e}'))
        .collect()
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
