//! Local information and news pages. Operator text is always escaped.
use crate::{AppState, handlers::AppError};
use askama::Template;
use axum::{
    Router,
    extract::{RawQuery, State},
    http::StatusCode,
    response::Response,
    routing::get,
};
use board_store::site_content::{self, NewsItem, SitePage};

#[derive(Template)]
#[template(path = "site_info.html")]
struct Info {
    title: &'static str,
    key: &'static str,
    page: Option<SitePage>,
    rules: Vec<Rule>,
}
impl Info {
    fn heading(&self) -> &str {
        self.page
            .as_ref()
            .map_or(self.title, |page| page.title.as_str())
    }
}
struct Rule {
    slug: String,
    title: String,
    body: String,
}
#[derive(Template)]
#[template(path = "site_news.html")]
struct News {
    items: Vec<NewsItem>,
    next: Option<i64>,
}

pub(crate) fn routes() -> Router<AppState> {
    let mut router = Router::new()
        .route("/news", get(news))
        .route("/stats", get(statistics))
        .merge(crate::site_submissions::routes());
    for (path, key, title) in [
        ("/about", "about", "About"),
        ("/faq", "faq", "Frequently Asked Questions"),
        ("/rules", "rules", "Rules"),
        ("/contact", "contact", "Contact"),
        ("/legal", "legal", "Legal information"),
        ("/press", "press", "Press"),
        ("/advertise", "advertise", "Advertising"),
        ("/support", "support", "Support"),
        ("/japanese", "japanese", "日本語"),
        ("/blog", "blog", "Blog"),
    ] {
        router = router.route(
            path,
            get(move |State(state): State<AppState>| async move { info(state, key, title).await }),
        );
    }
    router
}
pub(crate) async fn info(
    state: AppState,
    key: &'static str,
    title: &'static str,
) -> Result<Response, AppError> {
    let page = site_content::site_page(&state.pool, key).await?;
    let mut rules = Vec::new();
    if key == "rules" {
        let pages = site_content::board_rules(&state.pool).await?;
        for board in board_store::boards(&state.pool)
            .await?
            .into_iter()
            .filter(|b| !b.staff_only)
        {
            let body = pages
                .iter()
                .find(|p| p.board.as_deref() == Some(board.slug.as_str()))
                .map_or_else(
                    || "No additional board-specific rules have been published.".into(),
                    |p| p.body.clone(),
                );
            rules.push(Rule {
                slug: board.slug,
                title: board.title,
                body,
            });
        }
    }
    crate::output::html(
        &state,
        &Info {
            title,
            key,
            page,
            rules,
        },
    )
}
#[derive(Template)]
#[template(path = "site_statistics.html")]
struct Statistics {
    rows: Vec<board_store::homepage::BoardStatistics>,
    history: bool,
}
fn statistics_query(query: Option<&str>) -> Result<Option<&str>, AppError> {
    match query.filter(|q| !q.is_empty()) {
        None => Ok(None),
        Some(value) => value
            .strip_prefix("board=")
            .filter(|board| board_domain::BoardSlug::parse(board).is_ok())
            .map(Some)
            .ok_or(AppError(
                StatusCode::BAD_REQUEST,
                "Invalid statistics page.",
            )),
    }
}
async fn statistics(
    State(state): State<AppState>,
    RawQuery(query): RawQuery,
) -> Result<Response, AppError> {
    let board = statistics_query(query.as_deref())?;
    let rows = board_store::homepage::board_statistics(&state.pool, board).await?;
    if board.is_some() && rows.is_empty() {
        return Err(AppError(
            StatusCode::NOT_FOUND,
            "Board statistics not found.",
        ));
    }
    crate::output::html(
        &state,
        &Statistics {
            rows,
            history: board.is_some(),
        },
    )
}
#[derive(Debug, PartialEq, Eq)]
enum NewsQuery {
    Latest,
    Before(i64),
    Item(i64),
}
fn news_query(query: Option<&str>) -> Result<NewsQuery, AppError> {
    let invalid = || AppError(StatusCode::BAD_REQUEST, "Invalid news page.");
    let Some(query) = query.filter(|q| !q.is_empty()) else {
        return Ok(NewsQuery::Latest);
    };
    if query == "all" {
        return Ok(NewsQuery::Latest);
    }
    if query.len() > 12 {
        return Err(invalid());
    }
    let (key, value) = query.split_once('=').ok_or_else(invalid)?;
    if value.is_empty() || value.starts_with('0') || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid());
    }
    let id = value
        .parse::<i64>()
        .ok()
        .filter(|id| (1..=10000).contains(id))
        .ok_or_else(invalid)?;
    match key {
        "before" => Ok(NewsQuery::Before(id)),
        "item" => Ok(NewsQuery::Item(id)),
        _ => Err(invalid()),
    }
}
async fn news(
    State(state): State<AppState>,
    RawQuery(query): RawQuery,
) -> Result<Response, AppError> {
    let (mut items, listing) = match news_query(query.as_deref())? {
        NewsQuery::Latest => (site_content::news_page(&state.pool, None, 26).await?, true),
        NewsQuery::Before(id) => (
            site_content::news_page(&state.pool, Some(id), 26).await?,
            true,
        ),
        NewsQuery::Item(id) => (
            vec![
                site_content::news_item(&state.pool, id)
                    .await?
                    .ok_or(AppError(StatusCode::NOT_FOUND, "News entry not found."))?,
            ],
            false,
        ),
    };
    let has_more = listing && items.len() > 25;
    items.truncate(25);
    let next = has_more.then(|| items.last().expect("full news page").id);
    crate::output::html(&state, &News { items, next })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn statistics_queries_are_single_canonical_board_names() {
        assert_eq!(statistics_query(None).unwrap(), None);
        assert_eq!(statistics_query(Some("board=g")).unwrap(), Some("g"));
        for query in [
            "board=",
            "board=G",
            "board=../g",
            "board=g&board=b",
            "other=g",
            "board=%67",
        ] {
            assert!(statistics_query(Some(query)).is_err());
        }
    }
    #[test]
    fn news_queries_have_one_bounded_meaning() {
        assert_eq!(news_query(None).unwrap(), NewsQuery::Latest);
        assert_eq!(news_query(Some("all")).unwrap(), NewsQuery::Latest);
        assert_eq!(
            news_query(Some("before=10000")).unwrap(),
            NewsQuery::Before(10000)
        );
        assert_eq!(news_query(Some("item=1")).unwrap(), NewsQuery::Item(1));
        for query in [
            "before=0",
            "before=01",
            "before=10001",
            "item=-1",
            "item=%31",
            "item=1&before=2",
            "all=1",
            "x=1",
            "item=999999999999999999",
        ] {
            assert!(news_query(Some(query)).is_err(), "{query}");
        }
    }
    #[test]
    fn information_and_news_escape_operator_content_and_keep_local_navigation() {
        let page = SitePage {
            key: "faq".into(),
            board: None,
            title: "Owned <title>".into(),
            body: "owned <script>& text".into(),
            revision: 2,
            updated_at: chrono::Utc::now(),
        };
        let html = Info {
            title: "FAQ",
            key: "faq",
            page: Some(page),
            rules: vec![],
        }
        .render()
        .unwrap();
        assert!(html.contains("Owned &#60;title&#62;"));
        assert!(html.contains("id=\"what4chan\""));
        assert!(!html.contains("<script>"));
        assert!(html.contains("Revision 2"));
        let html = Info {
            title: "FAQ",
            key: "faq",
            page: None,
            rules: vec![],
        }
        .render()
        .unwrap();
        for local in ["/rules", "/news", "/contact", "/blotter"] {
            assert!(html.contains(local));
        }
        let html = News {
            items: vec![NewsItem {
                id: 1,
                published_at: chrono::Utc::now(),
                title: "Owned <news>".into(),
                author: "Editor & name".into(),
                body: "<script>text</script>".into(),
            }],
            next: Some(1),
        }
        .render()
        .unwrap();
        assert!(!html.contains("<script>"));
        assert!(html.contains("/news?item=1#1"));
        assert!(html.contains("/news?before=1"));
        assert!(
            News {
                items: vec![],
                next: None
            }
            .render()
            .unwrap()
            .contains("No news has been published.")
        );
    }
}
