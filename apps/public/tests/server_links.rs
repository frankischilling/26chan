#![cfg(feature = "database-tests")]
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::Request,
};
use rand_core::{OsRng, RngCore};
use sqlx::PgPool;
use std::net::SocketAddr;
use tower::ServiceExt;

async fn get(app: &Router, path: &str) -> String {
    let response = app
        .clone()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), 200, "{path}");
    String::from_utf8(
        to_bytes(response.into_body(), 1_000_000)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}

async fn submit(app: &Router, board: &str, parent: i64, mode: usize, comment: &str) -> i64 {
    let fields = [
        ("resto", parent.to_string()),
        ("sub", "Owned server links".into()),
        ("com", comment.into()),
        ("pwd", "owned-server-links-password".into()),
    ];
    let (kind, body) = if mode & 1 == 0 {
        (
            "application/x-www-form-urlencoded",
            url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs(fields.iter().map(|(key, value)| (*key, value)))
                .finish(),
        )
    } else {
        let mut body = String::new();
        for (key, value) in &fields {
            body.push_str(&format!(
                "--owned-links\r\nContent-Disposition: form-data; name=\"{key}\"\r\n\r\n{value}\r\n"
            ));
        }
        body.push_str("--owned-links--\r\n");
        ("multipart/form-data; boundary=owned-links", body)
    };
    let response = app
        .clone()
        .oneshot(
            Request::post(format!(
                "/{board}/{}",
                if mode & 2 == 0 {
                    "post"
                } else {
                    "imgboard.php"
                }
            ))
            .header("origin", "http://127.0.0.1:3000")
            .header("content-type", kind)
            .header(
                "accept",
                if mode & 4 == 0 {
                    "application/json"
                } else {
                    "text/html"
                },
            )
            .extension(ConnectInfo(
                "192.0.2.14:1234".parse::<SocketAddr>().unwrap(),
            ))
            .body(Body::from(body))
            .unwrap(),
        )
        .await
        .unwrap();
    if mode & 4 == 0 {
        assert_eq!(response.status(), 200);
        let value: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap()).unwrap();
        value["pid"].as_i64().unwrap_or_else(|| panic!("{value}"))
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

fn fixture_key() -> std::sync::Arc<board_domain::poster_id::PosterIdKey> {
    use rand_core::RngCore;
    let mut bytes = [0u8; 32];
    rand_core::OsRng.fill_bytes(&mut bytes);
    let encoded: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    std::sync::Arc::new(board_domain::poster_id::PosterIdKey::parse(&encoded).unwrap())
}

async fn exercise(owner: &PgPool, public: &PgPool, board: &str) {
    // Reserve an ID without inserting a post, so this quote cannot accidentally
    // resolve against another concurrently running fixture.
    let missing: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(owner)
        .await
        .unwrap();
    let raw_missing = format!("000{missing}");
    let missing_label = format!("&gt;&gt;&gt;/po/{raw_missing}");
    let missing_quote = format!("<span class=\"deadlink\">{missing_label}</span>");
    let (app, api) = board_public::routers_with_options(
        public.clone(),
        board_public::PublicRouterOptions {
            origin: "http://127.0.0.1:3000".into(),
            production: false,
            media: None,
            limits: board_config::PublicRequestLimits::default(),
            proxy_uid: None,
            tripcode_key: None,
            poster_id_key: Some(fixture_key()),
            country_database: None,
        },
    );
    let external = "https://example.org/path";
    let initial =
        format!("{external} https://www.4chan.org/faq >>>/g/catalog >>>/g/a+b >>>/g/rules/3");
    for mode in 0..8 {
        let op = submit(&app, board, 0, mode, &initial).await;
        let reply = format!(
            "{external} https://boards.4chan.org/{board}/thread/{op} https://boards.4chan.org/{board}/fooXphp?res={op} https://boards.4chan.org/{board}/thread/{op}#p0 >>>/po/{raw_missing} >>>/po/1e2"
        );
        let id = submit(&app, board, op, mode, &reply).await;
        let saved = board_store::find_post(public, board, id).await.unwrap();
        assert_eq!(saved.comment, reply);
        assert_eq!(saved.comment_format, 104);
        let path = format!("/{board}/thread/{op}");
        let html = get(&app, &path).await;
        assert!(html.contains(&format!("href=\"#p{op}\">&gt;&gt;{op}</a>")));
        assert!(html.contains(&missing_quote));
        assert!(!html.contains(&format!("href=\"/po/post/{missing}\"")));
        assert!(!html.contains("/po/catalog#s=1e2"));
        assert!(!html.contains("href=\"/po/post/1\""));
        assert!(!html.contains("fooXphp"));
        for destination in [
            "/g/catalog",
            "/g/catalog#s=a+b",
            "/rules#g3",
            "https://www.4chan.org/faq",
        ] {
            assert!(
                html.contains(&format!("href=\"{destination}\"")),
                "{destination}"
            );
        }
        assert!(!html.contains(&format!("href=\"{external}\"")));
        for router in [&app, &api] {
            let value: serde_json::Value =
                serde_json::from_str(&get(router, &format!("{path}.json")).await).unwrap();
            let comment = value["posts"][1]["com"].as_str().unwrap();
            assert!(comment.contains(&format!("href=\"#p{op}\"")));
            assert!(!comment.contains(&format!("href=\"{external}\"")));
            assert!(!comment.contains("boards.4chan.org"));
            assert!(comment.contains(&missing_quote));
        }
        let snapshot: serde_json::Value =
            serde_json::from_str(&get(&app, &format!("/_watch/{board}/thread/{op}/posts")).await)
                .unwrap();
        assert!(
            snapshot["posts"][1]["html"]
                .as_str()
                .unwrap()
                .contains(&format!("href=\"#p{op}\""))
        );
        assert!(
            snapshot["posts"][1]["html"]
                .as_str()
                .unwrap()
                .contains(&missing_quote)
        );
        let before = html;
        sqlx::query("UPDATE content.boards SET comment_code_spacing=true,comment_sjis_spacing=true WHERE slug=$1").bind(board).execute(owner).await.unwrap();
        assert_eq!(get(&app, &path).await, before);
        sqlx::query("UPDATE content.boards SET comment_code_spacing=false,comment_sjis_spacing=false WHERE slug=$1").bind(board).execute(owner).await.unwrap();
        // Source catalog search sees static-link spelling before quote-number
        // resolution, independently of the safe HTML attributes on the page.
        let catalog = get(
            &app,
            &format!(
                "/{board}/catalog?q={}",
                url::form_urlencoded::byte_serialize(b"a+b").collect::<String>()
            ),
        )
        .await;
        assert!(catalog.contains(&format!("id=\"thread-{op}\"")));
        assert!(catalog.contains("&#38;gt;&#38;gt;&#38;gt;/g/a+b"));
    }
    let old = submit(&app, board, 0, 0, external).await;
    sqlx::query("UPDATE content.posts SET comment_format=40 WHERE id=$1")
        .bind(old)
        .execute(owner)
        .await
        .unwrap();
    assert!(
        get(&app, &format!("/{board}/thread/{old}"))
            .await
            .contains(&format!("href=\"{external}\""))
    );
    assert!(
        sqlx::query("UPDATE content.posts SET comment_format=104 WHERE id=$1")
            .bind(old)
            .execute(public)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn persisted_links_use_source_normalization_in_both_forms_routes_and_response_modes() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut nonce = [0u8; 4];
    OsRng.fill_bytes(&mut nonce);
    let board = format!("sl{:08x}", u32::from_be_bytes(nonce));
    // Retain all eight route/form OP controls on this owned board without
    // letting the unrelated actor quota preempt link-normalization checks.
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,posting_reply_seconds,posting_image_seconds,posting_thread_seconds,user_thread_limit) VALUES($1,'Owned server links','',1000,100,100,100,10,0,0,0,100)")
        .bind(&board).execute(&owner).await.unwrap();
    let result = tokio::spawn({
        let owner = owner.clone();
        let public = public.clone();
        let board = board.clone();
        async move { exercise(&owner, &public, &board).await }
    })
    .await;
    public.close().await;
    for query in [
        "DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(query)
            .bind(&board)
            .execute(&owner)
            .await
            .unwrap();
    }
    owner.close().await;
    result.unwrap();
}
