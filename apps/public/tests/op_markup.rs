#![cfg(feature = "database-tests")]

use axum::{
    Router,
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::Request,
};
use rand_core::{OsRng, RngCore};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{net::SocketAddr, time::Duration};
use tower::ServiceExt;

const RAW: &str = "[b]<script>[/b][i]italic[/i][red]red[/red][green]green[/green][blue]blue[/blue]";
const PASSWORD: &str = "owned-op-markup-password";

async fn submit(
    app: &Router,
    slug: &str,
    parent: i64,
    peer: Option<&str>,
    password: &str,
    mode: usize,
) -> i64 {
    let fields = [
        ("mode", "regist".into()),
        ("resto", parent.to_string()),
        ("sub", "Owned OP markup".into()),
        ("com", RAW.into()),
        ("pwd", password.into()),
    ];
    let (content_type, body) = if mode & 1 == 0 {
        (
            "application/x-www-form-urlencoded",
            url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs(fields.iter().map(|(k, v)| (*k, v)))
                .finish(),
        )
    } else {
        let mut body = String::new();
        for (name, value) in &fields {
            body.push_str(&format!(
                "--owned\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            ));
        }
        body.push_str("--owned--\r\n");
        ("multipart/form-data; boundary=owned", body)
    };
    let route = if mode & 2 == 0 {
        "post"
    } else {
        "imgboard.php"
    };
    let mut request = Request::post(format!("/{slug}/{route}"))
        .header("origin", "http://127.0.0.1:3000")
        .header(
            "accept",
            if mode & 4 == 0 {
                "application/json"
            } else {
                "text/html"
            },
        )
        .header("x-forwarded-for", "192.0.2.10")
        .header("content-type", content_type)
        .body(Body::from(body))
        .unwrap();
    if let Some(peer) = peer {
        request
            .extensions_mut()
            .insert(ConnectInfo(peer.parse::<SocketAddr>().unwrap()));
    }
    let response = app.clone().oneshot(request).await.unwrap();
    if mode & 4 == 0 {
        assert_eq!(response.status(), 200);
        let json: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap()).unwrap();
        assert!(json.get("error").is_none(), "{json}");
        json["pid"].as_i64().unwrap()
    } else {
        assert_eq!(response.status(), 303);
        response.headers()["location"]
            .to_str()
            .unwrap()
            .split("#p")
            .nth(1)
            .unwrap()
            .parse()
            .unwrap()
    }
}

async fn get(app: &Router, path: &str) -> String {
    let response = app
        .clone()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), 200, "{path}");
    String::from_utf8(
        to_bytes(response.into_body(), 2_000_000)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}

async fn exercise(owner: PgPool, public: PgPool, slug: String) {
    let (app, api) = board_public::routers(public.clone(), "http://127.0.0.1:3000".into(), false);
    let op = submit(&app, &slug, 0, Some("192.0.2.10:6000"), PASSWORD, 0).await;
    assert_eq!(
        board_store::find_post(&public, &slug, op)
            .await
            .unwrap()
            .comment_format,
        24
    );
    let mut expected = vec![(op, true)];
    for mode in 0..8 {
        for (peer, password, allowed) in [
            (Some("192.0.2.10:6001"), "different-password", true),
            (Some("[::ffff:192.0.2.10]:6002"), "different-password", true),
            (Some("192.0.2.11:6000"), PASSWORD, true),
            (Some("192.0.2.11:6001"), "different-password", false),
            (None, PASSWORD, true),
            (None, "different-password", false),
        ] {
            let id = submit(&app, &slug, op, peer, password, mode).await;
            let saved = board_store::find_post(&public, &slug, id).await.unwrap();
            assert_eq!(
                saved.comment_format,
                if allowed { 24 } else { 8 },
                "mode={mode}, allowed={allowed}"
            );
            assert!(saved.comment.contains("[b]"));
            expected.push((id, allowed));
            let setting: Option<String> =
                sqlx::query_scalar("SELECT current_setting('board.source_op_reply', true)")
                    .fetch_one(&public)
                    .await
                    .unwrap();
            assert_ne!(
                setting.as_deref(),
                Some("true"),
                "transaction context leaked into pooled connection"
            );
        }
    }
    // Password matching must not become the address-only self-bump identity.
    let owner_rows: i64 =
        sqlx::query_scalar("SELECT count(*) FROM post_secrets.op_replies WHERE thread_id=$1")
            .bind(op)
            .fetch_one(&owner)
            .await
            .unwrap();
    assert_eq!(owner_rows, 16);
    for router in [&app, &api] {
        let json = get(router, &format!("/{slug}/thread/{op}.json")).await;
        assert!(!json.contains("argon2"));
        assert!(!json.contains("192.0.2."));
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        for (id, allowed) in &expected {
            let post = value["posts"]
                .as_array()
                .unwrap()
                .iter()
                .find(|post| post["no"] == *id)
                .unwrap();
            assert!(post.get("comment_format").is_none());
            assert_eq!(
                post["com"].as_str().unwrap().contains("class=\"mu-s\""),
                *allowed
            );
            assert!(!post["com"].as_str().unwrap().contains("<script>"));
        }
    }
    let page = get(&app, &format!("/{slug}/thread/{op}")).await;
    for class in ["mu-s", "mu-i", "mu-r", "mu-g", "mu-b"] {
        assert!(page.contains(&format!("class=\"{class}\"")));
    }
    let catalog = get(&app, &format!("/{slug}/catalog")).await;
    assert!(!catalog.contains("[b]&"));
    let path = format!("/{slug}/thread/{op}.json");
    let before = get(&api, &path).await;
    sqlx::query("UPDATE content.boards SET op_markup=false WHERE slug=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    assert_eq!(
        get(&api, &path).await,
        before,
        "later board policy rewrote saved formatting"
    );
    let plain = submit(&app, &slug, op, Some("192.0.2.12:6000"), PASSWORD, 0).await;
    assert_eq!(
        board_store::find_post(&public, &slug, plain)
            .await
            .unwrap()
            .comment_format,
        8
    );
    let plain_op = submit(&app, &slug, 0, Some("192.0.2.12:6001"), PASSWORD, 0).await;
    assert_eq!(
        board_store::find_post(&public, &slug, plain_op)
            .await
            .unwrap()
            .comment_format,
        8
    );
    // An operator changes policy while the real submitter is blocked on its board lock.
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&public)
        .await
        .unwrap();
    let mut tx = owner.begin().await.unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE content.boards SET op_markup=true WHERE slug=$1")
        .bind(&slug)
        .execute(&mut *tx)
        .await
        .unwrap();
    let child_app = app.clone();
    let child_slug = slug.clone();
    let posting = tokio::spawn(async move {
        submit(
            &child_app,
            &child_slug,
            op,
            Some("192.0.2.13:6000"),
            PASSWORD,
            0,
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT $2=ANY(pg_blocking_pids($1))")
                .bind(pid)
                .bind(blocker)
                .fetch_one(&owner)
                .await
                .unwrap();
            if waiting {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("posting did not reach the actual policy lock");
    tx.commit().await.unwrap();
    let locked = posting.await.unwrap();
    assert_eq!(
        board_store::find_post(&public, &slug, locked)
            .await
            .unwrap()
            .comment_format,
        24
    );
    // A proof from the earlier read cannot outlive an operator's hash change.
    let mut tx = owner.begin().await.unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE content.boards SET op_markup=true WHERE slug=$1")
        .bind(&slug)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE post_secrets.deletion SET password_hash='owned-rotated-hash' WHERE post_id=$1",
    )
    .bind(op)
    .execute(&mut *tx)
    .await
    .unwrap();
    let child_app = app.clone();
    let child_slug = slug.clone();
    let posting = tokio::spawn(async move {
        submit(
            &child_app,
            &child_slug,
            op,
            Some("192.0.2.15:6000"),
            PASSWORD,
            0,
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT $2=ANY(pg_blocking_pids($1))")
                .bind(pid)
                .bind(blocker)
                .fetch_one(&owner)
                .await
                .unwrap();
            if waiting {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("password proof did not reach the actual policy lock");
    tx.commit().await.unwrap();
    let stale = posting.await.unwrap();
    assert_eq!(
        board_store::find_post(&public, &slug, stale)
            .await
            .unwrap()
            .comment_format,
        8
    );
    // Missing legacy password state denies password proof, while the saved peer still works.
    sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id=$1")
        .bind(op)
        .execute(&owner)
        .await
        .unwrap();
    for (peer, allowed) in [("192.0.2.14:6000", false), ("192.0.2.10:6003", true)] {
        let id = submit(&app, &slug, op, Some(peer), PASSWORD, 0).await;
        assert_eq!(
            board_store::find_post(&public, &slug, id)
                .await
                .unwrap()
                .comment_format,
            if allowed { 24 } else { 8 }
        );
    }
    for field in ["op_markup", "comment_format", "op_password_proof"] {
        let request = Request::post(format!("/{slug}/post"))
            .header("origin", "http://127.0.0.1:3000")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(format!(
                "resto={op}&com=owned&pwd=owned-password&{field}=true"
            )))
            .unwrap();
        assert_eq!(app.clone().oneshot(request).await.unwrap().status(), 422);
    }
    for query in [
        "UPDATE content.boards SET op_markup=true WHERE slug=$1",
        "UPDATE content.posts SET comment_format=24 WHERE board=$1",
    ] {
        assert!(
            sqlx::query(query)
                .bind(&slug)
                .execute(&public)
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn source_op_markup_uses_address_or_password_and_preserves_locked_posting_policy() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = PgPoolOptions::new()
        .max_connections(1)
        .connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut random = [0u8; 5];
    OsRng.fill_bytes(&mut random);
    let slug: String = random.iter().map(|b| format!("{b:02x}")).collect();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,op_markup) VALUES($1,'OP markup','Owned fixture',4000,200,150,100,10,true)").bind(&slug).execute(&owner).await.unwrap();
    let result = tokio::spawn(exercise(owner.clone(), public.clone(), slug.clone())).await;
    public.close().await;
    sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)").bind(&slug).execute(&owner).await.unwrap();
    for query in [
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(query)
            .bind(&slug)
            .execute(&owner)
            .await
            .unwrap();
    }
    owner.close().await;
    result.unwrap();
}
