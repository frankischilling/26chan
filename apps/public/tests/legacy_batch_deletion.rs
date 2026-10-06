#![cfg(feature = "database-tests")]

use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use rand_core::OsRng;
use sqlx::PgPool;
use tower::ServiceExt;

const ORIGIN: &str = "http://127.0.0.1:3000";
const PASSWORD: &str = "owned-batch-deletion-password";

struct Fixture {
    owner: PgPool,
    public: PgPool,
    board: String,
    hash: String,
    app: Router,
}

impl Fixture {
    async fn post(&self, parent: i64) -> i64 {
        board_store::create_post(
            &self.public,
            &self.board,
            parent,
            &board_store::NewPost {
                name: "Anonymous".into(),
                subject: "Owned batch fixture".into(),
                comment: "Synthetic selected post".into(),
                deletion_hash: self.hash.clone(),
                sage: false,
            },
        )
        .await
        .unwrap()
    }

    async fn deleted(&self, id: i64) -> bool {
        sqlx::query_scalar("SELECT deleted FROM content.posts WHERE id=$1")
            .bind(id)
            .fetch_one(&self.owner)
            .await
            .unwrap()
    }

    async fn attachment(&self, id: i64) {
        // Synthetic metadata only; no upload queue or filesystem is involved.
        let object = format!("{id:032x}");
        sqlx::query("INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler) VALUES($1,$2,$2,'owned.png',100,10,10,false)")
            .bind(id).bind(object).execute(&self.owner).await.unwrap();
    }

    async fn file_deleted(&self, id: i64) -> bool {
        sqlx::query_scalar("SELECT file_deleted FROM content.post_media WHERE post_id=$1")
            .bind(id)
            .fetch_one(&self.owner)
            .await
            .unwrap()
    }

    async fn submit(
        &self,
        fields: &[(String, String)],
        multipart: bool,
        origin: Option<&str>,
    ) -> (StatusCode, String) {
        let (content_type, body) = if multipart {
            let mut body = String::new();
            for (name, value) in fields {
                body.push_str(&format!(
                    "--owned\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
                ));
            }
            body.push_str("--owned--\r\n");
            ("multipart/form-data; boundary=owned", body)
        } else {
            (
                "application/x-www-form-urlencoded",
                url::form_urlencoded::Serializer::new(String::new())
                    .extend_pairs(fields)
                    .finish(),
            )
        };
        let mut request = Request::post(format!("/{}/imgboard.php", self.board))
            .header("content-type", content_type);
        if let Some(origin) = origin {
            request = request.header("origin", origin);
        }
        let response = self
            .app
            .clone()
            .oneshot(request.body(Body::from(body)).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8(body.to_vec()).unwrap())
    }

    async fn batch(&self, ids: &[i64], multipart: bool, file_only: bool) -> (StatusCode, String) {
        self.submit(&fields(ids, file_only), multipart, Some(ORIGIN))
            .await
    }

    async fn exercise(&self) {
        for multipart in [false, true] {
            let a = self.post(0).await;
            let b = self.post(0).await;
            for origin in [None, Some("null"), Some("https://untrusted.example")] {
                assert_eq!(
                    self.submit(&fields(&[a, b], false), multipart, origin)
                        .await
                        .0,
                    StatusCode::FORBIDDEN
                );
                assert!(!self.deleted(a).await && !self.deleted(b).await);
            }
            let (status, html) = self.batch(&[b, a], multipart, false).await;
            assert_eq!(status, StatusCode::OK);
            assert!(html.contains("Updating index"));
            assert!(!html.contains(PASSWORD));
            assert!(self.deleted(a).await && self.deleted(b).await);
            let single = self.post(0).await;
            assert_eq!(
                self.batch(&[single], multipart, false).await.0,
                StatusCode::OK
            );
            assert!(self.deleted(single).await);

            // Deliberately submit higher IDs first. Sorting or preflight-all
            // would leave `first` undeleted when the later protected item fails.
            for protection in ["password", "sticky", "age"] {
                let denied = self.post(0).await;
                match protection {
                    "password" => {
                        sqlx::query("UPDATE post_secrets.deletion SET password_hash='invalid-other-owner' WHERE post_id=$1").bind(denied).execute(&self.owner).await.unwrap();
                    }
                    "sticky" => {
                        sqlx::query("UPDATE content.threads SET sticky=true WHERE id=$1")
                            .bind(denied)
                            .execute(&self.owner)
                            .await
                            .unwrap();
                    }
                    _ => {
                        sqlx::query("UPDATE content.posts SET created_at=clock_timestamp()-interval '30 days' WHERE id=$1").bind(denied).execute(&self.owner).await.unwrap();
                    }
                }
                let first = self.post(0).await;
                let last = self.post(0).await;
                let (status, html) = self.batch(&[first, denied, last], multipart, false).await;
                assert_eq!(status, StatusCode::FORBIDDEN);
                assert!(html.contains(match protection {
                    "password" => "Error: Password incorrect.",
                    "sticky" => "Error: You cannot delete this post.",
                    _ => "Error: You cannot delete a post this old.",
                }));
                assert!(!html.contains("The deletion was completed."));
                assert!(self.deleted(first).await);
                assert!(!self.deleted(denied).await && !self.deleted(last).await);
                assert_eq!(
                    self.batch(&[denied, last], multipart, false).await.0,
                    StatusCode::FORBIDDEN
                );
                assert!(!self.deleted(last).await);
            }

            // An OP deletion removes its replies. A selected reply encountered
            // afterwards reports the source age error, preserving the OP mutation.
            let op = self.post(0).await;
            let reply = self.post(op).await;
            let last = self.post(0).await;
            assert_eq!(
                self.batch(&[op, reply, last], multipart, false).await.0,
                StatusCode::FORBIDDEN
            );
            assert!(self.deleted(op).await && self.deleted(reply).await);
            assert!(!self.deleted(last).await);
            let op = self.post(0).await;
            let reply = self.post(op).await;
            assert_eq!(
                self.batch(&[reply, op], multipart, false).await.0,
                StatusCode::OK
            );
            assert!(self.deleted(op).await && self.deleted(reply).await);

            let a = self.post(0).await;
            let b = self.post(0).await;
            let (status, html) = self.batch(&[a, i64::MAX, b], multipart, false).await;
            assert_eq!(status, StatusCode::FORBIDDEN);
            assert!(html.contains("Error: You cannot delete a post this old."));
            assert!(self.deleted(a).await && !self.deleted(b).await);
            assert_eq!(
                self.batch(&[i64::MAX, b], multipart, false).await.0,
                StatusCode::FORBIDDEN
            );
            assert!(!self.deleted(b).await);
            let (status, html) = self.batch(&[i64::MAX], multipart, false).await;
            assert_eq!(status, StatusCode::OK);
            assert!(html.contains("Updating index"));

            let uncredentialed = self.post(0).await;
            sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id=$1")
                .bind(uncredentialed)
                .execute(&self.owner)
                .await
                .unwrap();
            assert_eq!(
                self.batch(&[uncredentialed], multipart, false).await.0,
                StatusCode::NOT_FOUND
            );
            assert!(!self.deleted(uncredentialed).await);
            for board in [format!("zz{}", &self.board[2..]), self.board.clone()] {
                if board == self.board {
                    sqlx::query("UPDATE content.boards SET staff_only=true WHERE slug=$1")
                        .bind(&self.board)
                        .execute(&self.owner)
                        .await
                        .unwrap();
                }
                for ids in [
                    vec![i64::MAX],
                    vec![uncredentialed],
                    vec![i64::MAX, uncredentialed],
                ] {
                    let inaccessible = Fixture {
                        owner: self.owner.clone(),
                        public: self.public.clone(),
                        board: board.clone(),
                        hash: self.hash.clone(),
                        app: self.app.clone(),
                    };
                    assert_eq!(
                        inaccessible.batch(&ids, multipart, false).await.0,
                        StatusCode::NOT_FOUND
                    );
                }
                if board == self.board {
                    sqlx::query("UPDATE content.boards SET staff_only=false WHERE slug=$1")
                        .bind(&self.board)
                        .execute(&self.owner)
                        .await
                        .unwrap();
                }
            }
            let modern = self
                .app
                .clone()
                .oneshot(
                    Request::post(format!("/{}/delete", self.board))
                        .header("origin", ORIGIN)
                        .header("content-type", "application/x-www-form-urlencoded")
                        .body(Body::from(format!("no={}&password={PASSWORD}", i64::MAX)))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(modern.status(), StatusCode::NOT_FOUND);

            let op = self.post(0).await;
            let reply = self.post(op).await;
            for id in [op, reply] {
                self.attachment(id).await;
            }
            assert_eq!(
                self.batch(&[op, reply], multipart, true).await.0,
                StatusCode::OK
            );
            for id in [op, reply] {
                assert!(!self.deleted(id).await && self.file_deleted(id).await);
            }
            let a = self.post(0).await;
            let denied = self.post(0).await;
            let last = self.post(0).await;
            for id in [a, denied, last] {
                self.attachment(id).await;
            }
            sqlx::query("UPDATE content.threads SET sticky=true WHERE id=$1")
                .bind(denied)
                .execute(&self.owner)
                .await
                .unwrap();
            assert_eq!(
                self.batch(&[a, denied, last], multipart, true).await.0,
                StatusCode::FORBIDDEN
            );
            assert!(self.file_deleted(a).await);
            assert!(!self.file_deleted(denied).await && !self.file_deleted(last).await);
            for id in [a, denied, last] {
                assert!(!self.deleted(id).await);
            }

            let attached = self.post(0).await;
            let text_only = self.post(0).await;
            self.attachment(attached).await;
            assert_eq!(
                self.batch(&[attached, text_only], multipart, true).await.0,
                StatusCode::NOT_FOUND
            );
            assert!(self.file_deleted(attached).await);
            assert!(!self.deleted(text_only).await);

            // Invalid syntax is rejected before any item mutates storage.
            let a = self.post(0).await;
            for mut bad in [
                fields(&[a, a], false),
                fields(&[], false),
                fields(&[a], false),
                fields(&[a], false),
                fields(&[a], false),
            ]
            .into_iter()
            .enumerate()
            {
                match bad.0 {
                    2 => bad.1.push(("onlyimgdel".into(), "false".into())),
                    3 => bad.1.push(("017".into(), "delete".into())),
                    4 => bad.1.push(("password".into(), PASSWORD.into())),
                    _ => {}
                }
                assert_eq!(
                    self.submit(&bad.1, multipart, Some(ORIGIN)).await.0,
                    StatusCode::UNPROCESSABLE_ENTITY
                );
                assert!(!self.deleted(a).await);
            }
            let mut over = fields(&[a], false);
            over.extend((1..=18).map(|i| ((i64::MAX - i).to_string(), "delete".into())));
            assert_eq!(
                self.submit(&over, multipart, Some(ORIGIN)).await.0,
                StatusCode::UNPROCESSABLE_ENTITY
            );
            assert!(!self.deleted(a).await);
        }

        // IDs beyond JavaScript's exact range remain decimal strings end to end.
        let high: i64 = 9_007_199_254_740_993 + self.board[2..].parse::<i64>().unwrap() * 2;
        for id in [high, high + 1] {
            sqlx::query("INSERT INTO content.threads(id,board) VALUES($1,$2)")
                .bind(id)
                .bind(&self.board)
                .execute(&self.owner)
                .await
                .unwrap();
            sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous','','Owned precise ID')").bind(id).bind(&self.board).execute(&self.owner).await.unwrap();
            sqlx::query("INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES($1,$2)")
                .bind(id)
                .bind(&self.hash)
                .execute(&self.owner)
                .await
                .unwrap();
        }
        assert_eq!(
            self.batch(&[high + 1, high], false, false).await.0,
            StatusCode::OK
        );
        assert!(self.deleted(high).await && self.deleted(high + 1).await);
    }
}

fn fields(ids: &[i64], file_only: bool) -> Vec<(String, String)> {
    let mut fields = vec![
        ("mode".into(), "usrdel".into()),
        ("pwd".into(), PASSWORD.into()),
    ];
    fields.extend(ids.iter().map(|id| (id.to_string(), "delete".into())));
    if file_only {
        fields.push(("onlyimgdel".into(), "on".into()));
    }
    fields
}

#[tokio::test]
async fn legacy_batches_commit_in_submission_order_and_stop_at_the_first_error() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let seed: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&owner)
        .await
        .unwrap();
    let board = format!("lb{seed}");
    // Explicit synthetic policy; imported board policies must remain unchanged.
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,image_limit,deletion_known_min_seconds,deletion_unknown_min_seconds,deletion_max_seconds) VALUES($1,'Legacy batch deletion','Owned fixtures',200,100,100,1000,10,100,0,0,86400)")
        .bind(&board).execute(&owner).await.unwrap();
    let limits = board_config::PublicRequestLimits::from_lookup(|key| match key {
        "PUBLIC_WRITES_PER_MINUTE" => Some("120".into()),
        _ => None,
    })
    .unwrap();
    let app =
        board_public::routers_with_limits(public.clone(), ORIGIN.into(), false, None, limits).0;
    let fixture = Fixture {
        owner: owner.clone(),
        public: public.clone(),
        board: board.clone(),
        app,
        hash: Argon2::default()
            .hash_password(PASSWORD.as_bytes(), &SaltString::generate(&mut OsRng))
            .unwrap()
            .to_string(),
    };
    let outcome = tokio::spawn(async move {
        fixture.exercise().await;
    })
    .await;
    for statement in [
        "DELETE FROM content.post_media WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(statement)
            .bind(&board)
            .execute(&owner)
            .await
            .unwrap();
    }
    public.close().await;
    owner.close().await;
    outcome.unwrap();
}
