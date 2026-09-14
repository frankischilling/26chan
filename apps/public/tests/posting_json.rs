#![cfg(feature = "database-tests")]
use axum::{
    body::Body,
    http::{HeaderMap, Request, StatusCode},
};
use http_body_util::BodyExt;
use rand_core::{OsRng, RngCore};
use tower::ServiceExt;

fn request(path: &str, fields: &str, accept: &str) -> Request<Body> {
    Request::post(path)
        .header("origin", "http://127.0.0.1:3000")
        .header("accept", accept)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(fields.to_owned()))
        .unwrap()
}
async fn json(
    response: axum::response::Response,
    expected: StatusCode,
) -> (HeaderMap, serde_json::Value) {
    assert_eq!(response.status(), expected);
    assert_eq!(response.headers()["content-type"], "application/json");
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert!(
        response.headers()["vary"]
            .to_str()
            .unwrap()
            .contains("Accept")
    );
    let (parts, body) = response.into_parts();
    let bytes = body.collect().await.unwrap().to_bytes();
    (parts.headers, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn negotiated_posting_persists_posts_and_preserves_failures_and_origin_checks() {
    let owner = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut random = [0_u8; 4];
    OsRng.fill_bytes(&mut random);
    let board = format!(
        "pj{}",
        random
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Posting JSON','Owned synthetic response fixture',100,100,100,100,10)").bind(&board).execute(&owner).await.unwrap();
    let (app, api) = board_public::routers(public.clone(), "http://127.0.0.1:3000".into(), false);
    let fields = "name=Anonymous&sub=JSON+thread&com=Line+one%0D%0ALine+two&password=owned-json-password&track=1&awt=1&email=nonoko";
    let (headers, posted) = json(
        app.clone()
            .oneshot(request(
                &format!("/{board}/post"),
                fields,
                "application/json",
            ))
            .await
            .unwrap(),
        StatusCode::OK,
    )
    .await;
    assert_eq!(posted.as_object().unwrap().len(), 2);
    assert_eq!(posted["tid"], 0);
    let thread = posted["pid"].as_i64().unwrap();
    assert!(thread > 0);
    assert!(!headers.contains_key("location"));
    let cookies: Vec<_> = headers
        .get_all("set-cookie")
        .iter()
        .map(|c| c.to_str().unwrap())
        .collect();
    assert!(
        cookies
            .iter()
            .any(|c| c.starts_with(&format!("4chan_awt={thread};")))
    );
    assert!(
        cookies
            .iter()
            .any(|c| c.starts_with(&format!("board-posted-{thread}={thread}.1;")))
    );
    assert!(
        cookies
            .iter()
            .all(|c| c.contains(&format!("Path=/{board}/; SameSite=Strict"))
                && !c.contains("Domain="))
    );
    assert_eq!(
        board_store::find_post(&public, &board, thread)
            .await
            .unwrap()
            .comment,
        "Line one\nLine two"
    );
    let reply_fields =
        format!("resto={thread}&com=Real+reply&password=owned-json-password&track=1&awt=1");
    let (headers, reply) = json(
        app.clone()
            .oneshot(request(
                &format!("/{board}/imgboard.php"),
                &reply_fields,
                "application/json",
            ))
            .await
            .unwrap(),
        StatusCode::OK,
    )
    .await;
    assert_eq!(reply["tid"], thread);
    let reply_id = reply["pid"].as_i64().unwrap();
    assert!(reply_id > thread);
    assert_eq!(
        board_store::find_post(&public, &board, reply_id)
            .await
            .unwrap()
            .thread_id,
        thread
    );
    assert!(headers.get_all("set-cookie").iter().any(|c| {
        c.to_str()
            .unwrap()
            .starts_with(&format!("board-posted-{reply_id}={thread}.0;"))
    }));
    assert!(
        !headers
            .get_all("set-cookie")
            .iter()
            .any(|c| c.to_str().unwrap().starts_with("4chan_awt="))
    );

    for route in ["post", "imgboard.php"] {
        let path = format!("/{board}/{route}");
        let (headers, rejected) = json(
            app.clone()
                .oneshot(request(
                    &path,
                    "com=&password=owned-json-password&track=1&awt=1",
                    "application/json",
                ))
                .await
                .unwrap(),
            StatusCode::OK,
        )
        .await;
        assert_eq!(rejected.as_object().unwrap().len(), 1);
        assert!(rejected["error"].as_str().unwrap().contains("comment"));
        assert!(!headers.contains_key("set-cookie"));
        assert_eq!(
            app.clone()
                .oneshot(request(
                    &path,
                    "com=&password=owned-json-password",
                    "text/html"
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        for accept in ["*/*", "application/json;q=1", "application/json, text/html"] {
            let response = app
                .clone()
                .oneshot(request(&path, fields, accept))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::SEE_OTHER);
            assert_eq!(response.headers()["location"], format!("/{board}/"));
        }
        let mut forbidden = request(&path, fields, "application/json");
        forbidden
            .headers_mut()
            .insert("origin", "http://foreign.invalid".parse().unwrap());
        assert_eq!(
            app.clone().oneshot(forbidden).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
        let mut site = request(&path, fields, "application/json");
        site.headers_mut()
            .insert("sec-fetch-site", "cross-site".parse().unwrap());
        assert_eq!(
            app.clone().oneshot(site).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            api.clone()
                .oneshot(request(&path, fields, "application/json"))
                .await
                .unwrap()
                .status(),
            StatusCode::METHOD_NOT_ALLOWED
        );
    }
    let path = format!("/{board}/post");
    let (_, malformed) = json(
        app.clone()
            .oneshot(request(
                &path,
                "password=do-not-echo-this&resto=not-an-id",
                "application/json",
            ))
            .await
            .unwrap(),
        StatusCode::UNPROCESSABLE_ENTITY,
    )
    .await;
    assert_eq!(
        malformed,
        serde_json::json!({"error":"Invalid posting form."})
    );
    let mut unsupported = request(&path, fields, "application/json");
    unsupported
        .headers_mut()
        .insert("content-type", "application/json".parse().unwrap());
    let (_, malformed) = json(
        app.clone().oneshot(unsupported).await.unwrap(),
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
    )
    .await;
    assert_eq!(malformed["error"], "Invalid posting form.");
    let (headers, oversized) = json(
        app.clone()
            .oneshot(request(
                &path,
                &format!("com={}", "x".repeat(262_145)),
                "application/json",
            ))
            .await
            .unwrap(),
        StatusCode::PAYLOAD_TOO_LARGE,
    )
    .await;
    assert_eq!(oversized["error"], "Invalid posting form.");
    assert!(!headers.contains_key("set-cookie"));
    sqlx::query("UPDATE content.threads SET closed=true WHERE id=$1")
        .bind(thread)
        .execute(&owner)
        .await
        .unwrap();
    let (headers, closed) = json(
        app.clone()
            .oneshot(request(&path, &reply_fields, "application/json"))
            .await
            .unwrap(),
        StatusCode::OK,
    )
    .await;
    assert!(closed["error"].as_str().unwrap().contains("closed"));
    assert!(!headers.contains_key("set-cookie"));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=$1")
        .bind(&board)
        .fetch_one(&owner)
        .await
        .unwrap();
    assert_eq!(count, 8);
    public.close().await;
    let (headers, unavailable) = json(
        app.oneshot(request(&path, fields, "application/json"))
            .await
            .unwrap(),
        StatusCode::SERVICE_UNAVAILABLE,
    )
    .await;
    assert_eq!(
        unavailable,
        serde_json::json!({"error":"Storage is unavailable. Try again later."})
    );
    assert!(!headers.contains_key("set-cookie"));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM content.posts WHERE board=$1")
            .bind(&board)
            .fetch_one(&owner)
            .await
            .unwrap(),
        count
    );
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
}
