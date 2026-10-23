#![cfg(feature = "database-tests")]
//! Persisted text caches compared with executed original feed generators.
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::Value;
use sqlx::PgPool;
use std::collections::BTreeMap;
use tower::ServiceExt;

async fn get(app: &Router, path: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 4_194_304).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn normalize(value: &mut Value, ids: &BTreeMap<i64, i64>) {
    match value {
        Value::Array(rows) => {
            for row in rows {
                normalize(row, ids);
            }
        }
        Value::Object(fields) => {
            // These values are controlled helper inputs in this oracle. Their
            // clock and address contracts have separate tests.
            for key in ["now", "time", "unique_ips"] {
                fields.remove(key);
            }
            for key in ["no", "resto", "tail_id"] {
                if let Some(number) = fields.get(key).and_then(Value::as_i64)
                    && let Some(source) = ids.get(&number)
                {
                    fields.insert(key.into(), (*source).into());
                }
            }
            for field in fields.values_mut() {
                normalize(field, ids);
            }
        }
        _ => {}
    }
}

async fn seed(owner: &PgPool, slug: &str, case: &Value) -> BTreeMap<i64, i64> {
    let mut ids = BTreeMap::new();
    for (rank, thread) in case["threads"].as_array().unwrap().iter().enumerate() {
        let source = thread["id"].as_i64().unwrap();
        let children = thread["children"].as_array().unwrap();
        let id:i64=sqlx::query_scalar("INSERT INTO content.threads(board,sticky,bumped_at,reply_count,undead) VALUES($1,$2,to_timestamp(1704067200-$3),$4,$5) RETURNING id")
            .bind(slug).bind(thread["sticky"].as_bool().unwrap()).bind(rank as f64).bind(children.len() as i32).bind(thread["undead"].as_bool().unwrap_or(false)).fetch_one(owner).await.unwrap();
        ids.insert(id, source);
        sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at) VALUES($1,$2,$1,'Anonymous','','plain',to_timestamp(1704067200))")
            .bind(id).bind(slug).execute(owner).await.unwrap();
        let mut children: Vec<_> = children.iter().collect();
        children.sort_by_key(|c| c["id"].as_i64().unwrap());
        for child in children {
            let child_id:i64=sqlx::query_scalar("INSERT INTO content.posts(board,thread_id,name,subject,comment,file_deleted,created_at) VALUES($1,$2,'Anonymous','','plain',$3,to_timestamp(1704067200)) RETURNING id")
                .bind(slug).bind(id).bind(child["deleted"].as_bool().unwrap()).fetch_one(owner).await.unwrap();
            ids.insert(child_id, child["id"].as_i64().unwrap());
        }
        sqlx::query("UPDATE content.threads SET modified_at=to_timestamp($2) WHERE id=$1")
            .bind(id)
            .bind((1704067200 + source) as f64)
            .execute(owner)
            .await
            .unwrap();
    }
    for source in case["archived"].as_array().unwrap() {
        let id:i64=sqlx::query_scalar("INSERT INTO content.threads(board,archived_at,archive_expires_at,closed) VALUES($1,clock_timestamp(),clock_timestamp()+interval '1 day',true) RETURNING id")
            .bind(slug).fetch_one(owner).await.unwrap();
        sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous','','plain')")
            .bind(id).bind(slug).execute(owner).await.unwrap();
        ids.insert(id, source.as_i64().unwrap());
    }
    ids
}

#[tokio::test]
async fn persisted_feeds_match_source_order_counts_fields_and_omissions() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let reference: Value =
        serde_json::from_str(include_str!("../../../fixtures/json-feed-reference.json")).unwrap();
    for (index, case) in reference["cases"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .filter(|(_, c)| c["text_fixture"] == true)
    {
        let slug: String =
            sqlx::query_scalar("SELECT 'jf'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
                .fetch_one(&owner)
                .await
                .unwrap();
        sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,image_limit,replies_shown,json_tail_size,archive_retention_seconds,comment_spoiler_cleanup,custom_spoiler_count) VALUES($1,'Owned JSON feeds','',500,99,99,$2,2,99,$3,2,86400,true,4)")
            .bind(&slug).bind(if case["page_max"]==0 {20} else {4}).bind(case["shown"].as_i64().unwrap() as i32).execute(&owner).await.unwrap();
        let owned = owner.clone();
        let runtime = public.clone();
        let board = slug.clone();
        let case = case.clone();
        let result = tokio::spawn(async move {
            let ids = seed(&owned, &board, &case).await;
            let (html, api) = board_public::routers_with_media(
                runtime,
                "http://127.0.0.1:3000".into(),
                false,
                None,
            );
            for app in [&html, &api] {
                for thread in case["threads"].as_array().unwrap() {
                    let source = thread["id"].as_i64().unwrap();
                    if case["expected"]
                        .get(format!("/out/thread/{source}-tail.json"))
                        .is_none()
                    {
                        let actual = ids.iter().find(|(_, s)| **s == source).unwrap().0;
                        assert_eq!(
                            get(app, &format!("/{board}/thread/{actual}-tail.json"))
                                .await
                                .0,
                            StatusCode::NOT_FOUND
                        );
                    }
                }
                for (path, expected) in case["expected"].as_object().unwrap() {
                    let mut path = path.strip_prefix("/out/").unwrap().to_owned();
                    if let Some(key) = path.strip_prefix("thread/") {
                        let source = key
                            .split(['.', '-'])
                            .next()
                            .unwrap()
                            .parse::<i64>()
                            .unwrap();
                        let actual = ids.iter().find(|(_, s)| **s == source).unwrap().0;
                        path = path.replacen(&source.to_string(), &actual.to_string(), 1);
                    }
                    let (status, mut actual) = get(app, &format!("/{board}/{path}")).await;
                    assert_eq!(status, StatusCode::OK, "case {index}: {path}");
                    let mut expected = expected.clone();
                    if path == "archive.json" {
                        actual = Value::Array(
                            actual
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(|id| Value::from(*ids.get(&id.as_i64().unwrap()).unwrap()))
                                .collect(),
                        );
                    } else {
                        normalize(&mut actual, &ids);
                        normalize(&mut expected, &BTreeMap::new());
                    }
                    assert_eq!(actual, expected, "case {index}: {path}");
                }
            }
        })
        .await;
        sqlx::query("DELETE FROM content.posts WHERE board=$1")
            .bind(&slug)
            .execute(&owner)
            .await
            .unwrap();
        sqlx::query("DELETE FROM content.threads WHERE board=$1")
            .bind(&slug)
            .execute(&owner)
            .await
            .unwrap();
        sqlx::query("DELETE FROM content.boards WHERE slug=$1")
            .bind(&slug)
            .execute(&owner)
            .await
            .unwrap();
        result.unwrap();
    }
}

#[tokio::test]
async fn historical_upload_subjects_keep_tags_on_ops_only() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let slug: String =
        sqlx::query_scalar("SELECT 'jt'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,upload_board,catalog_enabled) VALUES($1,'Owned upload subjects','',500,100,99,100,10,true,false)")
        .bind(&slug).execute(&owner).await.unwrap();
    let owned = owner.clone();
    let runtime = public.clone();
    let board = slug.clone();
    let result=tokio::spawn(async move {
        let source:Value=serde_json::from_str(include_str!("../../../fixtures/json-post-state-reference.json")).unwrap();
        let (html,api)=board_public::routers_with_media(runtime,"http://127.0.0.1:3000".into(),false,None);
        for case in source["upload_cases"].as_array().unwrap() {
            let subject=case["media"]["sub"].as_str().unwrap();
            let id:i64=sqlx::query_scalar("INSERT INTO content.threads(board) VALUES($1) RETURNING id").bind(&board).fetch_one(&owned).await.unwrap();
            sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous',$3,'plain')").bind(id).bind(&board).bind(subject).execute(&owned).await.unwrap();
            sqlx::query("INSERT INTO content.posts(board,thread_id,name,subject,comment) VALUES($1,$2,'Anonymous',$3,'reply')").bind(&board).bind(id).bind(subject).execute(&owned).await.unwrap();
            for app in [&html,&api] {
                let (status,actual)=get(app,&format!("/{board}/thread/{id}.json")).await;
                assert_eq!(status,StatusCode::OK);
                for key in ["tag","sub"] {
                    assert_eq!(actual["posts"][0].get(key),case["expected"]["full"].get(key),"{key}: {subject}");
                }
                assert!(actual["posts"][1].get("tag").is_none());
                assert_eq!(actual["posts"][1]["sub"],subject);
            }
        }
        assert_eq!(get(&html,&format!("/{board}/catalog.json")).await.0,StatusCode::NOT_FOUND);
    }).await;
    sqlx::query("DELETE FROM content.posts WHERE board=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.threads WHERE board=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.boards WHERE slug=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    result.unwrap();
}
