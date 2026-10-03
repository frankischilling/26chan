#![cfg(feature = "database-tests")]

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Method, Request, StatusCode},
};
use rand_core::{OsRng, RngCore};
use roxmltree::{Document, Node};
use sqlx::PgPool;
use tower::ServiceExt;

struct Feed {
    status: StatusCode,
    headers: HeaderMap,
    body: String,
}

async fn request(app: &Router, board: &str, method: Method, etag: Option<&str>) -> Feed {
    let mut request = Request::builder()
        .method(method)
        .uri(format!("/{board}/index.rss"));
    if let Some(etag) = etag {
        request = request.header("if-none-match", etag);
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = String::from_utf8(
        to_bytes(response.into_body(), 1_000_000)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    Feed {
        status,
        headers,
        body,
    }
}

fn child<'a>(node: Node<'a, 'a>, name: &str) -> &'a str {
    node.children()
        .find(|node| node.has_tag_name(name))
        .and_then(|node| node.text())
        .unwrap_or_default()
}

fn numbers(feed: &str) -> Vec<i64> {
    Document::parse(feed)
        .unwrap()
        .descendants()
        .filter(|node| node.has_tag_name("item"))
        .map(|node| {
            child(node, "guid")
                .rsplit('/')
                .next()
                .unwrap()
                .parse()
                .unwrap()
        })
        .collect()
}

async fn insert_op(owner: &PgPool, board: &str, subject: &str, comment: &str) -> i64 {
    let mut transaction = owner.begin().await.unwrap();
    let id: i64 = sqlx::query_scalar("INSERT INTO content.threads(board) VALUES($1) RETURNING id")
        .bind(board)
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment,comment_format,created_at) VALUES($1,$2,$1,'Named <poster> & friend',$3,$4,104,'2024-11-03T06:30:00Z')")
        .bind(id).bind(board).bind(subject).bind(comment)
        .execute(&mut *transaction).await.unwrap();
    transaction.commit().await.unwrap();
    id
}

async fn exercise(owner: &PgPool, public: &PgPool, board: &str) {
    let app = board_public::router(public.clone(), "http://127.0.0.1:3000".into(), false);
    let empty = request(&app, board, Method::GET, None).await;
    assert_eq!(empty.status, StatusCode::OK, "{}", empty.body);
    assert_eq!(
        empty.headers["content-type"],
        "application/rss+xml; charset=utf-8"
    );
    assert_eq!(
        empty.headers["cache-control"],
        "public, max-age=0, must-revalidate"
    );
    assert!(numbers(&empty.body).is_empty());
    let document = Document::parse(&empty.body).unwrap();
    assert!(document.root_element().has_tag_name("rss"));
    assert_eq!(document.root_element().attribute("version"), Some("2.0"));
    let channel = document
        .descendants()
        .find(|node| node.has_tag_name("channel"))
        .unwrap();
    assert_eq!(
        child(channel, "title"),
        format!("/{board}/ - Owned <RSS> & board")
    );
    let self_link = channel
        .children()
        .find(|node| node.has_tag_name(("http://www.w3.org/2005/Atom", "link")))
        .unwrap();
    assert_eq!(
        self_link.attribute("href"),
        Some(format!("http://127.0.0.1:3000/{board}/index.rss").as_str())
    );
    assert_eq!(self_link.attribute("rel"), Some("self"));

    let mut ids = Vec::new();
    for index in 0..25 {
        ids.push(
            insert_op(
                owner,
                board,
                &format!("Owned thread {index}"),
                "First visible sentence. Later sentence.",
            )
            .await,
        );
    }
    sqlx::query("UPDATE content.threads SET sticky=true,bumped_at=clock_timestamp()+interval '1 year' WHERE id=$1")
        .bind(ids[0]).execute(owner).await.unwrap();
    sqlx::query("UPDATE content.posts SET deleted=true WHERE id=$1")
        .bind(ids[23])
        .execute(owner)
        .await
        .unwrap();
    sqlx::query("UPDATE content.threads SET deleted=true WHERE id=$1")
        .bind(ids[22])
        .execute(owner)
        .await
        .unwrap();
    sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 day' WHERE id=$1")
        .bind(ids[21]).execute(owner).await.unwrap();
    sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp()-interval '2 days',archive_expires_at=clock_timestamp()-interval '1 day' WHERE id=$1")
        .bind(ids[20]).execute(owner).await.unwrap();
    sqlx::query("INSERT INTO content.posts(board,thread_id,name,subject,comment) VALUES($1,$2,'Anonymous','Newest reply','Replies do not enter the feed')")
        .bind(board).bind(ids[24]).execute(owner).await.unwrap();

    let current = request(&app, board, Method::GET, None).await;
    assert_eq!(current.status, StatusCode::OK, "{}", current.body);
    let expected = std::iter::once(ids[24])
        .chain(ids[1..20].iter().rev().copied())
        .collect::<Vec<_>>();
    assert_eq!(numbers(&current.body), expected);
    assert_ne!(current.headers["etag"], empty.headers["etag"]);
    let etag = current.headers["etag"].to_str().unwrap();
    let cached = request(&app, board, Method::GET, Some(etag)).await;
    assert_eq!(cached.status, StatusCode::NOT_MODIFIED);
    assert!(cached.body.is_empty());
    let head = request(&app, board, Method::HEAD, None).await;
    assert_eq!(head.status, StatusCode::OK);
    assert_eq!(head.headers["etag"], current.headers["etag"]);
    assert!(head.body.is_empty());

    sqlx::query("UPDATE content.posts SET subject='<script> & owned ]]>',comment='Safe <script>text</script> & words',trip='!abcdefghij' WHERE id=$1")
        .bind(ids[24]).execute(owner).await.unwrap();
    let changed = request(&app, board, Method::GET, Some(etag)).await;
    assert_eq!(changed.status, StatusCode::OK);
    assert_ne!(changed.headers["etag"], current.headers["etag"]);
    let document = Document::parse(&changed.body).unwrap();
    let item = document
        .descendants()
        .find(|node| node.has_tag_name("item"))
        .unwrap();
    assert_eq!(child(item, "title"), "<script> & owned ]]>");
    assert_eq!(child(item, "pubDate"), "Sun, 03 Nov 2024 01:30:00 EST");
    assert_eq!(
        child(item, "link"),
        format!(
            "http://127.0.0.1:3000/{board}/thread/{}#{}",
            ids[24], ids[24]
        )
    );
    let creator = item
        .children()
        .find(|node| node.has_tag_name(("http://purl.org/dc/elements/1.1/", "creator")))
        .unwrap();
    assert_eq!(creator.text(), Some("Named <poster> & friend !abcdefghij"));
    let description = child(item, "description");
    assert!(!description.contains("<script>"));
    assert!(description.contains("script"));
    assert!(
        !description.contains("<img"),
        "An unattached post must not link a nonexistent file"
    );

    sqlx::query("UPDATE content.posts SET subject='',comment='short. A longer summary. Later words.' WHERE id=$1")
        .bind(ids[24]).execute(owner).await.unwrap();
    let summarized = request(&app, board, Method::GET, None).await;
    let document = Document::parse(&summarized.body).unwrap();
    let item = document
        .descendants()
        .find(|node| node.has_tag_name("item"))
        .unwrap();
    assert_eq!(child(item, "title"), " A longer summary");
    sqlx::query("UPDATE content.posts SET comment='[spoilerx] owned hidden text' WHERE id=$1")
        .bind(ids[24])
        .execute(owner)
        .await
        .unwrap();
    let spoiler = request(&app, board, Method::GET, None).await;
    let document = Document::parse(&spoiler.body).unwrap();
    let item = document
        .descendants()
        .find(|node| node.has_tag_name("item"))
        .unwrap();
    assert_eq!(child(item, "description").trim(), "(Spoilers)");

    sqlx::query("UPDATE content.boards SET forced_anon=true WHERE slug=$1")
        .bind(board)
        .execute(owner)
        .await
        .unwrap();
    sqlx::query("UPDATE content.posts SET comment='short' WHERE id=$1")
        .bind(ids[24])
        .execute(owner)
        .await
        .unwrap();
    let anonymous = request(&app, board, Method::GET, None).await;
    let document = Document::parse(&anonymous.body).unwrap();
    assert!(
        !document
            .descendants()
            .any(|node| node.has_tag_name(("http://purl.org/dc/elements/1.1/", "creator")))
    );
    let item = document
        .descendants()
        .find(|node| node.has_tag_name("item"))
        .unwrap();
    assert_eq!(child(item, "title"), format!("No. {}", ids[24]));

    sqlx::query("UPDATE content.posts SET deleted=true WHERE id=$1")
        .bind(ids[24])
        .execute(owner)
        .await
        .unwrap();
    let deleted = request(
        &app,
        board,
        Method::GET,
        Some(anonymous.headers["etag"].to_str().unwrap()),
    )
    .await;
    assert_eq!(deleted.status, StatusCode::OK);
    assert_eq!(
        numbers(&deleted.body),
        ids[..20].iter().rev().copied().collect::<Vec<_>>()
    );
    sqlx::query("UPDATE content.boards SET rss_enabled=false WHERE slug=$1")
        .bind(board)
        .execute(owner)
        .await
        .unwrap();
    assert_eq!(
        request(&app, board, Method::GET, Some(etag)).await.status,
        StatusCode::NOT_FOUND
    );
    sqlx::query("UPDATE content.boards SET rss_enabled=true,staff_only=true WHERE slug=$1")
        .bind(board)
        .execute(owner)
        .await
        .unwrap();
    assert_eq!(
        request(&app, board, Method::GET, None).await.status,
        StatusCode::NOT_FOUND
    );
    assert!(
        sqlx::query("UPDATE content.boards SET rss_enabled=true WHERE slug=$1")
            .bind(board)
            .execute(public)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn feeds_preserve_source_selection_xml_visibility_and_cache_invalidation() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut nonce = [0u8; 4];
    OsRng.fill_bytes(&mut nonce);
    let board = format!("rs{:08x}", u32::from_be_bytes(nonce));
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Owned <RSS> & board','',2000,100,100,100,10)")
        .bind(&board).execute(&owner).await.unwrap();
    let result = tokio::spawn({
        let owner = owner.clone();
        let public = public.clone();
        let board = board.clone();
        async move { exercise(&owner, &public, &board).await }
    })
    .await;
    public.close().await;
    for statement in [
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(statement)
            .bind(&board)
            .execute(&owner)
            .await
            .unwrap();
    }
    owner.close().await;
    result.unwrap();
}
