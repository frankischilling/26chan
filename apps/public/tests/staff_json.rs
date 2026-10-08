#![cfg(feature = "database-tests")]

use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
};
use serde_json::{Value, json};
use tower::ServiceExt;

async fn get(router: &Router, path: &str) -> Value {
    let response = router
        .clone()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), 200, "{path}");
    assert_eq!(response.headers()["content-type"], "application/json");
    serde_json::from_slice(&to_bytes(response.into_body(), 4_194_304).await.unwrap()).unwrap()
}

#[tokio::test]
async fn saved_staff_identities_and_omitted_badge_replies_follow_source_json_across_both_listeners()
{
    let owner = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let (html, api) = board_public::routers(public.clone(), "http://127.0.0.1:3000".into(), false);
    let board: String =
        sqlx::query_scalar("SELECT 'z'||substr(replace(gen_random_uuid()::text,'-',''),1,9)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,json_tail_size,archive_retention_seconds,archive_limit) VALUES($1,'Owned staff JSON','Synthetic saved public headers',1000,1000,1000,100,10,2,3600,10)")
        .bind(&board).execute(&owner).await.unwrap();
    let task_owner = owner.clone();
    let task_board = board.clone();
    let task_public = public.clone();
    let result = tokio::spawn(async move {
        let owner = task_owner;
        let board = task_board;
        let public = task_public;
        let fixture: Value = serde_json::from_str(include_str!("../../../crates/domain/tests/fixtures/staff-json.json")).unwrap();
        let cases = fixture["identity_cases"].as_array().unwrap();
        let thread: i64 = sqlx::query_scalar("INSERT INTO content.threads(id,board) VALUES(nextval('content.post_number'),$1) RETURNING id")
            .bind(&board).fetch_one(&owner).await.unwrap();
        sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Owned original','Owned JSON','Owned original body')")
            .bind(thread).bind(&board).execute(&owner).await.unwrap();
        // Operator-owned historical fixtures contain prepared public fields only.
        // Runtime posting and proof authority are qualified in staff posting tests.
        sqlx::query("UPDATE content.posts SET name='Owned & \"',poster_id='Ab12+/CD' WHERE id=$1")
            .bind(thread).execute(&owner).await.unwrap();
        let mut saved = Vec::new();
        let mut expected_groups = serde_json::Map::new();
        for badge in ["mod", "admin_highlight", "admin", "developer", "manager", "founder"] {
            for (raw_name, escaped_name, trip) in [
                ("Owned & \"", "Owned &amp; &quot;", None),
                ("Owned", "Owned", Some("!ozOtJW9BFA")),
                ("", "", Some("!!abcdefghijk")),
            ] {
                let id: i64 = sqlx::query_scalar("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES(nextval('content.post_number'),$1,$2,'Owned initial','','Owned saved public body') RETURNING id")
                    .bind(&board).bind(thread).fetch_one(&owner).await.unwrap();
                sqlx::query("UPDATE content.posts SET name=$2,trip=$3,capcode=$4,country=CASE WHEN $5 THEN 'US' END,country_name=CASE WHEN $5 THEN 'United States' END,board_flag=CASE WHEN NOT $5 THEN 'AC' END,flag_name=CASE WHEN NOT $5 THEN 'Owned flag' END WHERE id=$1")
                    .bind(id).bind(raw_name).bind(trip).bind(badge).bind(trip.is_none()).execute(&owner).await.unwrap();
                saved.push((id, badge, escaped_name, trip));
                let group = if badge == "admin_highlight" { "admin" } else { badge };
                expected_groups.entry(group.to_owned()).or_insert_with(|| json!([])).as_array_mut().unwrap().push(json!(id));
            }
        }
        let deleted: i64 = sqlx::query_scalar("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES(nextval('content.post_number'),$1,$2,'Owned deleted','','Owned deleted body') RETURNING id")
            .bind(&board).bind(thread).fetch_one(&owner).await.unwrap();
        sqlx::query("UPDATE content.posts SET capcode='admin',deleted=true WHERE id=$1").bind(deleted).execute(&owner).await.unwrap();
        let empty: i64 = sqlx::query_scalar("INSERT INTO content.threads(id,board) VALUES(nextval('content.post_number'),$1) RETURNING id")
            .bind(&board).fetch_one(&owner).await.unwrap();
        sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Owned badge OP','Owned badge OP','Owned OP without replies')")
            .bind(empty).bind(&board).execute(&owner).await.unwrap();
        sqlx::query("UPDATE content.posts SET capcode='founder' WHERE id=$1").bind(empty).execute(&owner).await.unwrap();
        for forced in [false, true] {
            for meta in [false, true] {
                sqlx::query("UPDATE content.boards SET forced_anon=$2,meta_board=$3 WHERE slug=$1")
                    .bind(&board).bind(forced).bind(meta).execute(&owner).await.unwrap();
                for router in [&html, &api] {
                    let full = get(router, &format!("/{board}/thread/{thread}.json")).await;
                    assert_eq!(full["posts"].as_array().unwrap().len(), 19);
                    assert_eq!(full["posts"][0]["replies"], 18);
                    assert_eq!(full["posts"][0]["id"], "Ab12+/CD");
                    assert_eq!(full["posts"][0]["name"], if forced || meta { "Anonymous" } else { "Owned &amp; &quot;" });
                    assert_eq!(full["posts"][0].get("capcode_replies"), meta.then_some(&Value::Object(expected_groups.clone())));
                    for (id, badge, name, trip) in &saved {
                        let actual = full["posts"].as_array().unwrap().iter().find(|post| post["no"] == *id).unwrap();
                        let expected = &cases.iter().find(|case| {
                            case["forced_anonymous"] == forced && case["meta_board"] == meta && case["capcode"] == *badge
                                && case["name"] == *name && case["trip"].as_str() == *trip
                        }).unwrap()["expected"];
                        assert_eq!(actual.get("name"), expected.get("name"), "{actual}");
                        assert_eq!(actual.get("trip"), expected.get("trip"), "{actual}");
                        assert_eq!(actual["capcode"], *badge);
                        for field in ["country", "country_name", "board_flag", "flag_name", "id", "host", "pwd", "email", "account_id", "capcode_replies"] {
                            assert!(actual.get(field).is_none(), "{field}: {actual}");
                        }
                    }
                    let tail = get(router, &format!("/{board}/thread/{thread}-tail.json")).await;
                    assert_eq!(tail["posts"].as_array().unwrap().len(), 3);
                    assert_eq!(tail["posts"][0]["tail_size"], 2);
                    assert_eq!(tail["posts"][0]["tail_id"], saved[15].0);
                    for field in ["name", "trip", "id", "capcode", "capcode_replies"] {
                        assert!(tail["posts"][0].get(field).is_none());
                    }
                    assert_eq!(&tail["posts"].as_array().unwrap()[1..], &full["posts"].as_array().unwrap()[17..]);
                    let index = get(router, &format!("/{board}/1.json")).await;
                    let preview = index["threads"].as_array().unwrap().iter().find(|entry| entry["posts"][0]["no"] == thread).unwrap();
                    assert_eq!(preview["posts"].as_array().unwrap().len(), 6);
                    assert_eq!(preview["posts"][0]["omitted_posts"], 13);
                    assert_eq!(preview["posts"][0].get("capcode_replies"), full["posts"][0].get("capcode_replies"));
                    let empty_preview = index["threads"].as_array().unwrap().iter().find(|entry| entry["posts"][0]["no"] == empty).unwrap();
                    assert!(empty_preview["posts"][0].get("capcode_replies").is_none());
                    assert_eq!(&preview["posts"].as_array().unwrap()[1..], &full["posts"].as_array().unwrap()[14..]);
                    let catalog = get(router, &format!("/{board}/catalog.json")).await;
                    let card = catalog[0]["threads"].as_array().unwrap().iter().find(|entry| entry["no"] == thread).unwrap();
                    assert_eq!(card.get("capcode_replies"), full["posts"][0].get("capcode_replies"));
                    assert_eq!(card["last_replies"], json!(&full["posts"].as_array().unwrap()[14..]));
                    let empty_card = catalog[0]["threads"].as_array().unwrap().iter().find(|entry| entry["no"] == empty).unwrap();
                    assert!(empty_card.get("capcode_replies").is_none());
                    let empty = get(router, &format!("/{board}/thread/{empty}.json")).await;
                    assert!(empty["posts"][0].get("capcode_replies").is_none());
                }
            }
        }
        // JSON archive projection drops saved labels, including historical ones.
        sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1").bind(thread).execute(&owner).await.unwrap();
        for router in [&html, &api] {
            let archived = get(router, &format!("/{board}/thread/{thread}.json")).await;
            assert_eq!(archived["posts"][0]["archived"], 1);
            assert_eq!(archived["posts"][0]["closed"], 1);
            assert!(archived["posts"].as_array().unwrap().iter().all(|post| post.get("id").is_none()));
            assert_eq!(archived["posts"][0]["capcode_replies"], Value::Object(expected_groups.clone()));
            assert_eq!(get(router, &format!("/{board}/archive.json")).await, json!([thread]));
        }
        let persisted: (String, Option<String>, Option<String>) = sqlx::query_as("SELECT name,trip,poster_id FROM content.posts WHERE id=$1")
            .bind(thread).fetch_one(&owner).await.unwrap();
        assert_eq!(persisted, ("Owned & \"".into(), None, Some("Ab12+/CD".into())));
        sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) SELECT nextval('content.post_number'),$1,$2,'Anonymous','','Owned snapshot limit fixture' FROM generate_series(1,1001)")
            .bind(&board).bind(empty).execute(&owner).await.unwrap();
        assert!(matches!(board_store::json_board_snapshot(&public,&board,board_store::BoardSelection::All,5).await,Err(board_store::StoreError::ReadLimit)));
        sqlx::query("UPDATE content.boards SET staff_only=true WHERE slug=$1").bind(&board).execute(&owner).await.unwrap();
        for router in [&html, &api] {
            let response = router.clone().oneshot(Request::get(format!("/{board}/thread/{thread}.json")).body(Body::empty()).unwrap()).await.unwrap();
            assert_eq!(response.status(), 404);
        }
        assert_eq!(sqlx::query_scalar::<_, i64>("SELECT count(*) FROM content.posts WHERE board=$1").bind(&board).fetch_one(&public).await.unwrap(), 0);
        assert_eq!(sqlx::query_scalar::<_, i64>("SELECT count(*) FROM content.boards WHERE slug=$1").bind(&board).fetch_one(&public).await.unwrap(), 0);
    }).await;
    for query in [
        "DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(query)
            .bind(&board)
            .execute(&owner)
            .await
            .unwrap();
    }
    public.close().await;
    owner.close().await;
    result.unwrap();
}
