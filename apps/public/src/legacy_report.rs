use crate::{AppState, handlers::AppError};
use askama::Template;
use axum::{
    extract::{Path, Query, State, rejection::QueryRejection},
    http::StatusCode,
    response::Response,
};
use serde::Deserialize;

#[derive(Deserialize)]
pub(crate) enum Mode {
    #[serde(rename = "report")]
    Report,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReportQuery {
    #[serde(rename = "mode")]
    _mode: Mode,
    no: String,
}

#[derive(Template)]
#[template(path = "report_form.html")]
struct ReportPage<'a> {
    board: &'a str,
    no: i64,
    thread: i64,
}

pub(crate) async fn get(
    State(state): State<AppState>,
    Path(board): Path<String>,
    query: Result<Query<ReportQuery>, QueryRejection>,
) -> Result<Response, AppError> {
    let invalid = || AppError(StatusCode::BAD_REQUEST, "Invalid reporting request.");
    let Query(query) = query.map_err(|_| invalid())?;
    let no = query.no.parse::<i64>().map_err(|_| invalid())?;
    if no <= 0 || no.to_string() != query.no {
        return Err(invalid());
    }
    let post = board_store::find_post(&state.pool, &board, no).await?;
    crate::output::html(
        &state,
        &ReportPage {
            board: &board,
            no,
            thread: post.thread_id,
        },
    )
}

#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    #[tokio::test]
    async fn malformed_queries_do_not_reach_an_unavailable_store() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
            .unwrap();
        let app = crate::router(pool, "http://127.0.0.1:3000".into(), false);
        for query in [
            "",
            "mode=report",
            "no=17",
            "mode=usrdel&no=17",
            "mode=report&mode=report&no=17",
            "mode=report&no=17&no=18",
            "mode=report&no=17&admin=1",
            "mode=report&no=0",
            "mode=report&no=-1",
            "mode=report&no=%2B17",
            "mode=report&no=017",
            "mode=report&no=1.0",
            "mode=report&no=9223372036854775808",
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::get(format!("/test/imgboard.php?{query}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{query}");
        }
    }
}
