#![cfg(feature = "database-tests")]

use axum::{Router, body::Body, http::Request};
use http_body_util::BodyExt;
use rand_core::{OsRng, RngCore};
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;

async fn get(app: &Router, path: &str) -> String {
    let response = app
        .clone()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), 200, "{path}");
    String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap()
}

async fn json(app: &Router, path: &str) -> Value {
    serde_json::from_str(&get(app, path).await).unwrap()
}

fn catalog_ids(html: &str) -> Vec<i64> {
    html.split("id=\"thread-")
        .skip(1)
        .map(|suffix| suffix.split('"').next().unwrap().parse().unwrap())
        .collect()
}

fn index_ids(html: &str) -> Vec<i64> {
    html.split("<section class=\"thread\" id=\"t")
        .skip(1)
        .map(|suffix| suffix.split('"').next().unwrap().parse().unwrap())
        .collect()
}

// Every card, including excluded cards held in the template, must retain its
// original SQL position. Sorting those positions models clearing q and switching
// from an alternate order back to bump order without another network request.
fn restored_ids(html: &str) -> Vec<i64> {
    let mut cards: Vec<(usize, i64)> = html
        .split("id=\"thread-")
        .skip(1)
        .map(|suffix| {
            let id = suffix.split('"').next().unwrap().parse().unwrap();
            let tag = suffix.split('>').next().unwrap();
            let position = tag
                .split("data-bump-position=\"")
                .nth(1)
                .expect("catalog card retains its SQL ordinal")
                .split('"')
                .next()
                .unwrap()
                .parse()
                .unwrap();
            (position, id)
        })
        .collect();
    cards.sort_unstable();
    for (position, (actual, _)) in cards.iter().enumerate() {
        assert_eq!(
            *actual, position,
            "ordinals are unique and contiguous, including hidden cards"
        );
    }
    cards.into_iter().map(|(_, id)| id).collect()
}

async fn assert_public_order(web: &Router, api: &Router, slug: &str, expected: &[i64]) {
    for suffix in ["", "?order=alt"] {
        let html = get(web, &format!("/{slug}/catalog{suffix}")).await;
        assert_eq!(catalog_ids(&html), expected, "catalog{suffix}");
        assert_eq!(restored_ids(&html), expected);
    }
    for app in [web, api] {
        for endpoint in ["catalog.json", "threads.json"] {
            let body = json(app, &format!("/{slug}/{endpoint}")).await;
            let ids: Vec<i64> = body
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|page| {
                    page["threads"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|thread| thread["no"].as_i64().unwrap())
                })
                .collect();
            assert_eq!(ids, expected, "{endpoint}");
            assert!(
                !body.to_string().contains("sticky_rank"),
                "private ordering metadata does not expand JSON contracts"
            );
        }
        for (page, pair) in expected.chunks(2).enumerate() {
            let body = json(app, &format!("/{slug}/{}.json", page + 1)).await;
            let ids: Vec<i64> = body["threads"]
                .as_array()
                .unwrap()
                .iter()
                .map(|thread| thread["posts"][0]["no"].as_i64().unwrap())
                .collect();
            assert_eq!(ids, pair, "JSON page {page}");
        }
    }
    assert_eq!(
        index_ids(&get(web, &format!("/{slug}/")).await),
        &expected[..2]
    );
    for (page, pair) in expected.chunks(2).enumerate() {
        assert_eq!(
            index_ids(&get(web, &format!("/{slug}/{page}")).await),
            pair,
            "HTML page {page}"
        );
        let body = json(web, &format!("/_watch/{slug}/page/{page}")).await;
        let ids: Vec<i64> = body["threads"]
            .as_array()
            .unwrap()
            .iter()
            .map(|thread| thread["thread"].as_str().unwrap().parse().unwrap())
            .collect();
        assert_eq!(ids, pair, "native snapshot page {page}");
        for id in pair {
            let stats = json(web, &format!("/_watch/{slug}/thread/{id}/stats")).await;
            assert_eq!(
                stats["page"].as_u64().unwrap(),
                (page + 1) as u64,
                "stats page for {id}"
            );
        }
    }
}

async fn exercise(owner: PgPool, public: PgPool, staff: PgPool, slug: String) {
    let mut ids = Vec::new();
    // Rank wins over intentionally reversed bumps. Equal ranks use bump then
    // descending ID. Nonsticky rank 60 must not outrank a newer ordinary thread.
    for (sticky, rank, bump) in [
        (false, 60_i16, 90),
        (true, 0, 80),
        (true, 60, 10),
        (true, 30, 40),
        (true, 30, 40),
        (true, 30, 50),
        (false, 0, 100),
        (true, 60, 5),
    ] {
        let id: i64 = sqlx::query_scalar("INSERT INTO content.threads(board,sticky,sticky_rank,bumped_at) VALUES($1,$2,$3,'2026-01-01T00:00:00Z'::timestamptz + $4 * interval '1 second') RETURNING id")
            .bind(&slug).bind(sticky).bind(rank).bind(bump).fetch_one(&owner).await.unwrap();
        sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous',$3,'Owned rank fixture')")
            .bind(id).bind(&slug).bind(if ids.len() % 2 == 0 { "Keep" } else { "Hide" }).execute(&owner).await.unwrap();
        ids.push(id);
    }
    for (index, count) in [(0, 1), (1, 2), (3, 3)] {
        for _ in 0..count {
            sqlx::query("INSERT INTO content.posts(board,thread_id,name,subject,comment) VALUES($1,$2,'Anonymous','','Owned reply')")
                .bind(&slug).bind(ids[index]).execute(&owner).await.unwrap();
        }
    }
    let order = |positions: &[usize]| {
        positions
            .iter()
            .map(|index| ids[*index])
            .collect::<Vec<_>>()
    };
    let expected = order(&[2, 7, 5, 4, 3, 1, 6, 0]);
    let (web, api) = board_public::routers(public, "http://127.0.0.1:3000".into(), false);
    assert_public_order(&web, &api, &slug, &expected).await;
    for (query, positions) in [
        ("alt", vec![2, 7, 5, 4, 3, 1, 6, 0]),
        ("date", vec![7, 5, 4, 3, 2, 1, 6, 0]),
        ("absdate", vec![3, 1, 2, 4, 5, 7, 0, 6]),
        ("r", vec![3, 1, 2, 4, 5, 7, 0, 6]),
    ] {
        for filter in ["", "&q=Keep", "&q=absent"] {
            let html = get(&web, &format!("/{slug}/catalog?order={query}{filter}")).await;
            let visible = html
                .split("<template id=\"catalogFiltered\">")
                .next()
                .unwrap();
            let visible_expected: Vec<i64> = positions
                .iter()
                .filter(|index| filter.is_empty() || (filter == "&q=Keep" && **index % 2 == 0))
                .map(|index| ids[*index])
                .collect();
            assert_eq!(catalog_ids(visible), visible_expected, "{query}{filter}");
            assert_eq!(restored_ids(&html), expected, "restore {query}{filter}");
        }
    }
    // Exercise the real staff column grant and ensure every public projection
    // immediately agrees after moving the formerly first sticky to rank zero.
    assert_eq!(
        sqlx::query("UPDATE content.threads SET sticky_rank=0 WHERE board=$1 AND id=$2")
            .bind(&slug)
            .bind(ids[2])
            .execute(&staff)
            .await
            .unwrap()
            .rows_affected(),
        1
    );
    assert_public_order(&web, &api, &slug, &order(&[7, 5, 4, 3, 1, 2, 6, 0])).await;

    // Text catalogs use the same source bump ordinal. Only alternate sort modes
    // omit the additional sticky-first bucket.
    sqlx::query("UPDATE content.boards SET text_only=true WHERE slug=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    for (query, positions) in [
        ("alt", vec![7, 5, 4, 3, 1, 2, 6, 0]),
        ("date", vec![7, 6, 5, 4, 3, 2, 1, 0]),
        ("absdate", vec![3, 1, 0, 2, 4, 5, 6, 7]),
        ("r", vec![3, 1, 0, 2, 4, 5, 6, 7]),
    ] {
        for filter in ["", "&q=Keep", "&q=absent"] {
            let html = get(&web, &format!("/{slug}/catalog?order={query}{filter}")).await;
            let visible = html
                .split("<template id=\"catalogFiltered\">")
                .next()
                .unwrap();
            let visible_expected: Vec<i64> = positions
                .iter()
                .filter(|index| filter.is_empty() || (filter == "&q=Keep" && **index % 2 == 0))
                .map(|index| ids[*index])
                .collect();
            assert_eq!(
                catalog_ids(visible),
                visible_expected,
                "text-only {query}{filter}"
            );
            assert_eq!(
                restored_ids(&html),
                order(&[7, 5, 4, 3, 1, 2, 6, 0]),
                "text restore {query}{filter}"
            );
        }
    }
}

#[tokio::test]
async fn ranked_stickies_agree_across_public_pages_catalogs_and_native_snapshots() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let staff = PgPool::connect(&std::env::var("STAFF_DATABASE_URL").unwrap())
        .await
        .unwrap();
    for (pool, role) in [(&public, "board_public"), (&staff, "board_staff")] {
        let actual: String = sqlx::query_scalar("SELECT current_user::text")
            .fetch_one(pool)
            .await
            .unwrap();
        assert_eq!(actual, role, "use actual restricted logins, never SET ROLE");
    }
    let mut random = [0_u8; 4];
    OsRng.fill_bytes(&mut random);
    let slug = format!("rk{:08x}", u32::from_le_bytes(random));
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,text_only) VALUES($1,'Owned rank order','Bounded HTTP rank fixture',1000,100,100,20,2,false)")
        .bind(&slug).execute(&owner).await.unwrap();
    let result = tokio::spawn(exercise(
        owner.clone(),
        public.clone(),
        staff.clone(),
        slug.clone(),
    ))
    .await;
    public.close().await;
    staff.close().await;
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
