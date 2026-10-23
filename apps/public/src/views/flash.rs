//! Upload-board text fragments. Every returned HTML fragment is escaped before
//! source-length truncation; no uploaded markup can enter the template.
use super::{BoardPage, PostView};
use board_store::Board;

fn truncate(value: &str, maximum: usize) -> String {
    if value.chars().count() > maximum {
        value.chars().take(maximum).collect::<String>() + "(...)"
    } else {
        value.into()
    }
}
fn filename(value: &str) -> String {
    truncate(&board_domain::source_html_entities(value), 25)
}
fn subject(value: &str, comment: &str) -> String {
    let encoded = board_domain::source_html_entities(value);
    let value = if encoded.is_empty() {
        comment.split('<').next().unwrap_or("").to_owned()
    } else {
        encoded
    };
    truncate(&value.replace("&#44;", ","), 30)
}
fn size(bytes: i64, thread: bool) -> String {
    let bytes = bytes.max(0) as u64;
    if bytes >= 1_048_576 {
        let rounded = (bytes * 100 + 524_288) / 1_048_576;
        let number = if rounded.is_multiple_of(100) {
            (rounded / 100).to_string()
        } else if rounded.is_multiple_of(10) {
            format!("{}.{:01}", rounded / 100, (rounded % 100) / 10)
        } else {
            format!("{}.{:02}", rounded / 100, rounded % 100)
        };
        format!("{number} MB")
    } else if bytes >= 1024 {
        let divisor = if thread { 1048 } else { 1024 };
        format!("{} KB", (bytes + divisor / 2) / divisor)
    } else {
        format!("{bytes} B")
    }
}

impl PostView {
    pub fn flash_icon_title(&self) -> &'static str {
        match self.post.capcode.as_deref() {
            Some("admin" | "admin_highlight") => "This user is the 4chan Administrator.",
            Some("mod") => "This user is a 4chan Moderator.",
            Some("developer") => "This user is a 4chan Developer.",
            Some("manager") => "This user is a 4chan Manager.",
            _ => "",
        }
    }
    pub fn flash_icon_alt(&self) -> &'static str {
        if self.post.capcode.as_deref() == Some("manager") {
            "Manager Icon"
        } else {
            self.flash_icon_title()
        }
    }
    pub fn flash_post_badge(&self) -> Option<board_domain::capcode::PublicCapcode> {
        match self.post.capcode.as_deref() {
            Some("admin" | "admin_highlight" | "mod" | "developer" | "manager") => self.capcode(),
            Some("verified") if self.post.id == self.post.thread_id => self.capcode(),
            _ => None,
        }
    }
    pub fn flash_mobile_shortened(&self, board: &Board) -> bool {
        board_domain::source_html_entities(self.display_subject(board))
            .chars()
            .count()
            > 28
    }
    pub fn flash_mobile_subject_html(&self, board: &Board) -> String {
        let value = board_domain::source_html_entities(self.display_subject(board));
        if value.chars().count() > 28 {
            truncate(&value, 25)
        } else {
            value
        }
    }
    pub fn flash_reply_href(&self, board: &Board) -> askama::Result<String> {
        let title =
            board_domain::source_html_entities(self.display_subject(board)).replace("&#44;", ",");
        let comment = crate::semantic_thread::stored_comment(self);
        let comment = if title.is_empty() {
            comment
                .replace("&#44;", ",")
                .split('<')
                .next()
                .unwrap_or("")
                .to_owned()
        } else {
            comment
        };
        let context = board_domain::semantic_context::generate(&title, &comment, false)
            .map_err(|_| askama::Error::Fmt)?;
        Ok(crate::semantic_thread::href(
            &board.slug,
            self.post.thread_id,
            &context,
        ))
    }
    pub fn flash_badge(&self) -> Option<board_domain::capcode::PublicCapcode> {
        if matches!(
            self.post.capcode.as_deref(),
            Some("admin" | "admin_highlight" | "mod")
        ) {
            self.capcode()
        } else {
            None
        }
    }

    pub fn display_subject<'a>(&'a self, board: &Board) -> &'a str {
        if board.upload_board && self.post.id == self.post.thread_id {
            board_domain::upload_board::subject(&self.post.subject).1
        } else {
            &self.post.subject
        }
    }
    pub fn flash_filename_html(&self) -> String {
        self.post
            .attachment
            .as_ref()
            .map_or_else(String::new, |file| {
                filename(board_domain::file_name::basename(&file.filename))
            })
    }
    pub fn flash_filename(&self) -> &str {
        self.post
            .attachment
            .as_ref()
            .map_or("", |file| board_domain::file_name::basename(&file.filename))
    }
    pub fn flash_file_url(&self) -> String {
        self.post
            .attachment
            .as_ref()
            .map_or_else(String::new, |file| {
                board_domain::file_name::url_component(&file.display_filename())
            })
    }
    pub fn flash_subject_html(&self) -> String {
        subject(
            board_domain::upload_board::subject(&self.post.subject).1,
            &crate::semantic_thread::stored_comment(self),
        )
    }
    pub fn flash_tag(&self) -> &'static str {
        board_domain::upload_board::subject(&self.post.subject)
            .0
            .map_or("", |category| category.short)
    }
    pub fn flash_category(&self) -> &'static str {
        board_domain::upload_board::subject(&self.post.subject)
            .0
            .map_or("", |category| category.name)
    }
    pub fn flash_client_category(&self) -> &'static str {
        let (category, _, prefixed) = board_domain::upload_board::subject(&self.post.subject);
        if !prefixed {
            "undefined"
        } else {
            category.map_or("null", |value| value.name)
        }
    }
    pub fn flash_listing_size(&self, bytes: &i64) -> String {
        size(*bytes, false)
    }
    pub fn flash_thread_size(&self, bytes: &i64) -> String {
        size(*bytes, true)
    }
}
impl BoardPage {
    pub fn flash_old(&self, id: &i64) -> bool {
        self.flash_old_ids.contains(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::{Tag, Token, WordPart};
    #[derive(askama::Template)]
    #[template(
        source = "{% import \"formatting.html\" as formatting %}{% import \"attachments.html\" as attachments %}{% include \"flash_post_content.html\" %}",
        ext = "html"
    )]
    struct SourceFlashFragment<'a> {
        item: &'a PostView,
        view: &'a super::super::ThreadView,
        board: &'a Board,
        media_origin: &'a str,
        spoiler_thumbnail: &'a str,
    }

    #[test]
    fn source_flash_badges_and_mobile_identity_parentage_remain_distinct() {
        use askama::Template;
        for reply in [false, true] {
            for code in [
                "admin",
                "admin_highlight",
                "mod",
                "developer",
                "manager",
                "verified",
                "founder",
                "",
            ] {
                let mut snapshot = crate::native_updater_snapshot::tests::fixture();
                snapshot.board.upload_board = true;
                snapshot.board.slug = "f".into();
                snapshot.thread.board = "f".into();
                let mut post = snapshot.posts.remove(usize::from(reply));
                post.board = "f".into();
                post.capcode = (!code.is_empty()).then(|| code.to_owned());
                post.trip = Some("!OwnedTrip1".into());
                let item = PostView::new(post);
                let view = super::super::ThreadView {
                    thread: snapshot.thread,
                    posts: vec![],
                    omitted: 0,
                    image_replies: 0,
                    tail_size: 0,
                    latest_reply_id: None,
                    catalog_position: None,
                    catalog_last_reply: None,
                };
                let html = SourceFlashFragment {
                    spoiler_thumbnail: "",
                    item: &item,
                    view: &view,
                    board: &snapshot.board,
                    media_origin: "",
                }
                .render()
                .unwrap();
                assert!(html.contains(&format!(
                    "id=\"pim{}\"><span class=\"nameBlock",
                    item.post.id
                )));
                assert_eq!(html.matches("class=\"postertrip\"").count(), 2);
                let visible = matches!(
                    code,
                    "admin" | "admin_highlight" | "mod" | "developer" | "manager"
                ) || (code == "verified" && !reply);
                assert_eq!(
                    html.contains("<strong class=\"capcode"),
                    visible,
                    "{code}/{reply}"
                );
                let interactive = code == "manager" || (code == "verified" && !reply);
                assert_eq!(
                    html.matches("class=\"capcode hand ").count(),
                    if interactive { 2 } else { 0 },
                    "{code}/{reply}"
                );
                if code == "manager" {
                    assert_eq!(
                        html.matches("title=\"Highlight posts by Managers\"")
                            .count(),
                        2
                    );
                    assert_eq!(html.matches("managericon@2x.gif 2x").count(), 2);
                    assert_eq!(html.matches("alt=\"Manager Icon\"").count(), 2);
                }
                if code == "admin" {
                    assert_eq!(
                        html.matches("title=\"This user is the 4chan Administrator.\"")
                            .count(),
                        2
                    );
                }
                assert_eq!(
                    item.flash_badge().is_some(),
                    matches!(code, "admin" | "admin_highlight" | "mod")
                );
            }
        }
    }

    #[test]
    fn initial_and_client_flash_metadata_keep_source_differences() {
        let mut snapshot = crate::native_updater_snapshot::tests::fixture();
        snapshot.board.upload_board = true;
        let mut item = PostView::new(snapshot.posts.remove(0));
        // The initial PHP thread divides KB by 1048, while its JSON client
        // uses the ordinary 1024-byte calculation.
        assert_eq!(item.flash_thread_size(&52_000), "50 KB");
        assert_eq!(item.file_size(&52_000), "51 KB");
        for (subject, category, displayed) in [
            ("3|Owned", "Game", "Owned"),
            ("99|Owned", "null", "Owned"),
            ("Owned", "undefined", "Owned"),
            ("3|0", "Game", ""),
        ] {
            item.post.subject = subject.into();
            assert_eq!(item.flash_client_category(), category);
            assert_eq!(item.client_subject(&snapshot.board), displayed);
        }
        snapshot.board.upload_board = false;
        assert_eq!(item.client_subject(&snapshot.board), "3|0");
    }

    #[test]
    fn row_fragments_match_executed_source_and_cannot_contain_tags() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../fixtures/upload-row-reference.json"
        ))
        .unwrap();
        for row in fixture["cases"].as_array().unwrap() {
            let expected = &row["expected"];
            let label = filename(row["filename"].as_str().unwrap());
            assert_eq!(label, expected["filename"]);
            assert!(!label.contains('<'));
            if expected["visible"] == true {
                let teaser = subject(
                    row["subject"].as_str().unwrap(),
                    row["comment"].as_str().unwrap(),
                );
                assert_eq!(teaser, expected["subject"]);
                assert!(!teaser.contains('<'));
            }
            assert_eq!(
                size(row["bytes"].as_i64().unwrap(), false),
                expected["listing_size"]
            );
            assert_eq!(
                size(row["bytes"].as_i64().unwrap(), true),
                expected["thread_size"]
            );
        }
    }
}
