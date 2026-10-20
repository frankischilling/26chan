//! Read-only synthetic sign-in pages using the actual template and CSP.
#![forbid(unsafe_code)]
use axum::{Router, routing::get};
const ORIGIN: &str = "http://127.0.0.1:3000";
async fn app() -> Router {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://unused@127.0.0.1:1/unused")
        .expect("closed fixture pool");
    pool.close().await;
    let mut app = Router::new().route("/readyz", get(|| async { "ready" }));
    for (path, mode) in [
        ("/signin", "request"),
        ("/fixture/verify", "verify"),
        ("/fixture/preview", "preview"),
        ("/fixture/logout", "logout"),
        ("/fixture/message", "message"),
    ] {
        app = app.route(
            path,
            get(move || async move {
                board_public::verification_fixture_response(mode, ORIGIN)
                    .expect("production verification response")
            }),
        );
    }
    for mode in ["thread", "reply", "report", "classic", "noscript", "qr"] {
        app = app.route(
            &format!("/fixture/captcha/{mode}"),
            get(move || async move {
                board_public::posting_captcha_fixture_response(mode, ORIGIN)
                    .expect("production CAPTCHA controls")
            }),
        );
    }
    for mode in [
        "login",
        "success",
        "authenticated",
        "logout",
        "error",
        "escaped",
        "recovery-request",
        "recovery-reset",
        "recovery-complete",
        "recovery-escaped",
        "checkout-purchase",
        "checkout-renew",
        "checkout-pending",
        "checkout-paid",
        "checkout-declined",
        "checkout-cancelled",
        "checkout-refunded",
        "checkout-disputed",
        "checkout-expired",
        "checkout-escaped",
    ] {
        app = app.route(
            &format!("/fixture/pass/{mode}"),
            get(move || async move {
                board_public::pass_fixture_response(mode, ORIGIN)
                    .expect("production Pass form response")
            }),
        );
    }
    app.fallback_service(board_public::router(pool, ORIGIN.into(), false))
}
#[tokio::main]
async fn main() {
    let app = app().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .expect("owned fixture listener");
    axum::serve(listener, app).await.expect("fixture server");
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;
    #[tokio::test]
    async fn purchase_fixture_is_routed_to_the_real_form() {
        let response = app()
            .await
            .oneshot(
                Request::get("/fixture/pass/checkout-purchase")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), 65536).await.unwrap();
        let html = std::str::from_utf8(&bytes).unwrap();
        assert!(html.contains("action=\"/pass/purchase\""));
        assert!(html.contains("name=\"gift_email\""));
    }
}
