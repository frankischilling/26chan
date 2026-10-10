#![cfg(feature = "database-tests")]

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Method, Request, StatusCode},
};
use board_domain::poster_id::PosterIdKey;
use rand_core::{OsRng, RngCore};
use sqlx::PgPool;
use std::sync::Arc;
use tower::ServiceExt;

const ORIGIN: &str = "http://127.0.0.1:3000";

struct Page {
    status: StatusCode,
    headers: HeaderMap,
    body: String,
}

fn router(pool: &PgPool) -> Router {
    board_public::routers_with_options(
        pool.clone(),
        board_public::PublicRouterOptions {
            origin: ORIGIN.into(),
            production: false,
            media: None,
            limits: board_config::PublicRequestLimits::default(),
            proxy_uid: None,
            poster_id_key: Some(Arc::new(PosterIdKey::parse(&"11".repeat(32)).unwrap())),
            tripcode_key: None,
            country_database: None,
        },
    )
    .0
}

async fn send(
    app: &Router,
    method: Method,
    path: &str,
    body: &str,
    headers: &[(&str, &str)],
) -> Page {
    let mut request = Request::builder().method(method).uri(path);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::from(body.to_owned())).unwrap())
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

async fn get(app: &Router, id: i64, cookie: Option<&str>) -> Page {
    let headers = cookie
        .map(|cookie| vec![("cookie", cookie)])
        .unwrap_or_default();
    send(app, Method::GET, &format!("/polls/{id}"), "", &headers).await
}

fn token(page: &Page) -> String {
    assert_eq!(page.status, StatusCode::OK, "{}", page.body);
    page.body
        .split("name=\"_ptkn\" value=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .into()
}

fn cookie(page: &Page) -> String {
    page.headers["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .into()
}

fn form(token: &str, option: &str) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .append_pair("action", "vote")
        .append_pair("id", option)
        .append_pair("_ptkn", token)
        .finish()
}

async fn post(app: &Router, id: i64, cookie: &str, body: &str) -> Page {
    send(
        app,
        Method::POST,
        &format!("/polls/{id}"),
        body,
        &[
            ("origin", ORIGIN),
            ("sec-fetch-site", "same-origin"),
            ("content-type", "application/x-www-form-urlencoded"),
            ("cookie", cookie),
        ],
    )
    .await
}

fn private(page: &Page) {
    assert!(
        page.headers["cache-control"]
            .to_str()
            .unwrap()
            .contains("no-store")
    );
    let csp = page.headers["content-security-policy"].to_str().unwrap();
    for rule in [
        "script-src 'none'",
        "connect-src 'none'",
        "worker-src 'none'",
        "form-action 'self'",
    ] {
        assert!(csp.split(';').any(|part| part.trim() == rule), "{csp}");
    }
    assert!(!page.body.contains("<script") && !page.body.contains("watcher.v"));
}

async fn counts(owner: &PgPool, poll: i64) -> (i64, i32, i64, Vec<Option<i64>>) {
    let (total, new, receipts): (i64, i32, i64) = sqlx::query_as(
        "SELECT vote_count,new_vote_count,(SELECT count(*) FROM poll_private.votes WHERE poll_id=$1) FROM poll_private.polls WHERE id=$1")
        .bind(poll).fetch_one(owner).await.unwrap();
    let scores = sqlx::query_scalar(
        "SELECT score FROM poll_private.options WHERE poll_id=$1 ORDER BY ordinal",
    )
    .bind(poll)
    .fetch_all(owner)
    .await
    .unwrap();
    (total, new, receipts, scores)
}

async fn exercise(owner: &PgPool, public: &PgPool, ids: [i64; 5]) {
    let [first, second, closed, hidden, unlisted] = ids;
    let app = router(public);
    for id in [first, second, unlisted] {
        let head = send(&app, Method::HEAD, &format!("/polls/{id}"), "", &[]).await;
        assert_eq!(head.status, StatusCode::OK);
        assert!(head.body.is_empty() && head.headers.get("set-cookie").is_none());
        private(&head);
    }
    let options = get(&app, first, None).await;
    private(&options);
    let saved_cookie = cookie(&options);
    let saved_token = token(&options);
    assert!(
        options.headers["set-cookie"]
            .to_str()
            .unwrap()
            .ends_with("Path=/; Max-Age=31536000; HttpOnly; SameSite=Strict")
    );
    assert!(options.body.contains(
        "id=\"poll-form\" action=\"\" method=\"POST\" enctype=\"application/x-www-form-urlencoded\""
    ));
    assert!(
        options
            .body
            .contains(&format!("data-tkn=\"{saved_token}\""))
    );
    assert!(options.body.contains("name=\"action\" value=\"vote\""));
    assert!(options.body.find("value=\"7\"").unwrap() < options.body.find("value=\"9\"").unwrap());
    assert!(options.body.contains("Owned &#60;caption&#62; &#38; first"));
    assert_eq!(
        counts(owner, first).await,
        (6, 0, 0, vec![Some(2), Some(4)])
    );
    let again = get(&app, first, Some(&saved_cookie)).await;
    assert!(again.headers.get("set-cookie").is_none());
    assert!(!token(&again).is_empty());

    let other_browser = get(&app, first, None).await;
    let other_cookie = cookie(&other_browser);
    let other_token = token(&other_browser);
    assert_ne!(saved_cookie, other_cookie);
    let second_form = get(&app, second, Some(&saved_cookie)).await;
    let cross_poll_token = token(&second_form);

    for (cookie_, token_) in [
        ("", saved_token.as_str()),
        ("board-poll=client-chosen", saved_token.as_str()),
        (other_cookie.as_str(), saved_token.as_str()),
        (saved_cookie.as_str(), cross_poll_token.as_str()),
        (saved_cookie.as_str(), "untrusted"),
    ] {
        let rejected = post(&app, first, cookie_, &form(token_, "7")).await;
        assert_eq!(rejected.status, StatusCode::FORBIDDEN);
        assert!(rejected.headers.get("set-cookie").is_none());
    }
    let key = PosterIdKey::parse(&"11".repeat(32))
        .unwrap()
        .poll_voting_key();
    let now = chrono::Utc::now().timestamp();
    let expired_voter = key.generate_voter(now - 3600).unwrap();
    let expired_cookie = format!("board-poll={}", expired_voter.credential());
    let expired_token = key.form_token(&expired_voter, first, now - 3600).unwrap();
    assert_eq!(
        post(&app, first, &expired_cookie, &form(&expired_token, "7"))
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    let duplicate = format!("{saved_cookie}; {saved_cookie}");
    assert_eq!(
        get(&app, first, Some(&duplicate)).await.status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        post(&app, first, &duplicate, &form(&saved_token, "7"))
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
    let forged = get(&app, first, Some("board-poll=client-chosen")).await;
    assert_ne!(cookie(&forged), "board-poll=client-chosen");

    for invalid in [
        "action=vote&_ptkn=missing-option".to_owned(),
        format!("{}&id=9", form(&saved_token, "7")),
        format!("{}&_ptkn=other", form(&saved_token, "7")),
        format!("{}&unexpected=1", form(&saved_token, "7")),
        form(&saved_token, "-1"),
        form(&saved_token, "9223372036854775808"),
        form(&saved_token, "7junk"),
        form(&saved_token, "7").replace("action=vote", "action=delete"),
    ] {
        assert_eq!(
            post(&app, first, &saved_cookie, &invalid).await.status,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    assert_eq!(
        post(&app, first, &saved_cookie, &"x".repeat(1025))
            .await
            .status,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    for headers in [
        vec![
            ("content-type", "application/x-www-form-urlencoded"),
            ("cookie", saved_cookie.as_str()),
        ],
        vec![
            ("origin", "https://foreign.invalid"),
            ("content-type", "application/x-www-form-urlencoded"),
            ("cookie", saved_cookie.as_str()),
        ],
        vec![
            ("origin", ORIGIN),
            ("sec-fetch-site", "cross-site"),
            ("content-type", "application/x-www-form-urlencoded"),
            ("cookie", saved_cookie.as_str()),
        ],
    ] {
        assert_eq!(
            send(
                &app,
                Method::POST,
                &format!("/polls/{first}"),
                &form(&saved_token, "7"),
                &headers
            )
            .await
            .status,
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        post(&app, first, &saved_cookie, &form(&saved_token, "77"))
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        counts(owner, first).await,
        (6, 0, 0, vec![Some(2), Some(4)])
    );

    let voted = post(&app, first, &saved_cookie, &form(&saved_token, "7")).await;
    assert_eq!(voted.status, StatusCode::SEE_OTHER, "{}", voted.body);
    assert_eq!(voted.headers["location"], format!("/polls/results/{first}"));
    assert!(voted.headers.get("set-cookie").is_none());
    private(&voted);
    assert_eq!(
        counts(owner, first).await,
        (7, 1, 1, vec![Some(3), Some(4)])
    );
    let duplicate = post(&app, first, &saved_cookie, &form(&saved_token, "9")).await;
    assert_eq!(duplicate.status, StatusCode::SEE_OTHER);
    assert_eq!(
        counts(owner, first).await,
        (7, 1, 1, vec![Some(3), Some(4)])
    );
    let own_results = get(&app, first, Some(&saved_cookie)).await;
    assert!(
        own_results
            .body
            .contains("class=\"poll-res-tbl pollResults\"")
    );
    assert!(own_results.body.contains("42.86% (3)"));
    assert!(!own_results.body.contains("<form") && own_results.headers.get("set-cookie").is_none());
    private(&own_results);
    let public_results = send(
        &app,
        Method::GET,
        &format!("/polls/results/{first}"),
        "",
        &[],
    )
    .await;
    assert_eq!(public_results.status, StatusCode::OK);
    assert!(public_results.body.contains("Total votes: 7"));
    assert!(public_results.headers.get("set-cookie").is_none());

    // A fresh router supplies an independent process rate budget for the
    // remaining admission cases; vote identities still live in PostgreSQL.
    let app = router(public);
    assert_eq!(
        post(&app, first, &other_cookie, &form(&other_token, "9"))
            .await
            .status,
        StatusCode::SEE_OTHER
    );
    assert_eq!(
        counts(owner, first).await,
        (8, 2, 2, vec![Some(3), Some(5)])
    );
    sqlx::query("UPDATE poll_private.polls SET accepting_votes=false WHERE id=$1")
        .bind(first)
        .execute(owner)
        .await
        .unwrap();
    assert_eq!(
        post(&app, first, &saved_cookie, &form(&saved_token, "7"))
            .await
            .status,
        StatusCode::SEE_OTHER
    );
    let new_voter = key.generate_voter(now).unwrap();
    let new_cookie = format!("board-poll={}", new_voter.credential());
    let closed_token = key.form_token(&new_voter, first, now).unwrap();
    assert_eq!(
        post(&app, first, &new_cookie, &form(&closed_token, "7"))
            .await
            .status,
        StatusCode::CONFLICT
    );
    assert_eq!(
        counts(owner, first).await,
        (8, 2, 2, vec![Some(3), Some(5)])
    );
    let old = get(&app, closed, None).await;
    assert_eq!(old.status, StatusCode::OK);
    assert!(old.body.contains("Voting is unavailable.") && !old.body.contains("<form"));
    assert!(old.headers.get("set-cookie").is_none());
    for id in [hidden, hidden + 100] {
        let hidden_token = key.form_token(&new_voter, id, now).unwrap();
        let response = post(&app, id, &new_cookie, &form(&hidden_token, "7")).await;
        assert_eq!(response.status, StatusCode::NOT_FOUND);
        assert!(!response.body.contains("Owned") && response.headers.get("set-cookie").is_none());
    }
    let direct = get(&app, unlisted, None).await;
    assert!(!token(&direct).is_empty());
    assert_eq!(
        post(
            &app,
            unlisted,
            &cookie(&direct),
            &form(&token(&direct), "7")
        )
        .await
        .status,
        StatusCode::SEE_OTHER
    );
    assert_eq!(
        counts(owner, unlisted).await,
        (1, 1, 1, vec![Some(1), Some(0)])
    );
    sqlx::query("UPDATE poll_private.polls SET published=false WHERE id=$1")
        .bind(first)
        .execute(owner)
        .await
        .unwrap();
    assert_eq!(
        post(&app, first, &saved_cookie, &form(&saved_token, "7"))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    sqlx::query("UPDATE poll_private.options SET score=NULL WHERE poll_id=$1 AND id=77")
        .bind(second)
        .execute(owner)
        .await
        .unwrap();
    let incomplete = get(&app, second, None).await;
    assert_eq!(incomplete.status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(incomplete.headers.get("set-cookie").is_none() && !incomplete.body.contains("<form"));
    sqlx::query("DELETE FROM poll_private.options WHERE poll_id=$1")
        .bind(second)
        .execute(owner)
        .await
        .unwrap();
    let empty = get(&app, second, None).await;
    assert_eq!(empty.status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(empty.headers.get("set-cookie").is_none() && !empty.body.contains("<form"));
    assert_eq!(counts(owner, second).await, (0, 0, 0, vec![]));
}

#[tokio::test]
async fn native_poll_forms_bind_votes_to_cookie_poll_and_expiry_without_exposing_private_receipts()
{
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut lock = owner.acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock(2250109)")
        .execute(&mut *lock)
        .await
        .unwrap();
    let mut nonce = [0u8; 8];
    OsRng.fill_bytes(&mut nonce);
    let base = 20_000_000_000_i64 + (u64::from_be_bytes(nonce) % 1_000_000_000) as i64 * 8;
    let ids = [base, base + 1, base + 2, base + 3, base + 4];
    let marker = format!("OwnedPollHttp{base}");
    let result = tokio::spawn({
        let owner = owner.clone(); let public = public.clone(); let marker = marker.clone();
        async move {
            let mut tx = owner.begin().await.unwrap();
            for (index, id) in ids.iter().enumerate() {
                sqlx::query("INSERT INTO poll_private.polls(id,title,description,vote_count,published,accepting_votes) VALUES($1,$2,'Owned <description> & text',$3,$4,$5)")
                    .bind(id).bind(&marker).bind(if index == 0 { 6_i64 } else { 0 }).bind(index != 3).bind(index != 2)
                    .execute(&mut *tx).await.unwrap();
                for (option, ordinal, caption, score) in [(if index == 1 { 77_i64 } else { 7 }, 1_i32, "Owned <caption> & first", if index == 0 { 2_i64 } else { 0 }), (9, 2, "Owned second", if index == 0 { 4 } else { 0 })] {
                    sqlx::query("INSERT INTO poll_private.options(poll_id,id,ordinal,caption,score) VALUES($1,$2,$3,$4,$5)")
                        .bind(id).bind(option).bind(ordinal).bind(caption).bind(score).execute(&mut *tx).await.unwrap();
                }
            }
            tx.commit().await.unwrap();
            exercise(&owner, &public, ids).await;
        }
    }).await;
    public.close().await;
    sqlx::query("DELETE FROM poll_private.polls WHERE id=ANY($1) AND title=$2")
        .bind(ids.as_slice())
        .bind(&marker)
        .execute(&owner)
        .await
        .unwrap();
    let leftovers: i64 =
        sqlx::query_scalar("SELECT count(*) FROM poll_private.votes WHERE poll_id=ANY($1)")
            .bind(ids.as_slice())
            .fetch_one(&owner)
            .await
            .unwrap();
    assert_eq!(leftovers, 0);
    sqlx::query("SELECT pg_advisory_unlock(2250109)")
        .execute(&mut *lock)
        .await
        .unwrap();
    drop(lock);
    owner.close().await;
    result.unwrap();
}
