use crate::store::Report;
use askama::Template;
use board_domain::comment_markup::Tag;
use board_domain::drawing_annotation::DrawingAnnotation;
use board_domain::formatting::{Line, Token, parse_saved_comment_with_limits};
use board_domain::word_break::WordPart;
#[derive(Template)]
#[template(path = "login.html")]
pub struct Login;
#[derive(Template)]
#[template(path = "posting.html")]
pub struct Posting {
    pub upload_enabled: bool,
    pub receipt: Option<crate::uploads::Receipt>,
    pub public_origin: String,
    pub boards: Vec<(String, String, i32, String, String)>,
    pub comment_max_units: usize,
    pub query: crate::handlers::PostingQuery,
    pub csrf: String,
    pub recent: bool,
    pub admin: bool,
    pub badges: Vec<(String, String)>,
    pub selected_badge: String,
    pub ordinary_ready: bool,
    pub flags: Vec<(String, String)>,
    pub flag_catalog: Vec<(String, String, String)>,
}
impl Posting {
    pub fn can_submit(&self) -> bool {
        self.receipt.as_ref().is_none_or(|receipt| receipt.ready)
    }
}
pub struct Preview {
    pub report: Report,
    pub lines: Vec<Line>,
    pub drawing: Option<DrawingAnnotation>,
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
            && board_domain::board_flags::flag(&self.report.board_flag_type, code).is_some()
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
        let drawing = DrawingAnnotation::from_saved(
            report.drawing_time_seconds,
            report.drawing_source_post_id,
        );
        let lines = parse_saved_comment_with_limits(
            &report.comment,
            report.comment_format,
            &report.board,
            report.wordfilter_payload.as_deref(),
            if report.staff_authorized_limits {
                board_domain::PostLimits::authorized(board_domain::MAX_AUTHORIZED_COMMENT_CHARS)
                    .expect("finite persisted staff bound")
            } else {
                board_domain::PostLimits::ordinary(board_domain::MAX_COMMENT_CHARS)
            },
        );
        Self {
            report,
            lines,
            drawing,
        }
    }
}
#[derive(Template)]
#[template(path = "queue.html")]
pub struct Queue {
    pub media_origin: String,
    pub reports: Vec<Preview>,
    pub csrf: String,
    pub recent: bool,
    pub can_permaage: bool,
    pub can_clear_reporter: bool,
    pub can_cleanup: bool,
    pub moderator: bool,
    pub can_post: bool,
    pub discussion: bool,
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_md5_disclosure_is_archived_available_and_canonical() {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        // The source decodes these exact 16 bytes from the public Base64 MD5.
        let bytes = STANDARD.decode("ABEiM0RVZneImaq7zN3u/w==").unwrap();
        assert_eq!(bytes.len(), 16);
        let digest: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        assert_eq!(digest, "00112233445566778899aabbccddeeff");
        for (raw, available, archived, visible) in [
            (Some(digest.as_str()), true, true, true),
            (Some(digest.as_str()), true, false, false),
            (Some(digest.as_str()), false, true, false),
            (None, true, true, false),
            (Some(""), true, true, false),
            (Some("00112233445566778899aabbccddeef"), true, true, false),
            (Some("00112233445566778899aabbccddeeff0"), true, true, false),
            (Some("00112233445566778899AABBCCDDEEFF"), true, true, false),
            (Some("00112233445566778899aabbccddeefg"), true, true, false),
            (Some("<script>alert(1)</script>xxxxxxxx"), true, true, false),
        ] {
            let mut report = Report {
                id: 1,
                board: "test".into(),
                post_id: 1,
                thread_id: 1,
                reason: "<b>untrusted report</b>".into(),
                category_id: None,
                category_kind: None,
                name: "<em>name</em>".into(),
                trip: Some("<em>untrusted trip</em>".into()),
                poster_id: Some("<b>untrusted ID</b>".into()),
                capcode: None,
                country: None,
                country_name: None,
                board_flag: None,
                board_flag_type: "pol".into(),
                flag_name: None,
                subject: "<i>subject</i>".into(),
                comment_format: 0,
                staff_authorized_limits: false,
                wordfilter_payload: None,
                drawing_time_seconds: None,
                drawing_source_post_id: None,
                comment: "<b>comment</b>\n[spoiler]<i>text</i>[/spoiler]\n>>>/po/42 >>>/\"evil/42"
                    .into(),
                state: "open".into(),
                closed: false,
                sticky: false,
                permasage: false,
                permaage: false,
                undead: false,
                archived: false,
                archives_enabled: true,
                deleted: false,
                spoilers_enabled: false,
                image_spoiler: false,
                attachment: None,
            };
            report.archived = archived;
            report.attachment = Some(crate::store::Attachment {
                post_id: 1,
                filename: "synthetic.png".into(),
                bytes: 100,
                width: 500,
                height: 300,
                spoiler: true,
                tim: 1,
                thumbnail_width: Some(250),
                thumbnail_height: Some(150),
                output_format: board_store::media_assets::MediaFormat::Png,
                available,
                md5: raw.map(str::to_owned),
            });
            let html = Queue {
                media_origin: "http://127.0.0.1:3002".into(),
                reports: vec![report.into()],
                csrf: "example".into(),
                recent: true,
                can_permaage: false,
                can_clear_reporter: false,
                can_cleanup: false,
                moderator: true,
                can_post: false,
                discussion: false,
            }
            .render()
            .unwrap();
            assert_eq!(html.contains("<summary>File MD5</summary>"), visible);
            assert_eq!(html.contains(&format!("<code>{digest}</code>")), visible);
            assert_eq!(html.contains("Approved normalized file MD5:"), visible);
            assert!(!html.contains("<script"));
            assert!(!html.contains("onclick="));
        }
    }

    #[test]
    fn preview_escapes_untrusted_text_with_typed_formatting() {
        let report = Report {
            id: 1,
            board: "test".into(),
            post_id: 1,
            thread_id: 1,
            reason: "<b>untrusted report</b>".into(),
            category_id: None,
            category_kind: None,
            name: "<em>name</em>".into(),
            trip: Some("<em>untrusted trip</em>".into()),
            poster_id: Some("<b>untrusted ID</b>".into()),
            capcode: None,
            country: None,
            country_name: None,
            board_flag: None,
            board_flag_type: "pol".into(),
            flag_name: None,
            subject: "<i>subject</i>".into(),
            comment_format: 0,
            staff_authorized_limits: false,
            wordfilter_payload: None,
            drawing_time_seconds: None,
            drawing_source_post_id: None,
            comment: "<b>comment</b>\n[spoiler]<i>text</i>[/spoiler]\n>>>/po/42 >>>/\"evil/42"
                .into(),
            state: "open".into(),
            closed: false,
            sticky: false,
            permasage: false,
            permaage: false,
            undead: false,
            archived: false,
            archives_enabled: true,
            deleted: false,
            spoilers_enabled: false,
            image_spoiler: false,
            attachment: None,
        };
        let html = Queue {
            media_origin: "http://127.0.0.1:3002".into(),
            reports: vec![report.into()],
            csrf: "example".into(),
            recent: true,
            can_permaage: false,
            can_clear_reporter: false,
            can_cleanup: false,
            moderator: true,
            can_post: true,
            discussion: true,
        }
        .render()
        .unwrap();
        assert!(!html.contains("action=\"/reporter-clear\""));
        assert!(!html.contains("<b>"));
        assert!(!html.contains("<em>"));
        assert!(!html.contains("<i>"));
        assert!(html.contains("<p>Reason: "));
        assert!(!html.contains("<p>Category: "));
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
    fn drawing_preview_keeps_the_original_comment_and_plain_source_reference() {
        for (seconds, source, suffix) in [
            (
                Some(59),
                None,
                "<br><br><small><b>Oekaki Post</b> (Time: 59s)</small>",
            ),
            (
                Some(90),
                Some(42),
                "<br><br><small><b>Oekaki Post</b> (Time: 2m, Source: &gt;&gt;42)</small>",
            ),
            (
                Some(3630),
                Some(42),
                "<br><br><small><b>Oekaki Post</b> (Time: 1h 1m, Source: &gt;&gt;42)</small>",
            ),
            (
                Some(90),
                Some(-1),
                "<br><br><small><b>Oekaki Post</b> (Time: 2m)</small>",
            ),
            (None, None, ""),
            (None, Some(42), ""),
            (Some(0), Some(42), ""),
            (Some(5_184_001), None, ""),
        ] {
            let preview = Preview::from(Report {
                id: 1,
                board: "i".into(),
                post_id: 1,
                thread_id: 1,
                reason: "Owned preview".into(),
                category_id: None,
                category_kind: None,
                name: "Anonymous".into(),
                trip: None,
                poster_id: None,
                capcode: None,
                country: None,
                country_name: None,
                board_flag: None,
                board_flag_type: "pol".into(),
                flag_name: None,
                subject: String::new(),
                comment: "A plain comment".into(),
                drawing_time_seconds: seconds,
                drawing_source_post_id: source,
                comment_format: 0,
                staff_authorized_limits: false,
                wordfilter_payload: None,
                state: "open".into(),
                closed: false,
                sticky: false,
                permasage: false,
                permaage: false,
                undead: false,
                archived: false,
                archives_enabled: false,
                deleted: false,
                spoilers_enabled: false,
                image_spoiler: false,
                attachment: None,
            });
            assert_eq!(preview.report.comment, "A plain comment");
            assert_eq!(
                board_domain::formatting::plain_text(&preview.lines),
                "A plain comment"
            );
            let html = Queue {
                media_origin: String::new(),
                reports: vec![preview],
                csrf: "owned-fixture".into(),
                recent: true,
                can_permaage: false,
                can_clear_reporter: false,
                can_cleanup: false,
                moderator: true,
                can_post: false,
                discussion: false,
            }
            .render()
            .unwrap();
            let contents = html
                .split_once("<blockquote>")
                .unwrap()
                .1
                .split_once("</blockquote>")
                .unwrap()
                .0;
            assert_eq!(
                contents,
                format!("A plain comment{suffix}"),
                "{seconds:?}/{source:?}"
            );
            assert!(!contents.contains("href=") && !contents.contains("<a "));
        }
    }

    #[test]
    fn category_preview_uses_captured_title_and_identity() {
        for (kind, label, title) in [
            (1, "rule", "<script>untrusted category</script>".to_owned()),
            (2, "illegal", String::new()),
            (1, "rule", "😀".repeat(1024)),
        ] {
            let report = Report {
                id: 1,
                board: "test".into(),
                post_id: 1,
                thread_id: 1,
                reason: title.clone(),
                category_id: Some(42),
                category_kind: Some(kind),
                name: "Anonymous".into(),
                trip: None,
                poster_id: None,
                capcode: None,
                country: None,
                country_name: None,
                board_flag: None,
                board_flag_type: "pol".into(),
                flag_name: None,
                subject: String::new(),
                comment: String::new(),
                comment_format: 0,
                staff_authorized_limits: false,
                wordfilter_payload: None,
                drawing_time_seconds: None,
                drawing_source_post_id: None,
                state: "open".into(),
                closed: false,
                sticky: false,
                permasage: false,
                permaage: false,
                undead: false,
                archived: false,
                archives_enabled: true,
                deleted: false,
                spoilers_enabled: false,
                image_spoiler: false,
                attachment: None,
            };
            let html = Queue {
                media_origin: String::new(),
                reports: vec![report.into()],
                csrf: "category-fixture".into(),
                recent: true,
                can_permaage: false,
                can_clear_reporter: false,
                can_cleanup: false,
                moderator: true,
                can_post: false,
                discussion: false,
            }
            .render()
            .unwrap();
            assert!(html.contains(&format!("<p>Category: 42 ({label})")));
            assert!(!html.contains("<p>Reason: "));
            assert!(!html.contains("<script>"));
            assert!(!html.contains("priority"));
            if title.is_empty() {
                assert!(html.contains("<p>Category: 42 (illegal)</p>"));
            } else if title.starts_with('<') {
                assert!(
                    html.contains("&#60;script&#62;untrusted category&#60;/script&#62;")
                        || html.contains("&lt;script&gt;untrusted category&lt;/script&gt;")
                );
            } else {
                assert!(html.contains(&title));
            }
            assert!(html.contains("Resolve report"));
            assert!(html.contains("Dismiss report"));
        }
    }

    #[test]
    fn stamped_preview_keeps_markup_and_escapes_hostile_text() {
        for format in [0, 8, 9, 15, 24, 31, 40, 47, 56, 63, 104, 105, 111, 120, 127] {
            let preview = Preview::from(Report {
                id: 1,
                board: "test".into(),
                post_id: 1,
                thread_id: 1,
                reason: "Owned preview".into(),
                category_id: None,
                category_kind: None,
                name: "Anonymous".into(),
                trip: None,
                poster_id: None,
                capcode: None,
                country: None,
                country_name: None,
                board_flag: None,
                board_flag_type: "pol".into(),
                flag_name: None,
                subject: String::new(),
                comment_format: format,
                staff_authorized_limits: false,
                wordfilter_payload: None,
                drawing_time_seconds: None,
                drawing_source_post_id: None,
                comment: "[spoiler]<b>first</b>\n>>42[/spoiler] [b]<script>owned</script>[/b] https://www.4chan.org/faq https://example.org/path"
                    .into(),
                state: "open".into(),
                closed: false,
                sticky: false,
                permasage: false,
                permaage: false,
                undead: false,
                archived: false,
                archives_enabled: true,
                deleted: false,
                spoilers_enabled: false,
                image_spoiler: false,
                attachment: None,
            });
            let html = Queue {
                media_origin: String::new(),
                reports: vec![preview],
                csrf: "owned-fixture".into(),
                recent: true,
                can_permaage: false,
                can_clear_reporter: false,
                can_cleanup: false,
                moderator: true,
                can_post: true,
                discussion: true,
            }
            .render()
            .unwrap();
            assert_eq!(html.contains("<s>"), format & 1 != 0);
            assert_eq!(html.contains("class=\"mu-s\""), format & 16 != 0);
            assert!(!html.contains("<script>owned"));
            assert!(!html.contains("<b>first"));
            assert!(!html.contains("href=\"/test/post/42\""));
            assert!(html.contains("<span>&gt;&gt;42</span>"));
            assert!(html.contains("href=\"https://www.4chan.org/faq\""));
            assert_eq!(
                html.contains("href=\"https://example.org/path\""),
                format & 64 == 0
            );
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
                category_id: None,
                category_kind: None,
                name: "Anonymous".into(),
                trip: None,
                poster_id: None,
                capcode: None,
                country: None,
                country_name: None,
                board_flag: None,
                board_flag_type: "pol".into(),
                flag_name: None,
                subject: String::new(),
                comment: comment.clone(),
                comment_format: format,
                staff_authorized_limits: false,
                wordfilter_payload: None,
                drawing_time_seconds: None,
                drawing_source_post_id: None,
                state: "open".into(),
                closed: false,
                sticky: false,
                permasage: true,
                permaage: true,
                undead: false,
                archived: false,
                archives_enabled: true,
                deleted: false,
                spoilers_enabled: false,
                image_spoiler: false,
                attachment: None,
            };
            let html = Queue {
                media_origin: "http://127.0.0.1:3002".into(),
                reports: vec![report.into()],
                csrf: "example".into(),
                recent: true,
                can_permaage: true,
                can_clear_reporter: true,
                can_cleanup: false,
                moderator: true,
                can_post: true,
                discussion: true,
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
            assert!(html.contains("action=\"/reporter-clear\""));
            assert!(html.contains("name=\"report_id\" value=\"1\""));
            assert!(html.contains("Clear reporter"));
        }
    }
}
