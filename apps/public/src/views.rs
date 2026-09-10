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
    pub board: Board,
    pub threads: Vec<ThreadView>,
    pub parent: i64,
    pub previous: String,
    pub next: String,
    pub catalog: bool,
}

impl BoardPage {
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
    pub thread: Thread,
    pub posts: Vec<PostView>,
    pub omitted: usize,
}
pub struct PostView {
    pub post: Post,
    pub lines: Vec<Line>,
    pub now: String,
}
impl PostView {
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

#[derive(Template)]
#[template(path = "comment.html")]
pub struct Comment<'a> {
    pub lines: &'a [Line],
    pub board: &'a str,
}

#[derive(Template)]
#[template(path = "error.html")]
pub struct Message<'a> {
    pub title: &'a str,
    pub message: &'a str,
}
