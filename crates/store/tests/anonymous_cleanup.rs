#![cfg(feature = "database-tests")]
// This binary deliberately gates minting; keep its single scenario separate
// from independently parallel anonymous-session cases.
mod support;
use board_domain::anonymous_session::Capability;
use board_store::{
    AnonymousPostingContext, NewPost, PostIdentityKeys, PostMetadata, PostingContext, StoreError,
    anonymous_session::PostingSession,
};
use chrono::{Timelike, Utc};
use sqlx::PgPool;
use std::{net::IpAddr, time::Duration};

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

    async fn cleanup(&self, tokens: &[[u8; 32]]) {
        let mut tx = support::begin_cleanup_with_sessions(
            &self.owner,
            std::slice::from_ref(&self.board),
            tokens,
        )
        .await;
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

// This control exposes the old fixture's waiting edge without deliberately
// completing a deadlock. Production expiry and registration are unchanged.
async fn waits_for(owner: &PgPool, reader: i32, blocker: i32) -> bool {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let blocked: bool = sqlx::query_scalar("SELECT $1=ANY(pg_blocking_pids($2))")
                .bind(blocker)
                .bind(reader)
                .fetch_one(owner)
                .await
                .unwrap();
            if blocked {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap_or(false)
}

struct OwnedMint(tokio::task::JoinHandle<Result<i64, StoreError>>);
impl Drop for OwnedMint {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[tokio::test]
async fn cleanup_session_prelocks_avoid_expiry_cascade_waits_without_disabling_retirement() {
    for prelock in [false, true] {
        let victim = Fixture::new().await;
        let other = Fixture::new().await;
        let expired = Capability::generate().unwrap();
        let live = Capability::generate().unwrap();
        let minted = Capability::generate().unwrap();
        let healthy = Capability::generate().unwrap();
        let victim_tokens = [expired.storage_hash(), live.storage_hash()];
        let other_tokens = [minted.storage_hash(), healthy.storage_hash()];
        let owner = victim.owner.clone();
        let public = victim.public.clone();
        let board = victim.board.clone();
        let other_board = other.board.clone();
        let mut probe = tokio::spawn(async move {
            let f = Fixture {
                owner: owner.clone(),
                public: public.clone(),
                board: board.clone(),
            };
            let post = f.create(0, session(&expired, true)).await.unwrap();
            let live_post = f.create(0, session(&live, true)).await.unwrap();
            let mint_pool = sqlx::postgres::PgPoolOptions::new()
                .max_connections(1)
                .acquire_timeout(Duration::from_secs(3))
                .connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
                .await
                .unwrap();
            let mint_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                .fetch_one(&mint_pool)
                .await
                .unwrap();
            // Prevent another mint sweeping this owned victim during setup.
            let mut gate = owner.begin().await.unwrap();
            sqlx::query(
                "SELECT singleton FROM post_secrets.anonymous_policy WHERE singleton FOR UPDATE",
            )
            .fetch_one(&mut *gate)
            .await
            .unwrap();
            let gate_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                .fetch_one(&mut *gate)
                .await
                .unwrap();
            sqlx::query(
                "UPDATE post_secrets.anonymous_sessions SET expires_at=1 WHERE token_hash=$1",
            )
            .bind(victim_tokens[0].as_slice())
            .execute(&owner)
            .await
            .unwrap();
            let eligible: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM post_secrets.anonymous_sessions WHERE expires_at<=1",
            )
            .fetch_one(&owner)
            .await
            .unwrap();
            assert!(
                eligible <= 64,
                "owned victim must fit the bounded expiry sweep"
            );
            let mut staged = if prelock {
                support::begin_cleanup_with_sessions(
                    &owner,
                    std::slice::from_ref(&board),
                    &victim_tokens[..1],
                )
                .await
            } else {
                support::begin_cleanup(&owner, std::slice::from_ref(&board)).await
            };
            let staged_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                .fetch_one(&mut *staged)
                .await
                .unwrap();
            sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id=$1")
                .bind(post)
                .execute(&mut *staged)
                .await
                .unwrap();
            sqlx::query("DELETE FROM content.posts WHERE board=$1 AND id=$2")
                .bind(&board)
                .bind(post)
                .execute(&mut *staged)
                .await
                .unwrap();
            let mint_fixture = Fixture {
                owner: owner.clone(),
                public: mint_pool.clone(),
                board: other_board.clone(),
            };
            let mut mint = OwnedMint(tokio::spawn(async move {
                mint_fixture.create(0, session(&minted, true)).await
            }));
            let reached_gate = waits_for(&owner, mint_pid, gate_pid).await;
            gate.commit().await.unwrap();
            let (legacy_wait, early) = if prelock {
                (
                    false,
                    tokio::time::timeout(Duration::from_secs(5), &mut mint.0)
                        .await
                        .ok(),
                )
            } else {
                (waits_for(&owner, mint_pid, staged_pid).await, None)
            };
            // Release the conflicting child lock before asserting either branch.
            staged.rollback().await.unwrap();
            let finished_before_release = early.is_some();
            let result = match early {
                Some(result) => result,
                None => tokio::time::timeout(Duration::from_secs(5), &mut mint.0)
                    .await
                    .expect("mint settles after staged cleanup release"),
            };
            assert!(reached_gate, "mint must reach the observed policy gate");
            assert!(
                if prelock {
                    finished_before_release
                } else {
                    legacy_wait
                },
                "prelock={prelock}: expected cleanup-order control was not observed"
            );
            let minted_post = result
                .expect("owned mint task")
                .expect("owned mint succeeds");
            let retained: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM post_secrets.anonymous_sessions WHERE token_hash=$1)",
            )
            .bind(victim_tokens[0].as_slice())
            .fetch_one(&owner)
            .await
            .unwrap();
            assert_eq!(
                retained, prelock,
                "only the protected expiry candidate is skipped"
            );
            // A later unlocked mint still retires the expired session/membership.
            let healthy_fixture = Fixture {
                owner: owner.clone(),
                public: mint_pool.clone(),
                board: other_board.clone(),
            };
            let healthy_post = healthy_fixture
                .create(0, session(&healthy, true))
                .await
                .unwrap();
            let retired: bool = sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM post_secrets.anonymous_sessions WHERE token_hash=$1) AND NOT EXISTS(SELECT 1 FROM post_secrets.anonymous_posts WHERE post_id=$2)")
                .bind(victim_tokens[0].as_slice()).bind(post).fetch_one(&owner).await.unwrap();
            assert!(retired);
            let live_membership: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM post_secrets.anonymous_posts WHERE post_id=$1 AND token_hash=$2)")
                .bind(live_post).bind(victim_tokens[1].as_slice()).fetch_one(&owner).await.unwrap();
            assert!(live_membership);
            assert!(board_store::find_post(&public, &board, post).await.is_ok());
            for id in [minted_post, healthy_post] {
                assert!(
                    board_store::find_post(&mint_pool, &other_board, id)
                        .await
                        .is_ok()
                );
            }
            mint_pool.close().await;
        });
        let outcome = match tokio::time::timeout(Duration::from_secs(60), &mut probe).await {
            Ok(outcome) => outcome,
            Err(_) => {
                probe.abort();
                tokio::time::timeout(Duration::from_secs(5), &mut probe)
                    .await
                    .expect("cancelled fixture probe settles")
            }
        };
        tokio::time::timeout(Duration::from_secs(10), victim.cleanup(&victim_tokens))
            .await
            .expect("owned victim cleanup settles");
        tokio::time::timeout(Duration::from_secs(10), other.cleanup(&other_tokens))
            .await
            .expect("owned mint cleanup settles");
        outcome.unwrap();
    }
}
