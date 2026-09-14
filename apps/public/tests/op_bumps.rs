#![cfg(feature = "database-tests")]
use axum::{body::Body, extract::ConnectInfo, http::Request};
use board_store::{NewPost, PostingContext, StoreError};
use chrono::{DateTime, Utc};
use http_body_util::BodyExt;
use rand_core::{OsRng, RngCore};
use sqlx::PgPool;
use std::{net::IpAddr, time::Duration};
use tower::ServiceExt;

fn post(sage: bool) -> NewPost {
    NewPost {
        name: "Anonymous".into(),
        subject: String::new(),
        comment: "Owned OP bump fixture".into(),
        deletion_hash: "unused-fixture-hash".into(),
        sage,
    }
}
fn context(seconds: i64, peer: &str) -> PostingContext {
    PostingContext {
        request_start: DateTime::from_timestamp(seconds, 0).unwrap(),
        peer: Some(peer.parse().unwrap()),
    }
}
struct Fixture {
    owner: PgPool,
    public: PgPool,
    slug: String,
}
impl Fixture {
    async fn create(&self, parent: i64, seconds: i64, peer: &str, sage: bool) -> i64 {
        board_store::create_post_with_context(
            &self.public,
            &self.slug,
            parent,
            &post(sage),
            None,
            context(seconds, peer),
        )
        .await
        .unwrap()
    }
    async fn clock(&self, id: i64, seconds: i64) {
        sqlx::query("UPDATE content.posts SET created_at=to_timestamp($2)+interval '900 milliseconds' WHERE id=$1")
            .bind(id).bind(seconds as f64).execute(&self.owner).await.unwrap();
    }
    async fn append(&self, id: i64, seconds: i64, peer: &str, sage: bool, bump: bool) -> i64 {
        sqlx::query("UPDATE content.threads SET bumped_at='2001-01-01T00:00:00Z' WHERE id=$1")
            .bind(id)
            .execute(&self.owner)
            .await
            .unwrap();
        let before = board_store::thread(&self.public, &self.slug, id)
            .await
            .unwrap();
        let reply = self.create(id, seconds, peer, sage).await;
        let after = board_store::thread(&self.public, &self.slug, id)
            .await
            .unwrap();
        assert_eq!(after.bumped_at > before.bumped_at, bump);
        assert_eq!(after.reply_count, before.reply_count + 1);
        reply
    }
    async fn private_count(&self, id: i64) -> (i64, i64) {
        sqlx::query_as("SELECT (SELECT count(*) FROM post_secrets.op_peers WHERE thread_id=$1),(SELECT count(*) FROM post_secrets.op_replies WHERE thread_id=$1)")
            .bind(id).fetch_one(&self.owner).await.unwrap()
    }
    async fn exercise(&self) {
        let op_peer = "192.0.2.10";
        let other = "192.0.2.11";
        let start = 1_700_000_000;
        let id = self.create(0, start, op_peer, false).await;
        self.clock(id, start).await;
        assert_eq!(self.private_count(id).await, (1, 0));
        self.append(id, start + 899, other, false, true).await;
        assert_eq!(
            self.private_count(id).await,
            (1, 0),
            "other reply addresses are not retained"
        );
        let first = self.append(id, start + 899, op_peer, false, false).await;
        self.clock(first, start - 1000).await;
        let second = self
            .append(id, start + 900, "::ffff:192.0.2.10", false, true)
            .await;
        self.clock(second, start + 900).await;
        let third = self.append(id, start + 1199, op_peer, false, false).await;
        self.clock(third, start + 900).await;
        let fourth = self.append(id, start + 1200, op_peer, true, false).await;
        self.clock(fourth, start + 1200).await;
        let fifth = self.append(id, start + 1499, op_peer, false, false).await;
        self.clock(fifth, start + 1500).await;
        assert_eq!(self.private_count(id).await, (1, 5));
        board_store::delete_post(&self.public, &self.slug, fifth)
            .await
            .unwrap();
        assert_eq!(self.private_count(id).await, (1, 4));
        // Latest surviving post number wins, not maximum timestamp.
        self.clock(first, start + 100_000).await;
        let sixth = self.append(id, start + 1500, op_peer, false, true).await;
        self.clock(sixth, start + 1500).await;
        for sticky in [false, true] {
            for permasage in [false, true] {
                for permaage in [false, true] {
                    sqlx::query(
                        "UPDATE content.threads SET sticky=$2,permasage=$3,permaage=$4 WHERE id=$1",
                    )
                    .bind(id)
                    .bind(sticky)
                    .bind(permasage)
                    .bind(permaage)
                    .execute(&self.owner)
                    .await
                    .unwrap();
                    let reply = self
                        .append(
                            id,
                            start + 1501,
                            op_peer,
                            false,
                            !sticky && !permasage && permaage,
                        )
                        .await;
                    self.clock(reply, start + 1501).await;
                }
            }
        }
        sqlx::query(
            "UPDATE content.threads SET sticky=false,permasage=false,permaage=false WHERE id=$1",
        )
        .bind(id)
        .execute(&self.owner)
        .await
        .unwrap();
        sqlx::query("UPDATE content.boards SET op_bump_limit=false WHERE slug=$1")
            .bind(&self.slug)
            .execute(&self.owner)
            .await
            .unwrap();
        self.append(id, start + 1502, op_peer, false, true).await;
        sqlx::query("UPDATE content.boards SET op_bump_limit=true,op_bump_initial_seconds=0,op_bump_repeat_seconds=0 WHERE slug=$1").bind(&self.slug).execute(&self.owner).await.unwrap();
        self.append(id, Utc::now().timestamp() + 1, op_peer, false, true)
            .await;
        // Failure after provisional bump/count update cannot add private membership.
        let before = board_store::thread(&self.public, &self.slug, id)
            .await
            .unwrap();
        let private_before = self.private_count(id).await;
        let mut invalid = post(false);
        invalid.deletion_hash = "x".repeat(257);
        assert!(matches!(
            board_store::create_post_with_context(
                &self.public,
                &self.slug,
                id,
                &invalid,
                None,
                context(Utc::now().timestamp() + 1, op_peer)
            )
            .await,
            Err(StoreError::Database(_))
        ));
        assert_eq!(self.private_count(id).await, private_before);
        let after = board_store::thread(&self.public, &self.slug, id)
            .await
            .unwrap();
        assert_eq!(
            (after.bumped_at, after.modified_at, after.reply_count),
            (before.bumped_at, before.modified_at, before.reply_count)
        );
        board_store::delete_post(&self.public, &self.slug, id)
            .await
            .unwrap();
        assert_eq!(self.private_count(id).await, (0, 0));
        self.http().await;
        self.concurrent().await;
    }
    async fn http(&self) {
        sqlx::query("UPDATE content.boards SET op_bump_initial_seconds=900,op_bump_repeat_seconds=300 WHERE slug=$1").bind(&self.slug).execute(&self.owner).await.unwrap();
        let app = board_public::router(self.public.clone(), "http://127.0.0.1:3000".into(), false);
        let send = |parent, peer: IpAddr| {
            let app = app.clone();
            let slug = self.slug.clone();
            async move {
                let response = app
                    .oneshot(
                        Request::post(format!("/{slug}/imgboard.php"))
                            .extension(ConnectInfo(std::net::SocketAddr::new(peer, 40000)))
                            .header("origin", "http://127.0.0.1:3000")
                            .header("content-type", "application/x-www-form-urlencoded")
                            .header("accept", "application/json")
                            .header("x-forwarded-for", "203.0.113.99")
                            .header("forwarded", "for=203.0.113.99")
                            .body(Body::from(format!(
                                "resto={parent}&com=Owned+HTTP+fixture&pwd=owned-secret"
                            )))
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), 200);
                let result: serde_json::Value = serde_json::from_slice(
                    &response.into_body().collect().await.unwrap().to_bytes(),
                )
                .unwrap();
                result["pid"].as_i64().unwrap_or_else(|| panic!("{result}"))
            }
        };
        let ip: IpAddr = "2001:db8::1".parse().unwrap();
        let id = send(0, ip).await;
        let before = board_store::thread(&self.public, &self.slug, id)
            .await
            .unwrap();
        send(id, ip).await;
        assert_eq!(
            board_store::thread(&self.public, &self.slug, id)
                .await
                .unwrap()
                .bumped_at,
            before.bumped_at
        );
        send(id, "2001:db8::2".parse().unwrap()).await;
        assert!(
            board_store::thread(&self.public, &self.slug, id)
                .await
                .unwrap()
                .bumped_at
                > before.bumped_at
        );
        assert_eq!(self.private_count(id).await, (1, 1));
        let saved: String =
            sqlx::query_scalar("SELECT host(peer) FROM post_secrets.op_peers WHERE thread_id=$1")
                .bind(id)
                .fetch_one(&self.owner)
                .await
                .unwrap();
        assert_eq!(saved, "2001:db8::1");
        for path in [
            format!("thread/{id}.json"),
            "1.json".into(),
            "catalog.json".into(),
            format!("thread/{id}"),
            "catalog".into(),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::get(format!("/{}/{path}", self.slug))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            let body = String::from_utf8(
                response
                    .into_body()
                    .collect()
                    .await
                    .unwrap()
                    .to_bytes()
                    .to_vec(),
            )
            .unwrap();
            for private in [
                "2001:db8",
                "203.0.113.99",
                "op_peers",
                "op_bump",
                "own_reply",
            ] {
                assert!(!body.contains(private));
            }
        }
        // Actual staff deletion runs privileged cleanup without private read access.
        let staff = PgPool::connect(&std::env::var("STAFF_DATABASE_URL").unwrap())
            .await
            .unwrap();
        assert_eq!(
            sqlx::query("SELECT * FROM post_secrets.op_peers")
                .execute(&staff)
                .await
                .unwrap_err()
                .as_database_error()
                .unwrap()
                .code()
                .as_deref(),
            Some("42501")
        );
        let mut tx = staff.begin().await.unwrap();
        sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
            .bind(&self.slug)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("UPDATE content.threads SET deleted=true WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.rollback().await.unwrap();
        assert_eq!(self.private_count(id).await, (1, 1));
        sqlx::query("UPDATE content.threads SET deleted=true WHERE id=$1")
            .bind(id)
            .execute(&staff)
            .await
            .unwrap();
        assert_eq!(self.private_count(id).await, (0, 0));
        staff.close().await;
    }
    async fn concurrent(&self) {
        let now = Utc::now().timestamp();
        let id = self.create(0, now, "192.0.2.30", false).await;
        self.clock(id, now - 1000).await;
        // Record actual bump transitions. Posting timestamps precede insertion
        // and cannot serve as a witness for which transaction changed the root.
        // Only this generated hexadecimal suffix and a typed integer enter DDL.
        assert_eq!(self.slug.len(), 10);
        assert!(self.slug.bytes().all(|byte| byte.is_ascii_hexdigit()));
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE TABLE post_secrets.op_audit_{}(bumped_at timestamptz NOT NULL); CREATE FUNCTION post_secrets.op_audit_{}() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$ BEGIN INSERT INTO post_secrets.op_audit_{} VALUES(NEW.bumped_at); RETURN NEW; END $$; REVOKE ALL ON FUNCTION post_secrets.op_audit_{}() FROM PUBLIC; CREATE TRIGGER op_audit_{} AFTER UPDATE OF bumped_at ON content.threads FOR EACH ROW WHEN (NEW.id={} AND NEW.bumped_at IS DISTINCT FROM OLD.bumped_at) EXECUTE FUNCTION post_secrets.op_audit_{}();", self.slug,self.slug,self.slug,self.slug,self.slug,id,self.slug))).execute(&self.owner).await.unwrap();
        sqlx::query("UPDATE content.threads SET bumped_at='2001-01-01T00:00:00Z' WHERE id=$1")
            .bind(id)
            .execute(&self.owner)
            .await
            .unwrap();
        let mut held = self.owner.begin().await.unwrap();
        // Exclude the test's initial clock reset from production transitions.
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DELETE FROM post_secrets.op_audit_{}",
            self.slug
        )))
        .execute(&mut *held)
        .await
        .unwrap();
        let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *held)
            .await
            .unwrap();
        sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
            .bind(&self.slug)
            .execute(&mut *held)
            .await
            .unwrap();
        let mut jobs = tokio::task::JoinSet::new();
        for _ in 0..8 {
            let pool = self.public.clone();
            let slug = self.slug.clone();
            jobs.spawn(async move {
                board_store::create_post_with_context(
                    &pool,
                    &slug,
                    id,
                    &post(false),
                    None,
                    context(now, "192.0.2.30"),
                )
                .await
                .unwrap()
            });
        }
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let count: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_locks WHERE NOT granted AND $1=ANY(pg_blocking_pids(pid))").bind(pid).fetch_one(&self.owner).await.unwrap();
                if count > 0 { break; }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.expect("healthy public writers reach the held board lock");
        held.commit().await.unwrap();
        while let Some(result) = jobs.join_next().await {
            result.unwrap();
        }
        let thread = board_store::thread(&self.public, &self.slug, id)
            .await
            .unwrap();
        let changes: Vec<DateTime<Utc>> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT bumped_at FROM post_secrets.op_audit_{}",
            self.slug
        )))
        .fetch_all(&self.owner)
        .await
        .unwrap();
        assert!(thread.bumped_at.timestamp() >= now);
        assert_eq!(
            changes,
            vec![thread.bumped_at],
            "only the first serialized reply can bump"
        );
        assert_eq!(thread.reply_count, 8);
        assert_eq!(self.private_count(id).await, (1, 8));
        sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1").bind(id).execute(&self.owner).await.unwrap();
        assert_eq!(self.private_count(id).await, (0, 0));
    }
}

#[tokio::test]
async fn source_op_peer_intervals_are_private_atomic_and_deletion_aware() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut random = [0; 5];
    OsRng.fill_bytes(&mut random);
    let slug: String = random.iter().map(|b| format!("{b:02x}")).collect();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'OP rules','Owned synthetic fixture',1000,1000,1000,10,10)").bind(&slug).execute(&owner).await.unwrap();
    let f = Fixture {
        owner: owner.clone(),
        public: public.clone(),
        slug: slug.clone(),
    };
    let result = tokio::spawn(async move { f.exercise().await }).await;
    public.close().await;
    assert_eq!(slug.len(), 10);
    assert!(slug.bytes().all(|byte| byte.is_ascii_hexdigit()));
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP TRIGGER IF EXISTS op_audit_{slug} ON content.threads; DROP FUNCTION IF EXISTS post_secrets.op_audit_{slug}(); DROP TABLE IF EXISTS post_secrets.op_audit_{slug};"))).execute(&owner).await.unwrap();
    sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)").bind(&slug).execute(&owner).await.unwrap();
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
