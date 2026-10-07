#![cfg(feature = "database-tests")]
mod support;

use board_domain::anonymous_session::{Capability, Fingerprints, State};
use board_store::{
    AnonymousPostingContext, NewPost, PostIdentityKeys, PostMetadata, PostingContext, StoreError,
    anonymous_session::{PostingSession, post_proof, snapshot},
};
use chrono::{Timelike, Utc};
use sqlx::PgPool;
use std::net::IpAddr;
use std::time::{Duration, Instant};

struct Fixture {
    owner: PgPool,
    public: PgPool,
    board: String,
}

impl Fixture {
    async fn new() -> Self {
        let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let public =
            board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
                .await
                .unwrap();
        let board: String =
            sqlx::query_scalar("SELECT 'an'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
                .fetch_one(&owner)
                .await
                .unwrap();
        sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,op_markup,deletion_known_min_seconds,deletion_unknown_min_seconds,posting_reply_seconds,posting_image_seconds,posting_thread_seconds) VALUES($1,'Owned anonymous session','Synthetic',2000,100,100,100,10,true,0,0,0,0,0)")
            .bind(&board).execute(&owner).await.unwrap();
        Self {
            owner,
            public,
            board,
        }
    }

    async fn create(&self, parent: i64, session: PostingSession) -> Result<i64, StoreError> {
        self.create_attached(parent, session, None).await
    }

    async fn create_attached(
        &self,
        parent: i64,
        session: PostingSession,
        attachment: Option<&board_store::post_media::NewAttachment>,
    ) -> Result<i64, StoreError> {
        self.create_from(
            parent,
            session,
            attachment,
            Some("198.51.100.7".parse().unwrap()),
        )
        .await
    }

    async fn create_from(
        &self,
        parent: i64,
        session: PostingSession,
        attachment: Option<&board_store::post_media::NewAttachment>,
        peer: Option<IpAddr>,
    ) -> Result<i64, StoreError> {
        support::create_post_with_anonymous_session(
            &self.public,
            &self.board,
            parent,
            &NewPost {
                name: "Anonymous".into(),
                subject: "Owned anonymous session".into(),
                comment: "Owned anonymous session content".into(),
                deletion_hash: "owned-private-deletion-hash".into(),
                sage: false,
            },
            attachment,
            AnonymousPostingContext {
                posting: PostingContext {
                    request_start: Utc::now(),
                    peer,
                    op_password_proof: None,
                },
                session,
            },
            PostMetadata {
                spoiler: false,
                keys: PostIdentityKeys {
                    tripcode: None,
                    poster_id: None,
                },
                country_database: None,
                flag: "",
                options: "",
            },
        )
        .await
    }

    async fn counts(&self) -> (i64, i64, i64, i64) {
        sqlx::query_as("SELECT (SELECT count(*) FROM content.posts WHERE board=$1),(SELECT count(*) FROM content.threads WHERE board=$1),(SELECT count(*) FROM content.reports WHERE board=$1),(SELECT count(*) FROM post_secrets.anonymous_posts a JOIN content.posts p ON p.id=a.post_id WHERE p.board=$1)")
            .bind(&self.board).fetch_one(&self.owner).await.unwrap()
    }

    async fn cleanup(&self, tokens: &[[u8; 32]]) {
        let mut tx = support::begin_cleanup(&self.owner, std::slice::from_ref(&self.board)).await;
        for query in [
            "DELETE FROM content.reports WHERE board=$1",
            "DELETE FROM content.post_media WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
            "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
            "DELETE FROM content.posts WHERE board=$1",
            "DELETE FROM content.threads WHERE board=$1",
            "DELETE FROM content.boards WHERE slug=$1",
        ] {
            sqlx::query(query)
                .bind(&self.board)
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        for token in tokens {
            sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
                .bind(token.as_slice())
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        tx.commit().await.unwrap();
    }
}

fn session(capability: &Capability, minted: bool) -> PostingSession {
    PostingSession {
        fingerprints: capability.fingerprints(Some("198.51.100.7".parse().unwrap()), *b"US"),
        minted,
        now: Utc::now().with_nanosecond(0).unwrap(),
    }
}

#[tokio::test]
async fn attachment_consumption_and_image_activity_commit_only_with_anonymous_membership() {
    let fixture = Fixture::new().await;
    sqlx::query("UPDATE content.boards SET image_limit=100 WHERE slug=$1")
        .bind(&fixture.board)
        .execute(&fixture.owner)
        .await
        .unwrap();
    let capability = Capability::generate().unwrap();
    let token = capability.storage_hash();
    let intake = board_store::media_intake::IntakeStore::connect(
        &std::env::var("INTAKE_DATABASE_URL").unwrap(),
    )
    .await
    .unwrap();
    let queue =
        board_store::media::MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
            .await
            .unwrap();
    let upload = intake.reserve("owned-anonymous.png").await.unwrap();
    let job = upload.id.clone();
    let f = Fixture {
        owner: fixture.owner.clone(),
        public: fixture.public.clone(),
        board: fixture.board.clone(),
    };
    let outcome = tokio::spawn(async move {
        intake
            .begin_upload(&upload.id, &upload.capability)
            .await
            .unwrap();
        intake
            .finish_upload(&upload.id, &upload.capability, 100)
            .await
            .unwrap();
        let claim = queue.claim().await.unwrap().unwrap();
        assert_eq!(claim.id, upload.id, "Requires an idle owned media queue");
        let lease = claim.lease_token.unwrap();
        let output = queue
            .prepare_output(
                &claim.id,
                &lease,
                &board_store::media_assets::OutputMetadata {
                    sha256: "b".repeat(64),
                    bytes: 123,
                    width: 10,
                    height: 20,
                },
            )
            .await
            .unwrap();
        queue
            .approve_output(&claim.id, &lease, &output.id)
            .await
            .unwrap();
        let attachment = board_store::post_media::NewAttachment {
            upload,
            spoiler: false,
        };
        let before = f.counts().await;
        let rejected = f
            .create_attached(0, session(&capability, false), Some(&attachment))
            .await;
        assert!(
            matches!(rejected, Err(StoreError::AuthorizationChanged)),
            "{rejected:?}"
        );
        assert_eq!(f.counts().await, before);
        assert!(snapshot(&f.public, &token).await.unwrap().is_none());
        board_store::post_media::check_upload(
            &f.public,
            &attachment.upload.id,
            &attachment.upload.capability,
        )
        .await
        .unwrap();
        let post = f
            .create_attached(0, session(&capability, true), Some(&attachment))
            .await
            .unwrap();
        let state = saved(&f.public, session(&capability, false).fingerprints).await;
        assert_eq!(
            (
                state.pending,
                state.post_count(),
                state.thread_count(),
                state.image_count()
            ),
            (7, 1, 1, 1)
        );
        assert_eq!(
            board_store::post_media::attachment(&f.public, post)
                .await
                .unwrap()
                .unwrap()
                .asset_id,
            output.id
        );
        let before_reuse = f.counts().await;
        assert!(matches!(
            f.create_attached(post, session(&capability, false), Some(&attachment))
                .await,
            Err(StoreError::Conflict(_))
        ));
        assert_eq!(f.counts().await, before_reuse);
        assert_eq!(
            saved(&f.public, session(&capability, false).fingerprints).await,
            state
        );
        let proof = post_proof(&f.public, &token, &f.board, post)
            .await
            .unwrap()
            .unwrap();
        board_store::delete_with_anonymous_proof(
            &f.public,
            &mut deletion_batch(
                &f.board,
                board_store::PublicDeletionContext {
                    request_start: Utc::now(),
                    session: None,
                },
            ),
            post,
            token,
            proof,
            true,
        )
        .await
        .unwrap();
        assert!(
            board_store::find_post(&f.public, &f.board, post)
                .await
                .is_ok()
        );
        assert!(
            board_store::post_media::attachment(&f.public, post)
                .await
                .unwrap()
                .unwrap()
                .file_deleted
        );
        assert_eq!(
            saved(&f.public, session(&capability, false).fingerprints)
                .await
                .image_count(),
            1
        );
        assert!(
            post_proof(&f.public, &token, &f.board, post)
                .await
                .unwrap()
                .is_some()
        );
        board_store::delete_with_anonymous_proof(
            &f.public,
            &mut deletion_batch(
                &f.board,
                board_store::PublicDeletionContext {
                    request_start: Utc::now(),
                    session: None,
                },
            ),
            post,
            token,
            proof,
            false,
        )
        .await
        .unwrap();
        assert!(
            board_store::find_post(&f.public, &f.board, post)
                .await
                .is_err()
        );
        assert!(
            post_proof(&f.public, &token, &f.board, post)
                .await
                .unwrap()
                .is_none()
        );
    })
    .await;
    fixture.cleanup(&[token]).await;
    for query in [
        "DELETE FROM media.assets WHERE job_id=$1",
        "DELETE FROM media.jobs WHERE id=$1",
    ] {
        sqlx::query(query)
            .bind(&job)
            .execute(&fixture.owner)
            .await
            .unwrap();
    }
    outcome.unwrap();
}

async fn saved(pool: &PgPool, fingerprints: Fingerprints) -> State {
    snapshot(pool, &fingerprints.token)
        .await
        .unwrap()
        .unwrap()
        .for_request(Utc::now().timestamp() as u64, &fingerprints)
}

#[tokio::test]
async fn successful_posts_and_reports_derive_private_activity_from_committed_rows() {
    let fixture = Fixture::new().await;
    let capability = Capability::generate().unwrap();
    let reporter = Capability::generate().unwrap();
    let tokens = [capability.storage_hash(), reporter.storage_hash()];
    let f = Fixture {
        owner: fixture.owner.clone(),
        public: fixture.public.clone(),
        board: fixture.board.clone(),
    };
    let outcome = tokio::spawn(async move {
        let first = session(&capability, true);
        let op = f.create(0, first).await.unwrap();
        let initial = saved(&f.public, first.fingerprints).await;
        assert_eq!((initial.posts, initial.threads, initial.reports, initial.pending), (0, 0, 0, 5));
        assert_eq!((initial.post_count(), initial.thread_count(), initial.image_count()), (1, 1, 0));
        assert!(!initial.is_known(first.now.timestamp() as u64, 1440, 0));
        assert!(post_proof(&f.public, &tokens[0], &f.board, op).await.unwrap().is_some());
        assert!(post_proof(&f.public, &tokens[1], &f.board, op).await.unwrap().is_none());
        let report_key = support::key(&f.board);
        let first_report_identity = report_key.public_report_rate_identity("198.51.100.7".parse().unwrap());
        board_store::report_with_anonymous_session(&f.public, &f.board, op, "Owned report", &first_report_identity, session(&capability, false)).await.unwrap();
        assert_eq!(saved(&f.public, first.fingerprints).await.pending, 13);
        let next = session(&capability, false);
        let now = next.now.timestamp();
        sqlx::query("UPDATE post_secrets.anonymous_sessions SET created_at=$2-86400,network_at=$2-86400,address_at=$2-86400,environment_at=$2-86400,activity_at=$2-1,action_at=$2-14400 WHERE token_hash=$1")
            .bind(tokens[0].as_slice()).bind(now).execute(&f.owner).await.unwrap();
        f.create(op, next).await.unwrap();
        let flushed = saved(&f.public, first.fingerprints).await;
        assert_eq!((flushed.posts, flushed.images, flushed.threads, flushed.reports, flushed.pending), (1, 0, 1, 1, 0));
        assert!(flushed.is_known(now as u64, 1440, 0));
        // A second independent reporter also has a distinct trusted peer;
        // changing only the anonymous capability does not reset admission.
        let reporter_peer = "198.51.100.8".parse().unwrap();
        let second_report_identity = report_key.public_report_rate_identity(reporter_peer);
        let mut reporter_session = session(&reporter, true);
        reporter_session.fingerprints = reporter.fingerprints(Some(reporter_peer), *b"US");
        board_store::report_with_anonymous_session(&f.public, &f.board, op, "Another owned reporter", &second_report_identity, reporter_session).await.unwrap();
        let report_only = saved(&f.public, reporter_session.fingerprints).await;
        assert_eq!((report_only.posts, report_only.pending, report_only.report_count()), (0, 8, 1));
        assert!(post_proof(&f.public, &tokens[1], &f.board, op).await.unwrap().is_none());
        assert_eq!(f.counts().await, (2, 1, 2, 2));
        let mut changed = session(&capability, false);
        changed.fingerprints = capability.fingerprints(Some("203.0.113.9".parse::<IpAddr>().unwrap()), *b"JP");
        f.create(op, changed).await.unwrap();
        let churn = saved(&f.public, changed.fingerprints).await;
        assert_eq!(churn.change_score, 3);
        assert_eq!((churn.network_at, churn.address_at, churn.environment_at), (changed.now.timestamp() as u64, changed.now.timestamp() as u64, changed.now.timestamp() as u64));
    }).await;
    fixture.cleanup(&tokens).await;
    outcome.unwrap();
}

#[tokio::test]
async fn missing_revoked_or_expired_authority_rolls_back_content_and_reports() {
    let fixture = Fixture::new().await;
    let capability = Capability::generate().unwrap();
    let missing = Capability::generate().unwrap();
    let tokens = [capability.storage_hash(), missing.storage_hash()];
    let f = Fixture {
        owner: fixture.owner.clone(),
        public: fixture.public.clone(),
        board: fixture.board.clone(),
    };
    let outcome = tokio::spawn(async move {
        let op = f.create(0, session(&capability, true)).await.unwrap();
        let before = f.counts().await;
        let report_identity =
            support::key(&f.board).public_report_rate_identity("198.51.100.7".parse().unwrap());
        for context in [session(&missing, false), session(&capability, true)] {
            assert!(matches!(
                f.create(op, context).await,
                Err(StoreError::AuthorizationChanged)
            ));
            assert!(matches!(
                board_store::report_with_anonymous_session(
                    &f.public,
                    &f.board,
                    op,
                    "Uncommitted report",
                    &report_identity,
                    context
                )
                .await,
                Err(StoreError::AuthorizationChanged)
            ));
            assert_eq!(f.counts().await, before);
        }
        assert!(snapshot(&f.public, &tokens[1]).await.unwrap().is_none());
        sqlx::query("UPDATE post_secrets.anonymous_sessions SET expires_at=1 WHERE token_hash=$1")
            .bind(tokens[0].as_slice())
            .execute(&f.owner)
            .await
            .unwrap();
        assert!(snapshot(&f.public, &tokens[0]).await.unwrap().is_none());
        assert!(
            post_proof(&f.public, &tokens[0], &f.board, op)
                .await
                .unwrap()
                .is_none()
        );
        assert!(matches!(
            f.create(op, session(&capability, false)).await,
            Err(StoreError::AuthorizationChanged)
        ));
        let after = f.counts().await;
        assert_eq!((after.0, after.1, after.2), (before.0, before.1, before.2));
        // Another mint may collect this deliberately expired session. Its
        // deletion bindings may disappear, while its content must survive.
        assert!(after.3 <= before.3);
    })
    .await;
    fixture.cleanup(&tokens).await;
    outcome.unwrap();
}

#[tokio::test]
async fn idle_activity_resets_without_losing_deletion_ownership_and_rotation_revokes_it() {
    let fixture = Fixture::new().await;
    let capability = Capability::generate().unwrap();
    let token = capability.storage_hash();
    let f = Fixture {
        owner: fixture.owner.clone(),
        public: fixture.public.clone(),
        board: fixture.board.clone(),
    };
    let outcome = tokio::spawn(async move {
        let op = f.create(0, session(&capability, true)).await.unwrap();
        let next = session(&capability, false);
        // The activity policy uses the application clock, which can differ
        // from the database clock within the accepted connection tolerance.
        sqlx::query("UPDATE post_secrets.anonymous_sessions SET created_at=$2,network_at=$2,address_at=$2,environment_at=$2,activity_at=$2,verified_level=1,posts=19,pending=15,change_score=12 WHERE token_hash=$1")
            .bind(token.as_slice()).bind(next.now.timestamp()-604801).execute(&f.owner).await.unwrap();
        let reset = saved(&f.public, next.fingerprints).await;
        assert_eq!((reset.posts, reset.pending, reset.verified_level, reset.change_score), (0, 0, 0, 0));
        assert!(post_proof(&f.public, &token, &f.board, op).await.unwrap().is_some());
        let reply = f.create(op, next).await.unwrap();
        let state = saved(&f.public, next.fingerprints).await;
        assert_eq!((state.posts, state.pending, state.verified_level, state.action_at), (0, 1, 0, next.now.timestamp() as u64));
        let proof = post_proof(&f.public, &token, &f.board, reply).await.unwrap().unwrap();
        sqlx::query("UPDATE post_secrets.deletion SET password_hash='owned-rotated-hash' WHERE post_id=$1")
            .bind(reply).execute(&f.owner).await.unwrap();
        assert!(post_proof(&f.public, &token, &f.board, reply).await.unwrap().is_none());
        assert!(matches!(board_store::delete_with_anonymous_proof(&f.public, &mut deletion_batch(&f.board, board_store::PublicDeletionContext { request_start: Utc::now(), session: None }), reply, token, proof, false).await, Err(StoreError::AuthorizationChanged)));
        assert!(!board_store::find_post(&f.public, &f.board, reply).await.unwrap().deleted);
        let op_proof = post_proof(&f.public, &token, &f.board, op).await.unwrap().unwrap();
        board_store::delete_with_anonymous_proof(&f.public, &mut deletion_batch(&f.board, board_store::PublicDeletionContext { request_start: Utc::now(), session: None }), op, token, op_proof, false).await.unwrap();
        assert!(matches!(board_store::find_post(&f.public, &f.board, op).await, Err(StoreError::NotFound)));
    }).await;
    fixture.cleanup(&[token]).await;
    outcome.unwrap();
}

#[tokio::test]
async fn public_credentials_cannot_write_private_activity_or_assume_its_owner() {
    let fixture = Fixture::new().await;
    for query in [
        "SELECT * FROM post_secrets.anonymous_sessions",
        "SELECT * FROM post_secrets.anonymous_posts",
        "SELECT * FROM post_secrets.anonymous_reports",
        "UPDATE post_secrets.anonymous_sessions SET verified_level=1 WHERE false",
        "UPDATE post_secrets.anonymous_policy SET session_limit=1000000 WHERE false",
        "SELECT post_secrets.advance_anonymous_session(NULL,NULL,NULL,NULL,true,1::smallint,0)",
        "SET ROLE board_anonymous_owner",
        "CREATE TABLE post_secrets.owned_anonymous_forbidden(id integer)",
    ] {
        let error = sqlx::query(query)
            .execute(&fixture.public)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501"),
            "{query}"
        );
    }
    fixture.cleanup(&[]).await;
}

#[tokio::test]
async fn deletion_rechecks_membership_session_hash_and_expiry_after_the_board_lock_wait() {
    let fixture = Fixture::new().await;
    let mutation_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mutation_pool)
        .await
        .unwrap();
    let capabilities: Vec<Capability> = (0..4).map(|_| Capability::generate().unwrap()).collect();
    let tokens: Vec<[u8; 32]> = capabilities.iter().map(Capability::storage_hash).collect();
    let f = Fixture {
        owner: fixture.owner.clone(),
        public: fixture.public.clone(),
        board: fixture.board.clone(),
    };
    let outcome = tokio::spawn(async move {
        for (index, capability) in capabilities.iter().enumerate() {
            let token = capability.storage_hash();
            let post = f.create(0, session(capability, true)).await.unwrap();
            let proof = post_proof(&f.public, &token, &f.board, post).await.unwrap().unwrap();
            let before = f.counts().await;
            let mut change = f.owner.begin().await.unwrap();
            sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE").bind(&f.board).execute(&mut *change).await.unwrap();
            match index {
                0 => { sqlx::query("DELETE FROM post_secrets.anonymous_posts WHERE post_id=$1").bind(post).execute(&mut *change).await.unwrap(); }
                1 => { sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1").bind(token.as_slice()).execute(&mut *change).await.unwrap(); }
                2 => { sqlx::query("UPDATE post_secrets.deletion SET password_hash='owned-concurrent-rotation' WHERE post_id=$1").bind(post).execute(&mut *change).await.unwrap(); }
                3 => { sqlx::query("UPDATE post_secrets.anonymous_sessions SET expires_at=1 WHERE token_hash=$1").bind(token.as_slice()).execute(&mut *change).await.unwrap(); }
                _ => unreachable!(),
            }
            let (pool, board) = (mutation_pool.clone(), f.board.clone());
            let deletion = tokio::spawn(async move { board_store::delete_with_anonymous_proof(&pool, &mut deletion_batch(&board, board_store::PublicDeletionContext { request_start: Utc::now(), session: None }), post, token, proof, false).await });
            let deadline = Instant::now() + Duration::from_millis(1500);
            loop {
                let blocked: bool = sqlx::query_scalar("SELECT cardinality(pg_blocking_pids($1))>0").bind(pid).fetch_one(&f.owner).await.unwrap();
                if blocked { break; }
                assert!(Instant::now() < deadline, "Anonymous deletion did not reach the held board lock");
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            change.commit().await.unwrap();
            assert!(matches!(deletion.await.unwrap(), Err(StoreError::AuthorizationChanged)));
            assert!(board_store::find_post(&f.public, &f.board, post).await.is_ok());
            let after = f.counts().await;
            assert_eq!((after.0, after.1, after.2), (before.0, before.1, before.2));
            assert!(post_proof(&f.public, &token, &f.board, post).await.unwrap().is_none());
        }
    }).await;
    fixture.cleanup(&tokens).await;
    outcome.unwrap();
}

#[tokio::test]
async fn persisted_activity_matches_every_non_dummy_source_vector() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let reference: serde_json::Value =
        serde_json::from_str(include_str!("../../../fixtures/anonymous-reference.json")).unwrap();
    let names = [
        "creation_ts",
        "mask_ts",
        "ip_ts",
        "env_ts",
        "activity_ts",
        "action_ts",
        "verified_level",
        "post_count",
        "img_count",
        "thread_count",
        "report_count",
        "action_buffer",
        "ip_change_score",
    ];
    let capability = Capability::generate().unwrap();
    let fingerprints = session(&capability, false).fingerprints;
    let mut checked = 0;
    for case in reference["activity_cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| !case["dummy"].as_bool().unwrap())
    {
        let now = Utc::now().timestamp();
        let shift = now - case["now"].as_i64().unwrap();
        let values = |state: &serde_json::Value| -> Vec<i64> {
            names
                .iter()
                .enumerate()
                .map(|(index, name)| {
                    let value = state[name].as_i64().unwrap();
                    if index < 6 && value > 0 {
                        value + shift
                    } else {
                        value
                    }
                })
                .collect()
        };
        let before = values(&case["before"]);
        let expected = values(&case["after"]);
        let mut tx = owner.begin().await.unwrap();
        sqlx::query("INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,created_at,network_at,address_at,environment_at,activity_at,action_at,verified_level,posts,images,threads,reports,pending,change_score,expires_at) VALUES($1,$2,$3,$4,($5::bigint[])[1],($5)[2],($5)[3],($5)[4],($5)[5],($5)[6],($5)[7]::smallint,($5)[8]::smallint,($5)[9]::smallint,($5)[10]::smallint,($5)[11]::smallint,($5)[12]::smallint,($5)[13]::smallint,$6+31536000)")
            .bind(fingerprints.token.as_slice()).bind(fingerprints.network.as_slice())
            .bind(fingerprints.address.as_slice()).bind(fingerprints.environment.as_slice())
            .bind(before).bind(now).execute(&mut *tx).await.unwrap();
        sqlx::query("SET LOCAL ROLE board_anonymous_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("SELECT post_secrets.advance_anonymous_session($1,$2,$3,$4,false,$5,$6)")
            .bind(fingerprints.token.as_slice())
            .bind(fingerprints.network.as_slice())
            .bind(fingerprints.address.as_slice())
            .bind(fingerprints.environment.as_slice())
            .bind(case["kind"].as_i64().unwrap() as i16)
            .bind(now)
            .execute(&mut *tx)
            .await
            .unwrap();
        let actual: Vec<i64> = sqlx::query_scalar("SELECT ARRAY[created_at,network_at,address_at,environment_at,activity_at,action_at,verified_level::bigint,posts::bigint,images::bigint,threads::bigint,reports::bigint,pending::bigint,change_score::bigint] FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
            .bind(fingerprints.token.as_slice()).fetch_one(&mut *tx).await.unwrap();
        assert_eq!(actual, expected, "persisted source activity case {checked}");
        tx.rollback().await.unwrap();
        checked += 1;
    }
    assert_eq!(checked, 486);
}

#[tokio::test]
async fn anonymous_op_markup_rechecks_authority_after_the_board_lock_wait() {
    let fixture = Fixture::new().await;
    let capabilities: Vec<_> = (0..4).map(|_| Capability::generate().unwrap()).collect();
    let tokens: Vec<_> = capabilities.iter().map(Capability::storage_hash).collect();
    let public = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&public)
        .await
        .unwrap();
    let f = Fixture {
        owner: fixture.owner.clone(),
        public,
        board: fixture.board.clone(),
    };
    let outcome = tokio::spawn(async move {
        for (index, capability) in capabilities.iter().enumerate() {
            let op = f.create(0, session(capability, true)).await.unwrap();
            let returning = session(capability, false);
            // No matching address and no legacy password proof: only current
            // anonymous membership can grant the source OP formatting bit.
            let control = f.create_from(op, returning, None, None).await.unwrap();
            assert_ne!(board_store::find_post(&f.public, &f.board, control).await.unwrap().comment_format & 16, 0);
            let mut change = f.owner.begin().await.unwrap();
            sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
                .bind(&f.board).execute(&mut *change).await.unwrap();
            let token = capability.storage_hash();
            match index {
                0 => { sqlx::query("DELETE FROM post_secrets.anonymous_posts WHERE post_id=$1").bind(op).execute(&mut *change).await.unwrap(); }
                1 => { sqlx::query("UPDATE post_secrets.deletion SET password_hash='owned-op-rotation' WHERE post_id=$1").bind(op).execute(&mut *change).await.unwrap(); }
                2 => { sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1").bind(token.as_slice()).execute(&mut *change).await.unwrap(); }
                3 => { sqlx::query("UPDATE post_secrets.anonymous_sessions SET expires_at=1 WHERE token_hash=$1").bind(token.as_slice()).execute(&mut *change).await.unwrap(); }
                _ => unreachable!(),
            }
            let before = f.counts().await;
            let child = Fixture { owner: f.owner.clone(), public: f.public.clone(), board: f.board.clone() };
            let posting = tokio::spawn(async move { child.create_from(op, returning, None, None).await });
            let deadline = Instant::now() + Duration::from_millis(1500);
            loop {
                let blocked: bool = sqlx::query_scalar("SELECT cardinality(pg_blocking_pids($1))>0").bind(pid).fetch_one(&f.owner).await.unwrap();
                if blocked { break; }
                assert!(Instant::now() < deadline, "Reply did not reach the held board lock");
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            change.commit().await.unwrap();
            let result = posting.await.unwrap();
            if index < 2 {
                let reply = result.unwrap();
                assert_eq!(board_store::find_post(&f.public, &f.board, reply).await.unwrap().comment_format & 16, 0);
            } else {
                assert!(matches!(result, Err(StoreError::AuthorizationChanged)), "{result:?}");
                let after = f.counts().await;
                assert_eq!((after.0, after.1, after.2), (before.0, before.1, before.2));
            }
        }
    }).await;
    fixture.cleanup(&tokens).await;
    outcome.unwrap();
}

#[tokio::test]
async fn same_session_replies_on_different_boards_do_not_upgrade_shared_locks() {
    let first = Fixture::new().await;
    let second = Fixture::new().await;
    support::share_key(&second.board, &first.board);
    let capability = Capability::generate().unwrap();
    let token = capability.storage_hash();
    let f = Fixture {
        owner: first.owner.clone(),
        public: first.public.clone(),
        board: first.board.clone(),
    };
    let s = Fixture {
        owner: second.owner.clone(),
        public: second.public.clone(),
        board: second.board.clone(),
    };
    let outcome = tokio::spawn(async move {
        let first = f;
        let second = s;
        let op1 = first.create(0, session(&capability, true)).await.unwrap();
        // Independent parent setup; retain the same posting actor and session.
        support::cleanup_posting(&first.owner, &first.board).await;
        let op2 = second.create(0, session(&capability, false)).await.unwrap();
        let mut held = first.owner.begin().await.unwrap();
        sqlx::query("SELECT token_hash FROM post_secrets.anonymous_sessions WHERE token_hash=$1 FOR SHARE")
            .bind(token.as_slice()).execute(&mut *held).await.unwrap();
        let mut requests = Vec::new();
        let mut pids = Vec::new();
        for (board, parent) in [(&first.board, op1), (&second.board, op2)] {
            let public = sqlx::postgres::PgPoolOptions::new().max_connections(1)
                .connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap()).await.unwrap();
            pids.push(sqlx::query_scalar::<_, i32>("SELECT pg_backend_pid()").fetch_one(&public).await.unwrap());
            let f = Fixture { owner: first.owner.clone(), public, board: board.clone() };
            let returning = session(&capability, false);
            requests.push(tokio::spawn(async move { f.create_from(parent, returning, None, None).await }));
        }
        let deadline = Instant::now() + Duration::from_millis(1500);
        loop {
            let blocked: i64 = sqlx::query_scalar("SELECT count(*) FROM unnest($1::integer[]) p WHERE cardinality(pg_blocking_pids(p))>0")
                .bind(&pids).fetch_one(&first.owner).await.unwrap();
            if blocked == 2 { break; }
            assert!(Instant::now() < deadline, "Both owned replies must reach the held session lock");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        held.commit().await.unwrap();
        for (request, fixture) in requests.into_iter().zip([&first, &second]) {
            let reply = tokio::time::timeout(Duration::from_secs(5), request).await.unwrap().unwrap().unwrap();
            assert_ne!(board_store::find_post(&fixture.public, &fixture.board, reply).await.unwrap().comment_format & 16, 0);
        }
    }).await;
    second.cleanup(&[]).await;
    first.cleanup(&[token]).await;
    outcome.unwrap();
}

async fn deletion_state(f: &Fixture) -> String {
    sqlx::query_scalar("SELECT jsonb_build_object('posts',(SELECT jsonb_agg(to_jsonb(p) ORDER BY id) FROM content.posts p WHERE board=$1),'threads',(SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM content.threads t WHERE board=$1),'media',(SELECT jsonb_agg(to_jsonb(m) ORDER BY post_id) FROM content.post_media m JOIN content.posts p ON p.id=m.post_id WHERE p.board=$1))::text")
        .bind(&f.board).fetch_one(&f.owner).await.unwrap()
}

#[tokio::test]
async fn public_deletion_gates_preserve_state_and_apply_to_both_authorities_and_modes() {
    use sha2::{Digest, Sha256};
    let f = Fixture::new().await;
    sqlx::query("UPDATE content.boards SET deletion_known_min_seconds=60,deletion_unknown_min_seconds=600,archive_retention_seconds=86400 WHERE slug=$1")
        .bind(&f.board).execute(&f.owner).await.unwrap();
    let capability = Capability::generate().unwrap();
    let token = capability.storage_hash();
    let post = f.create(0, session(&capability, true)).await.unwrap();
    let proof = post_proof(&f.public, &token, &f.board, post)
        .await
        .unwrap()
        .unwrap();
    let password_proof = Sha256::digest(b"owned-private-deletion-hash").into();
    for case in ["young", "old", "no_op", "sticky", "staff_reply", "archived"] {
        sqlx::query("UPDATE content.posts SET created_at=clock_timestamp()-interval '601 seconds' WHERE id=$1")
            .bind(post).execute(&f.owner).await.unwrap();
        let mut child = None;
        let expected = match case {
            "young" => {
                sqlx::query("UPDATE content.posts SET created_at=clock_timestamp() WHERE id=$1")
                    .bind(post)
                    .execute(&f.owner)
                    .await
                    .unwrap();
                "Error: You must wait longer before deleting this post."
            }
            "old" => {
                sqlx::query("UPDATE content.posts SET created_at=clock_timestamp()-interval '1801 seconds' WHERE id=$1").bind(post).execute(&f.owner).await.unwrap();
                "Error: You cannot delete a post this old."
            }
            "no_op" => {
                sqlx::query("UPDATE content.boards SET deletion_no_op=true WHERE slug=$1")
                    .bind(&f.board)
                    .execute(&f.owner)
                    .await
                    .unwrap();
                "Error: You cannot delete this post."
            }
            "sticky" => {
                sqlx::query("UPDATE content.threads SET sticky=true WHERE id=$1")
                    .bind(post)
                    .execute(&f.owner)
                    .await
                    .unwrap();
                "Error: You cannot delete this post."
            }
            "staff_reply" => {
                let id = f.create(post, session(&capability, false)).await.unwrap();
                sqlx::query("UPDATE content.posts SET capcode='mod' WHERE id=$1")
                    .bind(id)
                    .execute(&f.owner)
                    .await
                    .unwrap();
                child = Some(id);
                "Error: You cannot delete this post."
            }
            "archived" => {
                sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1").bind(post).execute(&f.owner).await.unwrap();
                let retained_hash: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM post_secrets.deletion WHERE post_id=$1)",
                )
                .bind(post)
                .fetch_one(&f.owner)
                .await
                .unwrap();
                assert!(!retained_hash, "New archives retire deletion authority");
                "Error: Password incorrect."
            }
            _ => unreachable!(),
        };
        let before = deletion_state(&f).await;
        for file_only in [false, true] {
            for anonymous in [false, true] {
                let result = if anonymous {
                    board_store::delete_with_anonymous_proof(
                        &f.public,
                        &mut deletion_batch(
                            &f.board,
                            board_store::PublicDeletionContext {
                                request_start: Utc::now(),
                                session: None,
                            },
                        ),
                        post,
                        token,
                        proof,
                        file_only,
                    )
                    .await
                } else {
                    board_store::delete_with_password_proof(
                        &f.public,
                        &mut deletion_batch(
                            &f.board,
                            board_store::PublicDeletionContext {
                                request_start: Utc::now(),
                                session: None,
                            },
                        ),
                        post,
                        password_proof,
                        file_only,
                    )
                    .await
                };
                if case == "archived" {
                    // 0091 retires the hash, so both stale proofs fail before
                    // the post-authority archive gate. HTTP still reports the
                    // same password error.
                    assert!(
                        matches!(result, Err(StoreError::AuthorizationChanged)),
                        "{case}: {result:?}"
                    );
                } else {
                    assert!(
                        matches!(result,Err(StoreError::PublicDeletionRejected(message)) if message==expected),
                        "{case}: {result:?}"
                    );
                }
                assert_eq!(deletion_state(&f).await, before, "{case}");
            }
        }
        sqlx::query("UPDATE content.boards SET deletion_no_op=false WHERE slug=$1")
            .bind(&f.board)
            .execute(&f.owner)
            .await
            .unwrap();
        sqlx::query("UPDATE content.threads SET sticky=false,archived_at=NULL,archive_expires_at=NULL WHERE id=$1").bind(post).execute(&f.owner).await.unwrap();
        if let Some(child) = child {
            board_store::delete_post(&f.public, &f.board, child)
                .await
                .unwrap();
        }
    }
    let reply = f.create(post, session(&capability, false)).await.unwrap();
    let reply_proof = post_proof(&f.public, &token, &f.board, reply)
        .await
        .unwrap()
        .unwrap();
    sqlx::query("UPDATE content.boards SET deletion_no_reply=true WHERE slug=$1")
        .bind(&f.board)
        .execute(&f.owner)
        .await
        .unwrap();
    let before = deletion_state(&f).await;
    assert!(matches!(
        board_store::delete_with_anonymous_proof(
            &f.public,
            &mut deletion_batch(
                &f.board,
                board_store::PublicDeletionContext {
                    request_start: Utc::now(),
                    session: None
                }
            ),
            reply,
            token,
            reply_proof,
            false
        )
        .await,
        Err(StoreError::PublicDeletionRejected(
            "Error: You cannot delete this post."
        ))
    ));
    assert_eq!(deletion_state(&f).await, before);
    // Trusted staff/fixture authority is intentionally outside public gates.
    board_store::delete_post(&f.public, &f.board, post)
        .await
        .unwrap();
    assert!(matches!(
        board_store::find_post(&f.public, &f.board, reply).await,
        Err(StoreError::NotFound)
    ));
    f.cleanup(&[token]).await;
}

#[tokio::test]
async fn deletion_known_age_uses_resumed_network_lifetime_and_never_verified_level() {
    use board_store::PublicDeletionContext;
    use sha2::{Digest, Sha256};
    let f = Fixture::new().await;
    // Keep rejected deletions live across this age matrix without exhausting
    // the unrelated per-actor OP quota on this owned board.
    sqlx::query("UPDATE content.boards SET user_thread_limit=100,deletion_known_min_seconds=60,deletion_unknown_min_seconds=600 WHERE slug=$1")
        .bind(&f.board).execute(&f.owner).await.unwrap();
    let capability = Capability::generate().unwrap();
    let token = capability.storage_hash();
    let mut minted = true;
    for (network_age, idle_age, changed_network, changed_environment, expected) in [
        (899, 0, false, false, false),
        (900, 0, false, false, true),
        (901, 0, true, false, false),
        (901, 0, false, true, true),
        (901, 604801, false, false, false),
    ] {
        for anonymous in [false, true] {
            let post = f.create(0, session(&capability, minted)).await.unwrap();
            minted = false;
            let proof = post_proof(&f.public, &token, &f.board, post)
                .await
                .unwrap()
                .unwrap();
            let request_start = Utc::now().with_nanosecond(0).unwrap();
            sqlx::query("UPDATE content.posts SET created_at=$2-interval '61 seconds' WHERE id=$1")
                .bind(post)
                .bind(request_start)
                .execute(&f.owner)
                .await
                .unwrap();
            sqlx::query("UPDATE post_secrets.anonymous_sessions SET created_at=$2-1000,network_at=$2-$3,address_at=$2-1000,environment_at=$2-1000,activity_at=$2-$4,verified_level=3,change_score=12 WHERE token_hash=$1")
                .bind(token.as_slice()).bind(request_start.timestamp()).bind(network_age as i64).bind(idle_age as i64).execute(&f.owner).await.unwrap();
            let mut current = session(&capability, false);
            current.now = request_start;
            current.fingerprints = capability.fingerprints(
                Some(
                    if changed_network {
                        "203.0.113.7"
                    } else {
                        "198.51.100.7"
                    }
                    .parse()
                    .unwrap(),
                ),
                if changed_environment { *b"GB" } else { *b"US" },
            );
            let context = PublicDeletionContext {
                request_start,
                session: Some(current),
            };
            let before = deletion_state(&f).await;
            let result = if anonymous {
                board_store::delete_with_anonymous_proof_context(
                    &f.public,
                    &mut deletion_batch(&f.board, context),
                    post,
                    token,
                    proof,
                    false,
                )
                .await
            } else {
                board_store::delete_with_password_proof_context(
                    &f.public,
                    &mut deletion_batch(&f.board, context),
                    post,
                    Sha256::digest(b"owned-private-deletion-hash").into(),
                    false,
                )
                .await
            };
            assert_eq!(
                result.is_ok(),
                expected,
                "network={network_age},idle={idle_age},network_change={changed_network},environment_change={changed_environment}: {result:?}"
            );
            if !expected {
                assert_eq!(deletion_state(&f).await, before);
            }
        }
    }
    f.cleanup(&[token]).await;
}

#[tokio::test]
async fn deletion_rechecks_policy_after_waiting_for_the_board_lock() {
    let f = Fixture::new().await;
    let capability = Capability::generate().unwrap();
    let token = capability.storage_hash();
    let post = f.create(0, session(&capability, true)).await.unwrap();
    let proof = post_proof(&f.public, &token, &f.board, post)
        .await
        .unwrap()
        .unwrap();
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&pool)
        .await
        .unwrap();
    let mut change = f.owner.begin().await.unwrap();
    sqlx::query("UPDATE content.boards SET deletion_known_min_seconds=60,deletion_unknown_min_seconds=600 WHERE slug=$1").bind(&f.board).execute(&mut *change).await.unwrap();
    let before = deletion_state(&f).await;
    let board = f.board.clone();
    let deletion = tokio::spawn(async move {
        board_store::delete_with_anonymous_proof(
            &pool,
            &mut deletion_batch(
                &board,
                board_store::PublicDeletionContext {
                    request_start: Utc::now(),
                    session: None,
                },
            ),
            post,
            token,
            proof,
            false,
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if sqlx::query_scalar::<_, bool>("SELECT cardinality(pg_blocking_pids($1))>0")
                .bind(pid)
                .fetch_one(&f.owner)
                .await
                .unwrap()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("Deletion reaches the held board lock");
    change.commit().await.unwrap();
    assert!(matches!(
        deletion.await.unwrap(),
        Err(StoreError::PublicDeletionRejected(
            "Error: You must wait longer before deleting this post."
        ))
    ));
    assert_eq!(deletion_state(&f).await, before);
    f.cleanup(&[token]).await;
}

#[tokio::test]
async fn imported_deletion_policy_is_bounded_and_runtime_read_only() {
    let f = Fixture::new().await;
    let denied: Vec<String> =
        sqlx::query_scalar("SELECT slug FROM content.boards WHERE deletion_no_op ORDER BY slug")
            .fetch_all(&f.owner)
            .await
            .unwrap();
    for board in [
        "a", "bant", "his", "int", "jp", "pol", "pw", "qa", "qst", "sp", "tv", "v", "vip", "vm",
        "vmg", "vrpg", "vst", "vt",
    ] {
        assert!(
            denied.iter().any(|slug| slug == board),
            "Missing imported OP deletion policy for {board}"
        );
    }
    let qa: bool =
        sqlx::query_scalar("SELECT deletion_no_reply FROM content.boards WHERE slug='qa'")
            .fetch_one(&f.owner)
            .await
            .unwrap();
    assert!(!qa, "qa's reply gate is commented out in the source");
    for statement in [
        "UPDATE content.boards SET deletion_no_op=true WHERE slug=$1",
        "UPDATE content.boards SET deletion_no_reply=true WHERE slug=$1",
        "UPDATE content.boards SET deletion_known_min_seconds=0 WHERE slug=$1",
        "UPDATE content.boards SET deletion_unknown_min_seconds=0 WHERE slug=$1",
        "UPDATE content.boards SET deletion_max_seconds=86400 WHERE slug=$1",
    ] {
        let error = sqlx::query(statement)
            .bind(&f.board)
            .execute(&f.public)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501")
        );
    }
    for statement in [
        "UPDATE content.boards SET deletion_known_min_seconds=-1 WHERE slug=$1",
        "UPDATE content.boards SET deletion_unknown_min_seconds=1800 WHERE slug=$1",
        "UPDATE content.boards SET deletion_max_seconds=0 WHERE slug=$1",
        "UPDATE content.boards SET deletion_max_seconds=86401 WHERE slug=$1",
    ] {
        let error = sqlx::query(statement)
            .bind(&f.board)
            .execute(&f.owner)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
    }
    f.cleanup(&[]).await;
}

#[tokio::test]
async fn deletion_rechecks_network_lifetime_after_waiting_for_the_board_lock() {
    let f = Fixture::new().await;
    let capability = Capability::generate().unwrap();
    let token = capability.storage_hash();
    let post = f.create(0, session(&capability, true)).await.unwrap();
    let proof = post_proof(&f.public, &token, &f.board, post)
        .await
        .unwrap()
        .unwrap();
    let start = Utc::now().with_nanosecond(0).unwrap();
    sqlx::query("UPDATE content.boards SET deletion_known_min_seconds=60,deletion_unknown_min_seconds=600 WHERE slug=$1").bind(&f.board).execute(&f.owner).await.unwrap();
    sqlx::query("UPDATE content.posts SET created_at=$2-interval '61 seconds' WHERE id=$1")
        .bind(post)
        .bind(start)
        .execute(&f.owner)
        .await
        .unwrap();
    sqlx::query("UPDATE post_secrets.anonymous_sessions SET network_at=$2-901 WHERE token_hash=$1")
        .bind(token.as_slice())
        .bind(start.timestamp())
        .execute(&f.owner)
        .await
        .unwrap();
    let context = board_store::PublicDeletionContext {
        request_start: start,
        session: Some(session(&capability, false)),
    };
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&pool)
        .await
        .unwrap();
    let mut change = f.owner.begin().await.unwrap();
    sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
        .bind(&f.board)
        .execute(&mut *change)
        .await
        .unwrap();
    sqlx::query("UPDATE post_secrets.anonymous_sessions SET network_at=$2 WHERE token_hash=$1")
        .bind(token.as_slice())
        .bind(start.timestamp())
        .execute(&mut *change)
        .await
        .unwrap();
    let before = deletion_state(&f).await;
    let board = f.board.clone();
    let deletion = tokio::spawn(async move {
        board_store::delete_with_anonymous_proof_context(
            &pool,
            &mut deletion_batch(&board, context),
            post,
            token,
            proof,
            false,
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if sqlx::query_scalar::<_, bool>("SELECT cardinality(pg_blocking_pids($1))>0")
                .bind(pid)
                .fetch_one(&f.owner)
                .await
                .unwrap()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("Deletion reaches the held board lock");
    change.commit().await.unwrap();
    assert!(matches!(
        deletion.await.unwrap(),
        Err(StoreError::PublicDeletionRejected(
            "Error: You must wait longer before deleting this post."
        ))
    ));
    assert_eq!(deletion_state(&f).await, before);
    f.cleanup(&[token]).await;
}

fn deletion_batch(
    board: &str,
    context: board_store::PublicDeletionContext,
) -> board_store::PublicDeletionBatch {
    static NEXT: std::sync::OnceLock<std::sync::atomic::AtomicU64> = std::sync::OnceLock::new();
    let nonce = NEXT
        .get_or_init(|| {
            std::sync::atomic::AtomicU64::new(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos() as u64,
            )
        })
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let peer = std::net::IpAddr::V6(std::net::Ipv6Addr::new(
        0x2001,
        0xdb8,
        7,
        0,
        (nonce >> 48) as u16,
        (nonce >> 32) as u16,
        (nonce >> 16) as u16,
        nonce as u16,
    ));
    let key = board_domain::poster_id::PosterIdKey::parse(&"42".repeat(32)).unwrap();
    board_store::PublicDeletionBatch::new(board, context, key.public_deletion_rate_identity(peer))
}
