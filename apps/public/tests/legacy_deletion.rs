#![cfg(feature = "database-tests")]

#[path = "support/posting.rs"]
mod posting;

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use tower::ServiceExt;

const ORIGIN: &str = "http://127.0.0.1:3000";
const PASSWORD: &str = "legacy-deletion-password";

fn form(
    uri: &str,
    fields: &[(&str, &str)],
    origin: Option<&str>,
    multipart: bool,
) -> Request<Body> {
    let mut request = Request::post(uri);
    if let Some(origin) = origin {
        request = request.header("origin", origin);
    }
    let body = if multipart {
        request = request.header("content-type", "multipart/form-data; boundary=owned");
        let mut body = String::new();
        for (name, value) in fields {
            body.push_str(&format!(
                "--owned\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            ));
        }
        body.push_str("--owned--\r\n");
        body
    } else {
        request = request.header("content-type", "application/x-www-form-urlencoded");
        url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(fields.iter().copied())
            .finish()
    };
    request.body(Body::from(body)).unwrap()
}

async fn thread_status(app: &Router, id: &str) -> StatusCode {
    app.clone()
        .oneshot(
            Request::get(format!("/fixture/thread/{id}.json"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn legacy_post_and_delete_share_password_origin_and_board_authorization() {
    let database =
        std::env::var("TEST_PUBLIC_DATABASE_URL").expect("TEST_PUBLIC_DATABASE_URL is required");
    let pool = board_store::connect_public(&database).await.unwrap();
    let admin = sqlx::PgPool::connect(
        &std::env::var("MIGRATION_DATABASE_URL").expect("MIGRATION_DATABASE_URL is required"),
    )
    .await
    .unwrap();
    let app = posting::routers_with_limits(
        pool.clone(),
        "fixture",
        ORIGIN.into(),
        false,
        None,
        board_config::PublicRequestLimits::default(),
    )
    .0;
    for multipart in [false, true] {
        let response = app
            .clone()
            .oneshot(form(
                "/fixture/imgboard.php",
                &[
                    ("mode", "regist"),
                    ("sub", "Owned legacy deletion"),
                    ("com", "A synthetic post for the legacy deletion route."),
                    ("pwd", PASSWORD),
                ],
                Some(ORIGIN),
                multipart,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response.headers()["location"].to_str().unwrap();
        let id = location
            .rsplit('/')
            .next()
            .unwrap()
            .split('#')
            .next()
            .unwrap()
            .to_owned();
        assert_eq!(thread_status(&app, &id).await, StatusCode::OK);
        let popup = app
            .clone()
            .oneshot(
                Request::get(format!("/fixture/imgboard.php?mode=report&no={id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(popup.status(), StatusCode::OK);
        assert_eq!(popup.headers()["x-frame-options"], "DENY");
        assert_eq!(popup.headers()["cache-control"], "private, no-store");
        assert!(
            popup.headers()["content-security-policy"]
                .to_str()
                .unwrap()
                .contains("script-src http://127.0.0.1:3000/static/report-popup.v1.js;")
        );
        let popup = popup.into_body().collect().await.unwrap().to_bytes();
        let popup = std::str::from_utf8(&popup).unwrap();
        assert!(popup.contains("action=\"/fixture/report\""));
        assert!(popup.contains(&format!("name=\"no\" value=\"{id}\"")));
        assert!(popup.contains("name=\"reason\""));
        let report = app
            .clone()
            .oneshot(form(
                "/fixture/report",
                &[
                    ("no", id.as_str()),
                    ("reason", "Owned legacy report <literal>"),
                ],
                Some(ORIGIN),
                false,
            ))
            .await
            .unwrap();
        assert_eq!(report.status(), StatusCode::OK);
        assert_eq!(report.headers()["cache-control"], "private, no-store");
        assert!(report.headers().get("set-cookie").is_some());
        let result = report.into_body().collect().await.unwrap().to_bytes();
        let result = std::str::from_utf8(&result).unwrap();
        assert!(result.contains("<h1>Report received</h1>"));
        assert!(result.contains("data-result=\"success\""));
        assert!(result.contains(&format!("data-post=\"{id}\"")));
        let reason: String = sqlx::query_scalar(
            "SELECT reason FROM content.reports WHERE board='fixture' AND post_id=$1",
        )
        .bind(id.parse::<i64>().unwrap())
        .fetch_one(&admin)
        .await
        .unwrap();
        assert_eq!(reason, "Owned legacy report <literal>");
        for (disable, restore, message) in [
            (
                "UPDATE content.boards SET can_report_posts=false WHERE slug=(SELECT board FROM content.posts WHERE id=$1)",
                "UPDATE content.boards SET can_report_posts=true WHERE slug=(SELECT board FROM content.posts WHERE id=$1)",
                "You cannot report posts on this board.",
            ),
            (
                "UPDATE content.threads SET sticky=true WHERE id=$1",
                "UPDATE content.threads SET sticky=false WHERE id=$1",
                "Error: You cannot report a sticky.",
            ),
            (
                "UPDATE content.posts SET capcode='mod' WHERE id=$1",
                "UPDATE content.posts SET capcode=NULL WHERE id=$1",
                "Error: You cannot report this post.",
            ),
        ] {
            let post_id = id.parse::<i64>().unwrap();
            let private_activity = "SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY a.report_id),'[]'::jsonb) FROM post_secrets.anonymous_reports a JOIN content.reports r ON r.id=a.report_id WHERE r.board='fixture' AND r.post_id=$1";
            let before: serde_json::Value = sqlx::query_scalar(private_activity)
                .bind(post_id)
                .fetch_one(&admin)
                .await
                .unwrap();
            sqlx::query(disable)
                .bind(post_id)
                .execute(&admin)
                .await
                .unwrap();
            let get = app
                .clone()
                .oneshot(
                    Request::get(format!("/fixture/imgboard.php?mode=report&no={id}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            let post = app
                .clone()
                .oneshot(form(
                    "/fixture/report",
                    &[("no", &id), ("reason", "Rejected HTTP report")],
                    Some(ORIGIN),
                    false,
                ))
                .await
                .unwrap();
            sqlx::query(restore)
                .bind(post_id)
                .execute(&admin)
                .await
                .unwrap();
            for response in [get, post] {
                assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
                assert_eq!(response.headers()["cache-control"], "private, no-store");
                assert_eq!(response.headers()["x-frame-options"], "DENY");
                assert!(response.headers().get("set-cookie").is_none());
                let csp = response.headers()["content-security-policy"]
                    .to_str()
                    .unwrap();
                assert!(
                    csp.contains("script-src http://127.0.0.1:3000/static/report-popup.v1.js;")
                );
                assert!(csp.contains("connect-src 'none';"));
                let bytes = response.into_body().collect().await.unwrap().to_bytes();
                let html = std::str::from_utf8(&bytes).unwrap();
                assert!(html.contains(message));
                assert!(html.contains("data-result=\"error\""));
                assert!(!html.contains("Report received"));
                assert!(!html.contains("data-result=\"success\""));
            }
            let reports: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM content.reports WHERE board='fixture' AND post_id=$1",
            )
            .bind(post_id)
            .fetch_one(&admin)
            .await
            .unwrap();
            assert_eq!(reports, 1);
            let after: serde_json::Value = sqlx::query_scalar(private_activity)
                .bind(post_id)
                .fetch_one(&admin)
                .await
                .unwrap();
            assert_eq!(after, before);
        }
        let reply = posting::create_post(
            &pool,
            "fixture",
            id.parse().unwrap(),
            &board_store::NewPost {
                name: "Anonymous".into(),
                subject: String::new(),
                comment: "Owned reportable reply in sticky thread".into(),
                deletion_hash: "owned-report-reply".into(),
                sage: false,
            },
        )
        .await
        .unwrap();
        sqlx::query("UPDATE content.threads SET sticky=true WHERE id=$1")
            .bind(id.parse::<i64>().unwrap())
            .execute(&admin)
            .await
            .unwrap();
        let reply_form = app
            .clone()
            .oneshot(
                Request::get(format!("/fixture/imgboard.php?mode=report&no={reply}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let reply_report = app
            .clone()
            .oneshot(form(
                "/fixture/report",
                &[
                    ("no", &reply.to_string()),
                    ("reason", "Owned sticky reply report"),
                ],
                Some(ORIGIN),
                false,
            ))
            .await
            .unwrap();
        sqlx::query("UPDATE content.threads SET sticky=false WHERE id=$1")
            .bind(id.parse::<i64>().unwrap())
            .execute(&admin)
            .await
            .unwrap();
        assert_eq!(reply_form.status(), StatusCode::OK);
        assert_eq!(reply_report.status(), StatusCode::OK);
        let result = reply_report.into_body().collect().await.unwrap().to_bytes();
        assert!(
            std::str::from_utf8(&result)
                .unwrap()
                .contains("data-result=\"success\"")
        );
        let reply_reports: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM content.reports WHERE board='fixture' AND post_id=$1",
        )
        .bind(reply)
        .fetch_one(&admin)
        .await
        .unwrap();
        assert_eq!(reply_reports, 1);
        sqlx::query("DELETE FROM content.reports WHERE board='fixture' AND post_id=$1")
            .bind(reply)
            .execute(&admin)
            .await
            .unwrap();
        let wrong_popup = app
            .clone()
            .oneshot(
                Request::get(format!("/demo/imgboard.php?mode=report&no={id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(wrong_popup.status(), StatusCode::NOT_FOUND);
        let fields = [
            ("mode", "usrdel"),
            (id.as_str(), "delete"),
            ("pwd", PASSWORD),
        ];
        for origin in [None, Some("null"), Some("https://untrusted.example")] {
            let response = app
                .clone()
                .oneshot(form("/fixture/imgboard.php", &fields, origin, multipart))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
        }
        let original_state = "SELECT jsonb_build_object('post',to_jsonb(p),'thread',to_jsonb(t)) FROM content.posts p JOIN content.threads t ON t.id=p.thread_id AND t.board=p.board WHERE p.board='fixture' AND p.id=$1";
        let before: serde_json::Value = sqlx::query_scalar(original_state)
            .bind(id.parse::<i64>().unwrap())
            .fetch_one(&admin)
            .await
            .unwrap();
        let response = app
            .clone()
            .oneshot(form("/demo/imgboard.php", &fields, Some(ORIGIN), multipart))
            .await
            .unwrap();
        // The target is missing on this accessible board: source single-delete
        // returns Updating index, without granting authority over another board.
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let html = std::str::from_utf8(&body).unwrap();
        assert!(html.contains("Updating index"));
        assert!(html.contains("href=\"/demo/\""));
        assert_eq!(thread_status(&app, &id).await, StatusCode::OK);
        let after: serde_json::Value = sqlx::query_scalar(original_state)
            .bind(id.parse::<i64>().unwrap())
            .fetch_one(&admin)
            .await
            .unwrap();
        assert_eq!(
            before, after,
            "Wrong-board deletion must leave the original post and thread unchanged"
        );
        let mut wrong = fields;
        wrong[2].1 = "wrong-password";
        let response = app
            .clone()
            .oneshot(form(
                "/fixture/imgboard.php",
                &wrong,
                Some(ORIGIN),
                multipart,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(thread_status(&app, &id).await, StatusCode::OK);

        let response = app
            .clone()
            .oneshot(form(
                "/fixture/imgboard.php?mode=usrdel",
                &fields[1..],
                Some(ORIGIN),
                multipart,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response.headers()["cache-control"]
                .to_str()
                .unwrap()
                .contains("no-store")
        );
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let html = std::str::from_utf8(&body).unwrap();
        assert!(html.contains("Updating index"));
        assert!(html.contains("href=\"/fixture/\""));
        assert!(!html.contains(PASSWORD));
        assert_eq!(thread_status(&app, &id).await, StatusCode::NOT_FOUND);
        let popup = app
            .clone()
            .oneshot(
                Request::get(format!("/fixture/imgboard.php?mode=report&no={id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(popup.status(), StatusCode::NOT_FOUND);
        sqlx::query("DELETE FROM content.reports WHERE board='fixture' AND post_id=$1")
            .bind(id.parse::<i64>().unwrap())
            .execute(&admin)
            .await
            .unwrap();
    }
    pool.close().await;
    admin.close().await;
}
