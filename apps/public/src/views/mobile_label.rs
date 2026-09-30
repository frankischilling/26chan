/// Presentation only: the released formatter measures serialized UTF-16 before
/// shortening decoded text. The result still crosses an escaped template.
pub struct MobileLabel {
    pub text: String,
    pub shortened: bool,
}

pub fn label(text: &str) -> MobileLabel {
    let serialized = board_domain::source_html_entities(text);
    let shortened = serialized.encode_utf16().count() > 30;
    let text = if shortened {
        let decoded = serialized
            .replacen("&#44;", ",", 1)
            .replace("&amp;", "&")
            .replace("&quot;", "\"")
            .replace("&#039;", "'")
            .replace("&lt;", "<")
            .replace("&gt;", ">");
        let units: Vec<_> = decoded.encode_utf16().take(30).collect();
        // A split surrogate is displayed as the replacement character by the
        // browser. Keep valid UTF-8 rather than constructing an invalid string.
        String::from_utf16_lossy(&units) + "(...)"
    } else {
        text.to_owned()
    };
    MobileLabel { text, shortened }
}

#[cfg(test)]
mod tests {
    use super::label;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Reference {
        cases: Vec<Case>,
    }

    #[derive(Deserialize)]
    struct Case {
        name: String,
        input: String,
        visible: String,
        shortened: bool,
    }

    #[test]
    fn labels_match_released_helper_vectors() {
        let reference: Reference = serde_json::from_str(include_str!(
            "../../../../docs/public-mobile-label-reference.json"
        ))
        .expect("pinned mobile label vectors");
        assert_eq!(reference.cases.len(), 10);
        for case in reference.cases {
            let actual = label(&case.input);
            assert_eq!(actual.text, case.visible, "{}", case.name);
            assert_eq!(actual.shortened, case.shortened, "{}", case.name);
        }
    }
}
