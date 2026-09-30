use crate::store::Report;
use askama::Template;
use board_domain::comment_markup::Tag;
use board_domain::formatting::{Line, Token, parse_post_comment};
use board_domain::word_break::WordPart;
#[derive(Template)]
#[template(path = "login.html")]
pub struct Login;
#[derive(Template)]
#[template(path = "posting.html")]
pub struct Posting {
    pub public_origin: String,
    pub boards: Vec<(String, String)>,
    pub query: crate::handlers::PostingQuery,
    pub csrf: String,
    pub recent: bool,
    pub admin: bool,
}
pub struct Preview {
    pub report: Report,
    pub lines: Vec<Line>,
}
impl Preview {
    pub fn capcode(&self) -> Option<board_domain::capcode::Capcode> {
        self.report
            .capcode
            .as_deref()
            .and_then(board_domain::capcode::Capcode::parse)
    }
    pub fn flag(&self) -> Option<(&'static str, &str)> {
        if let (Some(code), Some(name)) = (&self.report.board_flag, &self.report.flag_name)
            && board_domain::country::board_flag(code).is_some()
        {
            return Some(("Board flag", name));
        }
        if let (Some(code), Some(name)) = (&self.report.country, &self.report.country_name)
            && board_domain::country::country_code(code)
        {
            return Some(("Country", name));
        }
        None
    }
}
impl From<Report> for Preview {
    fn from(report: Report) -> Self {
        let lines = parse_post_comment(&report.comment, report.comment_format);
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
    pub admin: bool,
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
            trip: Some("<em>untrusted trip</em>".into()),
            poster_id: Some("<b>untrusted ID</b>".into()),
            capcode: None,
            country: None,
            country_name: None,
            board_flag: None,
            flag_name: None,
            subject: "<i>subject</i>".into(),
            comment_format: 0,
            comment: "<b>comment</b>\n[spoiler]<i>text</i>[/spoiler]\n>>>/po/42 >>>/\"evil/42"
                .into(),
            state: "open".into(),
            closed: false,
            sticky: false,
            permasage: false,
            permaage: false,
            deleted: false,
            attachment: None,
        };
        let html = Queue {
            media_origin: "http://127.0.0.1:3002".into(),
            reports: vec![report.into()],
            csrf: "example".into(),
            recent: true,
            admin: false,
        }
        .render()
        .unwrap();
        assert!(!html.contains("<b>"));
        assert!(!html.contains("<em>"));
        assert!(!html.contains("<i>"));
        assert!(html.contains("class=\"postertrip\""));
        assert!(html.contains("untrusted trip"));
        assert!(html.contains("untrusted ID"));
        assert!(
            html.contains("&#60;b&#62;comment&#60;/b&#62;")
                || html.contains("&lt;b&gt;comment&lt;/b&gt;")
        );
        assert!(html.contains("class=\"spoiler\""));
        assert!(html.contains("<span>&gt;&gt;&gt;/po/42</span>"));
        assert!(!html.contains("href=\"/po/post/42\""));
        assert!(html.contains("Enable permasage"));
        assert!(!html.contains("Enable permaage"));
        assert!(!html.contains("Disable permaage"));
    }

    #[test]
    fn stamped_preview_keeps_markup_and_escapes_hostile_text() {
        for format in [0, 8, 9, 15, 24, 31, 40, 47, 56, 63] {
            let preview = Preview::from(Report {
                id: 1,
                board: "test".into(),
                post_id: 1,
                thread_id: 1,
                reason: "Owned preview".into(),
                name: "Anonymous".into(),
                trip: None,
                poster_id: None,
                capcode: None,
                country: None,
                country_name: None,
                board_flag: None,
                flag_name: None,
                subject: String::new(),
                comment_format: format,
                comment: "[spoiler]<b>first</b>\n>>42[/spoiler] [b]<script>owned</script>[/b]"
                    .into(),
                state: "open".into(),
                closed: false,
                sticky: false,
                permasage: false,
                permaage: false,
                deleted: false,
                attachment: None,
            });
            let html = Queue {
                media_origin: String::new(),
                reports: vec![preview],
                csrf: "owned-fixture".into(),
                recent: true,
                admin: false,
            }
            .render()
            .unwrap();
            assert_eq!(html.contains("<s>"), format & 1 != 0);
            assert_eq!(html.contains("class=\"mu-s\""), format & 16 != 0);
            assert!(!html.contains("<script>owned"));
            assert!(!html.contains("<b>first"));
            assert!(!html.contains("href=\"/test/post/42\""));
            assert!(html.contains("<span>&gt;&gt;42</span>"));
            assert!(html.contains("href=\"/comment-markup.css\""));
        }
    }

    #[test]
    fn preview_preserves_a_maximum_unicode_comment() {
        for format in [0, 40] {
            let comment = "😀".repeat(board_domain::MAX_COMMENT_CHARS);
            let report = Report {
                id: 1,
                board: "test".into(),
                post_id: 1,
                thread_id: 1,
                reason: "Synthetic full preview".into(),
                name: "Anonymous".into(),
                trip: None,
                poster_id: None,
                capcode: None,
                country: None,
                country_name: None,
                board_flag: None,
                flag_name: None,
                subject: String::new(),
                comment: comment.clone(),
                comment_format: format,
                state: "open".into(),
                closed: false,
                sticky: false,
                permasage: true,
                permaage: true,
                deleted: false,
                attachment: None,
            };
            let html = Queue {
                media_origin: "http://127.0.0.1:3002".into(),
                reports: vec![report.into()],
                csrf: "example".into(),
                recent: true,
                admin: true,
            }
            .render()
            .unwrap();
            assert_eq!(html.matches('😀').count(), board_domain::MAX_COMMENT_CHARS);
            assert_eq!(
                html.matches("<wbr>").count(),
                if format == 40 {
                    board_domain::MAX_COMMENT_CHARS / 35
                } else {
                    0
                }
            );
            if format == 0 {
                assert!(html.contains(&comment));
            }
            assert!(html.contains("permasage: true, permaage: true"));
            assert!(html.contains("Disable permasage"));
            assert!(html.contains("Disable permaage"));
        }
    }
}
