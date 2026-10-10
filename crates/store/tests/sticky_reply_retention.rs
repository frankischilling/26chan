#![cfg(feature = "database-tests")]
mod support;

use board_domain::anonymous_session::Capability;
use board_store::{
    AnonymousPostingContext, NewPost, PostIdentityKeys, PostMetadata, PostingContext, StoreError,
    anonymous_session::PostingSession,
};
use chrono::Utc;
use serde_json::Value;
use sqlx::PgPool;

fn post() -> NewPost {
    NewPost {
        name: "Anonymous".into(),
        subject: "Source retention fixture".into(),
        comment: "Synthetic, independently owned retention text".into(),
        deletion_hash: "retention-fixture-hash".into(),
        sage: false,
    }
}

struct Fixture {
    owner: PgPool,
    public: PgPool,
    board: String,
    op: i64,
}

impl Fixture {
    async fn new(cap: i32) -> Self {
        let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let public =
            board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
                .await
                .unwrap();
        let board: String =
            sqlx::query_scalar("SELECT 'sr'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
                .fetch_one(&owner)
                .await
                .unwrap();
        sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,image_limit,posting_reply_seconds,posting_image_seconds,posting_thread_seconds,user_thread_limit) VALUES($1,'Sticky retention fixture','Owned synthetic',2000,$2,$2,100,10,100,0,0,0,100)")
            .bind(&board)
            .bind(cap)
            .execute(&owner)
            .await
            .unwrap();
        let op = support::create_post(&public, &board, 0, &post())
            .await
            .unwrap();
        sqlx::query("UPDATE content.threads SET sticky=true,undead=true WHERE board=$1 AND id=$2")
            .bind(&board)
            .bind(op)
            .execute(&owner)
            .await
            .unwrap();
        Self {
            owner,
            public,
            board,
            op,
        }
    }

    async fn import_replies(&self, count: usize) -> Vec<i64> {
        // Synthetic historical posts have no recoverable posting identity. The
        // owner-only INSERT deliberately exercises the import branch in 0087.
        let mut ids: Vec<i64> = sqlx::query_scalar(
            "INSERT INTO content.posts(board,thread_id,name,subject,comment)
             SELECT $1,$2,'Anonymous','','Owned historical retention reply'
             FROM generate_series(1,$3) RETURNING id",
        )
        .bind(&self.board)
        .bind(self.op)
        .bind(count as i64)
        .fetch_all(&self.owner)
        .await
        .unwrap();
        ids.sort_unstable();
        sqlx::query("UPDATE content.threads SET reply_count=$3 WHERE board=$1 AND id=$2")
            .bind(&self.board)
            .bind(self.op)
            .bind(i32::try_from(count.min(1000)).unwrap())
            .execute(&self.owner)
            .await
            .unwrap();
        ids
    }

    async fn state(&self) -> (Vec<i64>, i32, i64) {
        let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM content.posts WHERE board=$1 AND thread_id=$2 AND id<>$2 AND NOT deleted ORDER BY id")
            .bind(&self.board).bind(self.op).fetch_all(&self.owner).await.unwrap();
        let cached: i32 =
            sqlx::query_scalar("SELECT reply_count FROM content.threads WHERE board=$1 AND id=$2")
                .bind(&self.board)
                .bind(self.op)
                .fetch_one(&self.owner)
                .await
                .unwrap();
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=$1 AND thread_id=$2 AND id<>$2 AND NOT deleted")
            .bind(&self.board).bind(self.op).fetch_one(&self.owner).await.unwrap();
        (ids, cached, count)
    }

    async fn cleanup(self, tokens: &[[u8; 32]], jobs: &[String]) {
        let mut tx = support::begin_cleanup_with_sessions(
            &self.owner,
            std::slice::from_ref(&self.board),
            tokens,
        )
        .await;
        support::cleanup_posting(&mut *tx, &self.board).await;
        for sql in [
            "DELETE FROM content.reports WHERE board=$1",
            "DELETE FROM content.post_media WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
            "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
            "DELETE FROM content.posts WHERE board=$1",
            "DELETE FROM content.threads WHERE board=$1",
            "DELETE FROM content.boards WHERE slug=$1",
        ] {
            sqlx::query(sql)
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
        sqlx::query("DELETE FROM media.assets WHERE job_id=ANY($1)")
            .bind(jobs)
            .execute(&self.owner)
            .await
            .unwrap();
        sqlx::query("DELETE FROM media.jobs WHERE id=ANY($1)")
            .bind(jobs)
            .execute(&self.owner)
            .await
            .unwrap();
        self.public.close().await;
        self.owner.close().await;
    }
}

#[tokio::test]
async fn pinned_sql_oracle_matches_small_and_thousand_reply_windows() {
    let reference: Value = serde_json::from_str(include_str!(
        "../../../fixtures/sticky-retention-reference.json"
    ))
    .unwrap();
    assert_eq!(reference["source_sticky_cap"], 1000);
    assert_eq!(reference["cases"].as_array().unwrap().len(), 9);
    for case in reference["cases"].as_array().unwrap() {
        let cap = case["capacity"].as_i64().unwrap() as i32;
        let fixture = Fixture::new(cap).await;
        let existing = fixture
            .import_replies(case["existing"].as_array().unwrap().len())
            .await;
        let before = fixture.state().await;
        assert_eq!(before.0, existing);
        if cap == 1 && !existing.is_empty() {
            assert!(matches!(
                support::create_post(&fixture.public, &fixture.board, fixture.op, &post()).await,
                Err(StoreError::Conflict(_))
            ));
            assert_eq!(fixture.state().await, before);
        } else {
            let new = support::create_post(&fixture.public, &fixture.board, fixture.op, &post())
                .await
                .unwrap();
            let survivors = fixture.state().await;
            let removed_count = case["pruned"].as_array().unwrap().len();
            let keep = existing.len() - removed_count;
            assert_eq!(
                survivors.0,
                [existing[removed_count..].to_vec(), vec![new]].concat(),
                "{}",
                case["case"]
            );
            assert_eq!(survivors.1 as usize, keep + 1, "{}", case["case"]);
            assert_eq!(survivors.2, case["visible_reply_count"].as_i64().unwrap());
            assert_eq!(survivors.0.len(), keep + 1);
        }
        fixture.cleanup(&[], &[]).await;
    }
}

async fn add_mock_approved_media(owner: &PgPool, post: i64) -> (String, String) {
    let job: String = sqlx::query_scalar("SELECT replace(gen_random_uuid()::text,'-','')")
        .fetch_one(owner)
        .await
        .unwrap();
    let asset: String = sqlx::query_scalar("SELECT replace(gen_random_uuid()::text,'-','')")
        .fetch_one(owner)
        .await
        .unwrap();
    let lease = "d".repeat(32);
    sqlx::query("INSERT INTO media.jobs(id,filename,state,input_bytes,attempts,lease_token,output_sha256,output_bytes) VALUES($1,'retention.png','published',100,1,$2,repeat('a',64),100)")
        .bind(&job).bind(&lease).execute(owner).await.unwrap();
    sqlx::query("INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at) VALUES($1,$2,$3,repeat('b',64),100,10,10,'approved',clock_timestamp())")
        .bind(&asset).bind(&job).bind(&lease).execute(owner).await.unwrap();
    sqlx::query("INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler) VALUES($1,$2,$3,'retention.png',100,10,10,false)")
        .bind(post).bind(&job).bind(&asset).execute(owner).await.unwrap();
    (job, asset)
}

#[tokio::test]
async fn rollover_retires_only_old_reply_secrets_reports_and_visible_media() {
    let fixture = Fixture::new(3).await;
    let owned = Capability::generate().unwrap();
    let token = owned.storage_hash();
    let first = support::create_post_with_anonymous_session(
        &fixture.public,
        &fixture.board,
        fixture.op,
        &post(),
        None,
        AnonymousPostingContext {
            posting: PostingContext {
                request_start: Utc::now(),
                peer: Some(support::peer()),
                op_password_proof: None,
            },
            session: PostingSession {
                fingerprints: owned.fingerprints(Some(support::peer()), *b"US"),
                minted: true,
                now: Utc::now(),
            },
        },
        PostMetadata {
            drawing: None,
            keys: PostIdentityKeys {
                tripcode: None,
                poster_id: None,
            },
            country_database: None,
            flag: "",
            options: "",
            spoiler: false,
        },
    )
    .await
    .unwrap();
    let second = support::create_post(&fixture.public, &fixture.board, fixture.op, &post())
        .await
        .unwrap();
    let third = support::create_post(&fixture.public, &fixture.board, fixture.op, &post())
        .await
        .unwrap();
    let (job, asset) = add_mock_approved_media(&fixture.owner, first).await;
    sqlx::query(
        "INSERT INTO content.reports(board,post_id,reason) VALUES($1,$2,'Owned original report')",
    )
    .bind(&fixture.board)
    .bind(first)
    .execute(&fixture.owner)
    .await
    .unwrap();
    for target in [first, second, third] {
        let hash: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM post_secrets.deletion WHERE post_id=$1)",
        )
        .bind(target)
        .fetch_one(&fixture.owner)
        .await
        .unwrap();
        assert!(hash);
    }
    let proof: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM post_secrets.anonymous_posts WHERE post_id=$1)",
    )
    .bind(first)
    .fetch_one(&fixture.owner)
    .await
    .unwrap();
    assert!(proof);
    let published: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM media.approved_post_assets WHERE board=$1 AND id=$2",
    )
    .bind(&fixture.board)
    .bind(&asset)
    .fetch_one(&fixture.owner)
    .await
    .unwrap();
    assert_eq!(published, 1);
    let newest = support::create_post(&fixture.public, &fixture.board, fixture.op, &post())
        .await
        .unwrap();
    assert_eq!(fixture.state().await, (vec![second, third, newest], 3, 3));
    let deleted: bool = sqlx::query_scalar("SELECT deleted FROM content.posts WHERE id=$1")
        .bind(first)
        .fetch_one(&fixture.owner)
        .await
        .unwrap();
    assert!(deleted);
    for sql in [
        "SELECT EXISTS(SELECT 1 FROM post_secrets.deletion WHERE post_id=$1)",
        "SELECT EXISTS(SELECT 1 FROM post_secrets.anonymous_posts WHERE post_id=$1)",
        "SELECT EXISTS(SELECT 1 FROM content.reports WHERE post_id=$1)",
        "SELECT EXISTS(SELECT 1 FROM content.visible_post_media WHERE post_id=$1)",
    ] {
        let remains: bool = sqlx::query_scalar(sql)
            .bind(first)
            .fetch_one(&fixture.owner)
            .await
            .unwrap();
        assert!(!remains, "{sql}");
    }
    let reader: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM media.approved_post_assets WHERE board=$1 AND id=$2",
    )
    .bind(&fixture.board)
    .bind(&asset)
    .fetch_one(&fixture.owner)
    .await
    .unwrap();
    assert_eq!(
        reader, 0,
        "Deleted media cannot remain public while bytes await collection"
    );
    let candidate: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM media.retirable_outputs WHERE id=$1)")
            .bind(&asset)
            .fetch_one(&fixture.owner)
            .await
            .unwrap();
    assert!(
        candidate,
        "Retired bytes remain eligible for isolated worker cleanup"
    );
    for target in [second, third, newest] {
        let retained: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM post_secrets.deletion WHERE post_id=$1)",
        )
        .bind(target)
        .fetch_one(&fixture.owner)
        .await
        .unwrap();
        assert!(retained, "Unpruned password {target} must survive");
    }
    fixture.cleanup(&[token], &[job]).await;
}

#[tokio::test]
async fn failed_post_and_simultaneous_writers_cannot_partially_prune() {
    let fixture = Fixture::new(3).await;
    let existing = fixture.import_replies(3).await;
    let before = fixture.state().await;
    let mut invalid = post();
    invalid.comment.clear();
    assert!(
        support::create_post(&fixture.public, &fixture.board, fixture.op, &invalid)
            .await
            .is_err()
    );
    assert_eq!(fixture.state().await, before);
    // An unconsumed upload is rejected only after the store has entered its
    // transactional window; rollback must restore deleted replies and counts.
    let intake = board_store::media_intake::IntakeStore::connect(
        &std::env::var("INTAKE_DATABASE_URL").unwrap(),
    )
    .await
    .unwrap();
    let upload = intake.reserve("retention-pending.png").await.unwrap();
    let pending = board_store::post_media::NewAttachment {
        upload,
        spoiler: false,
    };
    assert!(
        support::create_post_with_attachment(
            &fixture.public,
            &fixture.board,
            fixture.op,
            &post(),
            Some(&pending)
        )
        .await
        .is_err()
    );
    assert_eq!(fixture.state().await, before);
    let jobs = vec![pending.upload.id.clone()];
    let a = {
        let pool = fixture.public.clone();
        let board = fixture.board.clone();
        let op = fixture.op;
        tokio::spawn(async move {
            support::create_post_with_context(
                &pool,
                &board,
                op,
                &post(),
                None,
                PostingContext {
                    request_start: Utc::now(),
                    peer: Some("192.0.2.203".parse().unwrap()),
                    op_password_proof: None,
                },
            )
            .await
        })
    };
    let b = {
        let pool = fixture.public.clone();
        let board = fixture.board.clone();
        let op = fixture.op;
        tokio::spawn(async move {
            support::create_post_with_context(
                &pool,
                &board,
                op,
                &post(),
                None,
                PostingContext {
                    request_start: Utc::now(),
                    peer: Some("192.0.2.204".parse().unwrap()),
                    op_password_proof: None,
                },
            )
            .await
        })
    };
    let x = a.await.unwrap().unwrap();
    let y = b.await.unwrap().unwrap();
    let mut expected = [existing, vec![x, y]].concat();
    expected.sort_unstable();
    assert_eq!(
        fixture.state().await,
        (expected[expected.len() - 3..].to_vec(), 3, 3)
    );
    let op_live: bool = sqlx::query_scalar("SELECT NOT deleted FROM content.posts WHERE id=$1")
        .bind(fixture.op)
        .fetch_one(&fixture.owner)
        .await
        .unwrap();
    assert!(op_live);
    fixture.cleanup(&[], &jobs).await;
}

#[tokio::test]
async fn robot9000_rejection_restores_the_old_window_and_private_evidence() {
    let fixture = Fixture::new(3).await;
    let earlier = fixture.import_replies(2).await;
    sqlx::query("UPDATE content.boards SET robot9000=true WHERE slug=$1")
        .bind(&fixture.board)
        .execute(&fixture.owner)
        .await
        .unwrap();
    let first = support::create_post(&fixture.public, &fixture.board, fixture.op, &post())
        .await
        .unwrap();
    let before = fixture.state().await;
    assert_eq!(before.0, [earlier, vec![first]].concat());
    let hash_before: i64 = sqlx::query_scalar("SELECT count(*) FROM post_secrets.deletion d JOIN content.posts p ON p.id=d.post_id WHERE p.board=$1")
        .bind(&fixture.board).fetch_one(&fixture.owner).await.unwrap();
    assert!(matches!(
        support::create_post(&fixture.public, &fixture.board, fixture.op, &post()).await,
        Err(StoreError::Robot9000Rejected(_))
    ));
    assert_eq!(fixture.state().await, before);
    let hash_after: i64 = sqlx::query_scalar("SELECT count(*) FROM post_secrets.deletion d JOIN content.posts p ON p.id=d.post_id WHERE p.board=$1")
        .bind(&fixture.board).fetch_one(&fixture.owner).await.unwrap();
    assert_eq!(
        hash_after, hash_before,
        "Robot9000 rollback must restore retired hashes"
    );
    fixture.cleanup(&[], &[]).await;
}

#[tokio::test]
async fn forged_retirement_marker_cannot_remove_recent_unrelated_or_private_proofs() {
    let fixture = Fixture::new(3).await;
    let ids = fixture.import_replies(4).await;
    // Imported source-history rows deliberately have no deletion credentials.
    // Seed this test's own four passwords so the negative retirement assertions
    // cannot pass or fail vacuously on absent private authority.
    let inserted = sqlx::query(
        "INSERT INTO post_secrets.deletion(post_id,password_hash) \
        SELECT id,'owned-marker-test-hash' FROM content.posts \
        WHERE board=$1 AND thread_id=$2 AND id<>$2 AND NOT deleted",
    )
    .bind(&fixture.board)
    .bind(fixture.op)
    .execute(&fixture.owner)
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(inserted, ids.len() as u64);
    let initial: Vec<i64> = sqlx::query_scalar(
        "SELECT post_id FROM post_secrets.deletion \
        WHERE post_id=ANY($1) ORDER BY post_id",
    )
    .bind(&ids)
    .fetch_all(&fixture.owner)
    .await
    .unwrap();
    assert_eq!(
        initial, ids,
        "All four negative cases need real credentials"
    );
    let first = ids[0];
    let recent = *ids.last().unwrap();
    let denied = fixture.public.clone();
    let mut tx = denied.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
        .bind(&fixture.board)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM content.threads WHERE board=$1 AND id=$2 FOR UPDATE")
        .bind(&fixture.board)
        .bind(fixture.op)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SELECT set_config('board.sticky_prune_thread',$1,true)")
        .bind(fixture.op.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let error = sqlx::query("UPDATE content.posts SET deleted=true WHERE board=$1 AND id=$2")
        .bind(&fixture.board)
        .bind(recent)
        .execute(&mut *tx)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    tx.rollback().await.unwrap();
    let protected: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM post_secrets.deletion WHERE post_id=$1)")
            .bind(recent)
            .fetch_one(&fixture.owner)
            .await
            .unwrap();
    assert!(protected);
    // A marker for a different thread must never retire its credentials, even
    // if someone already has separate authority to soft-delete that post.
    let second_op = support::create_post(&fixture.public, &fixture.board, 0, &post())
        .await
        .unwrap();
    let unrelated = support::create_post(&fixture.public, &fixture.board, second_op, &post())
        .await
        .unwrap();
    let mut tx = fixture.public.begin().await.unwrap();
    sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
        .bind(&fixture.board)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SELECT set_config('board.sticky_prune_thread',$1,true)")
        .bind(fixture.op.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE content.posts SET deleted=true WHERE board=$1 AND id=$2")
        .bind(&fixture.board)
        .bind(unrelated)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let unrelated_hash: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM post_secrets.deletion WHERE post_id=$1)")
            .bind(unrelated)
            .fetch_one(&fixture.owner)
            .await
            .unwrap();
    assert!(
        unrelated_hash,
        "A different thread cannot inherit this retirement marker"
    );
    // The transaction marker cannot turn an ordinary parent into an undead
    // sticky, even for a reply old enough to satisfy its numeric window.
    sqlx::query("UPDATE content.threads SET undead=false WHERE board=$1 AND id=$2")
        .bind(&fixture.board)
        .bind(fixture.op)
        .execute(&fixture.owner)
        .await
        .unwrap();
    let mut tx = fixture.public.begin().await.unwrap();
    sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
        .bind(&fixture.board)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SELECT set_config('board.sticky_prune_thread',$1,true)")
        .bind(fixture.op.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let error = sqlx::query("UPDATE content.posts SET deleted=true WHERE board=$1 AND id=$2")
        .bind(&fixture.board)
        .bind(first)
        .execute(&mut *tx)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    tx.rollback().await.unwrap();
    sqlx::query("UPDATE content.threads SET undead=true WHERE board=$1 AND id=$2")
        .bind(&fixture.board)
        .bind(fixture.op)
        .execute(&fixture.owner)
        .await
        .unwrap();
    // A repeat UPDATE of an already-deleted historical row is not a fresh
    // transition, regardless of the caller-settable marker.
    sqlx::query("UPDATE content.posts SET deleted=true WHERE board=$1 AND id=$2")
        .bind(&fixture.board)
        .bind(first)
        .execute(&fixture.owner)
        .await
        .unwrap();
    let mut tx = fixture.public.begin().await.unwrap();
    sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
        .bind(&fixture.board)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SELECT set_config('board.sticky_prune_thread',$1,true)")
        .bind(fixture.op.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE content.posts SET deleted=true WHERE board=$1 AND id=$2")
        .bind(&fixture.board)
        .bind(first)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let retained_hash: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM post_secrets.deletion WHERE post_id=$1)")
            .bind(first)
            .fetch_one(&fixture.owner)
            .await
            .unwrap();
    assert!(
        retained_hash,
        "There is no automatic historical credential sweep"
    );
    let forbidden = sqlx::query("DELETE FROM post_secrets.anonymous_posts WHERE false")
        .execute(&fixture.public)
        .await
        .unwrap_err();
    assert_eq!(
        forbidden.as_database_error().unwrap().code().as_deref(),
        Some("42501")
    );
    // An owner-controlled private-board switch takes public posting, reads and
    // trigger retirement out of scope without changing retained credentials.
    sqlx::query("UPDATE content.boards SET staff_only=true WHERE slug=$1")
        .bind(&fixture.board)
        .execute(&fixture.owner)
        .await
        .unwrap();
    let private: i64 = sqlx::query("UPDATE content.posts SET deleted=true WHERE board=$1 AND id=$2")
        .bind(&fixture.board)
        .bind(ids[1])
        .execute(&fixture.public)
        .await
        .unwrap()
        .rows_affected() as i64;
    assert_eq!(private, 0);
    let private_hash: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM post_secrets.deletion WHERE post_id=$1)")
            .bind(ids[1])
            .fetch_one(&fixture.owner)
            .await
            .unwrap();
    assert!(private_hash);
    // A separately authenticated staff database role can prune that same
    // eligible old reply on a private board. The narrow definer honors the
    // actual invoker, and it cannot grant access to the public role.
    let staff = PgPool::connect(&std::env::var("STAFF_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut tx = staff.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
        .bind(&fixture.board)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM content.threads WHERE board=$1 AND id=$2 FOR UPDATE")
        .bind(&fixture.board)
        .bind(fixture.op)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SELECT set_config('board.sticky_prune_thread',$1,true)")
        .bind(fixture.op.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let changed = sqlx::query("UPDATE content.posts SET deleted=true WHERE board=$1 AND id=$2")
        .bind(&fixture.board)
        .bind(ids[1])
        .execute(&mut *tx)
        .await
        .unwrap()
        .rows_affected();
    assert_eq!(changed, 1);
    tx.commit().await.unwrap();
    let retired: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM post_secrets.deletion WHERE post_id=$1)")
            .bind(ids[1])
            .fetch_one(&fixture.owner)
            .await
            .unwrap();
    assert!(
        !retired,
        "Staff retention on a private board must retire only the selected post"
    );
    let survivor: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM post_secrets.deletion WHERE post_id=$1)")
            .bind(recent)
            .fetch_one(&fixture.owner)
            .await
            .unwrap();
    assert!(survivor);
    staff.close().await;
    fixture.cleanup(&[], &[]).await;
}

async fn wait_for_board_lock(owner: &PgPool, waiter: i32, holder: i32) {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let blocked: bool = sqlx::query_scalar("SELECT $2=ANY(pg_blocking_pids($1))")
                .bind(waiter)
                .bind(holder)
                .fetch_one(owner)
                .await
                .unwrap();
            if blocked {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the ordinary reply must wait on the retained board lock");
}

#[tokio::test]
async fn queued_reply_rechecks_changed_thread_protection_before_pruning() {
    let fixture = Fixture::new(3).await;
    let original = fixture.import_replies(3).await;
    let queued_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let queued_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&queued_pool)
        .await
        .unwrap();
    let mut owner_tx = fixture.owner.begin().await.unwrap();
    let owner_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *owner_tx)
        .await
        .unwrap();
    sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
        .bind(&fixture.board)
        .execute(&mut *owner_tx)
        .await
        .unwrap();
    let pool = queued_pool.clone();
    let board = fixture.board.clone();
    let op = fixture.op;
    let queued =
        tokio::spawn(async move { support::create_post(&pool, &board, op, &post()).await });
    wait_for_board_lock(&fixture.owner, queued_pid, owner_pid).await;
    sqlx::query("UPDATE content.threads SET sticky=false,undead=false WHERE board=$1 AND id=$2")
        .bind(&fixture.board)
        .bind(fixture.op)
        .execute(&mut *owner_tx)
        .await
        .unwrap();
    owner_tx.commit().await.unwrap();
    assert!(matches!(
        queued.await.unwrap(),
        Err(StoreError::Conflict(_))
    ));
    assert_eq!(fixture.state().await, (original.clone(), 3, 3));
    let mut owner_tx = fixture.owner.begin().await.unwrap();
    let owner_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *owner_tx)
        .await
        .unwrap();
    sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
        .bind(&fixture.board)
        .execute(&mut *owner_tx)
        .await
        .unwrap();
    let pool = queued_pool.clone();
    let board = fixture.board.clone();
    let op = fixture.op;
    let queued =
        tokio::spawn(async move { support::create_post(&pool, &board, op, &post()).await });
    wait_for_board_lock(&fixture.owner, queued_pid, owner_pid).await;
    sqlx::query("UPDATE content.threads SET sticky=true,undead=true WHERE board=$1 AND id=$2")
        .bind(&fixture.board)
        .bind(fixture.op)
        .execute(&mut *owner_tx)
        .await
        .unwrap();
    owner_tx.commit().await.unwrap();
    let newest = queued.await.unwrap().unwrap();
    assert_eq!(
        fixture.state().await,
        (vec![original[1], original[2], newest], 3, 3)
    );
    queued_pool.close().await;
    fixture.cleanup(&[], &[]).await;
}

#[tokio::test]
async fn preexisting_deleted_reply_gaps_do_not_count_toward_the_retained_window() {
    let fixture = Fixture::new(3).await;
    let existing = fixture.import_replies(4).await;
    sqlx::query("UPDATE content.posts SET deleted=true WHERE board=$1 AND id=$2")
        .bind(&fixture.board)
        .bind(existing[1])
        .execute(&fixture.owner)
        .await
        .unwrap();
    assert_eq!(
        fixture.state().await,
        (vec![existing[0], existing[2], existing[3]], 4, 3)
    );
    let newest = support::create_post(&fixture.public, &fixture.board, fixture.op, &post())
        .await
        .unwrap();
    assert_eq!(
        fixture.state().await,
        (vec![existing[2], existing[3], newest], 3, 3)
    );
    let deleted: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM content.posts WHERE board=$1 AND thread_id=$2 AND deleted ORDER BY id",
    )
    .bind(&fixture.board)
    .bind(fixture.op)
    .fetch_all(&fixture.owner)
    .await
    .unwrap();
    assert_eq!(deleted, vec![existing[0], existing[1]]);
    // Do not promote a pruned reply into an OP or touch a second thread.
    let still_open: bool = sqlx::query_scalar("SELECT NOT deleted FROM content.posts WHERE id=$1")
        .bind(fixture.op)
        .fetch_one(&fixture.owner)
        .await
        .unwrap();
    assert!(still_open);
    fixture.cleanup(&[], &[]).await;
}

#[tokio::test]
async fn closed_and_archived_threads_keep_their_existing_admission_barrier() {
    let fixture = Fixture::new(3).await;
    let existing = fixture.import_replies(3).await;
    sqlx::query("UPDATE content.threads SET closed=true WHERE board=$1 AND id=$2")
        .bind(&fixture.board)
        .bind(fixture.op)
        .execute(&fixture.owner)
        .await
        .unwrap();
    assert!(matches!(
        support::create_post(&fixture.public, &fixture.board, fixture.op, &post()).await,
        Err(StoreError::Conflict(_))
    ));
    assert_eq!(fixture.state().await, (existing.clone(), 3, 3));
    sqlx::query("UPDATE content.threads SET closed=false WHERE board=$1 AND id=$2")
        .bind(&fixture.board)
        .bind(fixture.op)
        .execute(&fixture.owner)
        .await
        .unwrap();
    let recent = support::create_post(&fixture.public, &fixture.board, fixture.op, &post())
        .await
        .unwrap();
    assert_eq!(
        fixture.state().await,
        (vec![existing[1], existing[2], recent], 3, 3)
    );
    // Archived threads must have sticky removed, matching the existing DB
    // archive constraint, and are not eligible for public replies afterward.
    sqlx::query("UPDATE content.boards SET archive_retention_seconds=3600 WHERE slug=$1")
        .bind(&fixture.board)
        .execute(&fixture.owner)
        .await
        .unwrap();
    sqlx::query("UPDATE content.threads SET sticky=false,closed=true,archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE board=$1 AND id=$2")
        .bind(&fixture.board).bind(fixture.op).execute(&fixture.owner).await.unwrap();
    assert!(
        support::create_post(&fixture.public, &fixture.board, fixture.op, &post())
            .await
            .is_err()
    );
    assert_eq!(
        fixture.state().await,
        (vec![existing[1], existing[2], recent], 3, 3)
    );
    fixture.cleanup(&[], &[]).await;
}
