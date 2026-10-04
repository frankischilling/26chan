//! Source flag choices on synthetic posts rendered by the production template.
use askama::Template;
use axum::{Router, response::Html, routing::get};

pub fn routes() -> Router {
    let mut routes = Router::new();
    for kind in ["pol", "mlp", "lgbt", "test"] {
        routes = routes.route(
            &format!("/flags/{kind}"),
            get(move || async move { Html(page(kind)) }),
        );
    }
    routes
}
fn page(kind: &str) -> String {
    let mut page = super::fixture_page(false, false, false, false);
    let flags = board_domain::board_flags::flags(kind);
    let id = 1_002_000;
    page.board.slug = format!("flag{kind}");
    page.board.title = format!("Owned {kind} flags");
    page.board.country_flags = false;
    page.board.board_flag_type = kind.into();
    page.board.board_flags = flags.iter().map(|flag| flag.code.to_owned()).collect();
    let mut view = page.threads.remove(0);
    let base = view.posts[0].post.clone();
    view.thread.id = id;
    view.thread.board = page.board.slug.clone();
    view.thread.reply_count = flags.len() as i32 - 1;
    view.latest_reply_id = Some(id + flags.len() as i64 - 1);
    view.image_replies = 0;
    view.posts = flags
        .iter()
        .enumerate()
        .map(|(index, flag)| {
            let mut post = base.clone();
            post.id = id + index as i64;
            post.thread_id = id;
            post.board = page.board.slug.clone();
            post.country = None;
            post.country_name = None;
            post.capcode = None;
            post.board_flag_type = kind.into();
            post.board_flag = Some(flag.code.into());
            post.flag_name = Some(flag.display.into());
            post.comment = format!("Owned source flag {}", flag.code);
            super::views::PostView::new(post)
        })
        .collect();
    page.threads = vec![view];
    page.parent = id;
    page.render().unwrap()
}
