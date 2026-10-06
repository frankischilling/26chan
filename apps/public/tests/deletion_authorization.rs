#![cfg(feature = "database-tests")]

use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use board_store::{
    NewPost, StoreError, media::MediaQueue, media_assets::OutputMetadata,
    media_intake::IntakeStore, post_media::NewAttachment,
};
use rand_core::OsRng;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tower::ServiceExt;

const ORIGIN: &str = "http://127.0.0.1:3000";
const PASSWORD: &str = "owned-deletion-authorization-password";

fn request(board: &str, id: i64, route: u8, file_only: bool) -> Request<Body> {
    request_with_password(board, id, route, file_only, PASSWORD)
}

fn request_with_password(
    board: &str,
    id: i64,
    route: u8,
    file_only: bool,
    password: &str,
) -> Request<Body> {
    let id = id.to_string();
    let mut fields = if route == 0 {
        vec![("no", id.as_str()), ("password", password)]
    } else {
        vec![
            ("mode", "usrdel"),
            (id.as_str(), "delete"),
            ("pwd", password),
        ]
    };
    if file_only {
        fields.push(if route == 0 {
            ("file_only", "true")
        } else {
            ("onlyimgdel", "on")
        });
    }
    let (content_type, body) = if route == 2 {
        let mut body = String::new();
        for (name, value) in &fields {
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
                .extend_pairs(&fields)
                .finish(),
        )
    };
    let action = if route == 0 { "delete" } else { "imgboard.php" };
    Request::post(format!("/{board}/{action}"))
        .header("origin", ORIGIN)
        .header("content-type", content_type)
        .body(Body::from(body))
        .unwrap()
}

struct Fixture {
    owner: PgPool,
    public: PgPool,
    intake: IntakeStore,
    queue: MediaQueue,
    board: String,
    boards: Arc<Mutex<Vec<String>>>,
    jobs: Arc<Mutex<Vec<String>>>,
    hash: String,
}

impl Fixture {
    async fn post(&self, parent: i64, attached: bool) -> i64 {
        let attachment = if attached {
            let upload = self
                .intake
                .reserve("owned-authorization.png")
                .await
                .unwrap();
            self.jobs.lock().unwrap().push(upload.id.clone());
            self.intake
                .begin_upload(&upload.id, &upload.capability)
                .await
                .unwrap();
            self.intake
                .finish_upload(&upload.id, &upload.capability, 100)
                .await
                .unwrap();
            let claim = self.queue.claim().await.unwrap().unwrap();
            assert_eq!(claim.id, upload.id, "This test owns an idle media queue");
            let token = claim.lease_token.unwrap();
            let output = self
                .queue
                .prepare_output(
                    &claim.id,
                    &token,
                    &OutputMetadata {
                        sha256: "a".repeat(64),
                        bytes: 123,
                        width: 10,
                        height: 20,
                    },
                )
                .await
                .unwrap();
            self.queue
                .approve_output(&claim.id, &token, &output.id)
                .await
                .unwrap();
            Some(NewAttachment {
                upload,
                spoiler: false,
            })
        } else {
            None
        };
        board_store::create_post_with_attachment(
            &self.public,
            &self.board,
            parent,
            &NewPost {
                name: "Anonymous".into(),
                subject: "Owned deletion authorization".into(),
                comment: "Owned post that must survive revoked deletion authority".into(),
                deletion_hash: self.hash.clone(),
                sage: false,
            },
            attachment.as_ref(),
        )
        .await
        .unwrap()
    }

    async fn eligibility(&self) {
        use axum::body::to_bytes;
        sqlx::query("UPDATE content.boards SET deletion_known_min_seconds=60,deletion_unknown_min_seconds=600,archive_retention_seconds=86400 WHERE slug=$1")
            .bind(&self.board).execute(&self.owner).await.unwrap();
        let limits = board_config::PublicRequestLimits::from_lookup(|key| match key {
            "PUBLIC_WRITES_PER_MINUTE" => Some("60".into()),
            _ => None,
        })
        .unwrap();
        let app = board_public::routers_with_limits(
            self.public.clone(),
            ORIGIN.into(),
            false,
            None,
            limits,
        )
        .0;
        for route in 0..3 {
            for file_only in [false, true] {
                let post = self.post(0, file_only).await;
                for (age, deny_op, password, message) in [
                    (
                        0,
                        false,
                        PASSWORD,
                        "Error: You must wait longer before deleting this post.",
                    ),
                    (0, false, "wrong-password", "Error: Password incorrect."),
                    (
                        1801,
                        true,
                        "wrong-password",
                        "Error: You cannot delete a post this old.",
                    ),
                    (
                        601,
                        true,
                        "wrong-password",
                        "Error: You cannot delete this post.",
                    ),
                ] {
                    sqlx::query("UPDATE content.posts SET created_at=clock_timestamp()-make_interval(secs=>$2) WHERE id=$1").bind(post).bind(age as f64).execute(&self.owner).await.unwrap();
                    sqlx::query("UPDATE content.boards SET deletion_no_op=$2 WHERE slug=$1")
                        .bind(&self.board)
                        .bind(deny_op)
                        .execute(&self.owner)
                        .await
                        .unwrap();
                    let before: (bool, chrono::DateTime<chrono::Utc>) = sqlx::query_as(
                        "SELECT deleted,modified_at FROM content.threads WHERE id=$1",
                    )
                    .bind(post)
                    .fetch_one(&self.owner)
                    .await
                    .unwrap();
                    let response = app
                        .clone()
                        .oneshot(request_with_password(
                            &self.board,
                            post,
                            route,
                            file_only,
                            password,
                        ))
                        .await
                        .unwrap();
                    assert_eq!(response.status(), StatusCode::FORBIDDEN);
                    let body = String::from_utf8(
                        to_bytes(response.into_body(), 65536)
                            .await
                            .unwrap()
                            .to_vec(),
                    )
                    .unwrap();
                    assert!(body.contains(message), "{body}");
                    let after: (bool, chrono::DateTime<chrono::Utc>) = sqlx::query_as(
                        "SELECT deleted,modified_at FROM content.threads WHERE id=$1",
                    )
                    .bind(post)
                    .fetch_one(&self.owner)
                    .await
                    .unwrap();
                    assert_eq!(before, after);
                    if file_only {
                        assert!(
                            !board_store::post_media::attachment(&self.public, post)
                                .await
                                .unwrap()
                                .unwrap()
                                .file_deleted
                        );
                    }
                }
                sqlx::query("UPDATE content.boards SET deletion_no_op=false WHERE slug=$1")
                    .bind(&self.board)
                    .execute(&self.owner)
                    .await
                    .unwrap();
                let response = app
                    .clone()
                    .oneshot(request(&self.board, post, route, file_only))
                    .await
                    .unwrap();
                assert_eq!(
                    response.status(),
                    if route == 0 {
                        StatusCode::SEE_OTHER
                    } else {
                        StatusCode::OK
                    }
                );
                if file_only {
                    assert!(
                        board_store::find_post(&self.public, &self.board, post)
                            .await
                            .is_ok()
                    );
                    assert!(
                        board_store::post_media::attachment(&self.public, post)
                            .await
                            .unwrap()
                            .unwrap()
                            .file_deleted
                    );
                } else {
                    assert!(matches!(
                        board_store::find_post(&self.public, &self.board, post).await,
                        Err(StoreError::NotFound)
                    ));
                }
            }
        }
    }

    async fn exercise(&self) {
        let limits = board_config::PublicRequestLimits::from_lookup(|key| match key {
            "PUBLIC_WRITES_PER_MINUTE" => Some("60".into()),
            _ => None,
        })
        .unwrap();
        let app = board_public::routers_with_limits(
            self.public.clone(),
            ORIGIN.into(),
            false,
            None,
            limits,
        )
        .0;
        let public_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&self.public)
            .await
            .unwrap();
        let parent = self.post(0, false).await;
        for route in 0..3 {
            for revoke in [false, true] {
                for file_only in [false, true] {
                    for reply in [false, true] {
                        let id = self.post(if reply { parent } else { 0 }, file_only).await;
                        if route == 2 {
                            sqlx::query("SET default_transaction_isolation='repeatable read'")
                                .execute(&self.public)
                                .await
                                .unwrap();
                        }
                        let mut change = self.owner.begin().await.unwrap();
                        let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                            .fetch_one(&mut *change)
                            .await
                            .unwrap();
                        sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
                            .bind(&self.board)
                            .execute(&mut *change)
                            .await
                            .unwrap();
                        if revoke {
                            sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id=$1")
                                .bind(id)
                                .execute(&mut *change)
                                .await
                                .unwrap();
                        } else {
                            sqlx::query("UPDATE post_secrets.deletion SET password_hash='owned-rotated-hash' WHERE post_id=$1")
                            .bind(id).execute(&mut *change).await.unwrap();
                        }
                        let deletion =
                            app.clone()
                                .oneshot(request(&self.board, id, route, file_only));
                        let deleting = tokio::spawn(deletion);
                        tokio::time::timeout(Duration::from_secs(5), async {
                        loop {
                            let blocked: bool =
                                sqlx::query_scalar("SELECT $2=ANY(pg_blocking_pids($1))")
                                    .bind(public_pid)
                                    .bind(blocker)
                                    .fetch_one(&self.owner)
                                    .await
                                    .unwrap();
                            if blocked {
                                break;
                            }
                            tokio::time::sleep(Duration::from_millis(10)).await;
                        }
                    })
                    .await
                    .expect(
                        "The real deletion must verify its password and wait on the mutation lock",
                    );
                        change.commit().await.unwrap();
                        let response = deleting.await.unwrap().unwrap();
                        assert_eq!(
                            response.status(),
                            StatusCode::FORBIDDEN,
                            "A queued deletion must reject changed or missing password state; route={route}, revoke={revoke}, file_only={file_only}, reply={reply}"
                        );
                        assert!(
                            board_store::find_post(&self.public, &self.board, id)
                                .await
                                .is_ok()
                        );
                        if file_only {
                            assert!(
                                !board_store::post_media::attachment(&self.public, id)
                                    .await
                                    .unwrap()
                                    .unwrap()
                                    .file_deleted
                            );
                        }
                        // A fresh request with restored, valid authority succeeds on the same path.
                        sqlx::query("INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES($1,$2) ON CONFLICT(post_id) DO UPDATE SET password_hash=EXCLUDED.password_hash")
                        .bind(id).bind(&self.hash).execute(&self.owner).await.unwrap();
                        let response = app
                            .clone()
                            .oneshot(request(&self.board, id, route, file_only))
                            .await
                            .unwrap();
                        assert_eq!(
                            response.status(),
                            if route == 0 {
                                StatusCode::SEE_OTHER
                            } else {
                                StatusCode::OK
                            }
                        );
                        if file_only {
                            assert!(
                                board_store::find_post(&self.public, &self.board, id)
                                    .await
                                    .is_ok()
                            );
                            assert!(
                                board_store::post_media::attachment(&self.public, id)
                                    .await
                                    .unwrap()
                                    .unwrap()
                                    .file_deleted
                            );
                        } else {
                            assert!(matches!(
                                board_store::find_post(&self.public, &self.board, id).await,
                                Err(StoreError::NotFound)
                            ));
                        }
                        assert!(
                            board_store::find_post(&self.public, &self.board, parent)
                                .await
                                .is_ok()
                        );
                        if route == 2 {
                            sqlx::query("SET default_transaction_isolation='read committed'")
                                .execute(&self.public)
                                .await
                                .unwrap();
                        }
                    }
                }
            }
        }
    }

    async fn credential_changes_wait_for_mutations(&self) {
        let id = self.post(0, false).await;
        for statement in [
            "UPDATE post_secrets.deletion SET password_hash='owned-later-hash' WHERE post_id=$1",
            "DELETE FROM post_secrets.deletion WHERE post_id=$1",
        ] {
            // The public role still cannot change the authority it is checking.
            let error = sqlx::query(statement)
                .bind(id)
                .execute(&self.public)
                .await
                .unwrap_err();
            assert_eq!(
                error
                    .as_database_error()
                    .and_then(|error| error.code())
                    .as_deref(),
                Some("42501")
            );

            let mut mutation = self.public.begin().await.unwrap();
            let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                .fetch_one(&mut *mutation)
                .await
                .unwrap();
            sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
                .bind(&self.board)
                .execute(&mut *mutation)
                .await
                .unwrap();
            let mut writer = self.owner.acquire().await.unwrap();
            let writer_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                .fetch_one(&mut *writer)
                .await
                .unwrap();
            let changing =
                tokio::spawn(
                    async move { sqlx::query(statement).bind(id).execute(&mut *writer).await },
                );
            tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    assert!(
                        !changing.is_finished(),
                        "Credential changes must wait for an in-flight mutation"
                    );
                    let blocked: bool = sqlx::query_scalar("SELECT $2=ANY(pg_blocking_pids($1))")
                        .bind(writer_pid)
                        .bind(blocker)
                        .fetch_one(&self.owner)
                        .await
                        .unwrap();
                    if blocked {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("The credential writer must reach the board lock without taking it explicitly");
            let before: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM post_secrets.deletion WHERE post_id=$1)",
            )
            .bind(id)
            .fetch_one(&mut *mutation)
            .await
            .unwrap();
            assert!(
                before,
                "Authority must remain present while this mutation owns the board"
            );
            mutation.commit().await.unwrap();
            assert_eq!(changing.await.unwrap().unwrap().rows_affected(), 1);
            let current: Option<String> = sqlx::query_scalar(
                "SELECT password_hash FROM post_secrets.deletion WHERE post_id=$1",
            )
            .bind(id)
            .fetch_optional(&self.public)
            .await
            .unwrap();
            if statement.starts_with("UPDATE") {
                assert_eq!(current.as_deref(), Some("owned-later-hash"));
            } else {
                assert!(current.is_none());
            }
            assert!(
                board_store::find_post(&self.public, &self.board, id)
                    .await
                    .is_ok()
            );
        }
    }

    async fn credential_reassignment_locks_both_boards(&self) {
        let next_board = format!("{}b", self.board);
        sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Credential reassignment','Owned fixture',200,100,100,100,10)")
            .bind(&next_board).execute(&self.owner).await.unwrap();
        self.boards.lock().unwrap().push(next_board.clone());
        let source = self.post(0, false).await;
        let target = board_store::create_post(
            &self.public,
            &next_board,
            0,
            &NewPost {
                name: "Anonymous".into(),
                subject: "Owned reassignment target".into(),
                comment: "Owned target for the operator credential test".into(),
                deletion_hash: self.hash.clone(),
                sage: false,
            },
        )
        .await
        .unwrap();
        sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id=$1")
            .bind(target)
            .execute(&self.owner)
            .await
            .unwrap();

        // Hold the second board for both directions: it is first the target's
        // board and then the source's board. Either missing lock must fail.
        for (from, to) in [(source, target), (target, source)] {
            let mut mutation = self.public.begin().await.unwrap();
            let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                .fetch_one(&mut *mutation)
                .await
                .unwrap();
            sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
                .bind(&next_board)
                .execute(&mut *mutation)
                .await
                .unwrap();
            let mut writer = self.owner.acquire().await.unwrap();
            let writer_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                .fetch_one(&mut *writer)
                .await
                .unwrap();
            let changing = tokio::spawn(async move {
                sqlx::query("UPDATE post_secrets.deletion SET post_id=$2 WHERE post_id=$1")
                    .bind(from)
                    .bind(to)
                    .execute(&mut *writer)
                    .await
            });
            tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    assert!(
                        !changing.is_finished(),
                        "Reassignment must lock both affected boards"
                    );
                    let blocked: bool = sqlx::query_scalar("SELECT $2=ANY(pg_blocking_pids($1))")
                        .bind(writer_pid)
                        .bind(blocker)
                        .fetch_one(&self.owner)
                        .await
                        .unwrap();
                    if blocked {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("The credential reassignment must reach the held source or target board lock");
            mutation.commit().await.unwrap();
            assert_eq!(changing.await.unwrap().unwrap().rows_affected(), 1);
            let saved: i64 = sqlx::query_scalar(
                "SELECT post_id FROM post_secrets.deletion WHERE post_id IN ($1,$2)",
            )
            .bind(source)
            .bind(target)
            .fetch_one(&self.public)
            .await
            .unwrap();
            assert_eq!(saved, to);
            assert!(
                board_store::find_post(&self.public, &self.board, source)
                    .await
                    .is_ok()
            );
            assert!(
                board_store::find_post(&self.public, &next_board, target)
                    .await
                    .is_ok()
            );
        }
    }
}

#[tokio::test]
async fn queued_post_and_file_deletions_recheck_rotated_and_revoked_passwords() {
    run(Scenario::QueuedDeletion).await;
}

#[tokio::test]
async fn credential_mutations_wait_for_the_in_flight_public_mutation() {
    run(Scenario::CredentialMutation).await;
}

#[tokio::test]
async fn credential_reassignment_waits_for_source_and_target_board_mutations() {
    run(Scenario::Reassignment).await;
}

#[tokio::test]
async fn source_eligibility_and_error_order_match_on_every_public_deletion_form() {
    run(Scenario::Eligibility).await;
}

enum Scenario {
    Eligibility,
    QueuedDeletion,
    CredentialMutation,
    Reassignment,
}

async fn run(scenario: Scenario) {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = PgPoolOptions::new()
        .max_connections(1)
        .connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let role: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&public)
        .await
        .unwrap();
    assert_eq!(role, "board_public");
    let seed: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&owner)
        .await
        .unwrap();
    let board = format!("da{seed:x}");
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,image_limit,deletion_known_min_seconds,deletion_unknown_min_seconds) VALUES($1,'Deletion authorization','Owned fixtures',200,100,100,100,10,100,0,0)")
        .bind(&board).execute(&owner).await.unwrap();
    let jobs = Arc::new(Mutex::new(Vec::new()));
    let boards = Arc::new(Mutex::new(vec![board.clone()]));
    let fixture = Fixture {
        owner: owner.clone(),
        public: public.clone(),
        board: board.clone(),
        boards: boards.clone(),
        jobs: jobs.clone(),
        intake: IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
            .await
            .unwrap(),
        queue: MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
            .await
            .unwrap(),
        hash: Argon2::default()
            .hash_password(PASSWORD.as_bytes(), &SaltString::generate(&mut OsRng))
            .unwrap()
            .to_string(),
    };
    let outcome = tokio::spawn(async move {
        match scenario {
            Scenario::Eligibility => fixture.eligibility().await,
            Scenario::QueuedDeletion => fixture.exercise().await,
            Scenario::CredentialMutation => fixture.credential_changes_wait_for_mutations().await,
            Scenario::Reassignment => fixture.credential_reassignment_locks_both_boards().await,
        }
    })
    .await;
    let boards = boards.lock().unwrap().clone();
    for board in boards {
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
    }
    let jobs = jobs.lock().unwrap().clone();
    sqlx::query("DELETE FROM media.assets WHERE job_id=ANY($1)")
        .bind(&jobs)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM media.jobs WHERE id=ANY($1)")
        .bind(&jobs)
        .execute(&owner)
        .await
        .unwrap();
    public.close().await;
    owner.close().await;
    outcome.unwrap();
}
