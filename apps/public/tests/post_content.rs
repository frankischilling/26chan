#![cfg(feature = "database-tests")]

#[path = "support/posting.rs"]
mod posting;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
};
use board_store::{NewPost, StoreError};
use rand_core::{OsRng, RngCore};
use sqlx::PgPool;
use std::time::Duration;
use tower::ServiceExt;

fn post(subject: &str) -> NewPost {
    NewPost {
        name: "Anonymous".into(),
        subject: subject.into(),
        comment: "Owned required-subject post".into(),
        deletion_hash: "owned-test-hash".into(),
        sage: false,
    }
}

async fn snapshot(owner: &PgPool, slug: &str) -> String {
    sqlx::query_scalar("SELECT jsonb_build_object('posts',(SELECT count(*) FROM content.posts WHERE board=$1),'threads',(SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM content.threads t WHERE board=$1),'sequence',(SELECT last_value FROM content.post_number))::text")
        .bind(slug).fetch_one(owner).await.unwrap()
}

async fn submit(
    app: &Router,
    slug: &str,
    index: usize,
    parent: i64,
    subject: &str,
    comment: &str,
) -> axum::response::Response {
    submit_options(app, slug, index, parent, (subject, comment), "").await
}

async fn submit_options(
    app: &Router,
    slug: &str,
    index: usize,
    parent: i64,
    fields: (&str, &str),
    options: &str,
) -> axum::response::Response {
    let (subject, comment) = fields;
    let fields = [
        ("email", options.to_owned()),
        ("mode", "regist".to_owned()),
        ("resto", parent.to_string()),
        ("sub", subject.to_owned()),
        ("com", comment.to_owned()),
        ("pwd", "owned-password".to_owned()),
    ];
    let (kind, body) = if index & 1 == 0 {
        (
            "application/x-www-form-urlencoded",
            url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs(fields.iter().map(|(key, value)| (*key, value)))
                .finish(),
        )
    } else {
        let mut body = String::new();
        for (name, value) in fields {
            body.push_str(&format!("--owned-required\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"));
        }
        body.push_str("--owned-required--\r\n");
        ("multipart/form-data; boundary=owned-required", body)
    };
    let route = if index & 2 == 0 {
        "post"
    } else {
        "imgboard.php"
    };
    let accept = if index & 4 == 0 {
        "application/json"
    } else {
        "text/html"
    };
    app.clone()
        .oneshot(
            Request::post(format!("/{slug}/{route}"))
                .extension(posting::distinct_transport(slug))
                .header("origin", "http://127.0.0.1:3000")
                .header("accept", accept)
                .header("content-type", kind)
                // These public hints must never replace the trusted socket peer.
                .header("x-forwarded-for", "203.0.113.9")
                .header("x-geo-asn", "64512")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap()
}

fn network_router(
    pool: PgPool,
    board: &str,
    peer: &str,
    database: Option<std::sync::Arc<board_domain::content_admission::network::asn::Database>>,
) -> Router {
    let options = board_public::PublicRouterOptions {
        origin: "http://127.0.0.1:3000".into(),
        production: false,
        media: None,
        limits: board_config::PublicRequestLimits::default(),
        proxy_uid: None,
        poster_id_key: Some(posting::key(board)),
        tripcode_key: None,
        country_database: None,
    };
    board_public::routers_with_network(pool, options, None, database, None)
        .0
        .layer(axum::Extension(axum::extract::ConnectInfo(
            std::net::SocketAddr::new(peer.parse().unwrap(), 40000),
        )))
}

async fn accepted_id(response: axum::response::Response, index: usize) -> i64 {
    if index & 4 == 0 {
        assert_eq!(response.status(), 200);
        let json: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
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

async fn exercise(owner: PgPool, public: PgPool, slug: String) {
    sqlx::query("UPDATE content.boards SET comment_spoiler_cleanup=true WHERE slug=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    let (app, api) = posting::routers(public.clone(), &slug, "http://127.0.0.1:3000".into(), false);
    let mut thread = 0;
    // Both post routes, form encodings and response modes use final rendered
    // content. A subject admits an OP but never an unattached blank reply.
    for index in 0..8 {
        let id = accepted_id(
            submit(&app, &slug, index, 0, "Owned subject", "").await,
            index,
        )
        .await;
        thread = id;
        let saved = board_store::find_post(&public, &slug, id).await.unwrap();
        assert_eq!(saved.subject, "Owned subject");
        assert!(saved.comment.is_empty());
        let response = api
            .clone()
            .oneshot(
                Request::get(format!("/{slug}/thread/{id}.json"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let json: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 16384).await.unwrap()).unwrap();
        assert_eq!(json["posts"][0]["sub"], "Owned subject");
        assert!(json["posts"][0].get("com").is_none());
        for (parent, subject, error) in [
            (0, "", "Error: New threads require a subject or comment."),
            (id, "Reply subject", "Error: No text entered."),
        ] {
            let before = snapshot(&owner, &slug).await;
            let response = submit(
                &app,
                &slug,
                index,
                parent,
                subject,
                "[spoiler] \n[/spoiler]",
            )
            .await;
            assert_eq!(response.status(), if index & 4 == 0 { 200 } else { 422 });
            let bytes = to_bytes(response.into_body(), 16384).await.unwrap();
            if index & 4 == 0 {
                let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(json["error"], error);
                assert!(json.get("pid").is_none());
            } else {
                assert!(std::str::from_utf8(&bytes).unwrap().contains(error));
            }
            assert_eq!(snapshot(&owner, &slug).await, before);
        }
    }
    for mask in 0..8 {
        sqlx::query("UPDATE content.boards SET comment_spoiler_cleanup=$2,comment_code_spacing=$3,comment_sjis_spacing=$4 WHERE slug=$1")
            .bind(&slug).bind(mask & 1 != 0).bind(mask & 2 != 0).bind(mask & 4 != 0)
            .execute(&owner).await.unwrap();
        for (raw, blank) in [
            ("[spoiler] \n[/spoiler]", mask & 1 != 0),
            ("[code]      [/code]", mask & 2 != 0),
            ("[code]       [/code]", false),
            ("[sjis][/sjis]", false),
        ] {
            for parent in [0, thread] {
                let before = snapshot(&owner, &slug).await;
                let draft = NewPost {
                    comment: raw.into(),
                    ..post("")
                };
                let result = posting::create_independent_post(&public, &slug, parent, &draft).await;
                if blank {
                    let expected = if parent == 0 {
                        "Error: New threads require a subject or comment."
                    } else {
                        "Error: No text entered."
                    };
                    assert!(matches!(result, Err(StoreError::Invalid(error)) if error == expected));
                    assert_eq!(snapshot(&owner, &slug).await, before);
                } else {
                    let saved = board_store::find_post(&public, &slug, result.unwrap())
                        .await
                        .unwrap();
                    assert_eq!(saved.comment_format, 232 + mask);
                }
            }
        }
    }
    // Witness an actual blocked insertion in both policy directions. Final
    // admission and the persisted stamp must use the policy after the lock.
    for enabled in [true, false] {
        sqlx::query("UPDATE content.boards SET comment_spoiler_cleanup=$2 WHERE slug=$1")
            .bind(&slug)
            .bind(!enabled)
            .execute(&owner)
            .await
            .unwrap();
        let before = snapshot(&owner, &slug).await;
        let mut locked = owner.begin().await.unwrap();
        let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *locked)
            .await
            .unwrap();
        sqlx::query("UPDATE content.boards SET comment_spoiler_cleanup=$2 WHERE slug=$1")
            .bind(&slug)
            .bind(enabled)
            .execute(&mut *locked)
            .await
            .unwrap();
        let pending = {
            let public = public.clone();
            let slug = slug.clone();
            tokio::spawn(async move {
                posting::create_independent_post(
                    &public,
                    &slug,
                    thread,
                    &NewPost {
                        comment: "[spoiler][/spoiler]".into(),
                        ..post("")
                    },
                )
                .await
            })
        };
        let observed = tokio::time::timeout(Duration::from_secs(5), async { loop {
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_locks WHERE NOT granted AND $1=ANY(pg_blocking_pids(pid)))")
                .bind(pid).fetch_one(&owner).await.unwrap();
            if waiting { break; } tokio::time::sleep(Duration::from_millis(10)).await;
        }}).await;
        locked.commit().await.unwrap();
        let result = pending.await.unwrap();
        observed.expect("posting waited for final markup policy");
        if enabled {
            assert!(matches!(
                result,
                Err(StoreError::Invalid("Error: No text entered."))
            ));
            assert_eq!(snapshot(&owner, &slug).await, before);
        } else {
            assert_eq!(
                board_store::find_post(&public, &slug, result.unwrap())
                    .await
                    .unwrap()
                    .comment_format,
                238
            );
        }
    }
    for enabled in [false, true] {
        sqlx::query("UPDATE content.boards SET dice_roll=$2,fortune_trip=$2 WHERE slug=$1")
            .bind(&slug)
            .bind(enabled)
            .execute(&owner)
            .await
            .unwrap();
        let response = app
            .clone()
            .oneshot(
                Request::get(format!("/{slug}/thread/{thread}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let body = String::from_utf8(
            to_bytes(response.into_body(), 1048576)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        let textarea = body
            .split("<textarea id=\"com\"")
            .nth(1)
            .unwrap()
            .split('>')
            .next()
            .unwrap();
        assert!(textarea.contains(&format!("data-randomizer-content=\"{enabled}\"")));
        assert_eq!(textarea.contains(" required"), !enabled);
    }
    let mut retained = Vec::new();
    for (enabled, profile) in [(false, 0), (true, 0), (true, 4)] {
        sqlx::query(
            "UPDATE content.boards SET word_filter_enabled=$2,word_filter_profile=$3 WHERE slug=$1",
        )
        .bind(&slug)
        .bind(enabled)
        .bind(profile as i16)
        .execute(&owner)
        .await
        .unwrap();
        for options in ["dice+1d1", "fortune"] {
            // Each independent route matrix gets a fresh process-local limiter;
            // production request ceilings remain unchanged.
            let (app, api) =
                posting::routers(public.clone(), &slug, "http://127.0.0.1:3000".into(), false);
            for index in if enabled {
                vec![0, 7]
            } else {
                (0..8).collect()
            } {
                for parent in [0, thread] {
                    let id = accepted_id(
                        submit_options(&app, &slug, index, parent, ("", ""), options).await,
                        index,
                    )
                    .await;
                    let saved = board_store::find_post(&public, &slug, id).await.unwrap();
                    assert!(board_domain::formatting::generated_content_format(
                        saved.comment_format
                    ));
                    assert!(
                        saved
                            .wordfilter_payload
                            .as_ref()
                            .unwrap()
                            .starts_with(b"WF03")
                    );
                    if options == "dice+1d1" {
                        assert_eq!(saved.dice_result.as_deref(), Some("Rolled 1 (1d1)"));
                    } else {
                        assert!(saved.fortune_text.is_some());
                        assert!(saved.fortune_color.is_some());
                    }
                    let prepared = board_domain::wordfiltered_comment::PreparedComment::decode(
                        saved.wordfilter_payload.as_deref().unwrap(),
                    )
                    .unwrap();
                    assert_eq!(prepared.rolls().is_some(), enabled && profile == 4);
                    assert!(saved.separate_dice_result().is_none());
                    assert!(saved.separate_fortune_text().is_none());
                    let thread_id = if parent == 0 { id } else { parent };
                    let response = api
                        .clone()
                        .oneshot(
                            Request::get(format!("/{slug}/thread/{thread_id}.json"))
                                .body(Body::empty())
                                .unwrap(),
                        )
                        .await
                        .unwrap();
                    assert_eq!(response.status(), 200);
                    let json: serde_json::Value = serde_json::from_slice(
                        &to_bytes(response.into_body(), 262144).await.unwrap(),
                    )
                    .unwrap();
                    let value = json["posts"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|post| post["no"].as_i64() == Some(id))
                        .unwrap();
                    let comment = value["com"].as_str().unwrap();
                    assert_eq!(comment, saved.comment);
                    let marker = if options == "dice+1d1" {
                        "Rolled 1 (1d1)"
                    } else {
                        "Your fortune:"
                    };
                    if profile != 4 {
                        assert_eq!(comment.matches(marker).count(), 1);
                    }
                    retained.push((id, saved.comment, saved.wordfilter_payload));
                }
            }
        }
    }
    // Changing board policy must not reroll or re-filter retained generated content.
    sqlx::query("UPDATE content.boards SET word_filter_enabled=false,word_filter_profile=0,dice_roll=false,fortune_trip=false WHERE slug=$1")
        .bind(&slug).execute(&owner).await.unwrap();
    for (id, comment, payload) in retained {
        let saved = board_store::find_post(&public, &slug, id).await.unwrap();
        assert_eq!(saved.comment, comment);
        assert_eq!(saved.wordfilter_payload, payload);
        assert_eq!(
            board_domain::filtered_formatting::source_projection(&saved.formatted_lines()),
            comment
        );
    }
    let range: i64 = sqlx::query_scalar("INSERT INTO admission.network_rules(boards,range_start,range_end) VALUES($1,3221226185,3221226185) RETURNING id")
        .bind(&slug).fetch_one(&owner).await.unwrap();
    let (network_app, _) =
        posting::routers(public.clone(), &slug, "http://127.0.0.1:3000".into(), false);
    let before = snapshot(&owner, &slug).await;
    for index in 0..8 {
        let response = submit(
            &network_app,
            &slug,
            index,
            thread,
            "",
            "Owned network reply",
        )
        .await;
        assert_eq!(response.status(), if index & 4 == 0 { 200 } else { 422 });
        let body = to_bytes(response.into_body(), 262144).await.unwrap();
        if index & 4 == 0 {
            let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert!(
                json["error"]
                    .as_str()
                    .unwrap()
                    .starts_with("Posting from your IP range")
            );
        } else {
            assert!(
                std::str::from_utf8(&body)
                    .unwrap()
                    .contains("Posting from your IP range")
            );
        }
        assert_eq!(snapshot(&owner, &slug).await, before);
    }
    sqlx::query(
        "UPDATE admission.network_rules SET range_start=NULL,range_end=NULL,asn=15169 WHERE id=$1",
    )
    .bind(range)
    .execute(&owner)
    .await
    .unwrap();
    let database = std::sync::Arc::new(
        board_domain::content_admission::network::asn::Database::from_bytes(
            include_bytes!("../../../crates/domain/tests/fixtures/GeoLite2-ASN-Test.mmdb").to_vec(),
        )
        .unwrap(),
    );
    for peer in ["1.0.0.1", "::ffff:1.0.0.1"] {
        let app = network_router(public.clone(), &slug, peer, Some(database.clone()));
        let before = snapshot(&owner, &slug).await;
        let response = submit(&app, &slug, 0, thread, "", "Matching offline ASN").await;
        assert_eq!(response.status(), 200);
        let json: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        assert!(
            json["error"]
                .as_str()
                .unwrap()
                .starts_with("Posting from your IP range")
        );
        assert_eq!(snapshot(&owner, &slug).await, before);
    }
    sqlx::query("UPDATE admission.network_rules SET asn=6730 WHERE id=$1")
        .bind(range)
        .execute(&owner)
        .await
        .unwrap();
    let ipv6 = network_router(
        public.clone(),
        &slug,
        "2001:1700::1",
        Some(database.clone()),
    );
    let before_ipv6 = snapshot(&owner, &slug).await;
    let response = submit(&ipv6, &slug, 0, thread, "", "IPv6 ASN admission").await;
    let json: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .starts_with("Posting from your IP range")
    );
    assert_eq!(snapshot(&owner, &slug).await, before_ipv6);
    sqlx::query("UPDATE admission.network_rules SET asn=15169 WHERE id=$1")
        .bind(range)
        .execute(&owner)
        .await
        .unwrap();
    for peer in ["1.128.0.1", "192.0.2.201"] {
        // The first peer resolves to a different ASN; the second is a successful
        // no-record lookup. The forged x-geo-asn header must not decide either.
        let app = network_router(public.clone(), &slug, peer, Some(database.clone()));
        accepted_id(
            submit(&app, &slug, 0, thread, "", "Unmatched offline ASN").await,
            0,
        )
        .await;
    }
    let unavailable = network_router(public.clone(), &slug, "1.0.0.1", None);
    let before = snapshot(&owner, &slug).await;
    assert_eq!(
        submit(&unavailable, &slug, 0, thread, "", "Missing ASN provider")
            .await
            .status(),
        503
    );
    assert_eq!(snapshot(&owner, &slug).await, before);
    // A successful ASN no-record must continue to a later matching range.
    let later: i64 = sqlx::query_scalar("INSERT INTO admission.network_rules(boards,range_start,range_end,position) VALUES($1,3221226185,3221226185,1) RETURNING id")
        .bind(&slug).fetch_one(&owner).await.unwrap();
    let app = network_router(public.clone(), &slug, "192.0.2.201", Some(database));
    let response = submit(&app, &slug, 0, thread, "", "Later range after ASN miss").await;
    let json: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .starts_with("Posting from your IP range")
    );
    assert_eq!(snapshot(&owner, &slug).await, before);
    sqlx::query("DELETE FROM admission.network_rules WHERE id=$1")
        .bind(later)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM admission.network_rules WHERE id=$1")
        .bind(range)
        .execute(&owner)
        .await
        .unwrap();
    accepted_id(
        submit(
            &network_app,
            &slug,
            0,
            thread,
            "",
            "Route works after owned policy removal",
        )
        .await,
        0,
    )
    .await;
}
#[tokio::test]
async fn post_markup_admission_checks_http_transactions_and_locked_policy() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut random = [0u8; 5];
    OsRng.fill_bytes(&mut random);
    let slug: String = random.iter().map(|b| format!("{b:02x}")).collect();
    // Retain the route and markup matrices' OPs on this owned board without
    // letting the unrelated actor quota preempt content-admission assertions.
    sqlx::query("INSERT INTO content.boards(captcha_enabled,slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,posting_reply_seconds,posting_image_seconds,posting_thread_seconds,user_thread_limit) VALUES(false,$1,'Final content admission','Owned fixture',1000,100,100,100,10,0,0,0,100)").bind(&slug).execute(&owner).await.unwrap();
    let result = tokio::spawn(exercise(owner.clone(), public.clone(), slug.clone())).await;
    public.close().await;
    sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)").bind(&slug).execute(&owner).await.unwrap();
    for statement in [
        "DELETE FROM admission.network_rules WHERE boards=$1",
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
