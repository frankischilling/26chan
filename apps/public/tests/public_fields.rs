#![cfg(feature = "database-tests")]

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use rand_core::{OsRng, RngCore};
use tower::ServiceExt;

fn form(path: &str, name: &str, subject: &str) -> Request<Body> {
    let encode = |value: &str| {
        value
            .as_bytes()
            .iter()
            .map(|byte| format!("%{byte:02X}"))
            .collect::<String>()
    };
    Request::builder()
        .method("POST")
        .uri(path)
        .header("origin", "http://127.0.0.1:3000")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(format!(
            "name={}&sub={}&com=Synthetic+field+boundary&password=owned-field-password&resto=0",
            encode(name),
            encode(subject)
        )))
        .unwrap()
}

#[tokio::test]
async fn both_public_routes_validate_input_bytes_and_persist_complete_fields() {
    let owner = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut random = [0_u8; 4];
    OsRng.fill_bytes(&mut random);
    let board = format!(
        "pf{}",
        random
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Field boundary','Owned synthetic field test',4000,100,100,100,10)")
        .bind(&board).execute(&owner).await.unwrap();
    let app = board_public::router(public.clone(), "http://127.0.0.1:3000".into(), false);
    let mut accepted = 0_i64;
    for route in ["post", "imgboard.php"] {
        for value in ["<&".repeat(50), "é".repeat(50), "😀".repeat(25)] {
            let path = format!("/{board}/{route}");
            let response = app
                .clone()
                .oneshot(form(&path, &value, &value))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::SEE_OTHER);
            let location = response.headers()["location"].to_str().unwrap();
            let id: i64 = location.split("#p").nth(1).unwrap().parse().unwrap();
            accepted += 1;
            let stored = board_store::find_post(&public, &board, id).await.unwrap();
            assert_eq!(stored.name, value);
            assert_eq!(stored.subject, value);
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("/{board}/thread/{id}.json"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(json["posts"][0]["name"], value);
            assert_eq!(json["posts"][0]["sub"], value);
            for (name, subject) in [
                (format!("{value}x"), value.clone()),
                (value.clone(), format!("{value}x")),
            ] {
                assert_eq!(
                    app.clone()
                        .oneshot(form(&path, &name, &subject))
                        .await
                        .unwrap()
                        .status(),
                    StatusCode::UNPROCESSABLE_ENTITY
                );
            }
            let count: i64 =
                sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=$1")
                    .bind(&board)
                    .fetch_one(&public)
                    .await
                    .unwrap();
            assert_eq!(count, accepted);
        }
    }
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
}
