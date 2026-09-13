#![cfg(feature = "database-tests")]

use axum::{Router, body::Body, http::Request};
use http_body_util::BodyExt;
use rand_core::{OsRng, RngCore};
use sqlx::PgPool;
use tower::ServiceExt;

async fn read(app: &Router, path: &str) -> (u16, String) {
    let response = app
        .clone()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status().as_u16();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

fn ids(html: &str) -> Vec<i64> {
    html.split("id=\"thread-")
        .skip(1)
        .map(|suffix| suffix.split('"').next().unwrap().parse().unwrap())
        .collect()
}

fn bump_limited(html: &str, id: i64) -> bool {
    html.split(&format!("id=\"meta-{id}\""))
        .nth(1)
        .unwrap()
        .split("</div>")
        .next()
        .unwrap()
        .contains("<i>R: <b>")
}

async fn exercise(owner: PgPool, public: PgPool, slug: String) {
    let app = board_public::router(public.clone(), "http://127.0.0.1:3000".into(), false);
    let mut threads = Vec::new();
    for (index, subject) in ["Alpha [.*]", "Bravo", "Crane", "Sticky"]
        .iter()
        .enumerate()
    {
        let id: i64 = sqlx::query_scalar("INSERT INTO content.threads(board,bumped_at,sticky,reply_count) VALUES($1,'2026-01-05T00:00:00Z'::timestamptz - $2 * interval '1 day',$3,$4) RETURNING id")
            .bind(&slug).bind(index as i32).bind(index == 3).bind(100 - index as i32).fetch_one(&owner).await.unwrap();
        sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous',$3,'Synthetic <script>fold</script>')")
            .bind(id).bind(&slug).bind(subject).execute(&owner).await.unwrap();
        threads.push(id);
    }
    let [a, b, c, s] = threads.as_slice() else {
        unreachable!()
    };
    let mut replies = Vec::new();
    for thread in [c, c, c, a, b, b, c] {
        let id: i64 = sqlx::query_scalar("INSERT INTO content.posts(board,thread_id,name,subject,comment,deleted) VALUES($1,$2,'Anonymous','','A reply', $3) RETURNING id")
            .bind(&slug).bind(thread).bind(replies.len() == 6).fetch_one(&owner).await.unwrap();
        replies.push(id);
    }
    for (query, expected) in [
        ("", vec![*s, *a, *b, *c]),
        ("?order=date", vec![*s, *c, *b, *a]),
        ("?order=absdate", vec![*s, *b, *a, *c]),
        ("?order=r", vec![*s, *c, *b, *a]),
    ] {
        let (status, page) = read(&app, &format!("/{slug}/catalog{query}")).await;
        assert_eq!(status, 200);
        assert_eq!(ids(&page), expected, "{query}");
        assert!(!page.contains("<script>fold</script>"));
        for (id, limited) in [(*a, true), (*b, true), (*c, false), (*s, false)] {
            assert_eq!(bump_limited(&page, id), limited, "{query} {id}");
        }
    }
    for (query, expected) in [
        ("q=aLpHa", vec![*a]),
        ("q=%5B.*%5D", vec![*a]),
        (
            "q=%3Cscript%3E",
            threads.iter().copied().rev().collect::<Vec<_>>(),
        ),
        ("q=absent", vec![]),
    ] {
        let (status, page) = read(&app, &format!("/{slug}/catalog?order=date&{query}")).await;
        assert_eq!(status, 200);
        assert_eq!(ids(&page), expected, "literal filter {query}");
        assert!(!page.contains("value=\"<script>"));
        if query == "q=absent" {
            assert!(page.contains("No matching threads."));
            assert!(!page.contains("No threads yet."));
        }
    }
    let (_, large) = read(
        &app,
        &format!("/{slug}/catalog?size=large&teaser=off&order=r"),
    )
    .await;
    assert!(large.contains("class=\"catalog large\""));
    assert!(!large.contains("class=\"teaser\""));
    assert!(large.contains("value=\"r\" selected"));
    assert!(large.contains("value=\"large\" selected"));
    assert!(large.contains("value=\"off\" selected"));
    sqlx::query("UPDATE content.posts SET deleted=true WHERE id=ANY($1)")
        .bind(&replies[4..6])
        .execute(&owner)
        .await
        .unwrap();
    let (_, page) = read(&app, &format!("/{slug}/catalog?order=absdate")).await;
    assert_eq!(ids(&page), vec![*s, *a, *c, *b]);
    let (_, page) = read(&app, &format!("/{slug}/catalog?order=r")).await;
    assert_eq!(ids(&page), vec![*s, *c, *a, *b]);
    assert!(bump_limited(&page, *b), "deletion does not restore bumping");
    let (_, json) = read(&app, &format!("/{slug}/thread/{b}.json")).await;
    let json: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(json["posts"][0]["replies"], 0);
    assert_eq!(json["posts"][0]["bumplimit"], 1);
    for (limit, limited) in [(100, false), (0, true)] {
        sqlx::query("UPDATE content.boards SET bump_limit=$1 WHERE slug=$2")
            .bind(limit)
            .bind(&slug)
            .execute(&owner)
            .await
            .unwrap();
        let (_, page) = read(&app, &format!("/{slug}/catalog")).await;
        assert_eq!(bump_limited(&page, *b), limited);
        let (_, json) = read(&app, &format!("/{slug}/thread/{b}.json")).await;
        let json: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(json["posts"][0]["bumplimit"].as_i64(), limited.then_some(1));
    }
    sqlx::query("UPDATE content.threads SET deleted=true WHERE id=$1")
        .bind(a)
        .execute(&owner)
        .await
        .unwrap();
    let (_, page) = read(&app, &format!("/{slug}/catalog?q=Alpha")).await;
    assert!(ids(&page).is_empty());
    for query in [
        "order=unknown".to_owned(),
        "size=large&size=small".into(),
        "unknown=1".into(),
        "q=%0A".into(),
        format!("q={}", "x".repeat(129)),
        format!("q={}", "%61".repeat(700)),
    ] {
        let (status, body) = read(&app, &format!("/{slug}/catalog?{query}")).await;
        assert_eq!(status, 400, "{query}");
        assert!(body.contains("Invalid catalog options."));
    }
    public.close().await;
}

#[tokio::test]
async fn catalog_options_use_visible_persisted_data_and_escape_literal_filters() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut random = [0_u8; 5];
    OsRng.fill_bytes(&mut random);
    let slug: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Catalog control fixture','Owned synthetic data',1000,100,99,10,10)").bind(&slug).execute(&owner).await.unwrap();
    let result = tokio::spawn(exercise(owner.clone(), public.clone(), slug.clone())).await;
    public.close().await;
    for query in [
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
    owner.close().await;
    result.unwrap();
}
