//! Page identity is plain text. Askama owns the final HTML escaping.
use super::PostView;
use board_store::Board;

pub(super) enum Page<'a> {
    Index(i64),
    Catalog,
    Archive,
    Thread { id: i64, op: Option<&'a PostView> },
}

fn prefix(board: &Board) -> String {
    if board.slug == "s4s" {
        format!("[{}]", board.slug)
    } else {
        format!("/{}/", board.slug)
    }
}

pub(super) fn heading(board: &Board) -> String {
    format!("{} - {}", prefix(board), board.title)
}

pub(super) fn browser_title(board: &Board, page: Page<'_>) -> String {
    let title = match page {
        Page::Index(page) if page > 1 => format!("{} - Page {page}", heading(board)),
        Page::Index(_) => heading(board),
        Page::Catalog => format!("{} - Catalog", heading(board)),
        Page::Archive => format!("{} - Archive", heading(board)),
        Page::Thread { id, op } => {
            // The source deliberately uses its board heading for private boards.
            let context = if board.staff_only {
                heading(board)
            } else {
                let context = op.map_or_else(String::new, |op| {
                    let subject = board_domain::source_html_entities(&op.post.subject);
                    let comment = if subject.is_empty() || board.upload_board {
                        crate::semantic_thread::stored_comment(op)
                    } else {
                        String::new()
                    };
                    board_domain::page_title::context(
                        &subject,
                        &comment,
                        board.upload_board,
                        board.comment_sjis_spacing,
                    )
                });
                let context = if context.is_empty() {
                    format!("No.{id}")
                } else {
                    context
                };
                format!("{} - {context}", prefix(board))
            };
            format!("{context} - {}", board.title)
        }
    };
    format!("{title} - 4chan")
}
