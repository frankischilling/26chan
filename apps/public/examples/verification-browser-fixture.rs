//! Read-only synthetic sign-in pages using the actual template and CSP.
#![forbid(unsafe_code)]
use axum::{Router, routing::get};
const ORIGIN: &str = "http://127.0.0.1:3000";
#[tokio::main]
async fn main() {
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
    let app = app.fallback_service(board_public::router(pool, ORIGIN.into(), false));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .expect("owned fixture listener");
    axum::serve(listener, app).await.expect("fixture server");
}
