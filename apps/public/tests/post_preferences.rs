#![cfg(feature = "database-tests")]

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, Response},
};
use serde_json::Value;
use tower::ServiceExt;

fn fixture_key() -> std::sync::Arc<board_domain::poster_id::PosterIdKey> {
    use rand_core::RngCore;
    let mut bytes = [0u8; 32];
    rand_core::OsRng.fill_bytes(&mut bytes);
    let encoded: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    std::sync::Arc::new(board_domain::poster_id::PosterIdKey::parse(&encoded).unwrap())
}

const ORIGIN: &str = "https://boards.example.com";

async fn submit(
    app: &Router,
    board: &str,
    thread: i64,
    name: &str,
    options: &str,
    index: usize,
) -> Response<Body> {
    let fields = [
        ("mode", "regist".to_string()),
        ("resto", thread.to_string()),
        ("name", name.to_string()),
        ("email", options.to_string()),
        ("com", "Owned preference integration".into()),
        ("sub", "Owned preferences".into()),
        ("pwd", "owned-preferences-password".into()),
    ];
    let (kind, body) = if index & 1 == 0 {
        (
            "application/x-www-form-urlencoded",
            url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs(fields.iter().map(|(name, value)| (*name, value)))
                .finish(),
        )
    } else {
        let mut body = String::new();
        for (name, value) in fields {
            body.push_str(&format!("--owned-preferences\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"));
        }
        body.push_str("--owned-preferences--\r\n");
        ("multipart/form-data; boundary=owned-preferences", body)
    };
    let route = if index & 2 == 0 {
        "post"
    } else {
        "imgboard.php"
    };
    app.clone()
        .oneshot(
            Request::post(format!("/{board}/{route}"))
                .extension(axum::extract::ConnectInfo(std::net::SocketAddr::from((
                    [127, 0, 0, 1],
                    54001,
                ))))
                .header("origin", ORIGIN)
                .header("accept", "application/json")
                .header("content-type", kind)
                .header("cookie", "4chan_name=Previous; options=Previous")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap()
}

fn cookies(response: &Response<Body>) -> Vec<String> {
    response
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|value| value.to_str().unwrap().to_string())
        .collect()
}

async fn result(response: Response<Body>) -> Value {
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 8192).await.unwrap();
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&bytes));
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn successful_posting_remembers_only_public_preferences_and_errors_do_not_change_them() {
    let owner = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT substr(replace(gen_random_uuid()::text,'-',''),1,10)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,posting_reply_seconds,posting_image_seconds,posting_thread_seconds) VALUES($1,'Posting preferences','Owned fixture',1000,100,100,100,10,0,0,0)").bind(&board).execute(&owner).await.unwrap();
    let app = board_public::routers_with_options(
        public.clone(),
        board_public::PublicRouterOptions {
            origin: ORIGIN.into(),
            production: true,
            media: None,
            limits: board_config::PublicRequestLimits::default(),
            proxy_uid: None,
            tripcode_key: None,
            poster_id_key: Some(fixture_key()),
            country_database: None,
        },
    )
    .0;
    let mut thread = 0;
    for index in 0..4 {
        let response = submit(
            &app,
            &board,
            thread,
            " 名 + <owned>#password",
            "sageNONOKO",
            index,
        )
        .await;
        let saved_cookies = cookies(&response);
        let value = result(response).await;
        assert!(value.get("error").is_none(), "{value}");
        assert_eq!(
            saved_cookies
                .iter()
                .filter(|cookie| !cookie.starts_with("__Host-board-anon="))
                .map(String::as_str)
                .collect::<Vec<_>>(),
            [
                "4chan_name=%E5%90%8D%20%2B%20%3Cowned%3E; Path=/; Max-Age=604800; SameSite=Strict; Secure",
                "options=sageNONOKO; Path=/; Max-Age=604800; SameSite=Strict; Secure",
            ]
        );
        let anonymous = saved_cookies
            .iter()
            .filter(|cookie| cookie.starts_with("__Host-board-anon="))
            .collect::<Vec<_>>();
        assert_eq!(anonymous.len(), 1);
        assert!(
            anonymous[0].contains("Path=/; Max-Age=31536000; HttpOnly; SameSite=Strict; Secure")
        );
        assert!(!anonymous[0].contains("Domain="));
        let id = value["pid"].as_i64().unwrap();
        if thread == 0 {
            thread = id;
        }
        let saved = board_store::find_post(&public, &board, id).await.unwrap();
        assert_eq!(saved.name, "名 + <owned>");
        assert_eq!(saved.trip.as_deref(), Some("!ozOtJW9BFA"));
    }
    let before = board_store::thread(&public, &board, thread).await.unwrap();
    let denied = submit(
        &app,
        &board,
        thread,
        "User##owned-private-secret",
        "sage",
        2,
    )
    .await;
    assert!(cookies(&denied).is_empty());
    assert_eq!(
        result(denied).await["error"],
        "Secure tripcodes are unavailable."
    );
    assert_eq!(
        board_store::thread(&public, &board, thread)
            .await
            .unwrap()
            .reply_count,
        before.reply_count
    );
    let cleared = submit(&app, &board, thread, "#password", "", 1).await;
    assert_eq!(
        cookies(&cleared)
            .into_iter()
            .filter(|cookie| !cookie.starts_with("__Host-board-anon="))
            .collect::<Vec<_>>(),
        [
            "4chan_name=; Path=/; Max-Age=0; SameSite=Strict; Secure",
            "options=; Path=/; Max-Age=0; SameSite=Strict; Secure",
        ]
    );
    assert!(result(cleared).await.get("error").is_none());
    let capcode = submit(
        &app,
        &board,
        thread,
        "User#password",
        "sagecapcode_private-auth-attempt",
        3,
    )
    .await;
    assert!(
        cookies(&capcode)
            .iter()
            .all(|value| !value.contains("private-auth-attempt"))
    );
    let id = result(capcode).await["pid"].as_i64().unwrap();
    assert_eq!(
        board_store::find_post(&public, &board, id)
            .await
            .unwrap()
            .name,
        "Anonymous"
    );
    sqlx::query("UPDATE content.boards SET forced_anon=true WHERE slug=$1")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    let anonymous = submit(
        &app,
        &board,
        thread,
        "User##owned-private-secret",
        "sage",
        3,
    )
    .await;
    assert_eq!(
        cookies(&anonymous)
            .into_iter()
            .filter(|cookie| !cookie.starts_with("__Host-board-anon="))
            .collect::<Vec<_>>(),
        ["options=sage; Path=/; Max-Age=604800; SameSite=Strict; Secure"]
    );
    let id = result(anonymous).await["pid"].as_i64().unwrap();
    let saved = board_store::find_post(&public, &board, id).await.unwrap();
    assert_eq!(saved.name, "Anonymous");
    assert_eq!(saved.trip, None);
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
    public.close().await;
    owner.close().await;
}
