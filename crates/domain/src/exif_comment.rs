//! Bounded EXIF extractor text and its source comparison spelling.
//! The legacy markup is comparison data, never trusted browser HTML.
use crate::source_html_entities;

pub const MAX_BYTES: usize = 16_384;
pub const MAX_ROWS: usize = 256;
pub const MAX_TIM: i64 = 9_007_199_254_740_991;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractedExif {
    text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("Invalid or oversized EXIF text.")]
pub struct InvalidExif;

impl ExtractedExif {
    /// A successful empty extraction is distinct from missing provenance.
    /// This validates text bounds only, not its origin or truthfulness.
    pub fn parse(text: &str) -> Result<Self, InvalidExif> {
        if text.len() > MAX_BYTES || text.split('\n').count() > MAX_ROWS || text.contains('\0') {
            return Err(InvalidExif);
        }
        Ok(Self { text: text.into() })
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn has_table(&self) -> bool {
        !self.text.is_empty() && self.text != "0"
    }

    /// Match the source's two-value explode assignment, including discarded
    /// later separators and the empty row following a trailing newline.
    pub fn comparison_markup(&self, tim: i64) -> Result<String, InvalidExif> {
        if !(1..=MAX_TIM).contains(&tim) {
            return Err(InvalidExif);
        }
        if !self.has_table() {
            return Ok(String::new());
        }
        let escaped = source_html_entities(&self.text);
        let mut output = format!(
            "<br><br><span class=\"abbr\">[EXIF data available. Click <a href=\"javascript:void(0)\" onclick=\"toggle('exif{tim}')\">here</a> to show/hide.]</span><br><table class=\"exif\" id=\"exif{tim}\">"
        );
        for line in escaped.split('\n') {
            let mut parts = line.split(": ");
            let tag = parts.next().expect("split always yields a value");
            let value = parts.next().unwrap_or_default();
            if value.is_empty() {
                output.push_str("<tr><td colspan=\"2\"><b>");
                output.push_str(tag);
                output.push_str("</b></td></tr>");
            } else {
                output.push_str("<tr><td>");
                output.push_str(tag);
                output.push_str("</td><td>");
                output.push_str(value);
                output.push_str("</td></tr>");
            }
        }
        output.push_str("</table>");
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_executed_source_table_branch() {
        let cases: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/exif-comment.json")).unwrap();
        for case in cases.as_array().unwrap() {
            let exif = ExtractedExif::parse(case["raw"].as_str().unwrap()).unwrap();
            assert_eq!(
                exif.comparison_markup(case["tim"].as_i64().unwrap())
                    .unwrap(),
                case["html"].as_str().unwrap(),
                "{case}"
            );
        }
    }

    #[test]
    fn rejects_excessive_or_ambiguous_data() {
        assert!(ExtractedExif::parse(&"x".repeat(MAX_BYTES)).is_ok());
        assert!(ExtractedExif::parse(&"x".repeat(MAX_BYTES + 1)).is_err());
        assert!(ExtractedExif::parse(&"\n".repeat(MAX_ROWS - 1)).is_ok());
        assert!(ExtractedExif::parse(&"\n".repeat(MAX_ROWS)).is_err());
        assert!(ExtractedExif::parse("Camera\0: hidden").is_err());
        let empty = ExtractedExif::parse("").unwrap();
        assert!(!empty.has_table());
        assert!(empty.comparison_markup(0).is_err());
        assert!(empty.comparison_markup(MAX_TIM + 1).is_err());
        assert!(!ExtractedExif::parse("0").unwrap().has_table());
        assert!(ExtractedExif::parse("\n").unwrap().has_table());
    }
}
