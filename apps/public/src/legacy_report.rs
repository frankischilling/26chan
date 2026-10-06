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

// Only this renderer can opt a response into the report popup script policy.
#[derive(Clone, Copy)]
pub(crate) struct ReportShell(());

#[derive(Template)]
#[template(path = "report_form.html")]
struct ReportPage<'a> {
    board: &'a str,
    no: i64,
    thread: i64,
}

#[derive(Template)]
#[template(path = "report_result.html")]
struct ReportResult<'a> {
    board: &'a str,
    no: i64,
    success: bool,
    message: &'a str,
}

pub(crate) fn safe_board(board: &str) -> bool {
    !board.is_empty()
        && board.len() <= 10
        && board
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

fn shell(mut response: Response, status: StatusCode) -> Response {
    *response.status_mut() = status;
    response.extensions_mut().insert(ReportShell(()));
    response.headers_mut().insert(
        "cache-control",
        axum::http::HeaderValue::from_static("private, no-store"),
    );
    response
}

pub(crate) fn success(state: &AppState, board: &str, no: i64) -> Result<Response, AppError> {
    crate::output::html(
        state,
        &ReportResult {
            board,
            no,
            success: true,
            message: "Your report was saved.",
        },
    )
    .map(|response| shell(response, StatusCode::OK))
}

pub(crate) fn error(state: &AppState, board: &str, no: Option<i64>, error: AppError) -> Response {
    use axum::response::IntoResponse;
    if !safe_board(board) {
        return error.into_response();
    }
    match crate::output::html(
        state,
        &ReportResult {
            board,
            no: no.filter(|no| *no > 0).unwrap_or(0),
            success: false,
            message: error.1,
        },
    ) {
        Ok(response) => shell(response, error.0),
        Err(error) => error.into_response(),
    }
}

pub(crate) async fn get(
    State(state): State<AppState>,
    Path(board): Path<String>,
    query: Result<Query<ReportQuery>, QueryRejection>,
) -> Result<Response, AppError> {
    let invalid = || AppError(StatusCode::BAD_REQUEST, "Invalid reporting request.");
    if !safe_board(&board) {
        return Err(invalid());
    }
    let Query(query) = match query {
        Ok(query) => query,
        Err(_) => return Ok(error(&state, &board, None, invalid())),
    };
    let no = match query.no.parse::<i64>() {
        Ok(no) if no > 0 && no.to_string() == query.no => no,
        _ => return Ok(error(&state, &board, None, invalid())),
    };
    let target = match board_store::report_target(&state.pool, &board, no).await {
        Ok(target) => target,
        Err(cause) => return Ok(error(&state, &board, Some(no), cause.into())),
    };
    crate::output::html(
        &state,
        &ReportPage {
            board: &target.board,
            no: target.post_id,
            thread: target.thread_id,
        },
    )
    .map(|response| shell(response, StatusCode::OK))
}

#[cfg(test)]
mod tests {
    use askama::Template;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn assert_report_policy(response: &axum::response::Response) {
        assert_eq!(response.headers()["cache-control"], "private, no-store");
        assert_eq!(response.headers()["x-frame-options"], "DENY");
        let csp = response.headers()["content-security-policy"]
            .to_str()
            .unwrap();
        let directive = |name: &str| {
            csp.split(';')
                .map(str::trim)
                .find(|part| part.starts_with(&format!("{name} ")))
                .unwrap()
        };
        assert_eq!(
            directive("script-src"),
            "script-src http://127.0.0.1:3000/static/report-popup.v1.js"
        );
        for name in [
            "script-src-attr",
            "connect-src",
            "worker-src",
            "frame-src",
            "frame-ancestors",
        ] {
            assert_eq!(directive(name), format!("{name} 'none'"));
        }
        assert_eq!(directive("form-action"), "form-action 'self'");
    }

    #[test]
    fn report_templates_escape_values_and_separate_success_from_error() {
        let form = super::ReportPage {
            board: "x\"<",
            no: 17,
            thread: 12,
        }
        .render()
        .unwrap();
        assert!(!form.contains("x\"<"));
        assert!(form.contains("name=\"no\" value=\"17\""));
        assert!(form.contains("name=\"reason\""));
        assert!(form.contains("/thread/12#p17"));
        assert!(!form.contains("Report received"));
        for success in [false, true] {
            let html = super::ReportResult {
                board: "test",
                no: 17,
                success,
                message: "<script>alert(1)</script>",
            }
            .render()
            .unwrap();
            assert!(!html.contains("<script>alert(1)</script>"));
            assert_eq!(html.contains("Report received"), success);
            assert_eq!(html.contains("data-result=\"success\""), success);
            assert!(html.contains("id=\"report-popup-return\" href=\"/test/\""));
            assert!(!html.contains("javascript:"));
        }
    }

    #[tokio::test]
    async fn malformed_report_forms_are_local_errors_without_storage_or_success() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
            .unwrap();
        let app = crate::router(pool, "http://127.0.0.1:3000".into(), false);
        for body in [
            "",
            "no=17",
            "no=nope&reason=test",
            "no=17&reason=a&reason=b",
            "no=17&reason=a&extra=b",
            "no=0&reason=test",
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::post("/test/report")
                        .header("origin", "http://127.0.0.1:3000")
                        .header("content-type", "application/x-www-form-urlencoded")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert!(response.status().is_client_error(), "{body}");
            assert_report_policy(&response);
            assert!(response.headers().get("set-cookie").is_none());
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            let html = std::str::from_utf8(&bytes).unwrap();
            assert!(html.contains("data-result=\"error\""));
            assert!(!html.contains("Report received"));
            assert!(!html.contains("data-result=\"success\""));
        }
    }

    #[tokio::test]
    async fn report_script_is_release_owned_and_generic_imgboard_errors_get_no_script() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
            .unwrap();
        let app = crate::router(pool, "http://127.0.0.1:3000".into(), false);
        let script = app
            .clone()
            .oneshot(
                Request::get("/static/report-popup.v1.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(script.status(), StatusCode::OK);
        assert_eq!(
            script.headers()["content-type"],
            "text/javascript; charset=utf-8"
        );
        let response = app
            .oneshot(
                Request::post("/test/imgboard.php")
                    .header("origin", "https://other.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let csp = response.headers()["content-security-policy"]
            .to_str()
            .unwrap();
        assert!(csp.contains("script-src 'none'"));
        assert!(!csp.contains("report-popup.v1.js"));
    }

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
            assert_report_policy(&response);
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            let html = std::str::from_utf8(&bytes).unwrap();
            assert!(html.contains("data-result=\"error\""));
            assert!(!html.contains("Report received"));
        }
    }
}
