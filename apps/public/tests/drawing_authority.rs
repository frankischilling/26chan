#![cfg(feature = "database-tests")]
#[path = "support/posting.rs"]
mod posting;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
};
use board_store::NewPost;
use sqlx::PgPool;
use tower::ServiceExt;

const ORIGIN: &str = "http://127.0.0.1:3000";
const MEDIA_ORIGIN: &str = "http://127.0.0.1:4001";
const SCRIPT: &str = "/static/tegaki/tegaki-0.9.4.v1.js";
const STYLE: &str = "/static/tegaki/tegaki-0.9.4.v1.css";
const FONT: &str = "/static/tegaki/tegaki-icons.v1.woff";

async fn request(app: &Router, method: &str, path: &str) -> (StatusCode, HeaderMap, String) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("origin", ORIGIN)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let (parts, body) = response.into_parts();
    (
        parts.status,
        parts.headers,
        String::from_utf8(to_bytes(body, 4_194_304).await.unwrap().to_vec()).unwrap(),
    )
}

fn directive<'a>(policy: &'a str, name: &str) -> Option<&'a str> {
    policy
        .split(';')
        .map(str::trim)
        .find(|value| value.split_ascii_whitespace().next() == Some(name))
}

fn authority(headers: &HeaderMap, enabled: bool) -> String {
    let policy = headers["content-security-policy"].to_str().unwrap();
    let script = directive(policy, "script-src").unwrap();
    let tegaki_scripts: Vec<_> = script
        .split_ascii_whitespace()
        .filter(|value| value.contains("tegaki"))
        .collect();
    let expected_script = format!("{ORIGIN}{SCRIPT}");
    assert_eq!(
        tegaki_scripts,
        if enabled {
            vec![expected_script.as_str()]
        } else {
            vec![]
        }
    );
    assert_eq!(
        directive(policy, "font-src"),
        enabled
            .then(|| format!("font-src {ORIGIN}{FONT}"))
            .as_deref()
    );
    assert!(
        !script
            .split_ascii_whitespace()
            .any(|value| value == "'self'"
                || value == "*"
                || value == ORIGIN
                || value == format!("{ORIGIN}/"))
    );
    assert!(!policy.contains("unsafe-inline") && !policy.contains("unsafe-eval"));
    assert!(!policy.contains("blob:") && !policy.contains("data:"));
    let images = directive(policy, "img-src").unwrap();
    assert!(!images.contains("tegaki"));
    assert!(
        !images
            .split_ascii_whitespace()
            .any(|value| value == "'self'" || value == "*")
    );
    for name in ["worker-src", "connect-src", "media-src", "frame-src"] {
        assert!(!directive(policy, name).unwrap().contains("tegaki"));
    }
    if !enabled {
        assert!(!policy.contains("tegaki"));
    }
    images.to_owned()
}

fn controls(html: &str, enabled: bool) {
    assert_eq!(html.contains("data-drawing-allowed=\"true\""), enabled);
    assert_eq!(html.contains("data-drawing-draw"), enabled);
    assert_eq!(html.contains(&format!("href=\"{STYLE}\"")), enabled);
    assert!(!html.contains(SCRIPT), "the editor must remain lazy");
    assert!(!html.contains("oe-r-cb") && !html.contains("oeReplay("));
}

async fn exercise(owner: &PgPool, public: &PgPool, slug: &str) {
    let id = posting::create_post(
        public,
        slug,
        0,
        &NewPost {
            name: "Anonymous".into(),
            subject: "Owned drawing authority".into(),
            comment: "Renderer authority fixture".into(),
            deletion_hash: "owned-drawing-authority-hash".into(),
            sage: false,
        },
    )
    .await
    .unwrap();
    for media_enabled in [false, true] {
        let media = media_enabled.then(|| {
            board_config::PublicMediaSettings::development(
                "127.0.0.1:4000",
                &"a".repeat(64),
                MEDIA_ORIGIN,
            )
            .unwrap()
        });
        let (web, api) =
            posting::routers_with_media(public.clone(), slug, ORIGIN.into(), false, media);
        let mut image_authority = None;
        for (label, enabled, replay, text_only, image_limit) in [
            ("supported", true, false, false, 100),
            ("disabled", false, false, false, 100),
            ("replay unsupported", true, true, false, 100),
            ("text only", true, false, true, 100),
            ("no images", true, false, false, 0),
            ("reenabled", true, false, false, 100),
        ] {
            sqlx::query("UPDATE content.boards SET oekaki=$2,oekaki_replays=$3,text_only=$4,image_limit=$5 WHERE slug=$1")
                .bind(slug).bind(enabled).bind(replay).bind(text_only).bind(image_limit)
                .execute(owner).await.unwrap();
            let admitted = media_enabled && enabled && !replay && !text_only && image_limit > 0;
            for path in [format!("/{slug}/"), format!("/{slug}/thread/{id}")] {
                for method in ["GET", "HEAD"] {
                    let (status, headers, html) = request(&web, method, &path).await;
                    assert_eq!(status, StatusCode::OK, "{label}: {method} {path}");
                    assert!(
                        headers["content-type"]
                            .to_str()
                            .unwrap()
                            .starts_with("text/html")
                    );
                    let images = authority(&headers, admitted);
                    assert_eq!(
                        image_authority.get_or_insert_with(|| images.clone()),
                        &images,
                        "drawing must not widen image authority"
                    );
                    if method == "GET" {
                        controls(&html, admitted);
                    } else {
                        assert!(html.is_empty());
                    }
                }
            }
        }
        // The same enabled board must not carry renderer authority onto other
        // representations, catalog pages, missing resources or rejected writes.
        for (app, path, expected) in [
            (&web, format!("/{slug}/catalog"), StatusCode::OK),
            (
                &web,
                format!("/{slug}/thread/9223372036854775807"),
                StatusCode::NOT_FOUND,
            ),
            (&api, format!("/{slug}/thread/{id}.json"), StatusCode::OK),
            (&api, "/boards.json".into(), StatusCode::OK),
        ] {
            let (status, headers, html) = request(app, "GET", &path).await;
            assert_eq!(status, expected, "{path}");
            authority(&headers, false);
            controls(&html, false);
        }
        let (status, headers, _) = request(&web, "POST", &format!("/{slug}/")).await;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
        authority(&headers, false);
        for archived in [false, true] {
            sqlx::query("UPDATE content.threads SET closed=true,archived_at=CASE WHEN $2 THEN now() ELSE NULL END,archive_expires_at=CASE WHEN $2 THEN now()+interval '1 hour' ELSE NULL END WHERE board=$1 AND id=$3")
                .bind(slug).bind(archived).bind(id).execute(owner).await.unwrap();
            for method in ["GET", "HEAD"] {
                let (status, headers, html) =
                    request(&web, method, &format!("/{slug}/thread/{id}")).await;
                assert_eq!(status, StatusCode::OK);
                authority(&headers, false);
                if method == "GET" {
                    controls(&html, false);
                    assert!(!html.contains("class=\"postEditor\""));
                }
            }
        }
        sqlx::query("UPDATE content.threads SET closed=false,archived_at=NULL,archive_expires_at=NULL WHERE board=$1 AND id=$2")
            .bind(slug).bind(id).execute(owner).await.unwrap();
    }
}

#[tokio::test]
async fn drawing_authority_requires_successful_supported_ordinary_html_from_the_real_renderer() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let slug: String =
        sqlx::query_scalar("SELECT 'da'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,image_limit,oekaki,archive_retention_seconds,posting_reply_seconds,posting_image_seconds,posting_thread_seconds) VALUES($1,'Owned drawing authority','Synthetic renderer fixture',2000,100,100,100,10,100,true,86400,0,0,0)")
        .bind(&slug).execute(&owner).await.unwrap();
    let owned = owner.clone();
    let runtime = public.clone();
    let board = slug.clone();
    let result = tokio::spawn(async move { exercise(&owned, &runtime, &board).await }).await;
    posting::cleanup_posting(&owner, &slug).await;
    for query in [
        "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(query)
            .bind(&slug)
            .execute(&owner)
            .await
            .unwrap();
    }
    public.close().await;
    owner.close().await;
    result.unwrap();
}
