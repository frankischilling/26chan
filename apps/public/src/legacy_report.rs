use crate::{AppState, handlers::AppError};
use askama::Template;
use axum::{
    extract::{OriginalUri, Path, Query, State, rejection::QueryRejection},
    http::{HeaderMap, StatusCode},
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

pub(crate) fn positive_id(value: &str) -> Option<i64> {
    value
        .parse::<i64>()
        .ok()
        .filter(|id| *id > 0 && id.to_string() == value)
}

#[derive(Template)]
#[template(path = "report_categorical_form.html")]
struct CategoricalReportPage<'a> {
    board: &'a str,
    no: i64,
    thread: i64,
    revision: i64,
    rules: Vec<&'a board_store::report_categories::CategoryChoice>,
    illegal: Option<&'a board_store::report_categories::CategoryChoice>,
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
    axum::Extension(peer): axum::Extension<crate::security::RequestPeer>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
    query: Result<Query<ReportQuery>, QueryRejection>,
) -> Result<Response, AppError> {
    let invalid = || AppError(StatusCode::BAD_REQUEST, "Invalid reporting request.");
    if !safe_board(&board) {
        return Err(invalid());
    }
    let reporting = url::form_urlencoded::parse(uri.query().unwrap_or("").as_bytes())
        .any(|(name, value)| name == "mode" && value == "report");
    if !reporting {
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
    // Target policy has precedence over identity availability and quota errors.
    // This advisory read neither reserves capacity nor registers a session.
    let identity = match crate::handlers::report_rate_identity(&state, peer) {
        Ok(identity) => identity,
        Err(cause) => return Ok(error(&state, &board, Some(no), cause)),
    };
    let capability = match crate::anonymous_session::Session::existing(&state, &headers).await {
        Ok(capability) => capability,
        Err(cause) => return Ok(error(&state, &board, Some(no), cause)),
    };
    let token = capability.map(|capability| capability.storage_hash());
    if let Err(cause) = board_store::report_admission::check_with_session(
        &state.pool,
        &board,
        no,
        &identity,
        token.as_ref(),
        chrono::Utc::now().timestamp(),
    )
    .await
    {
        return Ok(error(&state, &board, Some(no), cause.into()));
    }
    let categories =
        match board_store::report_categories::category_form(&state.pool, &board, no).await {
            Ok(categories) => categories,
            Err(cause) => return Ok(error(&state, &board, Some(no), cause.into())),
        };
    if let Some(revision) = categories.revision {
        use board_store::report_categories::CategoryKind;
        return crate::output::html(
            &state,
            &CategoricalReportPage {
                board: &target.board,
                no: target.post_id,
                thread: target.thread_id,
                revision,
                rules: categories
                    .categories
                    .iter()
                    .filter(|category| category.kind == CategoryKind::Rule)
                    .collect(),
                illegal: categories
                    .categories
                    .iter()
                    .find(|category| category.kind == CategoryKind::Illegal),
            },
        )
        .map(|response| shell(response, StatusCode::OK));
    }
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
    fn categorical_template_escapes_and_preserves_all_options_without_javascript() {
        use board_store::report_categories::{CategoryChoice, CategoryKind};
        let choices = [
            CategoryChoice {
                id: 17,
                title: "<script>\"&".into(),
                kind: CategoryKind::Rule,
            },
            CategoryChoice {
                id: 18,
                title: String::new(),
                kind: CategoryKind::Rule,
            },
            CategoryChoice {
                id: 19,
                title: "x".repeat(4096),
                kind: CategoryKind::Rule,
            },
            CategoryChoice {
                id: 31,
                title: "Illegal".into(),
                kind: CategoryKind::Illegal,
            },
        ];
        let html = super::CategoricalReportPage {
            board: "test",
            no: 20,
            thread: 10,
            revision: 2,
            rules: choices[..3].iter().collect(),
            illegal: Some(&choices[3]),
        }
        .render()
        .unwrap();
        assert!(!html.contains("<script>\"&"));
        assert!(html.contains("Category 18</option>"));
        assert!(html.contains(&"x".repeat(4096)));
        assert_eq!(html.matches("<option ").count(), 4);
        assert!(html.contains("name=\"cat_id\"><option value=\"\"></option><option value=\"17\">"));
        assert!(!html.contains(" required"));
        assert!(html.contains("/test/imgboard.php?mode=report&amp;no=20"));
        assert!(html.contains("name=\"revision\" value=\"2\""));
        assert!(html.contains("name=\"cat\" value=\"\" checked"));
        assert!(html.contains("name=\"cat\" value=\"31\""));
        assert!(!html.contains(" disabled"));
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
    async fn report_missing_key_or_trusted_peer_fails_before_storage_even_in_development() {
        for key_present in [false, true] {
            let pool = sqlx::postgres::PgPoolOptions::new()
                .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
                .unwrap();
            pool.close().await;
            let (app, _) = crate::routers_with_options(
                pool,
                crate::PublicRouterOptions {
                    origin: "http://127.0.0.1:3000".into(),
                    production: false,
                    media: None,
                    limits: board_config::PublicRequestLimits::default(),
                    proxy_uid: None,
                    poster_id_key: key_present.then(|| {
                        std::sync::Arc::new(
                            board_domain::poster_id::PosterIdKey::parse(&"1".repeat(64)).unwrap(),
                        )
                    }),
                    tripcode_key: None,
                    country_database: None,
                },
            );
            let response = app
                .oneshot(
                    Request::post("/test/report")
                        .header("origin", "http://127.0.0.1:3000")
                        .header("content-type", "application/x-www-form-urlencoded")
                        .header("x-forwarded-for", "192.0.2.10")
                        .extension(crate::security::RequestPeer(Some(
                            "192.0.2.10".parse().unwrap(),
                        )))
                        .body(Body::from("no=17&reason=test"))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
            assert_report_policy(&response);
            assert!(response.headers().get("set-cookie").is_none());
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            let html = std::str::from_utf8(&bytes).unwrap();
            assert!(html.contains("Public reporting is unavailable."));
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
            let reporting = query.contains("mode=report");
            if reporting {
                assert_report_policy(&response);
            } else {
                assert!(
                    response.headers()["content-security-policy"]
                        .to_str()
                        .unwrap()
                        .contains("script-src 'none'")
                );
            }
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            let html = std::str::from_utf8(&bytes).unwrap();
            assert_eq!(html.contains("data-result=\"error\""), reporting);
            assert!(!html.contains("Report received"));
        }
    }
}
