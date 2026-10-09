#![cfg(feature = "database-tests")]
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;

async fn directory(router: &Router) -> Vec<Value> {
    let response = router
        .clone()
        .oneshot(Request::get("/boards.json").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 4_194_304).await.unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    let boards = value["boards"].as_array().unwrap().clone();
    let slugs: Vec<_> = boards
        .iter()
        .map(|row| row["board"].as_str().unwrap())
        .collect();
    assert!(slugs.windows(2).all(|pair| pair[0] < pair[1]));
    boards
}

async fn exercise(owner: &PgPool, public: &PgPool, slugs: &[String]) {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../fixtures/board-format-metadata-reference.json"
    ))
    .unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 4);
    let (app, api) = board_public::routers(public.clone(), "http://127.0.0.1:3000".into(), false);
    let mut baseline = Vec::new();
    // Each owned public row visits all four stored combinations, then returns
    // to its initial state. Routers stay alive to catch stale-policy caching.
    for round in 0..=4 {
        for (index, slug) in slugs[..4].iter().enumerate() {
            let case = &cases[(round + index) % cases.len()];
            sqlx::query("UPDATE content.boards SET comment_code_spacing=$2,comment_sjis_spacing=$3 WHERE slug=$1")
                .bind(slug)
                .bind(case["code_tags"].as_bool().unwrap())
                .bind(case["sjis_tags"].as_bool().unwrap())
                .execute(owner)
                .await
                .unwrap();
        }
        for (router_index, router) in [&app, &api].into_iter().enumerate() {
            let boards = directory(router).await;
            for hidden in &slugs[4..] {
                assert!(!boards.iter().any(|row| row["board"] == *hidden));
            }
            for (index, slug) in slugs[..4].iter().enumerate() {
                let row = boards.iter().find(|row| row["board"] == *slug).unwrap();
                let case = &cases[(round + index) % cases.len()];
                for field in ["code_tags", "sjis_tags"] {
                    // get() distinguishes absence from JSON null; equality and
                    // as_i64 reject booleans, zero, strings and other numbers.
                    assert_eq!(
                        row.get(field),
                        case["expected"].get(field),
                        "{slug}: {field}, round {round}"
                    );
                    if let Some(value) = row.get(field) {
                        assert_eq!(value.as_i64(), Some(1));
                    }
                }
                let mut other = row.clone();
                other.as_object_mut().unwrap().remove("code_tags");
                other.as_object_mut().unwrap().remove("sjis_tags");
                if round == 0 && router_index == 0 {
                    assert_eq!(other["title"], "Owned format metadata");
                    assert_eq!(
                        other["meta_description"],
                        "Synthetic source metadata fixture"
                    );
                    assert_eq!(other["max_comment_chars"], 2000);
                    assert_eq!(other["per_page"], 10);
                    assert_eq!(other["pages"], 10);
                    assert_eq!(
                        other["cooldowns"],
                        json!({"threads": 0, "replies": 0, "images": 0})
                    );
                    for field in [
                        "max_filesize",
                        "max_webm_filesize",
                        "max_webm_duration",
                        "image_limit",
                    ] {
                        assert_eq!(other[field], 0, "No new media capability: {field}");
                    }
                    baseline.push(other);
                } else {
                    assert_eq!(
                        other, baseline[index],
                        "Unrelated metadata changed for {slug}"
                    );
                }
            }
        }
    }
}

#[tokio::test]
async fn source_format_metadata_tracks_stored_flags_on_both_public_routers() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let prefix: String =
        sqlx::query_scalar("SELECT 'fm'||substr(replace(gen_random_uuid()::text,'-',''),1,7)")
            .fetch_one(&owner)
            .await
            .unwrap();
    // Deliberately neither insertion nor source order is slug order. The last
    // two rows carry both flags but must be absent: JSON-disabled and private.
    let slugs: Vec<String> = ['z', 'a', 'y', 'b', 'q', 'p']
        .into_iter()
        .map(|suffix| format!("{prefix}{suffix}"))
        .collect();
    sqlx::query(
        "INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,posting_reply_seconds,posting_image_seconds,posting_thread_seconds,source_order,json_enabled,staff_only,comment_code_spacing,comment_sjis_spacing) SELECT slug,'Owned format metadata','Synthetic source metadata fixture',2000,100,100,100,10,0,0,0,(1000-position)::integer,position<>5,position=6,true,true FROM unnest($1::text[]) WITH ORDINALITY AS owned(slug,position)",
    )
    .bind(&slugs)
    .execute(&owner)
    .await
    .unwrap();
    let owned = owner.clone();
    let runtime = public.clone();
    let rows = slugs.clone();
    let result = tokio::spawn(async move { exercise(&owned, &runtime, &rows).await }).await;
    // Cleanup also runs after assertion failure; no shared source rows change.
    sqlx::query("DELETE FROM content.boards WHERE slug=ANY($1)")
        .bind(&slugs)
        .execute(&owner)
        .await
        .unwrap();
    public.close().await;
    owner.close().await;
    result.unwrap();
}
