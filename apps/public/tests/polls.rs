#![cfg(feature = "database-tests")]

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Method, Request, StatusCode},
};
use rand_core::{OsRng, RngCore};
use sqlx::PgPool;
use tower::ServiceExt;

const ORIGIN: &str = "http://127.0.0.1:3000";

struct Page {
    status: StatusCode,
    headers: HeaderMap,
    body: String,
}

async fn request(app: &Router, method: Method, path: &str) -> Page {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("origin", ORIGIN)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from("action=vote&id=1&_ptkn=untrusted"))
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
    Page {
        status,
        headers,
        body,
    }
}

fn inert(page: &Page) {
    assert!(page.headers.get("set-cookie").is_none());
    let csp = page.headers["content-security-policy"].to_str().unwrap();
    for directive in [
        "script-src 'none'",
        "connect-src 'none'",
        "worker-src 'none'",
    ] {
        assert!(csp.split(';').any(|part| part.trim() == directive), "{csp}");
    }
    for forbidden in [
        "<script",
        "<form",
        "<input",
        "data-tkn=",
        "_ptkn",
        "watcher.v",
        "google-analytics",
    ] {
        assert!(!page.body.contains(forbidden), "unexpected {forbidden}");
    }
}

fn escaped(page: &Page, raw: &str, escaped: &str) {
    assert!(!page.body.contains(raw), "unescaped HTML: {raw}");
    assert!(
        page.body.contains(escaped),
        "missing escaped HTML: {escaped}"
    );
}

async fn exercise(owner: &PgPool, public: &PgPool, ids: &[i64], low_id_owned: bool) {
    let (app, api) = board_public::routers(public.clone(), ORIGIN.into(), false);
    let listed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM content.published_polls WHERE catalogue_ordinal IS NOT NULL",
    )
    .fetch_one(public)
    .await
    .unwrap();
    if listed == 0 {
        let empty = request(&app, Method::GET, "/polls").await;
        assert_eq!(empty.status, StatusCode::OK);
        assert!(empty.body.contains("No polls are available yet."));
        inert(&empty);
    }
    let [first, second, hidden, unlisted] = <[i64; 4]>::try_from(ids).unwrap();
    // Pick unused operator slots, never replace another fixture's data.
    let slots: Vec<i32> = sqlx::query_scalar(
        "SELECT n FROM generate_series(1,200) n WHERE NOT EXISTS (SELECT 1 FROM poll_private.polls WHERE catalogue_ordinal=n) ORDER BY n LIMIT 2",
    ).fetch_all(owner).await.unwrap();
    assert_eq!(slots.len(), 2, "two free catalogue slots required");
    for (id, title, description, published, ordinal) in [
        (
            first,
            "Owned <title> & first",
            "Owned <description> & text",
            true,
            Some(slots[1]),
        ),
        (second, "Owned second poll", "", true, Some(slots[0])),
        (
            hidden,
            "PRIVATE POLL SENTINEL",
            "PRIVATE DESCRIPTION SENTINEL",
            false,
            None,
        ),
        (unlisted, "Owned unlisted poll", "", true, None),
    ] {
        sqlx::query("INSERT INTO poll_private.polls(id,title,description,vote_count,published,catalogue_ordinal) VALUES($1,$2,$3,6,$4,$5)")
            .bind(id).bind(title).bind(description).bind(published).bind(ordinal)
            .execute(owner).await.unwrap();
    }
    // Reverse IDs deliberately: source option order must come from the ordinal.
    for (id, ordinal, caption, score) in [
        (30_i64, 1_i32, "Owned <caption> & first", Some(2_i64)),
        (20, 2, "Owned second option", Some(3)),
        (10, 3, "Owned missing score", None),
    ] {
        sqlx::query("INSERT INTO poll_private.options(poll_id,id,ordinal,caption,score) VALUES($1,$2,$3,$4,$5)")
            .bind(first).bind(id).bind(ordinal).bind(caption).bind(score)
            .execute(owner).await.unwrap();
    }
    sqlx::query("INSERT INTO poll_private.options(poll_id,id,ordinal,caption,score) VALUES($1,1,1,'PRIVATE OPTION SENTINEL',9)")
        .bind(hidden).execute(owner).await.unwrap();
    let catalogue = request(&app, Method::GET, "/polls").await;
    assert_eq!(catalogue.status, StatusCode::OK, "{}", catalogue.body);
    inert(&catalogue);
    escaped(
        &catalogue,
        "Owned <title> & first",
        "Owned &#60;title&#62; &#38; first",
    );
    let first_link = format!("href=\"/polls/{first}\"");
    let second_link = format!("href=\"/polls/{second}\"");
    assert!(catalogue.body.find(&second_link).unwrap() < catalogue.body.find(&first_link).unwrap());
    assert!(!catalogue.body.contains("PRIVATE"));
    assert!(!catalogue.body.contains("Owned unlisted poll"));
    let home = request(&app, Method::GET, "/").await;
    assert_eq!(home.status, StatusCode::OK);
    assert!(home.body.contains("href=\"/polls\""));

    for path in [
        "/polls".to_owned(),
        format!("/polls/{first}"),
        format!("/polls/results/{first}"),
    ] {
        let get = request(&app, Method::GET, &path).await;
        assert_eq!(get.status, StatusCode::OK, "{path}: {}", get.body);
        assert!(
            get.headers["content-type"]
                .to_str()
                .unwrap()
                .starts_with("text/html")
        );
        inert(&get);
        let head = request(&app, Method::HEAD, &path).await;
        assert_eq!(head.status, get.status, "{path}");
        assert_eq!(head.headers["content-type"], get.headers["content-type"]);
        assert_eq!(
            head.headers["content-security-policy"],
            get.headers["content-security-policy"]
        );
        assert!(head.body.is_empty(), "{path}");
        inert(&head);
    }
    let options = request(&app, Method::GET, &format!("/polls/{first}")).await;
    escaped(
        &options,
        "Owned <description> & text",
        "Owned &#60;description&#62; &#38; text",
    );
    escaped(
        &options,
        "Owned <caption> & first",
        "Owned &#60;caption&#62; &#38; first",
    );
    assert!(options.body.contains("id=\"poll-desc\""));
    assert!(options.body.contains("href=\"/polls\""));
    assert!(
        options
            .body
            .contains(&format!("href=\"/polls/results/{first}\""))
    );
    assert!(
        options.body.find("Owned &#60;caption&#62;").unwrap()
            < options.body.find("Owned second option").unwrap()
    );
    assert!(
        options.body.find("Owned second option").unwrap()
            < options.body.find("Owned missing score").unwrap()
    );
    let results = request(&app, Method::GET, &format!("/polls/results/{first}")).await;
    for value in ["33.33% (2)", "50% (3)", "0% (0)", "Total votes: 6"] {
        assert!(
            results.body.contains(value),
            "missing {value}: {}",
            results.body
        );
    }
    assert!(!results.body.contains("50.00%"));
    assert!(results.body.contains(&first_link));
    escaped(
        &results,
        "Owned <title> & first",
        "Owned &#60;title&#62; &#38; first",
    );
    escaped(
        &results,
        "Owned <description> & text",
        "Owned &#60;description&#62; &#38; text",
    );
    escaped(
        &results,
        "Owned <caption> & first",
        "Owned &#60;caption&#62; &#38; first",
    );
    for id in [second, unlisted] {
        let page = request(&app, Method::GET, &format!("/polls/{id}")).await;
        assert_eq!(page.status, StatusCode::OK);
        assert!(!page.body.contains("id=\"poll-desc\""));
    }
    sqlx::query("UPDATE poll_private.polls SET vote_count=0 WHERE id=$1")
        .bind(first)
        .execute(owner)
        .await
        .unwrap();
    let zero = request(&app, Method::GET, &format!("/polls/results/{first}")).await;
    assert_eq!(zero.status, StatusCode::OK);
    for value in ["0% (2)", "0% (3)", "0% (0)", "Total votes: 0"] {
        assert!(zero.body.contains(value), "missing {value}: {}", zero.body);
    }
    assert!(!zero.body.contains("NaN") && !zero.body.contains("Infinity"));
    sqlx::query("UPDATE poll_private.polls SET vote_count=32 WHERE id=$1")
        .bind(first)
        .execute(owner)
        .await
        .unwrap();
    sqlx::query("UPDATE poll_private.options SET score=1 WHERE poll_id=$1 AND id=30")
        .bind(first)
        .execute(owner)
        .await
        .unwrap();
    let rounded = request(&app, Method::GET, &format!("/polls/results/{first}")).await;
    assert!(rounded.body.contains("3.13% (1)"));
    assert!(rounded.body.contains("9.38% (3)"));

    for path in [
        format!("/polls/{hidden}"),
        format!("/polls/results/{hidden}"),
        "/polls/0".into(),
        "/polls/-1".into(),
        "/polls/nope".into(),
        "/polls/9223372036854775808".into(),
        "/polls/9223372036854775807".into(),
        "/polls/results/0".into(),
        "/polls/results/nope".into(),
        "/polls/%FF".into(),
        "/polls/%2B1".into(),
        "/polls/results/%FF".into(),
        "/polls/results/9223372036854775808".into(),
    ] {
        for method in [Method::GET, Method::HEAD] {
            let head = method == Method::HEAD;
            let page = request(&app, method, &path).await;
            assert_eq!(page.status, StatusCode::NOT_FOUND, "{path}: {}", page.body);
            assert!(!page.body.contains("PRIVATE"));
            if head {
                assert!(page.body.is_empty(), "{path}");
            }
            inert(&page);
        }
    }
    let low = request(&app, Method::GET, "/polls/1").await;
    if low_id_owned {
        assert_eq!(low.status, StatusCode::OK);
    }
    inert(&low);

    for path in [
        "/polls".to_owned(),
        format!("/polls/{first}"),
        format!("/polls/results/{first}"),
    ] {
        for method in [Method::POST, Method::PUT, Method::PATCH, Method::DELETE] {
            let vote = method == Method::POST && path == format!("/polls/{first}");
            let response = request(&app, method, &path).await;
            // The historical reader has no configured signing key. The new
            // vote route must fail closed while the other writes stay absent.
            let expected = if vote {
                StatusCode::SERVICE_UNAVAILABLE
            } else {
                StatusCode::METHOD_NOT_ALLOWED
            };
            assert_eq!(response.status, expected, "{path}");
            inert(&response);
        }
        for method in [
            Method::GET,
            Method::HEAD,
            Method::POST,
            Method::PUT,
            Method::DELETE,
        ] {
            let is_read = method == Method::GET || method == Method::HEAD;
            let is_head = method == Method::HEAD;
            let response = request(&api, method, &path).await;
            if is_read {
                assert_eq!(response.status, StatusCode::NOT_FOUND, "API exposed {path}");
            }
            if is_head {
                assert!(response.body.is_empty());
            }
            assert!(!response.status.is_success(), "API exposed {path}");
            assert!(!response.body.contains("Owned") && !response.body.contains("PRIVATE"));
            assert!(response.headers.get("set-cookie").is_none());
        }
    }
    for path in [
        "/polls.json".to_owned(),
        format!("/polls/{first}.json"),
        format!("/polls/results/{first}.json"),
        format!("/polls/{hidden}"),
        format!("/polls/results/{hidden}"),
    ] {
        let response = request(&api, Method::GET, &path).await;
        assert_eq!(response.status, StatusCode::NOT_FOUND, "API exposed {path}");
        assert!(!response.body.contains("PRIVATE") && !response.body.contains("Owned"));
    }
    // The maximum legal detail projection still renders within the response budget.
    sqlx::query(
        "UPDATE poll_private.polls SET title=$2,description=$3,vote_count=1000000000 WHERE id=$1",
    )
    .bind(unlisted)
    .bind("<".repeat(512))
    .bind("<".repeat(16384))
    .execute(owner)
    .await
    .unwrap();
    sqlx::query("INSERT INTO poll_private.options(poll_id,id,ordinal,caption,score) SELECT $1,n,n,$2,1000000000 FROM generate_series(1,128) n")
        .bind(unlisted).bind("<".repeat(1024)).execute(owner).await.unwrap();
    let largest = request(&app, Method::GET, &format!("/polls/results/{unlisted}")).await;
    assert_eq!(
        largest.status,
        StatusCode::OK,
        "maximum legal poll must render"
    );
    inert(&largest);
    assert_eq!(largest.body.matches("100% (1000000000)").count(), 128);
    assert!(largest.body.contains(&"&#60;".repeat(16384)));
    assert!(largest.body.contains("Total votes: 1000000000"));

    let unchanged: i64 =
        sqlx::query_scalar("SELECT vote_count FROM poll_private.polls WHERE id=$1")
            .bind(first)
            .fetch_one(owner)
            .await
            .unwrap();
    assert_eq!(unchanged, 32);
    let unchanged: Option<i64> =
        sqlx::query_scalar("SELECT score FROM poll_private.options WHERE poll_id=$1 AND id=30")
            .bind(first)
            .fetch_one(owner)
            .await
            .unwrap();
    assert_eq!(unchanged, Some(1));
    for statement in [
        "SELECT * FROM poll_private.polls",
        "SELECT * FROM poll_private.options",
        "UPDATE content.published_polls SET vote_count=99 WHERE id=$1",
        "DELETE FROM content.published_polls WHERE id=$1",
        "UPDATE content.published_poll_options SET score=99 WHERE poll_id=$1",
        "DELETE FROM content.published_poll_options WHERE poll_id=$1",
    ] {
        let query = sqlx::query(statement);
        let query = if statement.contains("$1") {
            query.bind(first)
        } else {
            query
        };
        assert!(
            query.execute(public).await.is_err(),
            "public role allowed {statement}"
        );
    }
    let private_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM content.published_polls WHERE id=$1")
            .bind(hidden)
            .fetch_one(public)
            .await
            .unwrap();
    assert_eq!(private_count, 0);
    let private_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM content.published_poll_options WHERE poll_id=$1")
            .bind(hidden)
            .fetch_one(public)
            .await
            .unwrap();
    assert_eq!(private_count, 0);
}

#[tokio::test]
async fn polls_are_ordered_escaped_read_only_published_html_without_board_capabilities() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    // Shared with store poll fixtures; held on this connection through cleanup.
    let mut lock = owner.acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock(2250109)")
        .execute(&mut *lock)
        .await
        .unwrap();
    let mut nonce = [0u8; 4];
    OsRng.fill_bytes(&mut nonce);
    let base = 10_000_000_000_i64 + i64::from(u32::from_be_bytes(nonce)) * 4;
    let ids = vec![base, base + 1, base + 2, base + 3];
    let low_id_owned = sqlx::query("INSERT INTO poll_private.polls(id,title,description,vote_count,published) VALUES(1,'Owned low-ID regression','',0,true) ON CONFLICT(id) DO NOTHING")
        .execute(&owner).await.unwrap().rows_affected() == 1;
    let result = tokio::spawn({
        let owner = owner.clone();
        let public = public.clone();
        let ids = ids.clone();
        async move { exercise(&owner, &public, &ids, low_id_owned).await }
    })
    .await;
    public.close().await;
    let mut cleanup = ids;
    if low_id_owned {
        cleanup.push(1);
    }
    sqlx::query("DELETE FROM poll_private.polls WHERE id=ANY($1)")
        .bind(&cleanup)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("SELECT pg_advisory_unlock(2250109)")
        .execute(&mut *lock)
        .await
        .unwrap();
    drop(lock);
    owner.close().await;
    result.unwrap();
}
