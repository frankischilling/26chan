use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    response::Response,
};
use http_body_util::BodyExt;
use tower::ServiceExt;

const ORIGIN: &str = "http://127.0.0.1:3000";
const BLOCK: usize = 4096;

fn limits(response: usize, buffers: usize) -> board_config::PublicRequestLimits {
    board_config::PublicRequestLimits::from_lookup(|name| match name {
        "PUBLIC_MAX_RESPONSE_BYTES" => Some(response.to_string()),
        "PUBLIC_MAX_RESPONSE_BUFFER_BYTES" => Some(buffers.to_string()),
        _ => None,
    })
    .unwrap()
}

fn offline(response: usize, buffers: usize) -> (Router, Router) {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://board_public:unused@127.0.0.1:1/absent")
        .unwrap();
    board_public::routers_with_limits(pool, ORIGIN.into(), false, None, limits(response, buffers))
}

async fn request(app: &Router, method: &str, path: &str, etag: Option<&str>) -> Response {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("origin", ORIGIN);
    if let Some(etag) = etag {
        request = request.header("if-none-match", etag);
    }
    app.clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

fn derefer(destination: &str) -> String {
    format!(
        "/derefer?{}",
        url::form_urlencoded::Serializer::new(String::new())
            .append_pair("url", destination)
            .finish()
    )
}

#[tokio::test]
async fn configured_ceiling_rejects_dynamic_pages_and_styles_before_partial_success() {
    let (web, _) = offline(1, BLOCK);
    for path in [
        "/settings/theme".to_owned(),
        "/static/theme.css?worksafe=true".to_owned(),
        derefer("https://example.test/"),
    ] {
        let response = request(&web, "GET", &path, None).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE, "{path}");
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        assert!(
            response.headers()["cache-control"]
                .to_str()
                .unwrap()
                .contains("no-store")
        );
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(text.contains("Response exceeds the available output budget."));
        assert!(!text.contains("https://example.test/"));
    }
    assert_eq!(
        request(&web, "GET", "/healthz", None).await.status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn exact_html_limit_counts_utf8_and_escaping_without_changing_the_body() {
    let path = derefer(&format!("https://example.test/?q={}", "<&🙂".repeat(800)));
    let (web, _) = offline(131_072, 262_144);
    let response = request(&web, "GET", &path, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let expected = response.into_body().collect().await.unwrap().to_bytes();
    assert!(expected.len() > BLOCK);
    assert!(std::str::from_utf8(&expected).unwrap().contains("&#60;"));
    for (limit, status) in [
        (expected.len(), StatusCode::OK),
        (expected.len() - 1, StatusCode::SERVICE_UNAVAILABLE),
    ] {
        let (web, _) = offline(limit, 262_144);
        let response = request(&web, "GET", &path, None).await;
        assert_eq!(response.status(), status);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        if status == StatusCode::OK {
            assert_eq!(body, expected);
        } else {
            assert!(body.len() < expected.len());
            assert!(
                std::str::from_utf8(&body)
                    .unwrap()
                    .contains("output budget")
            );
        }
    }
}

#[tokio::test]
async fn unpolled_bodies_and_retained_slices_hold_blocks_and_dropping_them_recovers() {
    let (web, api) = offline(65_536, BLOCK);
    let path = derefer("https://example.test/");
    let response = request(&web, "GET", &path, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        request(&web, "GET", &path, None).await.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    for app in [&web, &api] {
        assert_eq!(
            request(app, "GET", "/healthz", None).await.status(),
            StatusCode::OK
        );
    }
    let mut body = response.into_body();
    let bytes = body.frame().await.unwrap().unwrap().into_data().unwrap();
    assert!(!bytes.is_empty() && bytes.len() < BLOCK);
    let retained = bytes.clone();
    let slice = bytes.slice(0..1);
    drop(bytes);
    drop(body);
    assert_eq!(
        request(&web, "GET", &path, None).await.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    drop(retained);
    assert_eq!(
        request(&web, "GET", &path, None).await.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    drop(slice);
    assert_eq!(
        request(&web, "GET", &path, None).await.status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn head_discards_encoded_blocks_without_waiting_for_response_object_drops() {
    let (web, _) = offline(65_536, BLOCK);
    let path = derefer("https://example.test/");
    let mut held = Vec::new();
    for _ in 0..40 {
        let response = request(&web, "HEAD", &path, None).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(axum::body::HttpBody::is_end_stream(response.body()));
        held.push(response);
    }
    assert_eq!(
        request(&web, "GET", &path, None).await.status(),
        StatusCode::OK
    );
    drop(held);
}

#[cfg(feature = "database-tests")]
mod persisted {
    use super::*;
    use rand_core::{OsRng, RngCore};
    use sha2::{Digest, Sha256};
    use sqlx::PgPool;

    async fn exercise(owner: PgPool, pool: PgPool, slug: String) {
        let (normal, _) = board_public::routers_with_limits(
            pool.clone(),
            ORIGIN.into(),
            false,
            None,
            limits(1_048_576, 4_194_304),
        );
        // Use retained multibyte text; posting policy removes the emoticon
        // ranges covered separately by the writer and derefer tests.
        let comment = "<&漢".repeat(800);
        let form = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("name", "Buffer fixture")
            .append_pair("sub", "Output budget")
            .append_pair("com", &comment)
            .append_pair("password", "owned-output-password")
            .append_pair("resto", "0")
            .finish();
        let response = normal
            .clone()
            .oneshot(
                Request::post(format!("/{slug}/post"))
                    .header("origin", ORIGIN)
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(form))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response.headers()["location"].to_str().unwrap();
        let thread = location
            .split('#')
            .next()
            .unwrap()
            .rsplit('/')
            .next()
            .unwrap()
            .parse::<i64>()
            .unwrap();
        drop(response);

        // Two real replies make the configured one-reply tail eligible. The
        // bounded numeric acknowledgement remains available with a one-byte
        // dynamic output ceiling after the write has committed.
        let (write_only, _) = board_public::routers_with_limits(
            pool.clone(),
            ORIGIN.into(),
            false,
            None,
            limits(1, BLOCK),
        );
        for index in 0..2 {
            let reply = url::form_urlencoded::Serializer::new(String::new())
                .append_pair("com", &format!("Owned tail reply {index}"))
                .append_pair("password", "owned-output-password")
                .append_pair("resto", &thread.to_string())
                .finish();
            let response = write_only
                .clone()
                .oneshot(
                    Request::post(format!("/{slug}/post"))
                        .header("origin", ORIGIN)
                        .header("accept", "application/json")
                        .header("content-type", "application/x-www-form-urlencoded")
                        .body(Body::from(reply))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let posted: serde_json::Value =
                serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                    .unwrap();
            assert_eq!(posted.as_object().unwrap().len(), 2);
            assert_eq!(posted["tid"], thread);
            assert!(posted["pid"].as_i64().unwrap() > thread);
        }

        let json_path = format!("/{slug}/thread/{thread}.json");
        let reference = request(&normal, "GET", &json_path, None).await;
        assert_eq!(reference.status(), StatusCode::OK);
        let etag = reference.headers()["etag"].to_str().unwrap().to_owned();
        let bytes = reference.into_body().collect().await.unwrap().to_bytes();
        assert!(bytes.len() > BLOCK);
        assert_eq!(etag, format!("\"{:x}\"", Sha256::digest(&bytes)));
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["posts"][0]["no"], thread);
        assert!(!value["posts"][0]["com"].as_str().unwrap().contains("<&"));

        let (web, api) = board_public::routers_with_limits(
            pool.clone(),
            ORIGIN.into(),
            false,
            None,
            limits(bytes.len(), 4_194_304),
        );
        for app in [&web, &api] {
            let response = request(app, "GET", &json_path, None).await;
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()["etag"], etag);
            assert_eq!(
                response.into_body().collect().await.unwrap().to_bytes(),
                bytes
            );
        }
        let (_, small_api) = board_public::routers_with_limits(
            pool.clone(),
            ORIGIN.into(),
            false,
            None,
            limits(bytes.len() - 1, 4_194_304),
        );
        let response = request(&small_api, "GET", &json_path, None).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers()["content-type"], "application/json");
        assert_eq!(response.headers()["access-control-allow-origin"], ORIGIN);
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert!(response.headers().get("etag").is_none());
        let error: serde_json::Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(
            error,
            serde_json::json!({"error": "Response exceeds the available output budget. Try again later."})
        );

        let (small_web, small_api) = board_public::routers_with_limits(
            pool.clone(),
            ORIGIN.into(),
            false,
            None,
            limits(1, BLOCK),
        );
        for path in [
            "/boards.json".to_owned(),
            json_path.clone(),
            format!("/{slug}/thread/{thread}-tail.json"),
            format!("/{slug}/threads.json"),
            format!("/{slug}/1.json"),
            format!("/{slug}/catalog.json"),
            format!("/{slug}/archive.json"),
        ] {
            for app in [&small_web, &small_api] {
                assert_eq!(
                    request(app, "GET", &path, None).await.status(),
                    StatusCode::SERVICE_UNAVAILABLE,
                    "{path}"
                );
            }
        }
        for path in [
            "/".to_owned(),
            format!("/{slug}/"),
            format!("/{slug}/catalog"),
            format!("/{slug}/archive"),
            format!("/{slug}/thread/{thread}"),
            format!("/_watch/{slug}/thread/{thread}.json"),
            format!("/_watch/{slug}/thread/{thread}/posts"),
            format!("/_watch/{slug}/thread/{thread}/posts-tail"),
            format!("/_watch/{slug}/post/{thread}"),
        ] {
            assert_eq!(
                request(&small_web, "GET", &path, None).await.status(),
                StatusCode::SERVICE_UNAVAILABLE,
                "{path}"
            );
        }

        // A fixed confirmation must not turn an already committed report into
        // an output-exhaustion error. It follows the bounded control-body path.
        let report = small_web
            .clone()
            .oneshot(
                Request::post(format!("/{slug}/report"))
                    .header("origin", ORIGIN)
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(format!(
                        "no={thread}&reason=Owned+output+budget+report"
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(report.status(), StatusCode::OK);
        let confirmation = report.into_body().collect().await.unwrap().to_bytes();
        assert!(
            std::str::from_utf8(&confirmation)
                .unwrap()
                .contains("Your report was saved.")
        );
        let reports: i64 = sqlx::query_scalar("SELECT count(*) FROM content.reports WHERE board=$1 AND post_id=$2 AND reason='Owned output budget report'")
            .bind(&slug).bind(thread).fetch_one(&owner).await.unwrap();
        assert_eq!(reports, 1);

        let archive = format!("/{slug}/archive.json");
        let page = derefer("https://example.test/");
        let (web, api) = board_public::routers_with_limits(
            pool.clone(),
            ORIGIN.into(),
            false,
            None,
            limits(65_536, BLOCK),
        );
        for ((first, first_path), (second, second_path)) in [
            ((&web, page.as_str()), (&api, archive.as_str())),
            ((&api, archive.as_str()), (&web, page.as_str())),
        ] {
            let response = request(first, "GET", first_path, None).await;
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                request(second, "GET", second_path, None).await.status(),
                StatusCode::SERVICE_UNAVAILABLE
            );
            let mut body = response.into_body();
            let data = body.frame().await.unwrap().unwrap().into_data().unwrap();
            let held = data.slice(0..1);
            drop(data);
            drop(body);
            assert_eq!(
                request(second, "GET", second_path, None).await.status(),
                StatusCode::SERVICE_UNAVAILABLE
            );
            drop(held);
            assert_eq!(
                request(second, "GET", second_path, None).await.status(),
                StatusCode::OK
            );
        }
        let control = request(&api, "GET", &archive, None).await;
        let archive_etag = control.headers()["etag"].to_str().unwrap().to_owned();
        drop(control);
        let mut empty = Vec::new();
        for _ in 0..40 {
            for (method, tag, expected) in [
                ("GET", Some(archive_etag.as_str()), StatusCode::NOT_MODIFIED),
                ("HEAD", None, StatusCode::OK),
                ("OPTIONS", None, StatusCode::NO_CONTENT),
            ] {
                let response = request(&api, method, &archive, tag).await;
                assert_eq!(response.status(), expected);
                assert_eq!(response.headers()["access-control-allow-origin"], ORIGIN);
                assert!(axum::body::HttpBody::is_end_stream(response.body()));
                empty.push(response);
            }
        }
        assert_eq!(
            request(&web, "GET", &page, None).await.status(),
            StatusCode::OK
        );
        drop(empty);

        let (one, two) = tokio::join!(
            request(&web, "GET", &page, None),
            request(&api, "GET", &archive, None),
        );
        let statuses = [one.status(), two.status()];
        assert_eq!(
            statuses
                .iter()
                .filter(|status| **status == StatusCode::OK)
                .count(),
            1
        );
        assert_eq!(
            statuses
                .iter()
                .filter(|status| **status == StatusCode::SERVICE_UNAVAILABLE)
                .count(),
            1
        );
        drop((one, two));
        assert_eq!(
            request(&api, "GET", &archive, None).await.status(),
            StatusCode::OK
        );

        // The rejected encodings do not mutate the stored post.
        let stored: String =
            sqlx::query_scalar("SELECT comment FROM content.posts WHERE board=$1 AND id=$2")
                .bind(&slug)
                .bind(thread)
                .fetch_one(&owner)
                .await
                .unwrap();
        assert_eq!(stored, comment);
    }

    #[tokio::test]
    async fn persisted_public_and_api_output_share_capacity_without_changing_json_or_visibility() {
        let owner = PgPool::connect(
            &std::env::var("MIGRATION_DATABASE_URL").expect("disposable owner database required"),
        )
        .await
        .unwrap();
        let public = board_store::connect_public(
            &std::env::var("TEST_PUBLIC_DATABASE_URL")
                .expect("disposable public database required"),
        )
        .await
        .unwrap();
        let mut random = [0_u8; 5];
        OsRng.fill_bytes(&mut random);
        let slug: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,archive_retention_seconds,archive_limit,json_tail_size) VALUES($1,'Output budgets','Owned output fixture',4000,100,50,10,10,3600,10,1)")
            .bind(&slug).execute(&owner).await.unwrap();
        let result = tokio::spawn(exercise(owner.clone(), public.clone(), slug.clone())).await;
        public.close().await;
        for statement in [
            "DELETE FROM content.reports WHERE board=$1",
            "DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
            "DELETE FROM content.posts WHERE board=$1",
            "DELETE FROM content.threads WHERE board=$1",
            "DELETE FROM content.boards WHERE slug=$1",
        ] {
            sqlx::query(statement)
                .bind(&slug)
                .execute(&owner)
                .await
                .unwrap();
        }
        owner.close().await;
        result.unwrap();
    }
}
