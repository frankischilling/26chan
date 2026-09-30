#![cfg(feature = "database-tests")]
use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
};
use serde_json::Value;
use std::{net::SocketAddr, sync::Arc};
use tower::ServiceExt;

fn options(database: bool) -> board_public::PublicRouterOptions {
    board_public::PublicRouterOptions {
        origin: "https://boards.example.com".into(),
        production: true,
        media: None,
        limits: board_config::PublicRequestLimits::default(),
        proxy_uid: None,
        poster_id_key: None,
        tripcode_key: None,
        country_database: database.then(|| {
            Arc::new(
                board_domain::country::CountryDatabase::from_bytes(
                    include_bytes!(
                        "../../../crates/domain/tests/fixtures/GeoIP2-Country-Test.mmdb"
                    )
                    .to_vec(),
                )
                .unwrap(),
            )
        }),
    }
}
async fn post(
    app: &Router,
    board: &str,
    thread: i64,
    peer: Option<&str>,
    extra: &str,
    multipart: bool,
) -> Value {
    let fields = format!("resto={thread}&sub=Owned&com=Owned+flag&pwd=owned-flag-password{extra}");
    let (content_type, body) = if multipart {
        let mut body = String::new();
        for field in fields.split('&') {
            let (name, value) = field.split_once('=').unwrap();
            body.push_str(&format!(
                "--ownedflag\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            ));
        }
        body.push_str("--ownedflag--\r\n");
        ("multipart/form-data; boundary=ownedflag", body)
    } else {
        ("application/x-www-form-urlencoded", fields)
    };
    let mut request = Request::post(format!("/{board}/post"))
        .header("origin", "https://boards.example.com")
        .header("accept", "application/json")
        .header("content-type", content_type)
        .header("cf-ipcountry", "US")
        .header("x-forwarded-for", "81.2.69.142")
        .header("x-board-client-ip", "81.2.69.142")
        .body(Body::from(body))
        .unwrap();
    if let Some(peer) = peer {
        request.extensions_mut().insert(axum::extract::ConnectInfo(
            peer.parse::<SocketAddr>().unwrap(),
        ));
    }
    let response = app.clone().oneshot(request).await.unwrap();
    serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap()).unwrap()
}
async fn get(app: &Router, path: &str) -> String {
    let response = app
        .clone()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), 200, "{path}");
    String::from_utf8(
        to_bytes(response.into_body(), 4_194_304)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}
async fn json(app: &Router, path: &str) -> Value {
    serde_json::from_str(&get(app, path).await).unwrap()
}

#[tokio::test]
async fn flags_are_persisted_from_verified_peers_and_locked_board_choices() {
    let owner = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT substr(replace(gen_random_uuid()::text,'-',''),1,10)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,country_flags,board_flags,json_tail_size) VALUES($1,'Owned flags','Synthetic fixture',1000,100,100,100,10,true,ARRAY['AC','UN'],1)").bind(&board).execute(&owner).await.unwrap();
    let outcome = tokio::spawn(exercise(owner.clone(), public.clone(), board.clone())).await;
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
    outcome.unwrap();
}
async fn exercise(owner: sqlx::PgPool, public: sqlx::PgPool, board: String) {
    let (app, api) = board_public::routers_with_options(public.clone(), options(true));
    let no_database = board_public::routers_with_options(public.clone(), options(false)).0;
    assert_eq!(
        post(&no_database, &board, 0, Some("81.2.69.142:9000"), "", false).await["error"],
        "Country flags are unavailable."
    );
    assert_eq!(
        post(&app, &board, 0, None, "", false).await["error"],
        "Posting transport identity is unavailable."
    );
    assert_eq!(
        post(&app, &board, 0, Some("81.2.69.142:9000"), "&flag=EU", false).await["error"],
        "Invalid board flag."
    );
    let thread = post(&app, &board, 0, Some("81.2.69.142:9000"), "&flag=0", false).await["pid"]
        .as_i64()
        .unwrap();
    let mapped = post(
        &app,
        &board,
        thread,
        Some("[::ffff:81.2.69.142]:9000"),
        "",
        false,
    )
    .await["pid"]
        .as_i64()
        .unwrap();
    let japan = post(&app, &board, thread, Some("[2001:218::]:9000"), "", true).await["pid"]
        .as_i64()
        .unwrap();
    let unknown = post(&app, &board, thread, Some("192.0.2.10:9000"), "", false).await["pid"]
        .as_i64()
        .unwrap();
    let custom = post(
        &app,
        &board,
        thread,
        Some("81.2.69.142:9000"),
        "&flag=AC",
        true,
    )
    .await["pid"]
        .as_i64()
        .unwrap();
    for router in [&app, &api] {
        let full = json(router, &format!("/{board}/thread/{thread}.json")).await;
        for (id, code, name) in [
            (thread, "GB", "United Kingdom"),
            (mapped, "GB", "United Kingdom"),
            (japan, "JP", "Japan"),
            (unknown, "XX", "Unknown"),
        ] {
            let p = full["posts"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["no"] == id)
                .unwrap();
            assert_eq!(p["country"], code);
            assert_eq!(p["country_name"], name);
            assert!(p.get("board_flag").is_none());
        }
        let selected = full["posts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["no"] == custom)
            .unwrap();
        assert_eq!(selected["board_flag"], "AC");
        assert_eq!(selected["flag_name"], "Anarcho-Capitalist");
        assert!(selected.get("country").is_none() && selected.get("country_name").is_none());
        let tail = json(router, &format!("/{board}/thread/{thread}-tail.json")).await;
        assert!(tail["posts"][0].get("country").is_none());
        assert_eq!(tail["posts"][1]["board_flag"], "AC");
        let boards = json(router, "/boards.json").await;
        let enabled = boards["boards"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["board"] == board)
            .unwrap();
        assert_eq!(enabled["country_flags"], 1);
        assert_eq!(
            enabled["board_flags"],
            serde_json::json!({"AC":"Anarcho-Capitalist","UN":"United Nations"})
        );
        let index = json(router, &format!("/{board}/1.json")).await;
        assert_eq!(index["threads"][0]["posts"][0]["country"], "GB");
        let catalog = json(router, &format!("/{board}/catalog.json")).await;
        assert_eq!(catalog[0]["threads"][0]["country"], "GB");
    }
    let html = get(&app, &format!("/{board}/thread/{thread}")).await;
    assert!(html.contains("title=\"United Kingdom\" class=\"flag flag-gb\""));
    assert!(html.contains("title=\"Anarcho-Capitalist\" class=\"bfl bfl-ac\""));
    assert!(
        html.contains("value=\"AC\"")
            && html.contains("value=\"UN\"")
            && !html.contains("value=\"EU\"")
    );
    assert!(!html.contains("81.2.69.142") && !html.contains("2001:218"));
    let snapshot = json(&app, &format!("/_watch/{board}/thread/{thread}/posts")).await;
    assert!(
        snapshot["posts"][0]["html"]
            .as_str()
            .unwrap()
            .contains("flag flag-gb")
    );
    let preview = json(&app, &format!("/_watch/{board}/post/{custom}")).await;
    assert!(
        preview["post"]["html"]
            .as_str()
            .unwrap()
            .contains("bfl bfl-ac")
    );
    #[cfg(feature = "browser-tests")]
    browser(&public, &board).await;
    for extra in [
        "&country=US",
        "&country_name=Forged",
        "&flag_name=Forged",
        "&board_flag=AC",
    ] {
        let result = post(&app, &board, thread, Some("81.2.69.142:9000"), extra, false).await;
        assert!(result.get("pid").is_none());
    }
    assert_eq!(
        post(
            &app,
            &board,
            thread,
            Some("81.2.69.142:9000"),
            "&flag=ac",
            false
        )
        .await["error"],
        "Invalid board flag."
    );
    let denied = sqlx::query("UPDATE content.posts SET country='US' WHERE id=$1")
        .bind(thread)
        .execute(&public)
        .await
        .unwrap_err();
    assert_eq!(
        denied.as_database_error().unwrap().code().as_deref(),
        Some("42501")
    );
    sqlx::query("UPDATE content.boards SET country_flags=false,board_flags='{}' WHERE slug=$1")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    let disabled = post(
        &no_database,
        &board,
        thread,
        Some("81.2.69.142:9000"),
        "",
        false,
    )
    .await["pid"]
        .as_i64()
        .unwrap();
    let saved = board_store::find_post(&public, &board, disabled)
        .await
        .unwrap();
    assert!(saved.country.is_none() && saved.board_flag.is_none());
    let original = board_store::find_post(&public, &board, thread)
        .await
        .unwrap();
    assert_eq!(original.country.as_deref(), Some("GB"));
    assert_eq!(
        post(
            &app,
            &board,
            thread,
            Some("81.2.69.142:9000"),
            "&flag=AC",
            false
        )
        .await["error"],
        "Invalid board flag."
    );
    let boards = json(&api, "/boards.json").await;
    let disabled = boards["boards"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["board"] == board)
        .unwrap();
    assert!(disabled.get("country_flags").is_none() && disabled.get("board_flags").is_none());
}

#[cfg(feature = "browser-tests")]
async fn browser(public: &sqlx::PgPool, board: &str) {
    use std::time::Duration;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let mut settings = options(true);
    settings.origin = origin.clone();
    settings.production = false;
    let app = board_public::routers_with_options(public.clone(), settings).0;
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let serving = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _ = stopped.await;
        })
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
    for key in [
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
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let outcome = tokio::time::timeout(
        Duration::from_secs(90),
        command
            .current_dir(root)
            .arg("tests/browser/post-flags.mjs")
            .arg(origin)
            .arg(board)
            .kill_on_drop(true)
            .output(),
    )
    .await;
    let _ = stop.send(());
    serving.await.unwrap();
    let output = outcome.unwrap().unwrap();
    assert!(
        output.status.success(),
        "Flag browser failed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
