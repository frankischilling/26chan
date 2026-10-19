use super::*;
use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use axum::{
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::Request,
    middleware,
};
use board_domain::{challenge_secrets::VerificationKey, verification_mail::MailKey};
use board_store::verification::VerificationStore;
use std::{net::SocketAddr, sync::Arc};
use tower::ServiceExt;

fn request(path: &str, cookie: &str, pairs: Option<&[(&str, &str)]>) -> Request<Body> {
    let mut builder = if pairs.is_some() {
        Request::post(path)
    } else {
        Request::get(path)
    };
    builder = builder.extension(ConnectInfo(
        "192.0.2.29:12345".parse::<SocketAddr>().unwrap(),
    ));
    if !cookie.is_empty() {
        builder = builder.header("cookie", cookie);
    }
    if let Some(pairs) = pairs {
        builder
            .header("origin", "http://127.0.0.1:3000")
            .header("sec-fetch-site", "same-origin")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(
                url::form_urlencoded::Serializer::new(String::new())
                    .extend_pairs(pairs.iter().copied())
                    .finish(),
            ))
            .unwrap()
    } else {
        builder.body(Body::empty()).unwrap()
    }
}
fn cookie(response: &Response) -> String {
    response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|value| value.to_str().unwrap())
        .filter(|value| !value.contains("Max-Age=0;"))
        .map(|value| value.split(';').next().unwrap())
        .collect::<Vec<_>>()
        .join("; ")
}
async fn body(response: Response) -> String {
    String::from_utf8(
        to_bytes(response.into_body(), 262144)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}
fn nonce(cookie: &str) -> String {
    let mut headers = HeaderMap::new();
    headers.insert(header::COOKIE, cookie.parse().unwrap());
    PassSecret::read(&headers, false).unwrap().unwrap().nonce()
}
#[tokio::test]
async fn public_pass_forms_use_private_issuer_and_ignore_forged_presentation_cookie() {
    let owner = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = sqlx::PgPool::connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let issuer =
        VerificationStore::connect(&std::env::var("TEST_VERIFICATION_DATABASE_URL").unwrap())
            .await
            .unwrap();
    let material = PassSessionToken::generate().unwrap().encode();
    let key = VerificationKey::parse(&material[3..]).unwrap();
    let identifier = material[3..13].to_ascii_uppercase();
    let token_hash = key.pass_identifier(&identifier).unwrap();
    let hash = Argon2::default()
        .hash_password(
            b"owned-public-pin",
            &SaltString::encode_b64(b"owned-public-pass").unwrap(),
        )
        .unwrap()
        .to_string();
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let id:i64=sqlx::query_scalar("INSERT INTO verification_private.pass_entitlements(token_hash,pin_hash,status,purchased_at,expires_at) VALUES($1,$2,0,'2020-01-02T00:00:00Z',clock_timestamp()+interval '1 year') RETURNING id")
  .bind(token_hash.as_slice()).bind(hash).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = listener.local_addr().unwrap();
    let service = board_verification::router(
        board_verification::AppState::new(
            issuer.clone(),
            VerificationKey::parse(&material[3..]).unwrap(),
            MailKey::parse(&material[3..]).unwrap(),
            &"11".repeat(32),
        )
        .unwrap(),
    );
    let serving = tokio::spawn(async move {
        axum::serve(listener, service).await.unwrap();
    });
    let state = AppState {
        asn_database: None,
        browser_hint_key: None,
        posting_browser_compat_key: None,
        pool: public.clone(),
        origin: "http://127.0.0.1:3000".into(),
        production: false,
        limits: Arc::new(crate::security::Limits::new(
            board_config::PublicRequestLimits::default(),
        )),
        media: None,
        verification: Some(VerificationClient::new(endpoint, "11".repeat(32)).unwrap()),
        proxy_uid: None,
        poster_id_key: None,
        tripcode_key: None,
        country_database: None,
    };
    let app = routes()
        .merge(crate::ui_assets::routes())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            crate::security::protect,
        ))
        .with_state(state);
    let work_owner = owner.clone();
    let outcome = tokio::spawn(async move {
        let response = app
            .clone()
            .oneshot(request("/auth", "pass_enabled=1", None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            "private, no-store"
        );
        let form_cookie = cookie(&response);
        let csrf = nonce(&form_cookie);
        let html = body(response).await;
        assert!(html.contains("name=\"pin\""));
        assert!(!html.contains("You are authenticated."));
        let fields = [
            ("id", identifier.as_str()),
            ("pin", "owned-public-pin"),
            ("csrf", csrf.as_str()),
        ];
        let mut bad = request("/auth", &form_cookie, Some(&fields));
        bad.headers_mut().remove("origin");
        assert_eq!(
            app.clone().oneshot(bad).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
        let mut bad = request("/auth", &form_cookie, Some(&fields));
        bad.headers_mut()
            .insert("origin", "https://outside.invalid".parse().unwrap());
        assert_eq!(
            app.clone().oneshot(bad).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            app.clone()
                .oneshot(request(
                    "/auth",
                    &form_cookie,
                    Some(&[
                        ("id", &identifier),
                        ("pin", "owned-public-pin"),
                        ("csrf", "wrong")
                    ])
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        let response = app
            .clone()
            .oneshot(request("/auth", &form_cookie, Some(&fields)))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let session_cookie = cookie(&response);
        assert!(session_cookie.contains("board-pass=p1_"));
        assert!(
            response
                .headers()
                .get_all(header::SET_COOKIE)
                .iter()
                .any(|v| v.to_str().unwrap().contains("HttpOnly; SameSite=Lax"))
        );
        let html = body(response).await;
        assert!(html.contains("Your device is now authorized."));
        assert!(!html.contains("p1_"));
        assert!(!html.contains("owned-public-pin"));
        let response = app
            .clone()
            .oneshot(request("/auth", &session_cookie, None))
            .await
            .unwrap();
        let logout_cookie = format!("{session_cookie}; {}", cookie(&response));
        let csrf = nonce(&logout_cookie);
        assert!(body(response).await.contains("You are authenticated."));
        let response = app
            .clone()
            .oneshot(request(
                "/auth",
                &logout_cookie,
                Some(&[("logout", "1"), ("csrf", &csrf), ("xhr", "1")]),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let value: serde_json::Value = serde_json::from_str(&body(response).await).unwrap();
        assert_eq!(value["status"], 4);
        let response = app
            .clone()
            .oneshot(request("/auth", &session_cookie, None))
            .await
            .unwrap();
        assert!(body(response).await.contains("name=\"pin\""));
        let response = app
            .clone()
            .oneshot(request("/auth", &session_cookie, None))
            .await
            .unwrap();
        let fresh_cookie = format!("{session_cookie}; {}", cookie(&response));
        let fresh_csrf = nonce(&fresh_cookie);
        let mut tx = work_owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_anonymous_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("UPDATE verification_private.pass_entitlements SET status=4 WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let response = app
            .clone()
            .oneshot(request(
                "/auth",
                &fresh_cookie,
                Some(&[
                    ("id", &identifier),
                    ("pin", "owned-public-pin"),
                    ("csrf", &fresh_csrf),
                ]),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response
                .headers()
                .get_all(header::SET_COOKIE)
                .iter()
                .any(|value| value
                    .to_str()
                    .unwrap()
                    .starts_with("board-pass=; Path=/; Max-Age=0;"))
        );
        assert!(body(response).await.contains("revoked due to spamming"));
        let css = app
            .clone()
            .oneshot(request("/static/pass-auth.v1.css", "", None))
            .await
            .unwrap();
        assert_eq!(css.status(), StatusCode::OK);
    })
    .await;
    serving.abort();
    let _ = serving.await;
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM verification_private.pass_entitlements WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    issuer.close().await;
    public.close().await;
    owner.close().await;
    outcome.unwrap();
}
