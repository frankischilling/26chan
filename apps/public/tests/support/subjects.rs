use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
};
use board_store::{NewPost, StoreError};
use sqlx::PgPool;
use std::time::Duration;
use tower::ServiceExt;

fn post(subject: &str) -> NewPost {
    NewPost {
        name: "Anonymous".into(),
        subject: subject.into(),
        comment: "Owned subject post".into(),
        deletion_hash: "owned-test-hash".into(),
        sage: false,
    }
}

pub async fn exercise(app: &Router, owner: &PgPool, public: &PgPool, slug: &str, thread: i64) {
    let historical = " Ｚ##\r\nold ";
    sqlx::query("UPDATE content.posts SET subject=$2 WHERE id=$1")
        .bind(thread)
        .bind(historical)
        .execute(owner)
        .await
        .unwrap();
    for (index, (code, sjis)) in [(false, false), (true, false), (false, true), (true, true)]
        .into_iter()
        .enumerate()
    {
        sqlx::query("UPDATE content.boards SET comment_code_spacing=$2,comment_sjis_spacing=$3 WHERE slug=$1")
            .bind(slug).bind(code).bind(sjis).execute(owner).await.unwrap();
        let raw = " Ｚ##ⓦ\t  <b>│\r\nEND \u{31350}";
        let expected = if sjis {
            "aw      <b>│END "
        } else if code {
            "aw      <b>END "
        } else {
            "aw <b>END "
        };
        let fields = [
            ("mode", "regist".to_owned()),
            ("resto", thread.to_string()),
            ("com", "Owned subject reply".to_owned()),
            ("sub", raw.to_owned()),
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
                body.push_str(&format!("--owned-subject\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"));
            }
            body.push_str("--owned-subject--\r\n");
            ("multipart/form-data; boundary=owned-subject", body)
        };
        let route = if index % 2 == 0 {
            "post"
        } else {
            "imgboard.php"
        };
        let response = app
            .clone()
            .oneshot(
                Request::post(format!("/{slug}/{route}"))
                    .header("origin", "http://127.0.0.1:3000")
                    .header("accept", "application/json")
                    .header("content-type", kind)
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let json: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        let id = json["pid"].as_i64().expect("accepted subject reply");
        assert_eq!(json["tid"], thread);
        assert_eq!(
            board_store::find_post(public, slug, id)
                .await
                .unwrap()
                .subject,
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
        assert_eq!(
            json["posts"]
                .as_array()
                .unwrap()
                .iter()
                .find(|post| post["no"] == id)
                .unwrap()["sub"],
            expected.replace('<', "&lt;").replace('>', "&gt;")
        );
        let response = app
            .clone()
            .oneshot(
                Request::get(format!("/{slug}/thread/{thread}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let html = String::from_utf8(
            to_bytes(response.into_body(), 1024 * 1024)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        let reply = html
            .split(&format!("id=\"pi{id}\""))
            .nth(1)
            .unwrap()
            .split("</div>")
            .next()
            .unwrap();
        // Source retains reply subjects in JSON but suppresses them in HTML.
        assert!(!reply.contains("class=\"subject\""));
        assert!(!reply.contains("<b>"));
    }
    for code in [false, true] {
        let mut locked = owner.begin().await.unwrap();
        let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *locked)
            .await
            .unwrap();
        sqlx::query("UPDATE content.boards SET comment_code_spacing=$2,comment_sjis_spacing=false WHERE slug=$1")
            .bind(slug).bind(code).execute(&mut *locked).await.unwrap();
        let pending = {
            let public = public.clone();
            let slug = slug.to_owned();
            tokio::spawn(async move {
                board_store::create_post(&public, &slug, thread, &post("Ａ\tＢ")).await
            })
        };
        let observed = tokio::time::timeout(Duration::from_secs(5), async { loop {
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_locks WHERE NOT granted AND $1=ANY(pg_blocking_pids(pid)))").bind(pid).fetch_one(owner).await.unwrap();
            if waiting { break; } tokio::time::sleep(Duration::from_millis(10)).await;
        } }).await;
        locked.commit().await.unwrap();
        let id = pending.await.unwrap().unwrap();
        observed.expect("subject waited for committed board policy");
        assert_eq!(
            board_store::find_post(public, slug, id)
                .await
                .unwrap()
                .subject,
            if code { "A    B" } else { "A B" }
        );
    }
    let raw = format!("A{}B", "\t".repeat(98));
    let id = board_store::create_post(public, slug, 0, &post(&raw))
        .await
        .unwrap();
    let expanded = format!("A{}B", " ".repeat(392));
    assert_eq!(
        board_store::find_post(public, slug, id)
            .await
            .unwrap()
            .subject,
        expanded
    );
    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/{slug}/catalog"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let html = String::from_utf8(
        to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(html.contains(&format!("<b>{expanded}</b>")));
    assert_eq!(
        board_store::find_post(public, slug, thread)
            .await
            .unwrap()
            .subject,
        historical
    );
    let before = board_store::thread(public, slug, thread).await.unwrap();
    for raw in ["#".repeat(101), "😀".repeat(26)] {
        assert!(matches!(
            board_store::create_post(public, slug, thread, &post(&raw)).await,
            Err(StoreError::Invalid("Name or subject is too long."))
        ));
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
