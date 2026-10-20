#![cfg(feature = "database-tests")]
//! Owned accounts exercise actual public/private HTTP; browser children receive no database authority.
use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use board_domain::{
    challenge_secrets::{PassSessionToken, VerificationKey},
    verification_mail::MailKey,
};
use serde_json::json;
use std::{net::SocketAddr, process::Stdio, sync::Arc, time::Duration};
use tokio::io::AsyncWriteExt;

#[tokio::test]
#[ignore = "Requires isolated verifier credentials; invokes an unprivileged HTTP client."]
async fn pass_states_through_public_posting_and_api() {
    exercise("http", false).await;
}

#[tokio::test]
#[ignore = "Requires isolated verifier credentials for the Pass-only board matrix."]
async fn pass_only_board_states_through_public_posting_and_api() {
    exercise("http", true).await;
}

#[cfg(feature = "browser-tests")]
#[tokio::test]
#[ignore = "Requires isolated verifier credentials and the installed Chromium fixture."]
async fn pass_states_through_native_forms_quick_reply_and_api() {
    exercise("browser", false).await;
}

#[cfg(feature = "browser-tests")]
#[tokio::test]
#[ignore = "Requires isolated verifier credentials and Chromium with scripts disabled."]
async fn pass_states_through_script_disabled_native_forms() {
    exercise("noscript", false).await;
}

fn owned_database(name: &str, role: &str) -> String {
    assert_eq!(
        std::env::var("APP_ENV").unwrap_or_else(|_| "development".into()),
        "development"
    );
    let value = std::env::var(name).expect("Owned fixture database configuration required");
    let parsed = url::Url::parse(&value).expect("Owned database URL required");
    assert!(
        matches!(parsed.scheme(), "postgres" | "postgresql")
            && parsed.username() == role
            && parsed.query().is_none()
            && parsed.fragment().is_none()
    );
    assert!(
        parsed.host_str().is_some_and(|host| host == "localhost"
            || host
                .trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback())),
        "Only an owned loopback database is allowed"
    );
    value
}

async fn exercise(mode: &str, pass_only: bool) {
    let owner = sqlx::PgPool::connect(&owned_database("MIGRATION_DATABASE_URL", "board_migrator"))
        .await
        .unwrap();
    let public =
        board_store::connect_public(&owned_database("TEST_PUBLIC_DATABASE_URL", "board_public"))
            .await
            .unwrap();
    let issuer = board_store::verification::VerificationStore::connect(&owned_database(
        "TEST_VERIFICATION_DATABASE_URL",
        "board_verifier",
    ))
    .await
    .unwrap();
    let owner_role: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&owner)
        .await
        .unwrap();
    assert_eq!(owner_role, "board_migrator");
    let material = PassSessionToken::generate().unwrap().encode();
    let key = VerificationKey::parse(&material[3..]).unwrap();
    let poster_material = PassSessionToken::generate().unwrap().encode();
    let board = format!("pm{}", &material[3..11]);
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,text_only,posting_reply_seconds,posting_image_seconds,posting_thread_seconds,captcha_enabled,captcha_twister,captcha_allow_bypass,pass_post_only) VALUES($1,'Owned Pass state matrix','Owned fixture',2000,100,100,100,10,true,0,0,0,true,true,true,$2)").bind(&board).bind(pass_only).execute(&owner).await.unwrap();
    let target: i64 =
        sqlx::query_scalar("INSERT INTO content.threads(board) VALUES($1) RETURNING id")
            .bind(&board)
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous','Owned Pass matrix target','Owned seed target')").bind(target).bind(&board).execute(&owner).await.unwrap();
    let pin = "owned-state-matrix-pin";
    let pin_hash = Argon2::default()
        .hash_password(
            pin.as_bytes(),
            &SaltString::encode_b64(b"owned-state-matrix").unwrap(),
        )
        .unwrap()
        .to_string();
    let mut accounts = Vec::new();
    let mut cases = Vec::new();
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    for (name, status) in [
        ("new", 6i16),
        ("active", 0),
        ("expired", 1),
        ("refunded", 2),
        ("chargeback", 3),
        ("spam_revoked", 4),
        ("content_revoked", 5),
        ("delayed", 7),
        ("renewed", 0),
    ] {
        let identifier = board_domain::checkout::new_pass_identifier().unwrap();
        let hash = key.pass_identifier(&identifier).unwrap();
        let account:i64=sqlx::query_scalar("INSERT INTO verification_private.pass_entitlements(token_hash,pin_hash,status,purchased_at,expires_at) VALUES($1,$2,$3,'2020-01-02T00:00:00Z',CASE WHEN $3=7 THEN NULL WHEN $3=1 THEN clock_timestamp()-interval '1 day' ELSE clock_timestamp()+interval '1 year' END) RETURNING id").bind(hash.as_slice()).bind(&pin_hash).bind(status).fetch_one(&mut *tx).await.unwrap();
        if name == "renewed" {
            sqlx::query("UPDATE verification_private.pass_entitlements SET status=1,expires_at=clock_timestamp()-interval '1 day' WHERE id=$1").bind(account).execute(&mut *tx).await.unwrap();
            sqlx::query("UPDATE verification_private.pass_entitlements SET status=0,expires_at=clock_timestamp()+interval '12 months' WHERE id=$1").bind(account).execute(&mut *tx).await.unwrap();
        }
        accounts.push(account);
        cases.push(json!({"name":name,"identifier":identifier,"pin":pin,"active":matches!(status,0|6|7),"account":account}));
    }
    tx.commit().await.unwrap();
    let private_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = private_listener.local_addr().unwrap();
    let private_app = board_verification::router(
        board_verification::AppState::new(
            issuer.clone(),
            key,
            MailKey::parse(&"22".repeat(32)).unwrap(),
            &"11".repeat(32),
        )
        .unwrap(),
    );
    let private_server = tokio::spawn(async move {
        axum::serve(private_listener, private_app).await.unwrap();
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let api_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let api_origin = format!("http://{}", api_listener.local_addr().unwrap());
    let verification = board_config::PublicVerificationSettings::parse(
        Some(&endpoint.to_string()),
        Some(&"11".repeat(32)),
    )
    .unwrap();
    let (app, api) = board_public::routers_with_verification(
        public.clone(),
        board_public::PublicRouterOptions {
            origin: origin.clone(),
            production: false,
            media: None,
            limits: board_config::PublicRequestLimits::default(),
            proxy_uid: None,
            poster_id_key: Some(Arc::new(
                board_domain::poster_id::PosterIdKey::parse(&poster_material[3..]).unwrap(),
            )),
            tripcode_key: None,
            country_database: None,
        },
        verification,
    );
    let serving = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let api_serving = tokio::spawn(async move {
        axum::serve(
            api_listener,
            api.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let mut command = tokio::process::Command::new("node");
    command.env_clear();
    for name in [
        "PATH",
        "SystemRoot",
        "WINDIR",
        "TEMP",
        "TMP",
        "USERPROFILE",
        "HOME",
        "LOCALAPPDATA",
        "APPDATA",
        "PLAYWRIGHT_BROWSERS_PATH",
        "LANG",
    ] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    let mut child = command
        .current_dir(root)
        .arg("tests/browser/pass-state-matrix.mjs")
        .arg(mode)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(&serde_json::to_vec(&json!({"origin":origin,"api":api_origin,"board":board,"target":target,"cases":cases})).unwrap()).await.unwrap();
    // Browser cases share one real peer and may wait through four 61-second
    // write windows. Individual browser actions retain their 12-second limit.
    let seconds = if mode == "http" { 240 } else { 420 };
    let output = tokio::time::timeout(Duration::from_secs(seconds), child.wait_with_output()).await;
    serving.abort();
    api_serving.abort();
    private_server.abort();
    let _ = serving.await;
    let _ = api_serving.await;
    let _ = private_server.await;
    let report = output
        .as_ref()
        .ok()
        .and_then(|result| result.as_ref().ok())
        .and_then(|result| serde_json::from_slice::<serde_json::Value>(&result.stdout).ok());
    let anonymous: Vec<Vec<u8>> = report
        .as_ref()
        .and_then(|value| value["anonymous"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str())
        .filter_map(board_domain::anonymous_session::Capability::parse)
        .map(|capability| capability.storage_hash().to_vec())
        .collect();
    let expected: Vec<(i64, i32, i64)> = report
        .as_ref()
        .and_then(|value| value["posted"].as_array())
        .into_iter()
        .flatten()
        .map(|value| {
            (
                value["id"].as_i64().unwrap_or(0),
                value["year"].as_i64().unwrap_or(-1) as i32,
                value["account"].as_i64().unwrap_or(0),
            )
        })
        .collect();
    let mut validation = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *validation)
        .await
        .unwrap();
    let links: Vec<(i64, i64)> = sqlx::query_as("SELECT m.post_id,e.id FROM verification_private.pass_posts m JOIN verification_private.pass_entitlements e ON e.token_hash=m.identity_hash WHERE e.id=ANY($1) ORDER BY m.post_id")
        .bind(&accounts).fetch_all(&mut *validation).await.unwrap();
    let credits:i64=sqlx::query_scalar("SELECT count(*) FROM verification_private.credits WHERE identity_key IN(SELECT sha256(convert_to('26chan:captcha-credit:browser:v1','UTF8')||value) FROM unnest($1::bytea[]) value)").bind(&anonymous).fetch_one(&mut *validation).await.unwrap();
    validation.rollback().await.unwrap();
    let posts: Vec<(i64, i32)> = sqlx::query_as(
        "SELECT id,since4pass FROM content.posts WHERE board=$1 AND id<>$2 ORDER BY id",
    )
    .bind(&board)
    .bind(target)
    .fetch_all(&owner)
    .await
    .unwrap();
    let saved: Vec<(i64, i32, i64)> = posts
        .iter()
        .map(|(id, year)| {
            (
                *id,
                *year,
                links
                    .iter()
                    .find(|(post, _)| post == id)
                    .map_or(0, |(_, account)| *account),
            )
        })
        .collect();
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=$1")
        .bind(&board)
        .fetch_one(&owner)
        .await
        .unwrap();
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
        .bind(&board)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM content.threads WHERE board=$1 ORDER BY id FOR UPDATE")
        .bind(&board)
        .execute(&mut *tx)
        .await
        .unwrap();
    for statement in [
        "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM post_secrets.posting_thread_actions WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(statement)
            .bind(&board)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=ANY($1)")
        .bind(anonymous)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM verification_private.pass_entitlements WHERE id=ANY($1)")
        .bind(accounts)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    issuer.close().await;
    public.close().await;
    owner.close().await;
    let output = output.unwrap().unwrap();
    assert!(
        output.status.success(),
        "Pass state client failed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        expected.len(),
        8,
        "four eligible states must each publish an OP and reply"
    );
    assert_eq!(
        saved, expected,
        "public output must match private account provenance and opt-in years"
    );
    assert_eq!(total, 9, "blocked states must not create any posts");
    assert_eq!(
        credits, 0,
        "Pass bypass must not manufacture CAPTCHA credits"
    );
}
