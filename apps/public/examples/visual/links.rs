//! Controlled quote targets rendered by the actual page and comment templates.
use super::views::PostView;
use askama::Template;
use board_domain::Token;

pub fn page() -> String {
    let mut page = super::fixture_page(false, false, false, false);
    page.board.slug = "g".into();
    page.board.title = "Owned source links".into();
    page.navigation_boards = vec![page.board.clone()];
    page.parent = 100;
    let thread = &mut page.threads[0];
    thread.thread.id = 100;
    thread.thread.closed = true;
    thread.thread.board = "g".into();
    for (index, view) in thread.posts.iter_mut().enumerate() {
        let mut post = view.post.clone();
        post.id = if index == 0 { 100 } else { 123 };
        post.thread_id = 100;
        post.subject = if index == 0 { "Owned source links" } else { "" }.into();
        post.board = "g".into();
        post.comment_format = 104;
        post.comment = if index == 0 { "Owned source link comparison" } else {
            ">>123 >>000123 >>>/g/+123 >>>/g/123e0\n>>999 >>>/g/-123 >>>/g/1e309 >>>/unknown/+123\nhttps://example.test/a?a=1&b=2 https://www.4chan.org/faq\nentityneedle & <tag> \"quoted\" 'apostrophe' &amp; &#39;"
        }.into();
        *view = PostView::new(post);
        for token in view.lines.iter_mut().flat_map(|line| &mut line.tokens) {
            if let Token::PostQuote(quote) = token {
                let target = (quote.id() == 123).then_some(100);
                quote.resolve("g", Some(100), target);
            }
        }
    }
    page.render()
        .expect("production source-link visual fixture")
}
