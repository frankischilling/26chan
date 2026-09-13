#![cfg(feature = "database-tests")]
use axum::{Router, body::Body, http::Request, response::Response};
use board_media::{ApprovedFiles, ObjectId, PublicationStore, Quarantine, ValidatedOutput};
use board_store::legacy_media::LegacyMediaStore;
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

async fn request(
    app: &Router,
    path: &str,
    method: &str,
    validator: Option<(&str, &str)>,
) -> Response {
    let mut request = Request::builder().uri(path).method(method);
    if let Some((key, value)) = validator {
        request = request.header(key, value);
    }
    app.clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}
async fn json(response: Response) -> Value {
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

#[tokio::test]
async fn legacy_manifest_upgrade_invalidates_public_and_api_dates_without_renumbering() {
    let owner = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let id = ObjectId::generate().unwrap().to_string();
    let board = id[..10].to_owned();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES ($1,'Legacy API test','Synthetic',2000,100,100,100,10)").bind(&board).execute(&owner).await.unwrap();
    let test_owner = owner.clone();
    let test_board = board.clone();
    let test_id = id.clone();
    let result = tokio::spawn(async move {
        let post = board_store::create_post(&public, &test_board, 0, &board_store::NewPost {
            name: "Anonymous".into(), subject: "Legacy fixture".into(), comment: "Keep the post".into(), deletion_hash: "synthetic-unused".into(), sage: false,
        }).await.unwrap();
        let mut pixels = b"IBRGBA01".to_vec();
        pixels.extend_from_slice(&500u32.to_be_bytes()); pixels.extend_from_slice(&300u32.to_be_bytes());
        for _ in 0..500*300 { pixels.extend_from_slice(&[30,50,90,255]); }
        let output = ValidatedOutput::read(pixels.as_slice()).await.unwrap();
        let full = output.encode().unwrap();
        sqlx::query("INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at) VALUES ($1,$1,$1,$2,$3,500,300,'approved',clock_timestamp())")
            .bind(&test_id).bind(full.sha256()).bind(full.len() as i64).execute(&test_owner).await.unwrap();
        sqlx::query("INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler) VALUES ($1,$2,$2,'legacy.png',$3,500,300,false)")
            .bind(post).bind(&test_id).bind(full.len() as i64).execute(&test_owner).await.unwrap();
        sqlx::query("UPDATE content.threads SET modified_at=clock_timestamp()-interval '1 hour' WHERE id=$1").bind(post).execute(&test_owner).await.unwrap();
        let temp = tempfile::tempdir().unwrap();
        let quarantine = Quarantine::new(temp.path().join("quarantine")).unwrap();
        let root = temp.path().join("objects");
        let store = PublicationStore::new(&root, &quarantine).unwrap();
        let guard = store.try_lock().unwrap();
        guard.install(test_id.parse().unwrap(), &full).unwrap();
        let (web, api) = board_public::routers(public, "http://127.0.0.1:3000".into(), false);
        let paths = [format!("/{test_board}/thread/{post}.json"), format!("/{test_board}/1.json"), format!("/{test_board}/catalog.json")];
        let mut old = Vec::new();
        for app in [&web, &api] {
            for path in &paths {
                let response = request(app, path, "GET", None).await;
                assert_eq!(response.status(),200);
                let etag = response.headers()["etag"].to_str().unwrap().to_owned();
                let modified = response.headers().get("last-modified").map(|h| h.to_str().unwrap().to_owned());
                old.push((etag, modified, json(response).await));
            }
        }
        let before = &old[0].2["posts"][0];
        assert!(before.get("md5").is_none()); assert!(before.get("tn_w").is_none());
        let admin = LegacyMediaStore::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap()).await.unwrap();
        let snapshot = admin.get(&test_id).await.unwrap();
        board_media_admin::backfill::complete_backfill(&admin,&guard,&ApprovedFiles::open(&root).unwrap(),&snapshot,&output).await.unwrap();
        for (app_index,app) in [&web,&api].into_iter().enumerate() {
            for (index,path) in paths.iter().enumerate() {
                let prior = &old[app_index*3+index];
                let response = request(app,path,"GET",Some(("if-none-match",&prior.0))).await;
                assert_eq!(response.status(),200); assert_ne!(response.headers()["etag"],prior.0);
                let value = json(response).await;
                if index == 0 {
                    let after = &value["posts"][0];
                    for key in ["no","tim","filename","ext","fsize","w","h","com"] { assert_eq!(before[key],after[key],"{key}"); }
                    assert_eq!(after["tn_w"],250); assert_eq!(after["tn_h"],150);
                    assert_eq!(after["md5"].as_str().unwrap().len(),24);
                    let date = prior.1.as_ref().unwrap();
                    let response = request(app,path,"GET",Some(("if-modified-since",date))).await;
                    assert_eq!(response.status(),200); assert_ne!(response.headers()["last-modified"],*date);
                    let head = request(app,path,"HEAD",Some(("if-modified-since",date))).await;
                    assert_eq!(head.status(),200);
                    assert!(head.into_body().collect().await.unwrap().to_bytes().is_empty());
                }
            }
        }
    }).await;
    for query in [
        "DELETE FROM content.post_media WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
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
    sqlx::query("DELETE FROM media.assets WHERE id=$1")
        .bind(&id)
        .execute(&owner)
        .await
        .unwrap();
    result.unwrap();
}
