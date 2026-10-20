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
    CheckoutSecret::read(&headers, false)
        .unwrap()
        .unwrap()
        .nonce()
}
#[tokio::test]
#[ignore = "Run explicitly with isolated checkout and verifier test credentials."]
async fn public_renewal_quotes_are_csrf_bound_idempotent_and_wait_for_signed_payment() {
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
    sqlx::query(
        "UPDATE verification_private.pass_contacts SET renewal_hash=$2 WHERE entitlement_id=$1",
    )
    .bind(id)
    .bind(key.pass_renewal("owned-renewal").unwrap().as_slice())
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query("UPDATE verification_private.pass_checkout_policy SET enabled=true,amount_minor=2000,currency='usd',provider='sandbox' WHERE singleton").execute(&mut *tx).await.unwrap();
    let expected:chrono::DateTime<chrono::Utc>=sqlx::query_scalar("SELECT (expires_at AT TIME ZONE 'UTC'+interval '12 months') AT TIME ZONE 'UTC' FROM verification_private.pass_entitlements WHERE id=$1").bind(id).fetch_one(&mut *tx).await.unwrap();
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
    let app = crate::pass_recovery::routes()
        .merge(crate::pass_auth::routes())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            crate::security::protect,
        ))
        .with_state(state);

    let response = app
        .clone()
        .oneshot(request("/pass?renew=owned-renewal", "", None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["referrer-policy"], "strict-origin");
    let form_cookie = cookie(&response);
    let csrf = nonce(&form_cookie);
    let text = body(response).await;
    assert!(text.contains("20.00 USD"));
    assert!(text.contains("No real payment"));
    let fields = [
        ("csrf", csrf.as_str()),
        ("reference", "owned-renewal"),
        ("amount_minor", "2000"),
        ("currency", "usd"),
    ];
    let mut cross = request("/pass/renew", &form_cookie, Some(&fields));
    cross
        .headers_mut()
        .insert("origin", "https://outside.example".parse().unwrap());
    cross
        .headers_mut()
        .insert("sec-fetch-site", "cross-site".parse().unwrap());
    assert_eq!(
        app.clone().oneshot(cross).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let bad = app
        .clone()
        .oneshot(request(
            "/pass/renew",
            &form_cookie,
            Some(&[
                ("csrf", "wrong"),
                ("reference", "owned-renewal"),
                ("amount_minor", "2000"),
                ("currency", "usd"),
            ]),
        ))
        .await
        .unwrap();
    assert_eq!(bad.status(), StatusCode::FORBIDDEN);
    let changed = app
        .clone()
        .oneshot(request(
            "/pass/renew",
            &form_cookie,
            Some(&[
                ("csrf", &csrf),
                ("reference", "owned-renewal"),
                ("amount_minor", "1999"),
                ("currency", "usd"),
            ]),
        ))
        .await
        .unwrap();
    assert!(body(changed).await.contains("price has changed"));
    let (one, two) = tokio::join!(
        app.clone()
            .oneshot(request("/pass/renew", &form_cookie, Some(&fields))),
        app.clone()
            .oneshot(request("/pass/renew", &form_cookie, Some(&fields)))
    );
    let one = one.unwrap();
    let two = two.unwrap();
    assert_eq!(one.status(), StatusCode::SEE_OTHER);
    assert_eq!(two.status(), StatusCode::SEE_OTHER);
    let location = one.headers()[header::LOCATION].to_str().unwrap().to_owned();
    assert_eq!(two.headers()[header::LOCATION], location);
    let parsed = url::Url::parse(&format!("http://127.0.0.1:3000{location}")).unwrap();
    let pairs: std::collections::BTreeMap<_, _> = parsed
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    let order = pairs["order"].clone();
    let pending = app
        .clone()
        .oneshot(request(&location, "", None))
        .await
        .unwrap();
    assert!(body(pending).await.contains("Sandbox payment pending"));
    let forged = app
        .clone()
        .oneshot(request(&format!("{location}&paid=1"), "", None))
        .await
        .unwrap();
    assert_eq!(forged.status(), StatusCode::BAD_REQUEST);
    let store = board_store::checkout::CheckoutStore::connect(
        &std::env::var("TEST_CHECKOUT_DATABASE_URL").unwrap(),
    )
    .await
    .unwrap();
    let callback_key = "owned-public-checkout-key-12345678901234567890";
    let callback = board_checkout::router(board_checkout::AppState::new(
        store.clone(),
        board_domain::checkout::signature::CallbackKey::new(
            board_domain::checkout::signature::Provider::Sandbox,
            callback_key,
        )
        .unwrap(),
    ));
    let event = serde_json::json!({"event":format!("paid_{order}"),"order":order,"provider_object":format!("sandbox_{order}"),"product":"pass_renewal","amount_minor":2000,"currency":"usd","kind":"paid","occurred_at":chrono::Utc::now().timestamp()});
    let event = serde_json::to_vec(&event).unwrap();
    let stamp = chrono::Utc::now().timestamp();
    let mut signed = b"26chan:checkout-sandbox:v1\0".to_vec();
    signed.extend_from_slice(format!("{stamp}.").as_bytes());
    signed.extend_from_slice(&event);
    let signature = ring::hmac::sign(
        &ring::hmac::Key::new(ring::hmac::HMAC_SHA256, callback_key.as_bytes()),
        &signed,
    );
    let signature = signature
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let callback_response = callback
        .clone()
        .oneshot(
            Request::post("/events/sandbox")
                .header("content-type", "application/json")
                .header("x-checkout-signature", format!("t={stamp},v1={signature}"))
                .body(Body::from(event))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(callback_response.status(), StatusCode::NO_CONTENT);
    let paid = app
        .clone()
        .oneshot(request(&location, "", None))
        .await
        .unwrap();
    assert!(body(paid).await.contains("Your Pass has been renewed"));
    // New and delayed gift purchases use the same actual HTTP issuer and signed
    // payment adapter. Neither checkout creation nor a browser return grants a Pass.
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE verification_private.pass_checkout_policy SET purchase_enabled=true,purchase_amount_minor=2300 WHERE singleton").execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let private_client = VerificationClient::new(endpoint, "11".repeat(32)).unwrap();
    let mail_store = board_store::verification::mail::MailStore::connect(
        &std::env::var("TEST_VERIFICATION_MAIL_DATABASE_URL").unwrap(),
    )
    .await
    .unwrap();
    let mut purchased = Vec::new();
    for delayed in [false, true] {
        let response = app
            .clone()
            .oneshot(request("/pass", "", None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let purchase_cookie = cookie(&response);
        let purchase_nonce = nonce(&purchase_cookie);
        let html = body(response).await;
        assert!(html.contains("23.00 USD") && html.contains("name=\"gift_email\""));
        let form = [
            ("csrf", purchase_nonce.as_str()),
            ("email", "buyer@example.org"),
            ("email_confirm", "buyer@example.org"),
            ("gift_email", "gift@example.org"),
            ("gift_confirm", "gift@example.org"),
            ("delayed", if delayed { "1" } else { "" }),
            ("amount_minor", "2300"),
            ("currency", "usd"),
        ];
        let mut wrong = form;
        wrong[0] = ("csrf", "wrong");
        assert_eq!(
            app.clone()
                .oneshot(request("/pass/purchase", &purchase_cookie, Some(&wrong)))
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        wrong = form;
        wrong[4] = ("gift_confirm", "someone-else@example.org");
        assert_eq!(
            app.clone()
                .oneshot(request("/pass/purchase", &purchase_cookie, Some(&wrong)))
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
        let mut cross = request("/pass/purchase", &purchase_cookie, Some(&form));
        cross
            .headers_mut()
            .insert("origin", "https://outside.example".parse().unwrap());
        cross
            .headers_mut()
            .insert("sec-fetch-site", "cross-site".parse().unwrap());
        assert_eq!(
            app.clone().oneshot(cross).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
        let (first, second) = tokio::join!(
            app.clone()
                .oneshot(request("/pass/purchase", &purchase_cookie, Some(&form))),
            app.clone()
                .oneshot(request("/pass/purchase", &purchase_cookie, Some(&form))),
        );
        let (first, second) = (first.unwrap(), second.unwrap());
        let (created, concurrent) = if first.status() == StatusCode::SEE_OTHER {
            (first, second)
        } else {
            (second, first)
        };
        assert_eq!(created.status(), StatusCode::SEE_OTHER);
        let location = created.headers()[header::LOCATION]
            .to_str()
            .unwrap()
            .to_owned();
        // The preparation lease may reject the other in-flight request; once
        // preparation finishes, the same form must resolve to the same order.
        if concurrent.status() == StatusCode::SEE_OTHER {
            assert_eq!(concurrent.headers()[header::LOCATION], location);
        } else {
            assert_eq!(concurrent.status(), StatusCode::SERVICE_UNAVAILABLE);
        }
        let retry = app
            .clone()
            .oneshot(request("/pass/purchase", &purchase_cookie, Some(&form)))
            .await
            .unwrap();
        assert_eq!(retry.headers()[header::LOCATION], location);
        let parsed = url::Url::parse(&format!("http://127.0.0.1:3000{location}")).unwrap();
        let order = parsed
            .query_pairs()
            .find(|(key, _)| key == "order")
            .unwrap()
            .1
            .into_owned();
        let pending = app
            .clone()
            .oneshot(request(&location, "", None))
            .await
            .unwrap();
        assert!(body(pending).await.contains("No Pass has been created"));
        let mut tx = owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_anonymous_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        let unpaid: Option<i64> = sqlx::query_scalar("SELECT entitlement_id FROM verification_private.pass_checkout_orders WHERE id=$1::text::uuid").bind(&order).fetch_one(&mut *tx).await.unwrap();
        assert!(unpaid.is_none());
        tx.rollback().await.unwrap();
        assert_eq!(
            send_payment(&callback, callback_key, &order, "pass_purchase", 2301).await,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            send_payment(&callback, callback_key, &order, "pass_purchase", 2300).await,
            StatusCode::NO_CONTENT
        );
        let paid = app
            .clone()
            .oneshot(request(&location, "", None))
            .await
            .unwrap();
        let paid = body(paid).await;
        assert!(paid.contains("one-use PIN setup link") && !paid.contains("gift@example.org"));
        let mut tx = owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_anonymous_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        let row: (i64, i16, Option<chrono::DateTime<chrono::Utc>>, Vec<u8>, Vec<u8>) = sqlx::query_as("SELECT e.id,e.status,e.expires_at,e.token_hash,c.encrypted_contact FROM verification_private.pass_checkout_orders o JOIN verification_private.pass_entitlements e ON e.id=o.entitlement_id JOIN verification_private.pass_contacts c ON c.entitlement_id=e.id WHERE o.id=$1::text::uuid").bind(&order).fetch_one(&mut *tx).await.unwrap();
        assert_eq!(row.1, if delayed { 7 } else { 6 });
        assert_eq!(row.2.is_none(), delayed);
        let identity: [u8; 32] = row.3.try_into().unwrap();
        let Some(Payload::Contact(contact)) = mail_key.open_pass(&identity, &row.4) else {
            panic!("Invalid purchase contact")
        };
        assert_eq!(contact.recipient(), "gift@example.org");
        assert!(paid.contains(contact.identifier()));
        tx.commit().await.unwrap();
        let lease = board_domain::challenge_secrets::ReceiptToken::generate()
            .unwrap()
            .storage_hash();
        let jobs = mail_store.claim_recovery(&lease, 10).await.unwrap();
        let job = jobs.iter().find(|job| job.request_id == order).unwrap();
        let setup_hash: [u8; 32] = job.token_hash.as_slice().try_into().unwrap();
        let Some(Payload::Setup {
            contact: mailed,
            token,
        }) = mail_key.open_pass(&setup_hash, &job.encrypted_payload)
        else {
            panic!("Invalid setup mail")
        };
        assert_eq!(mailed.recipient(), "gift@example.org");
        assert_eq!(mailed.identifier(), contact.identifier());
        let setup = protocol::PassSession {
            token: token.encode(),
        };
        assert!(matches!(
            private_client.pass_recovery_state(&setup).await.unwrap(),
            protocol::PassRecoveryResult::Rejected {}
        ));
        // A restricted delivery acknowledgement enables the one-use setup token.
        // The SMTP acceptance/rejection boundary is exercised by mailer tests.
        assert!(
            mail_store
                .finish_recovery(&order, &lease, true)
                .await
                .unwrap()
        );
        assert!(matches!(
            private_client.pass_recovery_state(&setup).await.unwrap(),
            protocol::PassRecoveryResult::Ready {}
        ));
        let reset = protocol::PassRecoveryComplete {
            token: token.encode(),
            pin: "owned-gift-pin".into(),
        };
        assert!(matches!(
            private_client.pass_recovery_complete(&reset).await.unwrap(),
            protocol::PassRecoveryResult::Complete {}
        ));
        assert!(matches!(
            private_client.pass_recovery_complete(&reset).await.unwrap(),
            protocol::PassRecoveryResult::Rejected {}
        ));
        assert!(matches!(
            private_client
                .pass_login(&protocol::PassLogin {
                    identifier: contact.identifier().into(),
                    pin: "owned-gift-pin".into(),
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
        let active: (i16, bool) = sqlx::query_as("SELECT status,expires_at IS NOT NULL FROM verification_private.pass_entitlements WHERE id=$1").bind(row.0).fetch_one(&mut *tx).await.unwrap();
        assert_eq!(active, (0, true));
        tx.commit().await.unwrap();
        purchased.push(row.0);
    }
    mail_store.close().await;
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let actual: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
        "SELECT expires_at FROM verification_private.pass_entitlements WHERE id=$1",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(actual, expected);
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM verification_private.pass_checkout_orders WHERE entitlement_id=$1",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(count, 1);
    sqlx::query("DELETE FROM verification_private.pass_checkout_orders WHERE entitlement_id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM verification_private.pass_entitlements WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .unwrap();
    for account in purchased {
        sqlx::query(
            "DELETE FROM verification_private.pass_checkout_orders WHERE entitlement_id=$1",
        )
        .bind(account)
        .execute(&mut *tx)
        .await
        .unwrap();
        sqlx::query("DELETE FROM verification_private.pass_entitlements WHERE id=$1")
            .bind(account)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    sqlx::query("UPDATE verification_private.pass_checkout_policy SET enabled=false,amount_minor=0,purchase_enabled=false,purchase_amount_minor=0 WHERE singleton").execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    serving.abort();
    let _ = serving.await;
    store.close().await;
    issuer.close().await;
    public.close().await;
    owner.close().await;
}

async fn send_payment(
    callback: &axum::Router,
    key: &str,
    order: &str,
    product: &str,
    amount: i64,
) -> StatusCode {
    let stamp = chrono::Utc::now().timestamp();
    let event = serde_json::to_vec(&serde_json::json!({"event":format!("paid_{order}_{amount}"),"order":order,"provider_object":format!("sandbox_{order}"),"product":product,"amount_minor":amount,"currency":"usd","kind":"paid","occurred_at":stamp})).unwrap();
    let mut signed = b"26chan:checkout-sandbox:v1\0".to_vec();
    signed.extend_from_slice(format!("{stamp}.").as_bytes());
    signed.extend_from_slice(&event);
    let signature = ring::hmac::sign(
        &ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key.as_bytes()),
        &signed,
    );
    let signature: String = signature
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    callback
        .clone()
        .oneshot(
            Request::post("/events/sandbox")
                .header("content-type", "application/json")
                .header("x-checkout-signature", format!("t={stamp},v1={signature}"))
                .body(Body::from(event))
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}
