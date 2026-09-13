use crate::store::Report;
use askama::Template;
use board_domain::formatting::{Line, Token, parse_comment};
#[derive(Template)]
#[template(path = "login.html")]
pub struct Login;
pub struct Preview {
    pub report: Report,
    pub lines: Vec<Line>,
}
impl From<Report> for Preview {
    fn from(report: Report) -> Self {
        let lines = parse_comment(&report.comment);
        Self { report, lines }
    }
}
#[derive(Template)]
#[template(path = "queue.html")]
pub struct Queue {
    pub media_origin: String,
    pub reports: Vec<Preview>,
    pub csrf: String,
    pub recent: bool,
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_escapes_untrusted_text_with_typed_formatting() {
        let report = Report {
            id: 1,
            board: "test".into(),
            post_id: 1,
            thread_id: 1,
            reason: "<b>untrusted report</b>".into(),
            name: "<em>name</em>".into(),
            subject: "<i>subject</i>".into(),
            comment: "<b>comment</b>\n[spoiler]<i>text</i>[/spoiler]".into(),
            state: "open".into(),
            closed: false,
            sticky: false,
            deleted: false,
            attachment: None,
        };
        let html = Queue {
            media_origin: "http://127.0.0.1:3002".into(),
            reports: vec![report.into()],
            csrf: "example".into(),
            recent: true,
        }
        .render()
        .unwrap();
        assert!(!html.contains("<b>"));
        assert!(!html.contains("<em>"));
        assert!(!html.contains("<i>"));
        assert!(
            html.contains("&#60;b&#62;comment&#60;/b&#62;")
                || html.contains("&lt;b&gt;comment&lt;/b&gt;")
        );
        assert!(html.contains("class=\"spoiler\""));
    }

    #[test]
    fn preview_preserves_a_maximum_unicode_comment() {
        let comment = "😀".repeat(board_domain::MAX_COMMENT_CHARS);
        let report = Report {
            id: 1,
            board: "test".into(),
            post_id: 1,
            thread_id: 1,
            reason: "Synthetic full preview".into(),
            name: "Anonymous".into(),
            subject: String::new(),
            comment: comment.clone(),
            state: "open".into(),
            closed: false,
            sticky: false,
            deleted: false,
            attachment: None,
        };
        let html = Queue {
            media_origin: "http://127.0.0.1:3002".into(),
            reports: vec![report.into()],
            csrf: "example".into(),
            recent: true,
        }
        .render()
        .unwrap();
        assert!(html.contains(&comment));
    }
}
