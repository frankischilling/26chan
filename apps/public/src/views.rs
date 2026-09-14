use askama::Template;
use board_domain::{Line, Token, parse_comment};
use board_store::{Board, Post, Thread};

#[derive(Template)]
#[template(path = "home.html")]
pub struct Home {
    pub boards: Vec<Board>,
}

#[derive(Template)]
#[template(path = "board.html")]
pub struct BoardPage {
    pub catalog_hidden: Vec<ThreadView>,
    pub board: Board,
    pub threads: Vec<ThreadView>,
    pub parent: i64,
    pub previous: String,
    pub next: String,
    pub catalog: bool,
    pub catalog_options: crate::catalog::Options,
    pub media_origin: String,
}

#[derive(Template)]
#[template(path = "upload.html")]
pub struct UploadPage {
    pub board: Board,
    pub form: UploadForm,
    pub ready: bool,
    pub message: &'static str,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UploadForm {
    pub upload_id: String,
    pub upload_capability: String,
    pub resto: i64,
}

impl BoardPage {
    pub fn posting_allowed(&self) -> bool {
        !self.catalog
            && (self.parent == 0
                || self
                    .threads
                    .first()
                    .is_some_and(|view| !view.thread.closed && view.thread.archived_at.is_none()))
    }

    pub fn archived(&self) -> bool {
        self.parent != 0
            && self
                .threads
                .first()
                .is_some_and(|view| view.thread.archived_at.is_some())
    }
}

#[derive(Template)]
#[template(path = "archive.html")]
pub struct ArchivePage {
    pub board: Board,
    pub entries: Vec<board_store::ArchiveEntry>,
}

pub struct ThreadView {
    pub tail_size: usize,
    pub latest_reply_id: Option<i64>,
    pub thread: Thread,
    pub posts: Vec<PostView>,
    pub omitted: usize,
    pub image_replies: i64,
}
impl ThreadView {
    pub fn bump_limited(&self, board: &Board) -> bool {
        board_domain::bump::limited(
            self.thread.sticky,
            self.thread.permaage,
            self.visible_replies() as u64,
            board.bump_limit as u32,
        )
    }

    /// HTML catalog rule; public JSON additionally excludes undead threads.
    pub fn image_limited(&self, board: &Board) -> bool {
        board_domain::image_limit::catalog_limited(
            self.thread.sticky,
            self.thread.permaage,
            self.image_replies as u64,
            board.image_limit as u32,
        )
    }

    pub fn visible_replies(&self) -> usize {
        self.omitted
            .saturating_add(self.posts.len())
            .saturating_sub(1)
    }
}
pub struct PostView {
    pub post: Post,
    pub lines: Vec<Line>,
    pub now: String,
}

#[derive(Template)]
#[template(path = "post_fragment.html")]
pub struct PostFragment<'a> {
    pub item: &'a PostView,
    pub view: &'a ThreadView,
    pub board: &'a Board,
    pub media_origin: &'a str,
    pub catalog: bool,
}

impl PostView {
    pub fn catalog_search_text(&self) -> String {
        crate::catalog::search_text(&self.post.subject, &self.lines)
    }

    pub fn catalog_size(&self, large: bool) -> (i64, i64) {
        let Some(file) = &self.post.attachment else {
            return (1, 1);
        };
        catalog_dimensions(
            file.thumbnail_width.unwrap_or(file.width),
            file.thumbnail_height.unwrap_or(file.height),
            if large { 250 } else { 150 },
        )
    }
    pub fn new(post: Post) -> Self {
        let lines = parse_comment(&post.comment);
        let now = post
            .created_at
            .with_timezone(&chrono_tz::America::New_York)
            .format("%m/%d/%y(%a)%H:%M:%S")
            .to_string();
        Self { post, lines, now }
    }
}

fn catalog_dimensions(width: i32, height: i32, limit: i64) -> (i64, i64) {
    let width = i64::from(width.max(1));
    let height = i64::from(height.max(1));
    let largest = width.max(height).max(limit);
    (
        (width * limit / largest).max(1),
        (height * limit / largest).max(1),
    )
}

#[cfg(test)]
mod catalog_tests {
    #[test]
    fn thumbnail_attributes_are_bounded_without_upscaling_or_integer_overflow() {
        for (input, expected) in [
            ((250, 150), (150, 90)),
            ((100, 250), (60, 150)),
            ((48, 32), (48, 32)),
            ((1, 1), (1, 1)),
            ((i32::MAX, i32::MAX), (150, 150)),
            ((i32::MAX, 1), (150, 1)),
            ((0, i32::MIN), (1, 1)),
        ] {
            assert_eq!(super::catalog_dimensions(input.0, input.1, 150), expected);
        }
    }
}

#[derive(Template)]
#[template(path = "comment.html")]
pub struct Comment<'a> {
    pub lines: &'a [Line],
    pub board: &'a str,
}

#[cfg(test)]
mod comment_tests {
    use super::*;
    use board_domain::{CommentSpacing, prepare_post_comment};

    #[test]
    fn prepared_comment_remains_escaped_text_in_the_real_template() {
        for (code, sjis) in [(false, false), (true, false), (false, true), (true, true)] {
            let text = prepare_post_comment(
                "Anonymous",
                "",
                " \tC <script> \r\n",
                1000,
                false,
                CommentSpacing::for_board("test", code, sjis),
            )
            .unwrap();
            assert_eq!(text, "C <script>");
            let lines = parse_comment(&text);
            let html = Comment {
                lines: &lines,
                board: "test",
            }
            .render()
            .unwrap();
            assert_eq!(html.trim(), "C &#60;script&#62;");
            assert!(!html.contains("<script>"));
        }
    }
}

#[derive(Template)]
#[template(path = "error.html")]
pub struct Message<'a> {
    pub title: &'a str,
    pub message: &'a str,
}
