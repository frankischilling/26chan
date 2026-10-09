#![cfg(feature = "database-tests")]
#[path = "support/posting.rs"]
mod posting;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Method, Request, StatusCode},
};
use board_store::{
    NewPost,
    blotter::{self, BlotterError},
};
use sqlx::{Connection, PgConnection, PgPool};
use tower::ServiceExt;
const ORIGIN: &str = "http://127.0.0.1:3000";

async fn request(app: &Router, method: Method, path: &str) -> (StatusCode, HeaderMap, String) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("origin", ORIGIN)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = String::from_utf8(
        to_bytes(response.into_body(), 2_000_000)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    (status, headers, body)
}
fn input(timestamp: i64, content: &str) -> blotter::BlotterInput {
    blotter::read_blotter(
        serde_json::to_vec(
            &serde_json::json!({"version":1,"published_at":timestamp,"content":content}),
        )
        .unwrap()
        .as_slice(),
    )
    .unwrap()
}
async fn ready(pool: &PgPool) -> bool {
    sqlx::query_scalar(blotter::BLOTTER_READINESS_SQL)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn exercise(owner: &PgPool, public: &PgPool, slug: &str, ids: Vec<i64>, date: i64) {
    let (app, api) = posting::routers(public.clone(), slug, ORIGIN.into(), false);
    assert!(ready(public).await);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM content.published_blotter")
        .fetch_one(public)
        .await
        .unwrap();
    if count == 0 {
        let (_, _, empty) = request(&app, Method::GET, "/blotter").await;
        assert!(empty.contains("No blotter messages."));
        assert!(
            !request(&app, Method::GET, &format!("/{slug}/"))
                .await
                .2
                .contains("id=\"blotter\"")
        );
    }
    let mut connection = owner.acquire().await.unwrap();
    for (index, id) in ids[..27].iter().enumerate() {
        let text = if index == 26 {
            "Owned <script>window.owned=true</script> & <a href=\"javascript:alert(1)\">unsafe</a>\nhttps://example.org".to_string()
        } else {
            format!("Owned announcement {index}")
        };
        let text = format!("{slug}: {text}");
        assert_eq!(
            blotter::publish_blotter(&mut connection, &input(date + index as i64, &text))
                .await
                .unwrap(),
            *id
        );
    }
    let first = blotter::blotter_page(public, None).await.unwrap();
    assert_eq!(first.messages.len(), 25);
    assert_eq!(first.messages[0].id, ids[26]);
    assert_eq!(first.next_offset, Some(ids[2]));
    let (status, headers, html) = request(&app, Method::GET, "/blotter").await;
    assert_eq!(status, StatusCode::OK);
    assert!(!html.contains("<script>window.owned") && !html.contains("href=\"javascript:"));
    assert!(html.contains("&#60;script&#62;window.owned=true&#60;/script&#62;"));
    assert!(html.contains(&format!("/blotter?offset={}", ids[2])));
    let csp = headers["content-security-policy"].to_str().unwrap();
    assert!(csp.contains("script-src 'none'") && csp.contains("connect-src 'none'"));
    assert!(headers.get("set-cookie").is_none());
    assert_eq!(request(&app, Method::HEAD, "/blotter").await.2, "");
    assert_eq!(
        request(&app, Method::POST, "/blotter").await.0,
        StatusCode::METHOD_NOT_ALLOWED
    );
    assert_ne!(
        request(&api, Method::GET, "/blotter").await.0,
        StatusCode::OK
    );
    for path in [
        "/blotter?offset=0",
        "/blotter?offset=01",
        "/blotter?offset=-1",
        "/blotter?offset=9223372036854775808",
        "/blotter?offset=1&offset=2",
        "/blotter?atom",
    ] {
        assert_eq!(
            request(&app, Method::GET, path).await.0,
            StatusCode::BAD_REQUEST,
            "{path}"
        );
    }
    let post = posting::create_post(
        public,
        slug,
        0,
        &NewPost {
            name: "Anonymous".into(),
            subject: "Owned blotter".into(),
            comment: "Owned body".into(),
            deletion_hash: "owned-blotter-fixture".into(),
            sage: false,
        },
    )
    .await
    .unwrap();
    for path in [format!("/{slug}/"), format!("/{slug}/thread/{post}")] {
        let (_, _, body) = request(&app, Method::GET, &path).await;
        assert!(body.contains("id=\"blotter\""));
        for id in &ids[24..27] {
            assert!(body.contains(&format!("data-utc=\"{}\"", date + id - ids[0])));
        }
        assert!(!body.contains("Owned announcement 23"));
        assert!(!body.contains("<script>window.owned"));
    }
    assert!(
        !request(&app, Method::GET, &format!("/{slug}/catalog"))
            .await
            .2
            .contains("id=\"blotter\"")
    );
    sqlx::query("UPDATE content.threads SET closed=true WHERE board=$1 AND id=$2")
        .bind(slug)
        .bind(post)
        .execute(owner)
        .await
        .unwrap();
    assert!(
        !request(&app, Method::GET, &format!("/{slug}/thread/{post}"))
            .await
            .2
            .contains("id=\"blotter\"")
    );
    sqlx::query("UPDATE content.boards SET show_blotter=false WHERE slug=$1")
        .bind(slug)
        .execute(owner)
        .await
        .unwrap();
    assert!(
        !request(&app, Method::GET, &format!("/{slug}/"))
            .await
            .2
            .contains("id=\"blotter\"")
    );
    sqlx::query("UPDATE content.boards SET show_blotter=true,staff_only=true WHERE slug=$1")
        .bind(slug)
        .execute(owner)
        .await
        .unwrap();
    assert_eq!(
        request(&app, Method::GET, &format!("/{slug}/")).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(&app, Method::GET, &format!("/{slug}/thread/{post}"))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert!(
        !sqlx::query_scalar::<_, bool>("SELECT show_blotter FROM content.boards WHERE slug='j'")
            .fetch_one(owner)
            .await
            .unwrap()
    );
    assert!(matches!(
        blotter::publish_blotter(&mut connection, &input(date + 26, "Stale")).await,
        Err(BlotterError::Stale)
    ));
    assert_eq!(
        blotter::publish_blotter(
            &mut connection,
            &input(date + 27, &format!("{slug}: Owned newly published"))
        )
        .await
        .unwrap(),
        ids[27]
    );
    blotter::retract_blotter(&mut connection, ids[1])
        .await
        .unwrap();
    let second = blotter::blotter_page(public, first.next_offset)
        .await
        .unwrap();
    assert!(second.messages.iter().any(|row| row.id == ids[0]));
    assert!(!second.messages.iter().any(|row| row.id >= ids[1]));
    assert!(
        !request(&app, Method::GET, &format!("/blotter?offset={}", ids[2]))
            .await
            .2
            .contains("Owned announcement 1</td>")
    );
    blotter::retract_blotter(&mut connection, ids[27])
        .await
        .unwrap();
    assert!(matches!(
        blotter::publish_blotter(
            &mut connection,
            &input(date + 27, "Cannot reuse retracted timestamp")
        )
        .await,
        Err(BlotterError::Stale)
    ));
    assert!(matches!(
        blotter::retract_blotter(&mut connection, 10001).await,
        Err(BlotterError::NotFound)
    ));
    let mut runtime = PgConnection::connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    assert!(matches!(
        blotter::publish_blotter(&mut runtime, &input(date + 28, "Forbidden")).await,
        Err(BlotterError::Role)
    ));
    assert!(matches!(
        blotter::retract_blotter(&mut runtime, ids[0]).await,
        Err(BlotterError::Role)
    ));
    for statement in [
        "SELECT * FROM blotter_private.messages",
        "INSERT INTO content.published_blotter(id,published_at,content) VALUES(9999,now(),'Forbidden')",
        "UPDATE content.published_blotter SET content='Forbidden' WHERE false",
        "DELETE FROM content.published_blotter WHERE false",
        "INSERT INTO blotter_private.messages(id,published_at,content) VALUES(9999,now(),'Forbidden')",
        "UPDATE blotter_private.messages SET content='Forbidden' WHERE false",
        "DELETE FROM blotter_private.messages WHERE false",
        "UPDATE content.boards SET show_blotter=false WHERE false",
    ] {
        assert!(
            sqlx::query(statement).execute(public).await.is_err(),
            "{statement}"
        );
    }
    drop(connection);
    for statement in [
        "ALTER TABLE blotter_private.messages DROP CONSTRAINT messages_published_at_check",
        "ALTER TABLE blotter_private.messages ALTER COLUMN published DROP NOT NULL",
        "GRANT UPDATE ON content.published_blotter TO board_public",
        "GRANT SELECT ON blotter_private.messages TO board_staff",
        "CREATE OR REPLACE VIEW content.published_blotter WITH (security_barrier=true) AS SELECT id,published_at,content FROM blotter_private.messages",
    ] {
        let mut tx = owner.begin().await.unwrap();
        sqlx::query(statement).execute(&mut *tx).await.unwrap();
        assert!(
            !sqlx::query_scalar::<_, bool>(blotter::BLOTTER_READINESS_SQL)
                .fetch_one(&mut *tx)
                .await
                .unwrap()
        );
        tx.rollback().await.unwrap();
    }
    for statement in [
        "INSERT INTO blotter_private.messages VALUES(10001,now(),'No',true)",
        "INSERT INTO blotter_private.messages VALUES(9999,now(),repeat('x',8193),true)",
        "INSERT INTO blotter_private.messages VALUES(9999,now(),'',true)",
        "INSERT INTO blotter_private.messages VALUES(9999,'infinity','No',true)",
        "INSERT INTO blotter_private.messages VALUES(9999,'1969-01-01','No',true)",
    ] {
        assert!(
            sqlx::query(statement).execute(owner).await.is_err(),
            "{statement}"
        );
    }
    assert!(ready(public).await);
}

#[tokio::test]
async fn local_blotter_is_ordered_bounded_escaped_snapshot_read_only_and_operator_maintained() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut lock = owner.acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock(2250118)")
        .execute(&mut *lock)
        .await
        .unwrap();
    let (base,date):(i64,i64)=sqlx::query_as("SELECT coalesce(max(id),0)+1,greatest(coalesce(extract(epoch FROM max(published_at))::bigint,0)+1,extract(epoch FROM now())::bigint) FROM blotter_private.messages").fetch_one(&owner).await.unwrap();
    assert!(
        base + 27 <= 10000,
        "Owned fixture needs 28 remaining announcement slots"
    );
    let ids: Vec<i64> = (base..base + 28).collect();
    let slug: String =
        sqlx::query_scalar("SELECT 'bl'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,posting_reply_seconds,posting_image_seconds,posting_thread_seconds) VALUES($1,'Owned blotter','Synthetic fixture',2000,100,100,100,10,0,0,0)").bind(&slug).execute(&owner).await.unwrap();
    let result = tokio::spawn({
        let owner = owner.clone();
        let public = public.clone();
        let slug = slug.clone();
        let ids = ids.clone();
        async move { exercise(&owner, &public, &slug, ids, date).await }
    })
    .await;
    let cleanup_posting = tokio::spawn({
        let owner = owner.clone();
        let slug = slug.clone();
        async move { posting::cleanup_posting(&owner, &slug).await }
    })
    .await;
    let mut cleanup_ok = cleanup_posting.is_ok();
    for statement in [
        "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        cleanup_ok &= sqlx::query(statement)
            .bind(&slug)
            .execute(&owner)
            .await
            .is_ok();
    }
    cleanup_ok &= sqlx::query(
        "DELETE FROM blotter_private.messages WHERE id=ANY($1) AND left(content,length($2))=$2",
    )
    .bind(&ids)
    .bind(format!("{slug}: "))
    .execute(&owner)
    .await
    .is_ok();
    cleanup_ok &= sqlx::query("SELECT pg_advisory_unlock(2250118)")
        .execute(&mut *lock)
        .await
        .is_ok();
    drop(lock);
    public.close().await;
    owner.close().await;
    result.unwrap();
    assert!(cleanup_ok, "Owned fixture cleanup failed");
}
