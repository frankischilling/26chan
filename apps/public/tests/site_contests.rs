#![cfg(feature = "database-tests")]

use axum::{
    Router,
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{Request, StatusCode, header},
    response::Response,
};
use board_domain::{
    anonymous_session::Capability,
    challenge_secrets::{PassSessionToken, ReceiptToken, VerificationKey},
    verification_mail::MailKey,
};
use board_media::{ApprovedFiles, PublicationStore, Quarantine, ValidatedOutput};
use board_store::{media::MediaQueue, media_assets::MediaReader, verification::VerificationStore};
use sqlx::PgPool;
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use tower::ServiceExt;

const PUBLIC: &str = "http://localhost:3000";
const MEDIA: &str = "http://127.0.0.1:3002";

async fn request(
    app: &Router,
    method: &str,
    path: &str,
    cookie: &str,
    origin: &str,
    body: String,
) -> Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header(header::COOKIE, cookie)
                .header(header::ORIGIN, origin)
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .extension(ConnectInfo(
                    "192.0.2.226:1234".parse::<SocketAddr>().unwrap(),
                ))
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn html(response: Response) -> String {
    String::from_utf8(
        to_bytes(response.into_body(), 262_144)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}

async fn campaign(owner: &PgPool, kind: i16, board: &str, closed: bool) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO submission_private.contest_campaigns
         (id,kind,enabled,boards,submission_starts,submission_ends,voting_starts,voting_ends,capacity,vote_capacity)
         VALUES(nextval('content.post_number'),$1,true,ARRAY[$2],1,2,3,$3,100,100) RETURNING id",
    )
    .bind(kind)
    .bind(board)
    .bind(if closed { 4_i64 } else { 253_402_300_799 })
    .fetch_one(owner)
    .await
    .unwrap()
}

async fn entry(owner: &PgPool, campaign: i64, board: &str, asset: &str, state: i16) -> i64 {
    let owner_hash = Capability::generate().unwrap().storage_hash();
    sqlx::query_scalar(
        "INSERT INTO submission_private.contest_entries
         (campaign_id,board,ownership_hash,actor_hash,author,email,media_id,state,created_at,updated_at)
         VALUES($1,$2,$3,$3,'Private banner credit','private@example.org',$4,$5,1,1) RETURNING id",
    )
    .bind(campaign)
    .bind(board)
    .bind(owner_hash.as_slice())
    .bind(asset)
    .bind(state)
    .fetch_one(owner)
    .await
    .unwrap()
}

#[tokio::test]
#[ignore = "Requires the owned separate verifier and media database credentials."]
async fn public_contest_listing_and_pass_vote_keep_scores_hidden_until_results() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut lock = owner.acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock(2250258)")
        .execute(&mut *lock)
        .await
        .unwrap();

    let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let reader = MediaReader::connect(&std::env::var("MEDIA_READ_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let quarantine = Quarantine::new(temp.path().join("quarantine")).unwrap();
    let root = temp.path().join("objects");
    let files = PublicationStore::new(&root, &quarantine).unwrap();
    let job = queue.reserve("Owned contest banner").await.unwrap();
    queue.queue(&job.id, 4).await.unwrap();
    let lease = queue.claim().await.unwrap().unwrap();
    assert_eq!(lease.id, job.id, "Requires an idle disposable queue");
    let mut pixels = b"IBRGBA01".to_vec();
    pixels.extend_from_slice(&468_u32.to_be_bytes());
    pixels.extend_from_slice(&60_u32.to_be_bytes());
    pixels.extend([100, 20, 30, 255].repeat(468 * 60));
    let output = ValidatedOutput::read(pixels.as_slice()).await.unwrap();
    let asset = board_media_admin::publish(
        &queue,
        &files,
        &job.id,
        lease.lease_token.as_deref().unwrap(),
        &output,
    )
    .await
    .unwrap();
    let media = board_media_http::router(board_media_http::AppState::new(
        reader,
        ApprovedFiles::open(&root).unwrap(),
        &board_config::Origin::parse(MEDIA).unwrap(),
        &board_config::Origin::parse(PUBLIC).unwrap(),
    ));

    let banner = campaign(&owner, 1, "g", false).await;
    let vtuber = campaign(&owner, 2, "all", true).await;
    let approved = entry(&owner, banner, "g", &asset.id, 1).await;
    let unapproved = entry(&owner, banner, "g", &"c".repeat(32), 0).await;
    let hidden = entry(&owner, vtuber, "all", &"b".repeat(32), 1).await;
    let pass = PassSessionToken::generate().unwrap();
    let member = {
        let identity = ReceiptToken::generate().unwrap().storage_hash();
        let mut tx = owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_anonymous_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        let member: i64 = sqlx::query_scalar(
            "INSERT INTO verification_private.pass_entitlements(token_hash,pin_hash,status,purchased_at,expires_at)
             VALUES($1,repeat('p',64),0,'2022-01-01',clock_timestamp()+interval '1 day') RETURNING id",
        ).bind(identity.as_slice()).fetch_one(&mut *tx).await.unwrap();
        sqlx::query(
            "INSERT INTO verification_private.pass_sessions(token_hash,entitlement_id,credential_version,created_at,expires_at)
             SELECT $1,id,credential_version,clock_timestamp(),clock_timestamp()+interval '1 hour'
             FROM verification_private.pass_entitlements WHERE id=$2",
        ).bind(pass.storage_hash().as_slice()).bind(member).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        member
    };

    let issuer =
        VerificationStore::connect(&std::env::var("TEST_VERIFICATION_DATABASE_URL").unwrap())
            .await
            .unwrap();
    let material = ReceiptToken::generate().unwrap().encode();
    let verifier = board_verification::router(
        board_verification::AppState::new(
            issuer,
            VerificationKey::parse(&material[3..]).unwrap(),
            MailKey::parse(&material[3..]).unwrap(),
            &"11".repeat(32),
        )
        .unwrap(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, verifier).await.unwrap() });
    let (web, _) = board_public::routers_with_events(
        public,
        board_public::PublicRouterOptions {
            origin: PUBLIC.into(),
            production: false,
            media: Some(
                board_config::PublicMediaSettings::development(
                    "127.0.0.1:3001",
                    &"22".repeat(32),
                    MEDIA,
                )
                .unwrap(),
            ),
            limits: Default::default(),
            proxy_uid: None,
            poster_id_key: None,
            tripcode_key: None,
            country_database: None,
        },
        board_config::PublicVerificationSettings::parse(
            Some(&endpoint.to_string()),
            Some(&"11".repeat(32)),
        )
        .unwrap(),
        Default::default(),
        false,
    );

    let session = Arc::new(Mutex::new(None));
    let test_session = session.clone();
    let test_owner = owner.clone();
    let test_asset = asset.id.clone();
    let result = tokio::spawn(async move {
        let path = format!("/contests/banners/{banner}/g");
        let get = async |path: &str, cookie: &str| {
            request(&web, "GET", path, cookie, PUBLIC, String::new()).await
        };
        let directory = html(get("/contests/banners", "").await).await;
        assert!(directory.contains(&path));
        assert_eq!(
            get(&format!("/contests/vtubers/{banner}/g"), "")
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            get(&format!("{path}?offset=100001"), "").await.status(),
            StatusCode::BAD_REQUEST
        );
        let vtuber_page = html(get(&format!("/contests/vtubers/{vtuber}/all"), "").await).await;
        assert!(vtuber_page.contains("Entries are visible while voting is open."));
        assert!(!vtuber_page.contains(&format!("Entry {hidden}")));

        let public_page = get(&path, "pass_enabled=1").await;
        assert_eq!(public_page.status(), StatusCode::OK);
        assert!(!public_page.headers().contains_key(header::SET_COOKIE));
        assert!(
            public_page.headers()[header::CONTENT_SECURITY_POLICY]
                .to_str()
                .unwrap()
                .contains("script-src 'none'")
        );
        let public_page = html(public_page).await;
        assert!(!public_page.contains("name=\"csrf\""));
        assert!(!public_page.contains("Votes received"));
        assert!(!public_page.contains("private@example.org"));
        assert!(!public_page.contains("Private banner credit"));
        assert!(!public_page.contains(&format!("Entry {unapproved}")));
        let media_path = format!("/media/{test_asset}.image");
        assert!(public_page.contains(&format!("{MEDIA}{media_path}")));
        let image = media
            .oneshot(
                Request::get(media_path)
                    .header(header::HOST, "127.0.0.1:3002")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(image.status(), StatusCode::OK);
        assert_eq!(image.headers()[header::CONTENT_TYPE], "image/png");
        assert!(
            to_bytes(image.into_body(), 153_600)
                .await
                .unwrap()
                .starts_with(b"\x89PNG\r\n\x1a\n")
        );

        let pass_only = format!("board-pass={}", pass.encode());
        let head = request(&web, "HEAD", &path, &pass_only, PUBLIC, String::new()).await;
        assert_eq!(head.status(), StatusCode::OK);
        assert!(!head.headers().contains_key(header::SET_COOKIE));
        let voting = get(&path, &pass_only).await;
        assert_eq!(voting.status(), StatusCode::OK);
        assert_eq!(voting.headers()[header::CACHE_CONTROL], "private, no-store");
        let new_cookies: Vec<_> = voting
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|value| value.to_str().unwrap().split(';').next().unwrap())
            .collect();
        let anonymous = new_cookies
            .iter()
            .find_map(|value| value.strip_prefix("board-anon="))
            .unwrap();
        *test_session.lock().unwrap() = Some(Capability::parse(anonymous).unwrap().storage_hash());
        let cookies = format!("{}; {}", pass_only, new_cookies.join("; "));
        let forged = format!(
            "board-pass={}; {}",
            PassSessionToken::generate().unwrap().encode(),
            new_cookies.join("; "),
        );
        let form = html(voting).await;
        let csrf = form
            .split("name=\"csrf\" value=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        let submit = |entry, nonce, origin| {
            request(
                &web,
                "POST",
                &path,
                &cookies,
                origin,
                format!("entry={entry}&csrf={nonce}"),
            )
        };
        assert_eq!(
            submit(approved, "bad", PUBLIC).await.status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            submit(approved, csrf, "https://other.invalid")
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            submit(unapproved, csrf, PUBLIC).await.status(),
            StatusCode::NOT_FOUND
        );
        let refusal = request(
            &web,
            "POST",
            &path,
            &forged,
            PUBLIC,
            format!("entry={approved}&csrf={csrf}"),
        )
        .await;
        assert_eq!(refusal.status(), StatusCode::FORBIDDEN);
        let accepted = submit(approved, csrf, PUBLIC).await;
        assert_eq!(accepted.status(), StatusCode::OK);
        assert!(html(accepted).await.contains("Vote recorded"));
        let duplicate = submit(approved, csrf, PUBLIC).await;
        assert_eq!(duplicate.status(), StatusCode::CONFLICT);
        assert!(html(duplicate).await.contains("already voted"));
        assert!(!html(get(&path, "").await).await.contains("Votes received"));

        sqlx::query("UPDATE submission_private.contest_campaigns SET voting_ends=4 WHERE id=$1")
            .bind(banner)
            .execute(&test_owner)
            .await
            .unwrap();
        let results = html(get(&path, "").await).await;
        assert!(results.contains("Votes received 1"));
        assert!(!results.contains("name=\"csrf\""));
        assert_eq!(
            submit(approved, csrf, PUBLIC).await.status(),
            StatusCode::CONFLICT
        );
    })
    .await;
    server.abort();
    let _ = server.await;
    for statement in [
        "DELETE FROM submission_private.contest_votes WHERE campaign_id=ANY($1)",
        "DELETE FROM submission_private.contest_entries WHERE campaign_id=ANY($1)",
    ] {
        sqlx::query(statement)
            .bind(vec![banner, vtuber])
            .execute(&owner)
            .await
            .unwrap();
    }
    sqlx::query("DELETE FROM submission_private.contest_campaigns WHERE id=ANY($1)")
        .bind(vec![banner, vtuber])
        .execute(&owner)
        .await
        .unwrap();
    let token = *session.lock().unwrap();
    if let Some(token) = token {
        sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
            .bind(token.as_slice())
            .execute(&owner)
            .await
            .unwrap();
    }
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM verification_private.pass_entitlements WHERE id=$1")
        .bind(member)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    sqlx::query("DELETE FROM media.assets WHERE id=$1")
        .bind(asset.id)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM media.jobs WHERE id=$1")
        .bind(job.id)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("SELECT pg_advisory_unlock(2250258)")
        .execute(&mut *lock)
        .await
        .unwrap();
    result.unwrap();
}
