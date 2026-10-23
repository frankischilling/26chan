#![cfg(feature = "database-tests")]
//! Actual restricted-role routes compared with the executed source generator.
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;

async fn directory(router: &Router) -> Vec<Value> {
    let response = router
        .clone()
        .oneshot(Request::get("/boards.json").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 4_194_304).await.unwrap();
    serde_json::from_slice::<Value>(&body).unwrap()["boards"]
        .as_array()
        .unwrap()
        .clone()
}

#[tokio::test]
async fn imported_directory_matches_source_with_and_without_media_configuration() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let reference: Value = serde_json::from_str(include_str!(
        "../../../fixtures/json-directory-reference.json"
    ))
    .unwrap();
    let cases = reference["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 82);
    let visibility: Vec<(String, bool, bool)> =
        sqlx::query_as("SELECT slug,json_enabled,staff_only FROM content.boards")
            .fetch_all(&owner)
            .await
            .unwrap();
    let mut mismatches = Vec::new();
    for media in [
        None,
        Some(
            board_config::PublicMediaSettings::development(
                "127.0.0.1:1",
                &"a".repeat(64),
                "http://localhost:3002",
            )
            .unwrap(),
        ),
    ] {
        let configured = media.is_some();
        let (html, api) = board_public::routers_with_media(
            public.clone(),
            "http://127.0.0.1:3000".into(),
            false,
            media,
        );
        for (listener, router) in [("HTML", html), ("API", api)] {
            let rows = directory(&router).await;
            let slugs: Vec<_> = rows.iter().map(|r| r["board"].as_str().unwrap()).collect();
            assert!(slugs.windows(2).all(|pair| pair[0] < pair[1]));
            for case in cases {
                let slug = case["board"].as_str().unwrap();
                let (_, json, staff) = visibility.iter().find(|row| row.0 == slug).unwrap();
                let actual = rows.iter().find(|row| row["board"] == slug);
                if !json || *staff {
                    assert!(actual.is_none(), "private/disabled {slug}");
                    continue;
                }
                let mut actual = actual.expect("public source board missing").clone();
                let object = actual.as_object_mut().unwrap();
                object.remove("board");
                object.remove("title");
                if actual != case["expected"] {
                    mismatches.push(format!(
                        "{slug} {listener} media={configured}: actual={actual}; expected={}",
                        case["expected"]
                    ));
                }
            }
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
    public.close().await;
    owner.close().await;
}

#[tokio::test]
async fn live_operator_policy_is_literal_and_does_not_grant_upload_authority() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let slug: String =
        sqlx::query_scalar("SELECT 'jd'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,image_limit,max_image_bytes,max_video_bytes,max_video_duration,min_image_width,min_image_height,video_enabled,posting_reply_seconds,posting_image_seconds,posting_thread_seconds) VALUES($1,'Owned directory policy','<owned & \"caption\">',500,100,44,15,3,37,6291456,7340032,230,64,96,false,11,22,33)")
        .bind(&slug).execute(&owner).await.unwrap();
    let owned = owner.clone();
    let runtime = public.clone();
    let board = slug.clone();
    let result = tokio::spawn(async move {
        let (html, api) =
            board_public::routers(runtime.clone(), "http://127.0.0.1:3000".into(), false);
        for text_only in [false, true] {
            sqlx::query("UPDATE content.boards SET text_only=$2,max_image_bytes=$3 WHERE slug=$1")
                .bind(&board)
                .bind(text_only)
                .bind(if text_only {
                    5_242_880_i64
                } else {
                    6_291_456_i64
                })
                .execute(&owned)
                .await
                .unwrap();
            for router in [&html, &api] {
                let rows = directory(router).await;
                let row = rows.iter().find(|row| row["board"] == board).unwrap();
                assert_eq!(row["meta_description"], "<owned & \"caption\">");
                assert_eq!(
                    row["max_filesize"],
                    if text_only { 5_242_880 } else { 6_291_456 }
                );
                assert_eq!(row["max_webm_filesize"], 7_340_032);
                assert_eq!(row["max_webm_duration"], 230);
                assert_eq!(row["image_limit"], 37);
                assert_eq!(row["pages"], 5);
                assert_eq!(row["min_image_width"], 64);
                assert_eq!(row["min_image_height"], 96);
                assert_eq!(
                    row.get("text_only"),
                    text_only.then_some(&serde_json::json!(1))
                );
                assert!(row.get("webm_audio").is_none());
            }
        }
        // Advertising source policy cannot mount an absent upload route, grant
        // policy writes, or promote literal description text into HTML.
        let response = html
            .clone()
            .oneshot(
                Request::post(format!("/{board}/upload"))
                    .header("origin", "http://127.0.0.1:3000")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert!(
            sqlx::query("UPDATE content.boards SET max_image_bytes=1 WHERE slug=$1")
                .bind(&board)
                .execute(&runtime)
                .await
                .is_err()
        );
        let response = html
            .oneshot(
                Request::get(format!("/{board}/"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = String::from_utf8(
            to_bytes(response.into_body(), 4_194_304)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(!body.contains("<owned"));
    })
    .await;
    sqlx::query("DELETE FROM content.boards WHERE slug=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    result.unwrap();
}

async fn legacy_request(
    app: &Router,
    method: &str,
    path: &str,
    content_type: &str,
    body: &str,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("origin", "http://127.0.0.1:3000")
        .header("content-type", content_type)
        .header("cookie", "board_session=untrusted; staff_session=untrusted")
        .body(Body::from(body.to_owned()))
        .unwrap();
    request
        .extensions_mut()
        .insert(axum::extract::ConnectInfo(std::net::SocketAddr::from((
            [192, 0, 2, 219],
            40000,
        ))));
    let (parts, body) = app.clone().oneshot(request).await.unwrap().into_parts();
    (
        parts.status,
        parts.headers,
        to_bytes(body, 4_194_304).await.unwrap().to_vec(),
    )
}

#[tokio::test]
async fn legacy_catalog_info_matches_source_without_write_or_staff_authority() {
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let sessions_before: i64 =
        sqlx::query_scalar("SELECT count(*) FROM post_secrets.anonymous_sessions")
            .fetch_one(&owner)
            .await
            .unwrap();
    let before: Vec<String> = sqlx::query_scalar(
        "SELECT to_jsonb(p)::text FROM content.posts p WHERE board='a' ORDER BY id",
    )
    .fetch_all(&owner)
    .await
    .unwrap();
    let (html, api) =
        board_public::routers_with_media(public, "http://127.0.0.1:3000".into(), false, None);
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../fixtures/json-directory-reference.json"
    ))
    .unwrap();
    let visibility: Vec<(String, bool, bool)> =
        sqlx::query_as("SELECT slug,json_enabled,staff_only FROM content.boards")
            .fetch_all(&owner)
            .await
            .unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let board = case["board"].as_str().unwrap();
        let (_, json, staff) = visibility.iter().find(|row| row.0 == board).unwrap();
        let path = format!("/{board}/imgboard.php?mode=cataloginfo");
        let (status, headers, body) =
            legacy_request(&html, "GET", &path, "application/x-www-form-urlencoded", "").await;
        assert!(!headers.contains_key("set-cookie"));
        if *staff || !*json {
            assert_eq!(status, StatusCode::NOT_FOUND, "{board}");
        } else {
            assert_eq!(status, StatusCode::OK, "{board}");
            assert_eq!(headers["content-type"], "application/json");
            assert_eq!(
                serde_json::from_slice::<Value>(&body).unwrap(),
                case["expected"],
                "{board}"
            );
        }
    }
    let expected = &fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["board"] == "a")
        .unwrap()["expected"];
    for (path, kind, body) in [
        (
            "/a/imgboard.php",
            "application/x-www-form-urlencoded",
            "mode=cataloginfo",
        ),
        (
            "/a/imgboard.php?mode=cataloginfo",
            "application/x-www-form-urlencoded",
            "",
        ),
        (
            "/a/imgboard.php",
            "multipart/form-data; boundary=owned",
            "--owned\r\nContent-Disposition: form-data; name=\"mode\"\r\n\r\ncataloginfo\r\n--owned--\r\n",
        ),
    ] {
        let (status, headers, body) = legacy_request(&html, "POST", path, kind, body).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "{path}: {}",
            String::from_utf8_lossy(&body)
        );
        assert!(!headers.contains_key("set-cookie"));
        assert_eq!(&serde_json::from_slice::<Value>(&body).unwrap(), expected);
    }
    for query in [
        "mode=cataloginfo&mode=cataloginfo",
        "mode=cataloginfo&res=1",
        "mode=cataloginfo&admin=1",
        "mode=cataloginfo&mode=report",
    ] {
        assert_eq!(
            legacy_request(
                &html,
                "GET",
                &format!("/a/imgboard.php?{query}"),
                "application/x-www-form-urlencoded",
                ""
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            legacy_request(
                &html,
                "POST",
                "/a/imgboard.php",
                "application/x-www-form-urlencoded",
                query
            )
            .await
            .0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    for mode in [
        "preview_html",
        "forcearchive",
        "movethread",
        "copythreads",
        "rebuildboardsjson",
        "rebuild_threads_by_id",
        "latest",
        "admindel",
    ] {
        let path = format!("/a/imgboard.php?mode={mode}");
        assert_eq!(
            legacy_request(&html, "GET", &path, "application/x-www-form-urlencoded", "")
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        legacy_request(
            &api,
            "GET",
            "/a/imgboard.php?mode=cataloginfo",
            "application/x-www-form-urlencoded",
            ""
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let after: Vec<String> = sqlx::query_scalar(
        "SELECT to_jsonb(p)::text FROM content.posts p WHERE board='a' ORDER BY id",
    )
    .fetch_all(&owner)
    .await
    .unwrap();
    let sessions_after: i64 =
        sqlx::query_scalar("SELECT count(*) FROM post_secrets.anonymous_sessions")
            .fetch_one(&owner)
            .await
            .unwrap();
    assert_eq!(sessions_before, sessions_after);
    let (status, headers, body) = legacy_request(
        &html,
        "HEAD",
        "/a/imgboard.php?mode=cataloginfo",
        "application/x-www-form-urlencoded",
        "",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_empty());
    assert_eq!(headers["content-type"], "application/json");
    assert_eq!(before, after);
}
