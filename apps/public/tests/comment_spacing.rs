#![cfg(feature = "database-tests")]

use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use board_store::{NewPost, StoreError};
use rand_core::{OsRng, RngCore};
use sqlx::PgPool;
use std::time::Duration;
use tower::ServiceExt;

#[path = "support/subjects.rs"]
mod subject_cases;

fn post(comment: &str) -> NewPost {
    NewPost {
        name: "Anonymous".into(),
        subject: String::new(),
        comment: comment.into(),
        deletion_hash: "owned-test-hash".into(),
        sage: false,
    }
}

async fn exercise(owner: PgPool, public: PgPool, slug: String) {
    let thread = board_store::create_post(&public, &slug, 0, &post("Owned OP"))
        .await
        .unwrap();
    // Policy changes apply only to new posts, including an existing unnormalized row.
    let historical = " historical\t  text Ｚ😀\u{31350}\r\n\r\n\r\n\r\n ";
    sqlx::query("UPDATE content.posts SET comment=$2 WHERE id=$1")
        .bind(thread)
        .bind(historical)
        .execute(&owner)
        .await
        .unwrap();
    // The expanded sanitation matrix exceeds the default single-peer write budget.
    // Keep this fixture bounded; http_limits.rs exercises actual throttling.
    let limits = board_config::PublicRequestLimits::from_lookup(|key| match key {
        "PUBLIC_WRITES_PER_MINUTE" => Some("60".into()),
        _ => None,
    })
    .unwrap();
    let (app, _) = board_public::routers_with_limits(
        public.clone(),
        "http://127.0.0.1:3000".into(),
        false,
        None,
        limits,
    );
    let raw = " \t A\t  B\r\n \r\n　\r\n\t\r\nC <script> Ｚⓦ✘😀│𠮷 \r\n";
    for (index, (code, sjis, expected)) in [
        (false, false, "A B\nC <script> awx𠮷"),
        (true, false, "A      B\n \n \n    \nC <script> awx𠮷"),
        (false, true, "A      B\n \n　\n    \nC <script> Ｚ│𠮷"),
        (true, true, "A      B\n \n　\n    \nC <script> Ｚ│𠮷"),
    ]
    .into_iter()
    .enumerate()
    {
        sqlx::query("UPDATE content.boards SET comment_code_spacing=$2,comment_sjis_spacing=$3 WHERE slug=$1")
            .bind(&slug).bind(code).bind(sjis).execute(&owner).await.unwrap();
        let id = board_store::create_post(&public, &slug, thread, &post(raw))
            .await
            .unwrap();
        assert_eq!(
            board_store::find_post(&public, &slug, id)
                .await
                .unwrap()
                .comment,
            expected
        );
        // Both public aliases and encodings reach the same persisted policy.
        let route = if index % 2 == 0 {
            "post"
        } else {
            "imgboard.php"
        };
        let fields = [
            ("mode", "regist".to_owned()),
            ("resto", thread.to_string()),
            ("com", raw.to_owned()),
            ("pwd", "owned-password".to_owned()),
        ];
        let (content_type, body) = if index < 2 {
            let body = url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs(fields.iter().map(|(key, value)| (*key, value)))
                .finish();
            ("application/x-www-form-urlencoded", body)
        } else {
            let mut body = String::new();
            for (name, value) in fields {
                body.push_str(&format!("--owned-spacing\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"));
            }
            body.push_str("--owned-spacing--\r\n");
            ("multipart/form-data; boundary=owned-spacing", body)
        };
        let response = app
            .clone()
            .oneshot(
                Request::post(format!("/{slug}/{route}"))
                    .header("origin", "http://127.0.0.1:3000")
                    .header("accept", "application/json")
                    .header("content-type", content_type)
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let value: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        let id = value["pid"].as_i64().expect("accepted owned post");
        assert_eq!(
            board_store::find_post(&public, &slug, id)
                .await
                .unwrap()
                .comment,
            expected
        );
        let response = app
            .clone()
            .oneshot(
                Request::get(format!("/{slug}/thread/{thread}.json"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let json: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
                .unwrap();
        let saved = json["posts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|post| post["no"] == id)
            .unwrap();
        assert_eq!(
            saved["com"],
            expected
                .replace('<', "&#60;")
                .replace('>', "&#62;")
                .replace('\n', "<br>")
        );
    }
    assert_eq!(
        board_store::find_post(&public, &slug, thread)
            .await
            .unwrap()
            .comment,
        historical
    );
    let html = app
        .clone()
        .oneshot(
            Request::get(format!("/{slug}/thread/{thread}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(html.status(), 200);
    let html = String::from_utf8(
        to_bytes(html.into_body(), 1024 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(html.contains("C &#60;script&#62;"));
    assert!(!html.contains("C <script>"));
    local_quotes(&app, &owner, &public, &slug, thread).await;
    line_rules(&app, &owner, &public, &slug, thread).await;
    subject_cases::exercise(&app, &owner, &public, &slug, thread).await;

    let denied = sqlx::query("UPDATE content.boards SET comment_code_spacing=true,comment_sjis_spacing=true WHERE slug=$1")
        .bind(&slug).execute(&public).await.unwrap_err();
    assert_eq!(
        denied.as_database_error().unwrap().code().as_deref(),
        Some("42501")
    );

    // Posting must use the committed policy after its actual board-lock wait.
    for code in [true, false] {
        let mut locked = owner.begin().await.unwrap();
        let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *locked)
            .await
            .unwrap();
        sqlx::query("UPDATE content.boards SET comment_code_spacing=$2,comment_sjis_spacing=false WHERE slug=$1")
            .bind(&slug).bind(code).execute(&mut *locked).await.unwrap();
        let pending = {
            let public = public.clone();
            let slug = slug.clone();
            tokio::spawn(async move {
                board_store::create_post(&public, &slug, thread, &post("Ａ\t  B😀")).await
            })
        };
        let observed = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_locks WHERE NOT granted AND $1=ANY(pg_blocking_pids(pid)))")
                    .bind(pid).fetch_one(&owner).await.unwrap();
                if waiting { break; }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await;
        locked.commit().await.unwrap();
        let result = pending.await.unwrap();
        observed.expect("actual posting transaction waited on the owned board lock");
        let id = result.unwrap();
        assert_eq!(
            board_store::find_post(&public, &slug, id)
                .await
                .unwrap()
                .comment,
            if code { "A      B" } else { "A B" }
        );
    }

    sqlx::query(
        "UPDATE content.boards SET max_comment_chars=3,comment_code_spacing=true WHERE slug=$1",
    )
    .bind(&slug)
    .execute(&owner)
    .await
    .unwrap();
    let id = board_store::create_post(&public, &slug, thread, &post("A\tB"))
        .await
        .unwrap();
    assert_eq!(
        board_store::find_post(&public, &slug, id)
            .await
            .unwrap()
            .comment,
        "A    B"
    );
    let id = board_store::create_post(&public, &slug, thread, &post("😀X"))
        .await
        .unwrap();
    assert_eq!(
        board_store::find_post(&public, &slug, id)
            .await
            .unwrap()
            .comment,
        "X"
    );
    let before = board_store::thread(&public, &slug, thread).await.unwrap();
    for route in ["post", "imgboard.php"] {
        for raw in ["😀", "\u{31350}", "|||", "😀😀😀X"] {
            let body = url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs([
                    ("mode", "regist"),
                    ("resto", &thread.to_string()),
                    ("com", raw),
                    ("pwd", "owned-password"),
                ])
                .finish();
            let response = app
                .clone()
                .oneshot(
                    Request::post(format!("/{slug}/{route}"))
                        .header("origin", "http://127.0.0.1:3000")
                        .header("content-type", "application/x-www-form-urlencoded")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), 422, "{route}: {raw:?}");
        }
    }
    assert!(matches!(
        board_store::create_post(&public, &slug, thread, &post(" A  ")).await,
        Err(StoreError::Invalid(_))
    ));
    sqlx::query("UPDATE content.boards SET max_comment_chars=16000 WHERE slug=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    assert!(matches!(
        board_store::create_post(
            &public,
            &slug,
            thread,
            &post(&format!("A{}B", "\t".repeat(4000)))
        )
        .await,
        Err(StoreError::Invalid(_))
    ));
    let after = board_store::thread(&public, &slug, thread).await.unwrap();
    assert_eq!(
        (after.reply_count, after.modified_at, after.http_modified_at),
        (
            before.reply_count,
            before.modified_at,
            before.http_modified_at
        )
    );
}

async fn local_quotes(
    app: &axum::Router,
    owner: &PgPool,
    public: &PgPool,
    slug: &str,
    thread: i64,
) {
    let target = board_store::create_post(public, slug, thread, &post("Owned quote target"))
        .await
        .unwrap();
    let raw = format!(
        "See >>>/{slug}/{target} and >>>/other/{target}.\n[spoiler]>>>/{slug}/{target}[/spoiler]\n>>>/{slug}/0000 >>>/{slug}/-1"
    );
    let expected = format!(
        "See >>{target} and >>>/other/{target}.\n[spoiler]>>{target}[/spoiler]\n>>0000 >>>/{slug}/-1"
    );
    // Existing text is read literally even after later posts use the rewrite.
    sqlx::query("UPDATE content.posts SET comment=$2 WHERE id=$1")
        .bind(target)
        .bind(&raw)
        .execute(owner)
        .await
        .unwrap();
    for (index, (code, sjis)) in [(false, false), (true, false), (false, true), (true, true)]
        .into_iter()
        .enumerate()
    {
        sqlx::query("UPDATE content.boards SET comment_code_spacing=$2,comment_sjis_spacing=$3 WHERE slug=$1")
            .bind(slug).bind(code).bind(sjis).execute(owner).await.unwrap();
        let route = if index % 2 == 0 {
            "post"
        } else {
            "imgboard.php"
        };
        let fields = [
            ("mode", "regist".to_owned()),
            ("resto", thread.to_string()),
            ("com", raw.clone()),
            ("pwd", "owned-password".to_owned()),
        ];
        let (content_type, body) = if index < 2 {
            (
                "application/x-www-form-urlencoded",
                url::form_urlencoded::Serializer::new(String::new())
                    .extend_pairs(fields.iter().map(|(key, value)| (*key, value)))
                    .finish(),
            )
        } else {
            let mut body = String::new();
            for (name, value) in fields {
                body.push_str(&format!("--owned-quotes\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"));
            }
            body.push_str("--owned-quotes--\r\n");
            ("multipart/form-data; boundary=owned-quotes", body)
        };
        let response = app
            .clone()
            .oneshot(
                Request::post(format!("/{slug}/{route}"))
                    .header("origin", "http://127.0.0.1:3000")
                    .header("accept", "application/json")
                    .header("content-type", content_type)
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let value: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        let id = value["pid"].as_i64().expect("accepted owned quote post");
        assert_eq!(
            board_store::find_post(public, slug, id)
                .await
                .unwrap()
                .comment,
            expected
        );
        let response = app
            .clone()
            .oneshot(
                Request::get(format!("/{slug}/thread/{thread}.json"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let json: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
                .unwrap();
        let saved = json["posts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|post| post["no"] == id)
            .unwrap();
        let markup = saved["com"].as_str().unwrap();
        assert!(markup.contains(&format!(
            "href=\"/{slug}/post/{target}\">&gt;&gt;{target}</a>"
        )));
        assert!(markup.contains(&format!(
            "href=\"/other/post/{target}\">&gt;&gt;&gt;/other/{target}</a>"
        )));
        assert!(markup.contains(&format!(
            "aria-label=\"Spoiler; focus to reveal\">&#62;&#62;{target}</span>"
        )));
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
        let html = String::from_utf8(
            to_bytes(response.into_body(), 1024 * 1024)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        let fragment = html
            .split(&format!("id=\"m{id}\""))
            .nth(1)
            .unwrap()
            .split("</blockquote>")
            .next()
            .unwrap();
        assert!(fragment.contains(&format!(
            "href=\"/{slug}/post/{target}\">&gt;&gt;{target}</a>"
        )));
        assert!(fragment.contains(&format!(
            "href=\"/other/post/{target}\">&gt;&gt;&gt;/other/{target}</a>"
        )));
        assert_eq!(
            board_store::find_post(public, slug, target)
                .await
                .unwrap()
                .comment,
            raw
        );
    }
    // A later reduction in raw budget cannot be evaded by shortening a quote.
    sqlx::query("UPDATE content.boards SET max_comment_chars=3 WHERE slug=$1")
        .bind(slug)
        .execute(owner)
        .await
        .unwrap();
    let before = board_store::thread(public, slug, thread).await.unwrap();
    assert!(matches!(
        board_store::create_post(public, slug, thread, &post(&format!(">>>/{slug}/1"))).await,
        Err(StoreError::Invalid(_))
    ));
    let after = board_store::thread(public, slug, thread).await.unwrap();
    assert_eq!(
        (after.reply_count, after.modified_at, after.http_modified_at),
        (
            before.reply_count,
            before.modified_at,
            before.http_modified_at
        )
    );
    sqlx::query("UPDATE content.boards SET max_comment_chars=1000 WHERE slug=$1")
        .bind(slug)
        .execute(owner)
        .await
        .unwrap();
}

async fn line_rules(app: &axum::Router, owner: &PgPool, public: &PgPool, slug: &str, thread: i64) {
    let historical = board_store::find_post(public, slug, thread)
        .await
        .unwrap()
        .comment;
    for (index, (code, sjis)) in [(false, false), (true, false), (false, true), (true, true)]
        .into_iter()
        .enumerate()
    {
        for spoilers in [false, true] {
            sqlx::query("UPDATE content.boards SET comment_max_lines=3,comment_spoiler_cleanup=$2,comment_code_spacing=$3,comment_sjis_spacing=$4 WHERE slug=$1")
                .bind(slug).bind(spoilers).bind(code).bind(sjis).execute(owner).await.unwrap();
            let raw = "a[spoiler]b[/spoiler]c <script>\r\nline1\r\nline2\r\nline3";
            let expected = if spoilers {
                "abc <script>\nline1\nline2\nline3"
            } else {
                "a[spoiler]b[/spoiler]c <script>\nline1\nline2\nline3"
            };
            let response = line_request(app, slug, thread, raw, index).await;
            let id = if index % 2 == 0 {
                assert_eq!(response.status(), 200);
                let value: serde_json::Value =
                    serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap())
                        .unwrap();
                value["pid"].as_i64().expect("accepted line-boundary post")
            } else {
                assert_eq!(response.status(), 303);
                response.headers()["location"]
                    .to_str()
                    .unwrap()
                    .rsplit("#p")
                    .next()
                    .unwrap()
                    .parse()
                    .unwrap()
            };
            assert_eq!(
                board_store::find_post(public, slug, id)
                    .await
                    .unwrap()
                    .comment,
                expected
            );
            let response = app
                .clone()
                .oneshot(
                    Request::get(format!("/{slug}/thread/{thread}.json"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            let json: serde_json::Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
                    .unwrap();
            let saved = json["posts"]
                .as_array()
                .unwrap()
                .iter()
                .find(|post| post["no"] == id)
                .unwrap()["com"]
                .as_str()
                .unwrap();
            assert!(saved.contains("&#60;script&#62;<br>line1<br>line2<br>line3"));
            assert_eq!(saved.contains("class=\"spoiler\""), !spoilers);
            let before = board_store::thread(public, slug, thread).await.unwrap();
            for (raw, error) in [
                (
                    "line0\nline1\nline2\nline3\nline4".to_owned(),
                    "Error: Too many lines.",
                ),
                (
                    "x\n".repeat(7) + "end",
                    "Error: Our system thinks your post is spam.",
                ),
            ] {
                let response = line_request(app, slug, thread, &raw, index).await;
                if index % 2 == 0 {
                    assert_eq!(response.status(), 200);
                    let value: serde_json::Value = serde_json::from_slice(
                        &to_bytes(response.into_body(), 4096).await.unwrap(),
                    )
                    .unwrap();
                    assert_eq!(value, serde_json::json!({"error":error}));
                } else {
                    assert_eq!(response.status(), 422);
                    let html = String::from_utf8(
                        to_bytes(response.into_body(), 65536)
                            .await
                            .unwrap()
                            .to_vec(),
                    )
                    .unwrap();
                    assert!(html.contains(error));
                }
                assert!(
                    matches!(board_store::create_post(public, slug, thread, &post(&raw)).await, Err(StoreError::Invalid(message)) if message == error)
                );
            }
            let after = board_store::thread(public, slug, thread).await.unwrap();
            assert_eq!(
                (after.reply_count, after.modified_at, after.http_modified_at),
                (
                    before.reply_count,
                    before.modified_at,
                    before.http_modified_at
                )
            );
        }
    }
    let denied = sqlx::query(
        "UPDATE content.boards SET comment_max_lines=0,comment_spoiler_cleanup=true WHERE slug=$1",
    )
    .bind(slug)
    .execute(public)
    .await
    .unwrap_err();
    assert_eq!(
        denied.as_database_error().unwrap().code().as_deref(),
        Some("42501")
    );
    // A stale policy read must not decide admission or stored spoiler cleanup.
    for (limit, spoilers) in [(1, true), (0, true), (1, false)] {
        let before = board_store::thread(public, slug, thread).await.unwrap();
        let mut locked = owner.begin().await.unwrap();
        let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *locked)
            .await
            .unwrap();
        sqlx::query("UPDATE content.boards SET comment_max_lines=$2,comment_spoiler_cleanup=$3 WHERE slug=$1")
            .bind(slug).bind(limit).bind(spoilers).execute(&mut *locked).await.unwrap();
        let pending = {
            let public = public.clone();
            let slug = slug.to_owned();
            tokio::spawn(async move {
                board_store::create_post(
                    &public,
                    &slug,
                    thread,
                    &post("a[spoiler]b[/spoiler]c\nend"),
                )
                .await
            })
        };
        let observed = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_locks WHERE NOT granted AND $1=ANY(pg_blocking_pids(pid)))")
                    .bind(pid).fetch_one(owner).await.unwrap();
                if waiting { break; }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await;
        locked.commit().await.unwrap();
        let result = pending.await.unwrap();
        observed.expect("posting waited on the owned line-policy lock");
        if limit == 0 {
            assert!(matches!(
                result,
                Err(StoreError::Invalid("Error: Too many lines."))
            ));
            let after = board_store::thread(public, slug, thread).await.unwrap();
            assert_eq!(
                (after.reply_count, after.modified_at, after.http_modified_at),
                (
                    before.reply_count,
                    before.modified_at,
                    before.http_modified_at
                )
            );
        } else {
            assert_eq!(
                board_store::find_post(public, slug, result.unwrap())
                    .await
                    .unwrap()
                    .comment,
                if spoilers {
                    "abc\nend"
                } else {
                    "a[spoiler]b[/spoiler]c\nend"
                }
            );
        }
    }
    assert_eq!(
        board_store::find_post(public, slug, thread)
            .await
            .unwrap()
            .comment,
        historical
    );
    sqlx::query("UPDATE content.boards SET comment_max_lines=70,comment_spoiler_cleanup=false WHERE slug=$1")
        .bind(slug).execute(owner).await.unwrap();
}

async fn line_request(
    app: &axum::Router,
    slug: &str,
    thread: i64,
    raw: &str,
    index: usize,
) -> axum::response::Response {
    let fields = [
        ("mode", "regist".to_owned()),
        ("resto", thread.to_string()),
        ("com", raw.to_owned()),
        ("pwd", "owned-password".to_owned()),
    ];
    let (kind, body) = if index < 2 {
        (
            "application/x-www-form-urlencoded",
            url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs(fields.iter().map(|(key, value)| (*key, value)))
                .finish(),
        )
    } else {
        let mut body = String::new();
        for (name, value) in fields {
            body.push_str(&format!("--owned-lines\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"));
        }
        body.push_str("--owned-lines--\r\n");
        ("multipart/form-data; boundary=owned-lines", body)
    };
    let (route, accept) = if index.is_multiple_of(2) {
        ("post", "application/json")
    } else {
        ("imgboard.php", "text/html")
    };
    app.clone()
        .oneshot(
            Request::post(format!("/{slug}/{route}"))
                .header("origin", "http://127.0.0.1:3000")
                .header("accept", accept)
                .header("content-type", kind)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn persisted_source_spacing_uses_locked_operator_policy_and_escaped_rendering() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut random = [0u8; 5];
    OsRng.fill_bytes(&mut random);
    let slug: String = random.iter().map(|b| format!("{b:02x}")).collect();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Spacing','Owned fixture',1000,100,100,10,10)")
        .bind(&slug).execute(&owner).await.unwrap();
    let result = tokio::spawn(exercise(owner.clone(), public.clone(), slug.clone())).await;
    public.close().await;
    sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)").bind(&slug).execute(&owner).await.unwrap();
    for statement in [
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
