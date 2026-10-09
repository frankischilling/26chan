#![cfg(feature = "database-tests")]

use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
};
use rand_core::{OsRng, RngCore};
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;

async fn get(app: &Router, path: &str) -> String {
    let response = app
        .clone()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), 200, "{path}");
    String::from_utf8(
        to_bytes(response.into_body(), 8 * 1024 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}
async fn json(app: &Router, path: &str) -> Value {
    serde_json::from_str(&get(app, path).await).unwrap()
}

struct FixtureThread {
    id: i64,
    sticky: bool,
    replies: Vec<i64>,
    images: Vec<i64>,
    deleted: i64,
}

async fn insert_post(
    owner: &PgPool,
    board: &str,
    thread: i64,
    id: Option<i64>,
    deleted: bool,
) -> i64 {
    let id = match id {
        Some(id) => id,
        None => sqlx::query_scalar("SELECT nextval('content.post_number')")
            .fetch_one(owner)
            .await
            .unwrap(),
    };
    sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment,deleted) VALUES($1,$2,$3,'Anonymous','Owned preview',$4,$5)")
        .bind(id).bind(board).bind(thread).bind(format!("Owned preview >>{thread}")).bind(deleted).execute(owner).await.unwrap();
    sqlx::query("INSERT INTO post_secrets.poster_contexts(post_id,thread_id,fingerprint,epoch) VALUES($1,$2,decode(repeat('11',32),'hex'),decode(repeat('aa',32),'hex'))")
        .bind(id).bind(thread).execute(owner).await.unwrap();
    id
}
async fn attach(owner: &PgPool, post: i64, deleted: bool) {
    let asset = format!("{post:032x}");
    sqlx::query("INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at) VALUES($1,$1,$1,repeat('a',64),100,10,10,'approved',clock_timestamp())")
        .bind(&asset).execute(owner).await.unwrap();
    sqlx::query("INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler,file_deleted) VALUES($1,$2,$2,'owned.png',100,10,10,false,$3)")
        .bind(post).bind(asset).bind(deleted).execute(owner).await.unwrap();
}
fn ids(posts: &Value, native: bool) -> Vec<i64> {
    posts
        .as_array()
        .unwrap()
        .iter()
        .map(|post| {
            if native {
                post["no"].as_str().unwrap().parse().unwrap()
            } else {
                post["no"].as_i64().unwrap()
            }
        })
        .collect()
}
fn by_id(threads: &[Value], id: i64, catalog: bool) -> &Value {
    threads
        .iter()
        .find(|entry| {
            if catalog {
                entry["no"] == id
            } else {
                entry["posts"][0]["no"] == id
            }
        })
        .unwrap()
}
fn assert_omissions(op: &Value, omitted: usize, images: usize) {
    if omitted == 0 {
        assert!(op.get("omitted_posts").is_none());
        assert!(op.get("omitted_images").is_none());
    } else {
        assert_eq!(op["omitted_posts"], omitted);
        assert_eq!(op["omitted_images"], images);
    }
    assert!(op.get("unique_ips").is_none());
}

async fn exercise(owner: PgPool, public: PgPool, board: String) {
    let mut fixtures = Vec::new();
    for sticky in [false, true] {
        for count in [0, 1, 3, 5, 7] {
            let id = sqlx::query_scalar(
                "INSERT INTO content.threads(board,sticky) VALUES($1,$2) RETURNING id",
            )
            .bind(&board)
            .bind(sticky)
            .fetch_one(&owner)
            .await
            .unwrap();
            insert_post(&owner, &board, id, Some(id), false).await;
            attach(&owner, id, false).await; // OP files never count as reply images.
            let mut replies = Vec::new();
            let mut images = Vec::new();
            for index in 1..=count {
                let reply = insert_post(&owner, &board, id, None, false).await;
                replies.push(reply);
                if index % 2 == 1 {
                    attach(&owner, reply, index == 3).await;
                    if index != 3 {
                        images.push(reply);
                    }
                }
            }
            let deleted = insert_post(&owner, &board, id, None, true).await;
            attach(&owner, deleted, false).await; // Neither latest selection nor image count includes deleted posts.
            fixtures.push(FixtureThread {
                id,
                sticky,
                replies,
                images,
                deleted,
            });
        }
    }
    let (web, api) = board_public::routers(public, "http://127.0.0.1:3000".into(), false);
    for configured in [0_i32, 1, 3, 5] {
        sqlx::query("UPDATE content.boards SET replies_shown=$2 WHERE slug=$1")
            .bind(&board)
            .bind(configured)
            .execute(&owner)
            .await
            .unwrap();
        let html = get(&web, &format!("/{board}/")).await;
        let native = json(&web, &format!("/_watch/{board}/page/0")).await;
        assert_eq!(native["version"], 2);
        assert_eq!(native["replies_shown"], configured);
        assert_eq!(native["threads"].as_array().unwrap().len(), fixtures.len());
        for app in [&web, &api] {
            let index = json(app, &format!("/{board}/1.json")).await;
            let catalog = json(app, &format!("/{board}/catalog.json")).await;
            let index_threads = index["threads"].as_array().unwrap();
            let catalog_threads = catalog[0]["threads"].as_array().unwrap();
            assert_eq!(index_threads.len(), fixtures.len());
            assert_eq!(catalog_threads.len(), fixtures.len());
            for fixture in &fixtures {
                let limit = if fixture.sticky {
                    configured.min(1)
                } else {
                    configured
                } as usize;
                let shown = fixture.replies.len().min(limit);
                let suffix = &fixture.replies[fixture.replies.len() - shown..];
                let expected: Vec<_> = std::iter::once(fixture.id)
                    .chain(suffix.iter().copied())
                    .collect();
                let omitted = fixture.replies.len() - shown;
                let omitted_images = fixture
                    .images
                    .iter()
                    .filter(|id| !suffix.contains(id))
                    .count();
                let index_thread = by_id(index_threads, fixture.id, false);
                let catalog_op = by_id(catalog_threads, fixture.id, true);
                assert_eq!(ids(&index_thread["posts"], false), expected);
                assert_eq!(index_thread["posts"][0]["replies"], fixture.replies.len());
                assert_eq!(index_thread["posts"][0]["images"], fixture.images.len());
                assert_eq!(catalog_op["replies"], fixture.replies.len());
                assert_eq!(catalog_op["images"], fixture.images.len());
                assert_omissions(&index_thread["posts"][0], omitted, omitted_images);
                assert_omissions(catalog_op, omitted, omitted_images);
                if fixture.replies.is_empty() {
                    assert!(catalog_op.get("last_replies").is_none());
                } else {
                    assert_eq!(ids(&catalog_op["last_replies"], false), suffix);
                }
                let thread_id = fixture.id.to_string();
                let native_thread = native["threads"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|entry| entry["thread"].as_str() == Some(thread_id.as_str()))
                    .unwrap();
                assert_eq!(ids(&native_thread["posts"], true), expected);
                assert_eq!(native_thread["omitted"], omitted);
                assert_eq!(native_thread["images"], fixture.images.len());
                for id in std::iter::once(&fixture.id).chain(fixture.replies.iter()) {
                    assert_eq!(
                        html.contains(&format!("id=\"p{id}\"")),
                        expected.contains(id),
                        "configured={configured}, sticky={}, id={id}",
                        fixture.sticky
                    );
                }
                assert!(!html.contains(&format!("id=\"p{}\"", fixture.deleted)));
                // Board previews retain resolved full thread destinations.
                assert!(
                    index_thread["posts"][0]["com"]
                        .as_str()
                        .unwrap()
                        .contains(&format!(
                            "href=\"/{board}/thread/{}#p{}\"",
                            fixture.id, fixture.id
                        ))
                );
            }
        }
    }
    // Preview policy never changes the full/tail thread contract, including uniques.
    let fixture = fixtures
        .iter()
        .find(|fixture| !fixture.sticky && fixture.replies.len() == 7)
        .unwrap();
    for app in [&web, &api] {
        let full = json(app, &format!("/{board}/thread/{}.json", fixture.id)).await;
        assert_eq!(full["posts"].as_array().unwrap().len(), 8);
        assert_eq!(full["posts"][0]["unique_ips"], 1);
        assert!(full["posts"][0].get("omitted_posts").is_none());
        let tail = json(app, &format!("/{board}/thread/{}-tail.json", fixture.id)).await;
        assert_eq!(tail["posts"].as_array().unwrap().len(), 3);
        assert_eq!(tail["posts"][0]["unique_ips"], 1);
    }
}

#[tokio::test]
async fn source_preview_policy_unifies_html_native_index_and_catalog() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut bytes = [0_u8; 4];
    OsRng.fill_bytes(&mut bytes);
    let board = format!("pv{:08x}", u32::from_le_bytes(bytes));
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,image_limit,json_tail_size) VALUES($1,'Owned preview policy','Owned policy matrix',1000,100,50,20,20,100,2)")
        .bind(&board).execute(&owner).await.unwrap();
    let result = tokio::spawn(exercise(owner.clone(), public.clone(), board.clone())).await;
    public.close().await;
    for statement in [
        "DELETE FROM media.assets WHERE id IN (SELECT m.asset_id FROM content.post_media m JOIN content.posts p ON p.id=m.post_id WHERE p.board=$1)",
        "DELETE FROM content.post_media WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
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
