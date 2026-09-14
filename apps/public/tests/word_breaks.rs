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

fn comment() -> String {
    format!(
        "[b]{}[/b]\n{}\n> {}\nhttps://example.org/{}\nleft{{{{w_br}}}}right <script>",
        "x".repeat(70),
        "界".repeat(35),
        "q".repeat(35),
        "a".repeat(70)
    )
}

async fn submit(app: &Router, slug: &str, parent: i64, mode: usize) -> i64 {
    let fields = [
        ("resto", parent.to_string()),
        ("sub", "Owned word breaks".into()),
        ("com", comment()),
        ("pwd", "owned-word-break-password".into()),
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
                "--owned\r\nContent-Disposition: form-data; name=\"{key}\"\r\n\r\n{value}\r\n"
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
    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/{slug}/{route}"))
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
                    "192.0.2.10:1234".parse::<SocketAddr>().unwrap(),
                ))
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
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
    let mut history = None;
    for mode in 0..8 {
        let op = submit(&app, &slug, 0, mode).await;
        let reply = submit(&app, &slug, op, mode).await;
        for id in [op, reply] {
            let saved = board_store::find_post(&public, &slug, id).await.unwrap();
            assert_eq!(saved.comment_format, 63);
            assert!(saved.comment.contains("{{w_br}}"));
            assert!(!saved.comment.contains("<wbr>"));
        }
        let html = get(&app, &format!("/{slug}/thread/{op}")).await;
        assert!(html.contains(&format!("{}<wbr>{}<wbr>", "x".repeat(35), "x".repeat(35))));
        assert!(html.contains(&format!("{}<wbr>", "界".repeat(35))));
        assert!(html.contains("left<wbr>right &#60;script&#62;"));
        assert!(!html.contains("<script>"));
        let destination = format!("https://example.org/{}", "a".repeat(70));
        assert!(html.contains(&format!("href=\"{destination}\"")));
        for router in [&app, &api] {
            let value: serde_json::Value =
                serde_json::from_str(&get(router, &format!("/{slug}/thread/{op}.json")).await)
                    .unwrap();
            for post in value["posts"].as_array().unwrap() {
                assert!(post["com"].as_str().unwrap().contains("<wbr>"));
                assert!(post.get("comment_format").is_none());
            }
        }
        let snapshot: serde_json::Value =
            serde_json::from_str(&get(&app, &format!("/_watch/{slug}/thread/{op}/posts")).await)
                .unwrap();
        assert!(
            snapshot["posts"][1]["html"]
                .as_str()
                .unwrap()
                .contains("<wbr>")
        );
        history = Some((op, html));
    }
    let (op, html) = history.unwrap();
    sqlx::query("UPDATE content.boards SET op_markup=false,comment_spoiler_cleanup=false,comment_code_spacing=false,comment_sjis_spacing=false WHERE slug=$1").bind(&slug).execute(&owner).await.unwrap();
    assert_eq!(get(&app, &format!("/{slug}/thread/{op}")).await, html);
    let id = submit(&app, &slug, op, 0).await;
    assert_eq!(
        board_store::find_post(&public, &slug, id)
            .await
            .unwrap()
            .comment_format,
        40
    );
    assert!(
        sqlx::query("UPDATE content.posts SET comment_format=8 WHERE id=$1")
            .bind(id)
            .execute(&public)
            .await
            .is_err()
    );
    let catalog = get(&app, &format!("/{slug}/catalog?teaser=on")).await;
    assert!(catalog.contains(&"x".repeat(70)));
    assert!(!catalog.contains("{{w_br}}"));
}

#[tokio::test]
async fn source_word_breaks_survive_posting_api_updater_and_policy_changes() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = PgPool::connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut random = [0u8; 5];
    OsRng.fill_bytes(&mut random);
    let slug: String = random.iter().map(|b| format!("{b:02x}")).collect();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,op_markup,comment_spoiler_cleanup,comment_code_spacing,comment_sjis_spacing) VALUES($1,'Word breaks','Owned fixture',4000,100,100,100,10,true,true,true,true)").bind(&slug).execute(&owner).await.unwrap();
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
