#![cfg(feature = "database-tests")]

use axum::{body::Body, http::Request};
use board_store::{NewPost, StoreError};
use chrono::{DateTime, Utc};
use http_body_util::BodyExt;
use rand_core::{OsRng, RngCore};
use sqlx::PgPool;
use std::time::Duration;
use tower::ServiceExt;

fn post(sage: bool) -> NewPost {
    NewPost {
        name: "Anonymous".into(),
        subject: String::new(),
        comment: "Owned age fixture".into(),
        deletion_hash: "unused-fixture-hash".into(),
        sage,
    }
}

struct Fixture {
    owner: PgPool,
    public: PgPool,
    slug: String,
    id: i64,
}

impl Fixture {
    async fn policy(&self, hours: i32) {
        sqlx::query("UPDATE content.boards SET permasage_hours=$2 WHERE slug=$1")
            .bind(&self.slug)
            .bind(hours)
            .execute(&self.owner)
            .await
            .unwrap();
    }

    async fn state(&self, op_seconds: i64, flags: (bool, bool, bool)) {
        // Deliberately different thread creation and bump clocks catch use of
        // either in place of the source OP post time. Retain fractional seconds.
        sqlx::query("UPDATE content.posts SET created_at=to_timestamp($2)+interval '900 milliseconds' WHERE id=$1")
            .bind(self.id).bind(op_seconds as f64).execute(&self.owner).await.unwrap();
        sqlx::query("UPDATE content.threads SET created_at='2000-01-01Z',bumped_at='2001-01-01Z',modified_at='2001-01-01Z',sticky=$2,permasage=$3,permaage=$4 WHERE id=$1")
            .bind(self.id).bind(flags.0).bind(flags.1).bind(flags.2).execute(&self.owner).await.unwrap();
    }

    async fn append(&self, request_seconds: i64, sage: bool, bump: bool) {
        let before = board_store::thread(&self.public, &self.slug, self.id)
            .await
            .unwrap();
        board_store::create_post_with_attachment_at(
            &self.public,
            &self.slug,
            self.id,
            &post(sage),
            None,
            DateTime::from_timestamp(request_seconds, 0).unwrap(),
        )
        .await
        .unwrap();
        let after = board_store::thread(&self.public, &self.slug, self.id)
            .await
            .unwrap();
        assert_eq!(after.bumped_at > before.bumped_at, bump);
        assert_eq!(after.reply_count, before.reply_count + 1);
        assert!(after.modified_at > before.modified_at);
    }

    async fn indicators(&self, app: &axum::Router) {
        for (suffix, kind) in [
            (format!("thread/{}.json", self.id), 0),
            (format!("thread/{}-tail.json", self.id), 1),
            ("1.json".into(), 2),
            ("catalog.json".into(), 3),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::get(format!("/{}/{suffix}", self.slug))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), 200, "{suffix}");
            let value: serde_json::Value =
                serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                    .unwrap();
            let op = match kind {
                0 | 1 => &value["posts"][0],
                2 => &value["threads"][0]["posts"][0],
                _ => &value[0]["threads"][0],
            };
            assert_eq!(op["no"], self.id);
            assert!(op.get("permasage_hours").is_none());
            if kind == 1 {
                assert_eq!(op["bumplimit"], 0);
            } else {
                assert!(op.get("bumplimit").is_none());
            }
        }
        let response = app
            .clone()
            .oneshot(
                Request::get(format!("/{}/catalog", self.slug))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let html = String::from_utf8(
            response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec(),
        )
        .unwrap();
        assert!(
            !html.contains("<i>R: <b>"),
            "age does not set the count indicator"
        );
    }

    async fn exercise(&self) {
        let created = 1_700_000_000;
        for hours in [0, 1, 48, 120, 168, 336, i32::MAX] {
            self.policy(hours).await;
            for (age, expected) in [
                (3599, true),
                (3600, hours != 1),
                (3601, hours != 1),
                (-1, true),
            ] {
                self.state(created, (false, false, false)).await;
                self.append(created + age, false, expected).await;
            }
            // Chrono's representable calendar is narrower than i64 Unix seconds;
            // its maximum policy arithmetic is covered in the domain test.
            if hours > 0 && hours < i32::MAX {
                for (offset, expected) in [(-1, true), (0, false), (1, false)] {
                    self.state(created, (false, false, false)).await;
                    self.append(created + i64::from(hours) * 3600 + offset, false, expected)
                        .await;
                }
            }
        }
        self.policy(1).await;
        for sticky in [false, true] {
            for permasage in [false, true] {
                for permaage in [false, true] {
                    for sage in [false, true] {
                        self.state(created, (sticky, permasage, permaage)).await;
                        self.append(created + 3600, sage, !sticky && !permasage && permaage)
                            .await;
                    }
                }
            }
        }
        self.state(created, (false, false, false)).await;
        let app = board_public::router(self.public.clone(), "http://127.0.0.1:3000".into(), false);
        self.indicators(&app).await;
        // Both production posting aliases and both accepted body encodings.
        for route in ["post", "imgboard.php"] {
            for multipart in [false, true] {
                self.state(created, (false, false, false)).await;
                let before = board_store::thread(&self.public, &self.slug, self.id)
                    .await
                    .unwrap();
                let (content_type, body) = if multipart {
                    (
                        "multipart/form-data; boundary=age",
                        format!(
                            "--age\r\nContent-Disposition: form-data; name=\"resto\"\r\n\r\n{}\r\n--age\r\nContent-Disposition: form-data; name=\"com\"\r\n\r\nOwned age reply\r\n--age\r\nContent-Disposition: form-data; name=\"pwd\"\r\n\r\nowned-secret\r\n--age--\r\n",
                            self.id
                        ),
                    )
                } else {
                    (
                        "application/x-www-form-urlencoded",
                        format!("resto={}&com=Owned+age+reply&pwd=owned-secret", self.id),
                    )
                };
                let response = app
                    .clone()
                    .oneshot(
                        Request::post(format!("/{}/{route}", self.slug))
                            .header("origin", "http://127.0.0.1:3000")
                            .header("accept", "application/json")
                            .header("content-type", content_type)
                            .header("x-request-start", created.to_string())
                            .body(Body::from(body))
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), 200);
                let result: serde_json::Value = serde_json::from_slice(
                    &response.into_body().collect().await.unwrap().to_bytes(),
                )
                .unwrap();
                assert!(result["pid"].as_i64().is_some_and(|id| id > 0), "{result}");
                assert_eq!(result["tid"], self.id);
                let after = board_store::thread(&self.public, &self.slug, self.id)
                    .await
                    .unwrap();
                assert_eq!(after.bumped_at, before.bumped_at);
                assert_eq!(after.reply_count, before.reply_count + 1);
            }
        }
        // Failed later insertion must roll back the accepted bump/count update.
        self.state(created, (false, false, true)).await;
        let before = board_store::thread(&self.public, &self.slug, self.id)
            .await
            .unwrap();
        let mut invalid = post(false);
        invalid.deletion_hash = "x".repeat(257);
        assert!(matches!(
            board_store::create_post(&self.public, &self.slug, self.id, &invalid).await,
            Err(StoreError::Database(_))
        ));
        let after = board_store::thread(&self.public, &self.slug, self.id)
            .await
            .unwrap();
        assert_eq!(
            (after.bumped_at, after.modified_at, after.reply_count),
            (before.bumped_at, before.modified_at, before.reply_count)
        );
        sqlx::query("UPDATE content.threads SET closed=true WHERE id=$1")
            .bind(self.id)
            .execute(&self.owner)
            .await
            .unwrap();
        assert!(matches!(
            board_store::create_post(&self.public, &self.slug, self.id, &post(false)).await,
            Err(StoreError::Conflict(_))
        ));
        sqlx::query("UPDATE content.threads SET closed=false WHERE id=$1")
            .bind(self.id)
            .execute(&self.owner)
            .await
            .unwrap();
        self.delayed_http_body().await;
        self.lock_wait().await;
    }

    async fn delayed_http_body(&self) {
        self.policy(1).await;
        self.state(1_700_000_000, (false, false, false)).await;
        let owner = self.owner.clone();
        let id = self.id;
        let body = Body::from_stream(futures_util::stream::once(async move {
            // The request clock is already captured when the body is polled.
            // Set a cutoff one whole second later and cross it before yielding.
            let polled = Utc::now().timestamp();
            sqlx::query("UPDATE content.posts SET created_at=to_timestamp($2)+interval '900 milliseconds' WHERE id=$1")
                .bind(id).bind((polled - 3599) as f64).execute(&owner).await.unwrap();
            while Utc::now().timestamp() <= polled {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            Ok::<_, std::io::Error>(bytes::Bytes::from(format!(
                "resto={id}&com=Delayed+owned+body&pwd=owned-secret"
            )))
        }));
        let app = board_public::router(self.public.clone(), "http://127.0.0.1:3000".into(), false);
        let response = app
            .oneshot(
                Request::post(format!("/{}/imgboard.php", self.slug))
                    .header("origin", "http://127.0.0.1:3000")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(body)
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            303,
            "ordinary no-JavaScript posting succeeds"
        );
        assert!(
            response.headers()["location"]
                .to_str()
                .unwrap()
                .starts_with(&format!("/{}/thread/{}#p", self.slug, self.id))
        );
        let after = board_store::thread(&self.public, &self.slug, self.id)
            .await
            .unwrap();
        assert!(
            after.bumped_at.timestamp() > 1_000_000_000,
            "production handler must retain its pre-body request clock"
        );
        board_store::create_post(&self.public, &self.slug, self.id, &post(false))
            .await
            .unwrap();
        assert_eq!(
            board_store::thread(&self.public, &self.slug, self.id)
                .await
                .unwrap()
                .bumped_at,
            after.bumped_at
        );
    }

    async fn lock_wait(&self) {
        // A real public-role write reaches the board lock before the cutoff. The
        // operator commits a new policy while it waits; the captured clock stays.
        self.policy(0).await;
        let start = Utc::now().timestamp();
        self.state(start - 3598, (false, false, false)).await;
        let mut lock = self.owner.begin().await.unwrap();
        let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *lock)
            .await
            .unwrap();
        sqlx::query("UPDATE content.boards SET permasage_hours=1 WHERE slug=$1")
            .bind(&self.slug)
            .execute(&mut *lock)
            .await
            .unwrap();
        let public = self.public.clone();
        let slug = self.slug.clone();
        let id = self.id;
        let writer = tokio::spawn(async move {
            board_store::create_post_with_attachment_at(
                &public,
                &slug,
                id,
                &post(false),
                None,
                DateTime::from_timestamp(start, 0).unwrap(),
            )
            .await
        });
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let blocked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_locks WHERE NOT granted AND $1=ANY(pg_blocking_pids(pid)))")
                    .bind(pid).fetch_one(&self.owner).await.unwrap();
                if blocked { break; }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.expect("healthy public writer must reach the held board lock");
        while Utc::now().timestamp() < start + 2 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(!writer.is_finished());
        lock.commit().await.unwrap();
        let reply = writer.await.unwrap().unwrap();
        let saved: DateTime<Utc> =
            sqlx::query_scalar("SELECT created_at FROM content.posts WHERE id=$1")
                .bind(reply)
                .fetch_one(&self.public)
                .await
                .unwrap();
        assert_eq!(
            saved,
            DateTime::from_timestamp(start, 0).unwrap(),
            "the persisted post clock must also survive the mutation lock wait"
        );
        let after = board_store::thread(&self.public, &self.slug, self.id)
            .await
            .unwrap();
        assert!(
            after.bumped_at.timestamp() > 1_000_000_000,
            "pre-cutoff request still bumps after lock wait"
        );
        assert_eq!(
            board_store::board(&self.public, &self.slug)
                .await
                .unwrap()
                .permasage_hours,
            1
        );
        let before = after.bumped_at;
        board_store::create_post(&self.public, &self.slug, self.id, &post(false))
            .await
            .unwrap();
        assert_eq!(
            board_store::thread(&self.public, &self.slug, self.id)
                .await
                .unwrap()
                .bumped_at,
            before,
            "new post sees the committed policy and elapsed cutoff"
        );
    }
}

#[tokio::test]
async fn source_age_policy_uses_request_start_and_op_seconds_without_changing_admission_or_flags() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut random = [0_u8; 5];
    OsRng.fill_bytes(&mut random);
    let slug: String = random.iter().map(|b| format!("{b:02x}")).collect();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,json_tail_size) VALUES($1,'Age rules','Owned synthetic fixture',1000,1000,1000,10,10,1)").bind(&slug).execute(&owner).await.unwrap();
    let id = board_store::create_post(&public, &slug, 0, &post(false))
        .await
        .unwrap();
    let fixture = Fixture {
        owner: owner.clone(),
        public: public.clone(),
        slug: slug.clone(),
        id,
    };
    let result = tokio::spawn(async move { fixture.exercise().await }).await;
    public.close().await;
    sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)").bind(&slug).execute(&owner).await.unwrap();
    for table in ["content.posts", "content.threads", "content.boards"] {
        let query = match table {
            "content.boards" => "DELETE FROM content.boards WHERE slug=$1",
            "content.threads" => "DELETE FROM content.threads WHERE board=$1",
            _ => "DELETE FROM content.posts WHERE board=$1",
        };
        sqlx::query(query)
            .bind(&slug)
            .execute(&owner)
            .await
            .unwrap();
    }
    owner.close().await;
    result.unwrap();
}
