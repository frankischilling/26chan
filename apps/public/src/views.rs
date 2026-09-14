use askama::Template;
use board_domain::comment_markup::Tag;
use board_domain::word_break::WordPart;
use board_domain::{Line, Token, parse_post_comment};
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
    pub catalog_last_reply: Option<board_store::CatalogReply>,
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
    pub fn catalog_teaser(&self, board: &Board) -> crate::catalog::teaser::Prepared {
        crate::catalog::teaser::prepare(&self.lines, &board.slug, board.into())
    }

    pub fn catalog_search_text(&self, teaser: &crate::catalog::teaser::Prepared) -> String {
        crate::catalog::search_text(&self.post.subject, teaser)
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
        let lines = parse_post_comment(&post.comment, post.comment_format);
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
    use board_domain::{CommentSpacing, parse_comment, prepare_post_comment};

    fn render(input: &str, format: i16) -> String {
        Comment {
            lines: &parse_post_comment(input, format),
            board: "test",
        }
        .render()
        .unwrap()
    }

    #[test]
    fn word_break_profiles_preserve_history_markup_boundaries_and_link_destinations() {
        let long = "x".repeat(70);
        for old in (8..=15).chain(24..=31) {
            assert_eq!(render(&long, old), long);
            assert_eq!(
                render(&long, old + 32),
                format!("{}<wbr>{}<wbr>", "x".repeat(35), "x".repeat(35))
            );
        }
        let input = format!(
            "{}[b]{}[/b]{}",
            "a".repeat(34),
            "b".repeat(35),
            "c".repeat(34)
        );
        assert_eq!(
            render(&input, 56),
            format!(
                "{}<span class=\"mu-s\">{}<wbr></span>{}",
                "a".repeat(34),
                "b".repeat(35),
                "c".repeat(34)
            )
        );
        let url = format!("https://example.org/{}", "x".repeat(70));
        let html = render(&url, 40);
        assert!(html.contains(&format!("href=\"{url}\"")));
        assert_eq!(html.matches("<wbr>").count(), 2);
        assert_eq!(render("left{{w_br}}right", 40), "left<wbr>right");
        assert_eq!(render("left{{w_br}}right", 8), "left{{w_br}}right");
        let hostile = format!("{}<script>alert(1)</script>", "x".repeat(35));
        let html = render(&hostile, 40);
        assert!(html.contains("<wbr>&#60;script&#62;"));
        assert!(!html.contains("<script>"));
    }

    #[test]
    fn stamped_policy_controls_only_literal_approved_markup() {
        for mask in (0..8).chain(16..24) {
            let format = 8 + mask;
            for (flag, raw, expected) in [
                (
                    16,
                    "[b]<script>[/b]",
                    "<span class=\"mu-s\">&#60;script&#62;</span>",
                ),
                (16, "[i]italic[/i]", "<span class=\"mu-i\">italic</span>"),
                (16, "[red]red[/red]", "<span class=\"mu-r\">red</span>"),
                (
                    16,
                    "[green]green[/green]",
                    "<span class=\"mu-g\">green</span>",
                ),
                (16, "[blue]blue[/blue]", "<span class=\"mu-b\">blue</span>"),
                (1, "[spoiler]a\nb[/spoiler]", "<s>a<br>b</s>"),
                (
                    2,
                    "[code]\nlong <script>\ntext[/code]",
                    "<pre class=\"prettyprint\">long &#60;script&#62;<br>text</pre>",
                ),
                (
                    4,
                    "[sjis]a  b\nc[/sjis]",
                    "<span class=\"sjis\">a  b<br>c</span>",
                ),
            ] {
                let html = render(raw, format);
                if mask & flag != 0 {
                    assert_eq!(html, expected);
                } else {
                    assert_eq!(
                        html,
                        raw.replace('<', "&#60;")
                            .replace('>', "&#62;")
                            .replace('\n', "<br>")
                    );
                }
                assert!(!html.contains("<script>"));
            }
        }
        assert_eq!(render("[spoiler] \n[spoiler]\t[/spoiler][/spoiler]", 9), "");
        assert_eq!(render("[code]123456[/code]", 10), "123456");
        assert_eq!(
            render("[code]1234567[/code]", 10),
            "<pre class=\"prettyprint\">1234567</pre>"
        );
        assert_eq!(
            render("[spoiler]x[code]1234567[/spoiler]z[/code]", 11),
            "<s>x<pre class=\"prettyprint\">1234567</s>z</pre>"
        );
        assert_eq!(
            render("[spoiler]https://example.org/ >>42[/spoiler]", 9),
            "<s><a href=\"https://example.org/\" rel=\"nofollow noreferrer noopener\">https://example.org/</a> <a class=\"quotelink\" href=\"/test/post/42\">&gt;&gt;42</a></s>"
        );
        assert_eq!(
            render("<s>x</s><pre onclick='x'>y</pre>", 15),
            "&#60;s&#62;x&#60;/s&#62;&#60;pre onclick=&#39;x&#39;&#62;y&#60;/pre&#62;"
        );
    }

    #[test]
    fn legacy_unknown_and_quote_boundaries_do_not_gain_markup_authority() {
        let raw = "[spoiler]>>42[/spoiler]";
        assert_eq!(
            render(raw, 0),
            "<span class=\"spoiler\" tabindex=\"0\" aria-label=\"Spoiler; focus to reveal\">&#62;&#62;42</span>"
        );
        for format in [-1, 1, 7, 16, i16::MAX] {
            assert_eq!(
                render("[spoiler]<b>\n>>42 https://example.org/[/spoiler]", format),
                "[spoiler]&#60;b&#62;<br>&#62;&#62;42 https://example.org/[/spoiler]"
            );
        }
        assert_eq!(
            render(">before[spoiler]>inside\n>next[/spoiler]>after", 9),
            "<span class=\"quote\">&#62;before</span><s>&#62;inside<br><span class=\"quote\">&#62;next</span></s>&#62;after"
        );
        assert_eq!(
            render(" >first\n >next\n  >two", 8),
            " &#62;first<br> <span class=\"quote\">&#62;next</span><br>  &#62;two"
        );
        assert_eq!(
            render(">before https://example.org/ after", 8),
            "<span class=\"quote\">&#62;before </span><a href=\"https://example.org/\" rel=\"nofollow noreferrer noopener\">https://example.org/</a> after"
        );
    }

    #[test]
    fn stored_local_quote_rewrite_uses_normal_links_and_escaped_spoiler_text() {
        let text = prepare_post_comment(
            "",
            "",
            "See >>>/test/42 and >>>/other/42.\n[spoiler]>>>/test/42[/spoiler]",
            1000,
            false,
            CommentSpacing::for_board("test", true, false),
        )
        .unwrap();
        let lines = parse_comment(&text);
        let html = Comment {
            lines: &lines,
            board: "test",
        }
        .render()
        .unwrap();
        assert!(html.contains("href=\"/test/post/42\">&gt;&gt;42</a>"));
        assert!(html.contains("href=\"/other/post/42\">&gt;&gt;&gt;/other/42</a>"));
        assert!(html.contains("aria-label=\"Spoiler; focus to reveal\">&#62;&#62;42</span>"));
    }

    #[test]
    fn line_cleanup_keeps_real_template_escaping_and_disabled_spoilers() {
        for spoilers in [false, true] {
            let text = prepare_post_comment(
                "",
                "",
                "a[spoiler]b[/spoiler]c <script>\r\nline1\r\nline2\r\nline3",
                1000,
                false,
                CommentSpacing::for_board("test", true, false).with_line_rules(3, spoilers),
            )
            .unwrap();
            let lines = parse_comment(&text);
            let html = Comment {
                lines: &lines,
                board: "test",
            }
            .render()
            .unwrap();
            assert!(html.contains("&#60;script&#62;<br>line1<br>line2<br>line3"));
            assert_eq!(html.contains("class=\"spoiler\""), !spoilers);
            assert!(!html.contains("<script>"));
        }
    }

    #[test]
    fn prepared_comment_remains_escaped_text_in_the_real_template() {
        for (code, sjis) in [(false, false), (true, false), (false, true), (true, true)] {
            for raw in [" \tC <script> \r\n", " \tC ＜script＞ \r\n"] {
                let text = prepare_post_comment(
                    "Anonymous",
                    "",
                    raw,
                    1000,
                    false,
                    CommentSpacing::for_board("test", code, sjis),
                )
                .unwrap();
                let retains_wide = sjis && raw.contains('＜');
                assert_eq!(
                    text,
                    if retains_wide {
                        "C ＜script＞"
                    } else {
                        "C <script>"
                    }
                );
                let lines = parse_comment(&text);
                let html = Comment {
                    lines: &lines,
                    board: "test",
                }
                .render()
                .unwrap();
                assert_eq!(
                    html.trim(),
                    if retains_wide {
                        "C ＜script＞"
                    } else {
                        "C &#60;script&#62;"
                    }
                );
                assert!(!html.contains("<script>"));
            }
        }
    }
}

#[derive(Template)]
#[template(path = "error.html")]
pub struct Message<'a> {
    pub title: &'a str,
    pub message: &'a str,
}
