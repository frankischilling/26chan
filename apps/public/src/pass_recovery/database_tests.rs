use super::*;
use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use axum::{
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::Request,
    middleware,
};
use board_domain::{
    challenge_secrets::{PassSessionToken, VerificationKey},
    pass_mail::{Contact, Payload, Recipient},
    verification_mail::MailKey,
};
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
    RecoverySecret::read(&headers, false)
        .unwrap()
        .unwrap()
        .nonce()
}
#[tokio::test]
#[ignore = "Run explicitly with isolated verifier and mailer test credentials."]
async fn public_recovery_requires_owner_mail_delivery_csrf_and_one_use_token() {
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
    let mail_key = MailKey::parse(&"22".repeat(32)).unwrap();
    let contact = Contact::new(
        Recipient::parse("owner+recovery@example.org").unwrap(),
        &identifier,
        "owned-renewal",
    )
    .unwrap();
    let recipient_hash = key.pass_recipient(contact.recipient()).unwrap();
    let encrypted = mail_key
        .seal_pass(&token_hash, &Payload::Contact(contact))
        .unwrap();
    sqlx::query("INSERT INTO verification_private.pass_contacts(entitlement_id,recipient_hash,encrypted_contact) VALUES($1,$2,$3)").bind(id).bind(recipient_hash.as_slice()).bind(encrypted).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = listener.local_addr().unwrap();
    let service = board_verification::router(
        board_verification::AppState::new(
            issuer.clone(),
            VerificationKey::parse(&material[3..]).unwrap(),
            MailKey::parse(&"22".repeat(32)).unwrap(),
            &"11".repeat(32),
        )
        .unwrap(),
    );
    let serving = tokio::spawn(async move {
        axum::serve(listener, service).await.unwrap();
    });
    let state = AppState {
        event_workflows: false,
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
        poster_id_key: Some(Arc::new(
            board_domain::poster_id::PosterIdKey::parse(&material[3..]).unwrap(),
        )),
        tripcode_key: None,
        country_database: None,
    };
    let client = state.verification.clone().unwrap();
    let app = routes()
        .merge(crate::pass_auth::routes())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            crate::security::protect,
        ))
        .with_state(state);

    let login = client
        .pass_login(&protocol::PassLogin {
            identifier: identifier.clone(),
            pin: "owned-public-pin".into(),
            peer: "192.0.2.29".parse().unwrap(),
            remember: false,
        })
        .await
        .unwrap();
    let protocol::PassLoginResult::Ready {
        token: old_session, ..
    } = login
    else {
        panic!("owned login failed")
    };
    let response = app
        .clone()
        .oneshot(request("/pass?reset", "", None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["referrer-policy"], "strict-origin");
    let form_cookie = cookie(&response);
    let csrf = nonce(&form_cookie);
    let bad = app
        .clone()
        .oneshot(request(
            "/pass",
            &form_cookie,
            Some(&[
                ("action", "request"),
                ("csrf", "wrong"),
                ("id", &identifier),
                ("email", "owner+recovery@example.org"),
            ]),
        ))
        .await
        .unwrap();
    assert_eq!(bad.status(), StatusCode::FORBIDDEN);
    let unknown = app
        .clone()
        .oneshot(request(
            "/pass",
            &form_cookie,
            Some(&[
                ("action", "request"),
                ("csrf", &csrf),
                ("id", &identifier),
                ("email", "unknown@example.org"),
            ]),
        ))
        .await
        .unwrap();
    let unknown_body = body(unknown).await;
    let known = app
        .clone()
        .oneshot(request(
            "/pass",
            &form_cookie,
            Some(&[
                ("action", "request"),
                ("csrf", &csrf),
                ("id", &identifier),
                ("email", "owner+recovery@example.org"),
            ]),
        ))
        .await
        .unwrap();
    assert_eq!(known.status(), StatusCode::OK);
    assert_eq!(
        body(known).await,
        unknown_body,
        "owner existence must not change the public response"
    );
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let (context,encrypted):(Vec<u8>,Vec<u8>)=sqlx::query_as("SELECT token_hash,encrypted_payload FROM verification_private.pass_recoveries WHERE entitlement_id=$1").bind(id).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let context: [u8; 32] = context.try_into().unwrap();
    let Payload::Recovery { recipient, token } = mail_key.open_pass(&context, &encrypted).unwrap()
    else {
        panic!("recovery payload expected")
    };
    assert_eq!(recipient.address(), "owner+recovery@example.org");
    let token = token.encode();
    let path = format!("/pass?reset={token}");
    let before = app.clone().oneshot(request(&path, "", None)).await.unwrap();
    assert!(body(before).await.contains("invalid or has expired"));
    let mailer = board_store::verification::mail::MailStore::connect(
        &std::env::var("TEST_VERIFICATION_MAIL_DATABASE_URL").unwrap(),
    )
    .await
    .unwrap();
    let lease = [33; 32];
    let jobs = mailer.claim_recovery(&lease, 1).await.unwrap();
    assert_eq!(jobs.len(), 1);
    assert!(
        mailer
            .finish_recovery(&jobs[0].request_id, &lease, true)
            .await
            .unwrap()
    );
    let ready = app.clone().oneshot(request(&path, "", None)).await.unwrap();
    assert_eq!(ready.status(), StatusCode::OK);
    assert_eq!(ready.headers()["referrer-policy"], "strict-origin");
    let reset_cookie = cookie(&ready);
    let reset_csrf = nonce(&reset_cookie);
    assert!(body(ready).await.contains("name=\"confirm\""));
    let reset_fields = [
        ("action", "complete"),
        ("csrf", reset_csrf.as_str()),
        ("token", token.as_str()),
        ("pin", "owned-new-pin"),
        ("confirm", "owned-new-pin"),
    ];
    let rejected = app
        .clone()
        .oneshot(request("/pass", "", Some(&reset_fields)))
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
    let (first, second) = tokio::join!(
        app.clone()
            .oneshot(request("/pass", &reset_cookie, Some(&reset_fields))),
        app.clone()
            .oneshot(request("/pass", &reset_cookie, Some(&reset_fields)))
    );
    let first = first.unwrap();
    let second = second.unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(second.status(), StatusCode::OK);
    let results = [body(first).await, body(second).await];
    assert_eq!(
        results
            .iter()
            .filter(|text| text.contains("Your PIN has been changed"))
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|text| text.contains("invalid or has expired"))
            .count(),
        1
    );
    let replay = app
        .clone()
        .oneshot(request("/pass", &reset_cookie, Some(&reset_fields)))
        .await
        .unwrap();
    assert!(body(replay).await.contains("invalid or has expired"));
    assert!(matches!(
        client
            .pass_state(&protocol::PassSession { token: old_session })
            .await
            .unwrap(),
        protocol::PassState::None {}
    ));
    assert!(matches!(
        client
            .pass_login(&protocol::PassLogin {
                identifier: identifier.clone(),
                pin: "owned-new-pin".into(),
                peer: "192.0.2.29".parse().unwrap(),
                remember: false
            })
            .await
            .unwrap(),
        protocol::PassLoginResult::Ready { .. }
    ));
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
    serving.abort();
    let _ = serving.await;
    mailer.close().await;
    issuer.close().await;
    public.close().await;
    owner.close().await;
}
