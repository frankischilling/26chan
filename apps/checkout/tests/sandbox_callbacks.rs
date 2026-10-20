#![cfg(feature = "database-tests")]
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use board_checkout::{AppState, router};
use board_domain::{
    challenge_secrets::ReceiptToken,
    checkout::signature::{CallbackKey, Provider},
};
use board_store::checkout::CheckoutStore;
use ring::hmac;
use serde_json::{Value, json};
use tower::ServiceExt;
const KEY: &str = "owned-sandbox-callback-key-12345678901234567890";
fn request(body: Vec<u8>, valid: bool) -> Request<Body> {
    let stamp = chrono::Utc::now().timestamp();
    let mut bytes = b"26chan:checkout-sandbox:v1\0".to_vec();
    bytes.extend_from_slice(format!("{stamp}.").as_bytes());
    bytes.extend_from_slice(&body);
    let signature = hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, KEY.as_bytes()), &bytes);
    let encoded = signature
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    Request::post("/events/sandbox")
        .header("content-type", "application/json")
        .header(
            "x-checkout-signature",
            format!(
                "t={stamp},v1={}",
                if valid { encoded } else { "00".repeat(32) }
            ),
        )
        .body(Body::from(body))
        .unwrap()
}
#[tokio::test]
#[ignore = "Requires isolated checkout, verifier and migration credentials."]
async fn signed_sandbox_callbacks_bind_order_and_renew_once_without_reviving_refunds() {
    let owner = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let verifier = sqlx::PgPool::connect(&std::env::var("TEST_VERIFICATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let store = CheckoutStore::connect(&std::env::var("TEST_CHECKOUT_DATABASE_URL").unwrap())
        .await
        .unwrap();
    assert!(matches!(
        CheckoutStore::connect(&std::env::var("TEST_VERIFICATION_DATABASE_URL").unwrap()).await,
        Err(board_store::StoreError::UnsafeRole)
    ));
    assert!(matches!(
        CheckoutStore::connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap()).await,
        Err(board_store::StoreError::UnsafeRole)
    ));
    let app = router(AppState::new(
        store.clone(),
        CallbackKey::new(Provider::Sandbox, KEY).unwrap(),
    ));
    let random = ReceiptToken::generate().unwrap();
    let hash = random.storage_hash();
    let material = random.encode();
    let hex = &material[3..35];
    let id = format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    );
    let object = format!("owned_{}", hex);
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let account:i64=sqlx::query_scalar("INSERT INTO verification_private.pass_entitlements(token_hash,pin_hash,status,purchased_at,expires_at) VALUES($1,repeat('owned-placeholder',3),0,clock_timestamp()-interval '1 year',clock_timestamp()+interval '20 days') RETURNING id").bind(hash.as_slice()).fetch_one(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO verification_private.pass_contacts(entitlement_id,recipient_hash,encrypted_contact,renewal_hash) VALUES($1,$2,decode(repeat('02',40),'hex'),$2)").bind(account).bind(hash.as_slice()).execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE verification_private.pass_checkout_policy SET enabled=true,amount_minor=2000,currency='usd',provider='sandbox' WHERE singleton").execute(&mut *tx).await.unwrap();
    let expected:chrono::DateTime<chrono::Utc>=sqlx::query_scalar("SELECT (expires_at AT TIME ZONE 'UTC'+interval '12 months') AT TIME ZONE 'UTC' FROM verification_private.pass_entitlements WHERE id=$1").bind(account).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let created: String = sqlx::query_scalar(
        "SELECT content.pass_create_renewal_order($1::text::uuid,$2,$2,$2,2000,'usd',$3)",
    )
    .bind(&id)
    .bind(hash.as_slice())
    .bind(&object)
    .fetch_one(&verifier)
    .await
    .unwrap();
    assert_eq!(created, "created");
    let mut event = json!({"event":format!("paid_{hex}"),"order":id,"provider_object":object,"product":"pass_renewal","amount_minor":2000,"currency":"usd","kind":"paid","occurred_at":chrono::Utc::now().timestamp()});
    let body = serde_json::to_vec(&event).unwrap();
    assert_eq!(
        app.clone()
            .oneshot(request(body.clone(), false))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let mut wrong = event.clone();
    wrong["amount_minor"] = json!(1999);
    assert_eq!(
        app.clone()
            .oneshot(request(serde_json::to_vec(&wrong).unwrap(), true))
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    let (one, two) = tokio::join!(
        app.clone().oneshot(request(body.clone(), true)),
        app.clone().oneshot(request(body, true))
    );
    assert_eq!(one.unwrap().status(), StatusCode::NO_CONTENT);
    assert_eq!(two.unwrap().status(), StatusCode::NO_CONTENT);
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let actual: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
        "SELECT expires_at FROM verification_private.pass_entitlements WHERE id=$1",
    )
    .bind(account)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(actual, expected);
    let count:i64=sqlx::query_scalar("SELECT count(*) FROM verification_private.pass_checkout_events WHERE order_id=$1::text::uuid").bind(&id).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(count, 1);
    tx.commit().await.unwrap();
    event["event"] = Value::String(format!("refund_{hex}"));
    event["kind"] = json!("refunded");
    assert_eq!(
        app.clone()
            .oneshot(request(serde_json::to_vec(&event).unwrap(), true))
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    event["event"] = Value::String(format!("late_{hex}"));
    event["kind"] = json!("paid");
    assert_eq!(
        app.clone()
            .oneshot(request(serde_json::to_vec(&event).unwrap(), true))
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let status: i16 =
        sqlx::query_scalar("SELECT status FROM verification_private.pass_entitlements WHERE id=$1")
            .bind(account)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(status, 2);
    sqlx::query("DELETE FROM verification_private.pass_checkout_orders WHERE id=$1::text::uuid")
        .bind(&id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM verification_private.pass_entitlements WHERE id=$1")
        .bind(account)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE verification_private.pass_checkout_policy SET enabled=false,amount_minor=0 WHERE singleton").execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    store.close().await;
    verifier.close().await;
    owner.close().await;
}
