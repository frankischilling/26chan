#![cfg(feature = "database-tests")]
mod support;

use board_domain::anonymous_session::Capability;
use board_store::{
    AnonymousPostingContext, NewPost, PostIdentityKeys, PostMetadata, PostingContext,
    anonymous_session::PostingSession,
};
use chrono::Utc;
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Transaction};
use std::time::{Duration, Instant};

#[derive(Clone)]
struct Fixture {
    owner: PgPool,
    public: PgPool,
    staff: PgPool,
    board: String,
    token: [u8; 32],
    ids: Vec<i64>,
    account: i64,
}

impl Fixture {
    async fn new() -> Self {
        async fn pool(variable: &str, expected: &str) -> PgPool {
            let pool = PgPool::connect(&std::env::var(variable).unwrap())
                .await
                .unwrap();
            let role: String = sqlx::query_scalar("SELECT current_user::text")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(
                role, expected,
                "Use the actual restricted login, not SET ROLE on an owner connection"
            );
            pool
        }
        let owner = pool("MIGRATION_DATABASE_URL", "board_migrator").await;
        let public = pool("TEST_PUBLIC_DATABASE_URL", "board_public").await;
        let staff = pool("STAFF_DATABASE_URL", "board_staff").await;
        let seed: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
            .fetch_one(&owner)
            .await
            .unwrap();
        let board = format!("fe{seed:x}");
        sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,posting_reply_seconds,posting_image_seconds,posting_thread_seconds,archive_retention_seconds) VALUES($1,'Fresh erasure','Owned fixture',2000,100,100,100,10,0,0,0,3600)")
            .bind(&board).execute(&owner).await.unwrap();
        let capability = Capability::generate().unwrap();
        let token = capability.storage_hash();
        let mut ids = Vec::new();
        for n in 0..4 {
            let parent = if n == 0 || n == 3 { 0 } else { ids[0] };
            let id = support::create_post_with_anonymous_session(
                &public,
                &board,
                parent,
                &post(),
                None,
                AnonymousPostingContext {
                    posting: PostingContext {
                        request_start: Utc::now(),
                        peer: Some(support::peer()),
                        op_password_proof: None,
                    },
                    session: PostingSession {
                        fingerprints: capability.fingerprints(Some(support::peer()), *b"US"),
                        minted: n == 0,
                        now: Utc::now(),
                    },
                },
                PostMetadata {
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
            ids.push(id);
        }
        let account: i64 = sqlx::query_scalar(
            "INSERT INTO staff_identity.accounts(role) VALUES('moderator') RETURNING id",
        )
        .fetch_one(&owner)
        .await
        .unwrap();
        // Private author bindings are owned fixture data, not new posting authority.
        sqlx::query("INSERT INTO staff_identity.discussion_posts(post_id,account_id) SELECT unnest($1::bigint[]),$2").bind(&ids).bind(account).execute(&owner).await.unwrap();
        Self {
            owner,
            public,
            staff,
            board,
            token,
            ids,
            account,
        }
    }

    async fn cleanup(&self) {
        let mut tx = support::begin_cleanup_with_sessions(
            &self.owner,
            std::slice::from_ref(&self.board),
            &[self.token],
        )
        .await;
        support::cleanup_posting(&mut *tx, &self.board).await;
        for query in [
            "DELETE FROM content.reports WHERE board=$1",
            "DELETE FROM content.moderation_audit WHERE board=$1",
            "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
            "DELETE FROM content.post_media WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
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
        sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
            .bind(self.token.as_slice())
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("DELETE FROM staff_identity.accounts WHERE id=$1")
            .bind(self.account)
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }

    async fn posts(&self) -> Vec<Value> {
        sqlx::query_scalar("SELECT to_jsonb(p) FROM content.posts p WHERE board=$1 ORDER BY id")
            .bind(&self.board)
            .fetch_all(&self.owner)
            .await
            .unwrap()
    }

    async fn authority(&self) -> Value {
        sqlx::query_scalar("SELECT jsonb_build_object('deletion',(SELECT jsonb_agg(to_jsonb(d) ORDER BY post_id) FROM post_secrets.deletion d WHERE post_id=ANY($1)), 'anonymous',(SELECT jsonb_agg(to_jsonb(a) ORDER BY post_id) FROM post_secrets.anonymous_posts a WHERE post_id=ANY($1)), 'history',(SELECT jsonb_agg(to_jsonb(h) ORDER BY post_id) FROM post_secrets.posting_history h WHERE post_id=ANY($1)))")
            .bind(&self.ids).fetch_one(&self.owner).await.unwrap()
    }

    async fn shared(&self) -> Value {
        sqlx::query_scalar("SELECT jsonb_build_object('session',(SELECT to_jsonb(s) FROM post_secrets.anonymous_sessions s WHERE token_hash=$1),'actions',(SELECT jsonb_agg(to_jsonb(a) ORDER BY actor_hash) FROM post_secrets.posting_thread_actions a WHERE board=$2),'audit',(SELECT jsonb_agg(to_jsonb(a) ORDER BY id) FROM content.moderation_audit a WHERE board=$2))")
            .bind(self.token.as_slice()).bind(&self.board).fetch_one(&self.owner).await.unwrap()
    }

    async fn sentinels(&self) {
        // Mutually exclusive metadata variants deliberately span OP and replies.
        // Seed saved payloads as the migrator while all production guards remain on.
        // Use the real codec with authorized bounds: WF02 matches the saved
        // staff_authorized_limits flag, and all payload parts remain decodable.
        let mut prepared = board_domain::wordfiltered_comment::prepare_with_limits(
            "Erase wordfilter sentinel",
            board_domain::comment_markup::MarkupPolicy::from_post_format(8).unwrap(),
            board_domain::wordfilter::Profile::Global,
            None,
            board_domain::PostLimits::authorized(2000).unwrap(),
        )
        .unwrap();
        prepared.freeze_format(&self.board);
        let payload = prepared.encode().unwrap();
        let projection = prepared.source_projection();
        assert_eq!(&payload[..4], b"WF02");
        assert_eq!(
            board_domain::wordfiltered_comment::PreparedComment::decode(&payload).unwrap(),
            prepared
        );
        sqlx::query("UPDATE content.posts SET name='Erase name sentinel',subject='Erase subject sentinel',comment=$3,trip='!1234567890',poster_id='AbCd1234',country='US',country_name='Erase country sentinel',comment_format=8,staff_authorized_limits=true,image_spoiler=true,wordfilter_payload=$2,wordfilter_search=$3,dice_result='Erase dice sentinel',created_at='2001-02-03 04:05:06+00' WHERE id=ANY($1)")
            .bind(&self.ids[..3]).bind(&payload).bind(&projection).execute(&self.owner).await.unwrap();
        sqlx::query("UPDATE content.posts SET json_op_poster_id='QrSt5678' WHERE id=$1")
            .bind(self.ids[0])
            .execute(&self.owner)
            .await
            .unwrap();
        sqlx::query("UPDATE content.posts SET capcode='mod',poster_id='Mod',country=NULL,country_name=NULL,board_flag='AC',flag_name='Erase flag sentinel',dice_result=NULL,fortune_text='Erase fortune sentinel',fortune_color='#123abc' WHERE id=$1")
            .bind(self.ids[1]).execute(&self.owner).await.unwrap();
        sqlx::query("UPDATE content.posts SET board_flag_type='test' WHERE id=$1")
            .bind(self.ids[2])
            .execute(&self.owner)
            .await
            .unwrap();
        sqlx::query("INSERT INTO content.moderation_audit(account_id,board,target_id,action) VALUES(42,$1,$2,'close')").bind(&self.board).bind(self.ids[0]).execute(&self.owner).await.unwrap();
    }

    async fn assert_thread_erased(&self, id: i64) {
        let row: Value =
            sqlx::query_scalar("SELECT to_jsonb(t) FROM content.threads t WHERE id=$1")
                .bind(id)
                .fetch_one(&self.owner)
                .await
                .unwrap();
        let epoch = "1970-01-01T00:00:00+00:00";
        assert_eq!(
            row,
            json!({"id":id,"board":self.board,"deleted":true,"content_erased":true,
            "created_at":epoch,"bumped_at":epoch,"modified_at":epoch,"http_modified_at":epoch,
            "reply_count":0,"sticky_rank":0,"sticky":false,"closed":false,"permasage":false,"permaage":false,"undead":false,
            "archived_at":null,"archive_expires_at":null}),
            "Every thread column must have a justified canonical tombstone value"
        );
        let peers: i64 =
            sqlx::query_scalar("SELECT count(*) FROM post_secrets.op_peers WHERE thread_id=$1")
                .bind(id)
                .fetch_one(&self.owner)
                .await
                .unwrap();
        assert_eq!(peers, 0);
    }

    async fn assert_erased(&self, ids: &[i64]) {
        let rows = self.posts().await;
        for id in ids {
            let row = rows.iter().find(|p| p["id"] == *id).unwrap();
            let mut expected = json!({"id":id,"board":self.board,"thread_id":row["thread_id"],"deleted":true,"content_erased":true,
                "name":"","subject":"","comment":"","created_at":"1970-01-01T00:00:00+00:00", "comment_format":0,
                "staff_authorized_limits":false,"image_spoiler":false,"board_flag_type":"pol"});
            for key in [
                "trip",
                "poster_id",
                "json_op_poster_id",
                "capcode",
                "country",
                "country_name",
                "board_flag",
                "flag_name",
                "wordfilter_payload",
                "wordfilter_search",
                "dice_result",
                "fortune_text",
                "fortune_color",
            ] {
                expected[key] = Value::Null;
            }
            assert_eq!(
                *row, expected,
                "Every retained post column must be explicitly justified for tombstone {id}"
            );
        }
        for table in [
            "post_secrets.deletion",
            "post_secrets.anonymous_posts",
            "post_secrets.op_replies",
            "post_secrets.poster_contexts",
            "post_secrets.posting_history",
            "staff_identity.discussion_posts",
        ] {
            // Identifiers come exclusively from the literal table allowlist
            // immediately above; all target IDs remain bound parameters.
            let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                "SELECT count(*) FROM {table} WHERE post_id=ANY($1)"
            )))
            .bind(ids)
            .fetch_one(&self.owner)
            .await
            .unwrap();
            assert_eq!(count, 0, "Target author authority remains in {table}");
        }
    }
}

fn post() -> NewPost {
    NewPost {
        name: "Anonymous".into(),
        subject: "Fresh erasure sentinel".into(),
        comment: "Fresh author and content sentinel".into(),
        deletion_hash: "fresh-erasure-password-sentinel".into(),
        sage: false,
    }
}
fn code(error: &sqlx::Error) -> String {
    error
        .as_database_error()
        .unwrap()
        .code()
        .unwrap()
        .into_owned()
}
async fn board_lock(tx: &mut Transaction<'_, Postgres>, board: &str) {
    sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
        .bind(board)
        .execute(&mut **tx)
        .await
        .unwrap();
}
async fn wait_behind(owner: &PgPool, waiter: i32, blocker: i32) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let blocked: bool = sqlx::query_scalar("SELECT $2=ANY(pg_blocking_pids($1))")
            .bind(waiter)
            .bind(blocker)
            .fetch_one(owner)
            .await
            .unwrap();
        if blocked {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "Did not observe expected lock dependency {waiter} -> {blocker}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn actual_roles_erase_all_payload_and_authority_transactionally_without_shared_state_loss() {
    for (staff, private) in [(false, false), (true, false), (true, true)] {
        let f = Fixture::new().await;
        let work = f.clone();
        let result = tokio::spawn(async move {
            let f = work;
            f.sentinels().await;
            if private {
                sqlx::query("UPDATE content.boards SET staff_only=true WHERE slug=$1")
                    .bind(&f.board)
                    .execute(&f.owner)
                    .await
                    .unwrap();
            }
            for table in [
                "post_secrets.deletion",
                "post_secrets.anonymous_posts",
                "post_secrets.posting_history",
                "staff_identity.discussion_posts",
            ] {
                // Only the fixed table allowlist above is interpolated.
                let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                    "SELECT count(*) FROM {table} WHERE post_id=ANY($1)"
                )))
                .bind(&f.ids)
                .fetch_one(&f.owner)
                .await
                .unwrap();
                assert_eq!(
                    count, 4,
                    "Coverage must start with real author rows in {table}"
                );
            }
            sqlx::query("UPDATE content.threads SET created_at='2001-02-03 04:05:06+00',bumped_at='2001-02-04 04:05:06+00',modified_at='2001-02-05 04:05:06+00',sticky=true,sticky_rank=60,closed=true,permasage=true,permaage=true,undead=true WHERE id=$1").bind(f.ids[0]).execute(&f.owner).await.unwrap();
            let before = f.posts().await;
            let authority = f.authority().await;
            assert_eq!(authority["deletion"].as_array().unwrap().len(), 4);
            assert_eq!(authority["anonymous"].as_array().unwrap().len(), 4);
            let shared = f.shared().await;
            let pool = if staff { &f.staff } else { &f.public };
            let mut tx = pool.begin().await.unwrap();
            board_lock(&mut tx, &f.board).await;
            sqlx::query("UPDATE content.posts SET deleted=true WHERE id=$1")
                .bind(f.ids[1])
                .execute(&mut *tx)
                .await
                .unwrap();
            let erased: bool = sqlx::query_scalar(
                "SELECT content_erased AND comment='' FROM content.posts WHERE id=$1",
            )
            .bind(f.ids[1])
            .fetch_one(&mut *tx)
            .await
            .unwrap();
            assert!(erased, "Erasure is synchronous within deletion transaction");
            assert_eq!(
                code(
                    &sqlx::query("SELECT 1/0")
                        .execute(&mut *tx)
                        .await
                        .unwrap_err()
                ),
                "22012"
            );
            tx.rollback().await.unwrap();
            assert_eq!(f.posts().await, before);
            assert_eq!(f.authority().await, authority);
            assert_eq!(f.shared().await, shared);
            let thread_before:Value=sqlx::query_scalar("SELECT to_jsonb(t) FROM content.threads t WHERE id=$1").bind(f.ids[0]).fetch_one(&f.owner).await.unwrap();
            let mut rollback=pool.begin().await.unwrap();
            board_lock(&mut rollback,&f.board).await;
            sqlx::query("UPDATE content.threads SET deleted=true WHERE id=$1").bind(f.ids[0]).execute(&mut *rollback).await.unwrap();
            let count:i64=sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE thread_id=$1 AND content_erased").bind(f.ids[0]).fetch_one(&mut *rollback).await.unwrap();
            assert_eq!(count,3,"Whole-thread descendants erase synchronously");
            rollback.rollback().await.unwrap();
            assert_eq!(f.posts().await,before);
            assert_eq!(f.authority().await,authority);
            assert_eq!(f.shared().await,shared);
            let thread_after:Value=sqlx::query_scalar("SELECT to_jsonb(t) FROM content.threads t WHERE id=$1").bind(f.ids[0]).fetch_one(&f.owner).await.unwrap();
            assert_eq!(thread_after,thread_before,"Thread canonicalization and descendant erasure roll back together");
            let mut tx = pool.begin().await.unwrap();
            board_lock(&mut tx, &f.board).await;
            sqlx::query("UPDATE content.posts SET deleted=true WHERE id=$1")
                .bind(f.ids[1])
                .execute(&mut *tx)
                .await
                .unwrap();
            tx.commit().await.unwrap();
            f.assert_erased(&[f.ids[1]]).await;
            assert_eq!(f.posts().await[3], before[3]);
            assert_eq!(f.shared().await, shared);
            // Whole-thread SQL is the rollover/expiry entry point; it must catch
            // children even when no individual post UPDATE is issued by Rust.
            let mut tx = pool.begin().await.unwrap();
            board_lock(&mut tx, &f.board).await;
            sqlx::query("UPDATE content.threads SET deleted=true WHERE id=$1")
                .bind(f.ids[0])
                .execute(&mut *tx)
                .await
                .unwrap();
            tx.commit().await.unwrap();
            f.assert_erased(&f.ids[..3]).await;
            f.assert_thread_erased(f.ids[0]).await;
            assert_eq!(f.posts().await[3], before[3]);
            assert_eq!(f.shared().await, shared);
            let remaining = f.authority().await;
            for key in ["deletion", "anonymous", "history"] {
                assert_eq!(
                    remaining[key].as_array().unwrap().len(),
                    1,
                    "Only unrelated target retains {key}"
                );
            }
        })
        .await;
        f.cleanup().await;
        result.unwrap();
    }
}

#[tokio::test]
async fn tombstones_cannot_resurrect_repopulate_or_regain_authority_and_live_empty_text_still_fails()
 {
    let f = Fixture::new().await;
    let work = f.clone();
    let result=tokio::spawn(async move {
        let f=work;
        board_store::delete_post(&f.public,&f.board,f.ids[0]).await.unwrap();
        f.assert_erased(&f.ids[..3]).await;
            f.assert_thread_erased(f.ids[0]).await;
        for query in ["UPDATE content.posts SET deleted=false WHERE id=$1", "UPDATE content.posts SET content_erased=false WHERE id=$1", "UPDATE content.posts SET comment='resurrected' WHERE id=$1", "UPDATE content.posts SET name='resurrected' WHERE id=$1", "UPDATE content.posts SET wordfilter_search='resurrected' WHERE id=$1", "UPDATE content.threads SET deleted=false WHERE id=$1", "UPDATE content.threads SET content_erased=false WHERE id=$1", "UPDATE content.threads SET bumped_at=clock_timestamp() WHERE id=$1"] {
            let error=sqlx::query(query).bind(f.ids[0]).execute(&f.owner).await.unwrap_err();
            assert_eq!(code(&error),"23514","{query}");
        }
        for pool in [&f.public,&f.staff] {
            let error=sqlx::query("UPDATE content.posts SET content_erased=false WHERE id=$1").bind(f.ids[0]).execute(pool).await.unwrap_err();
            assert_eq!(code(&error),"42501");
        }
        for pool in [&f.public,&f.owner] {
            let error=sqlx::query("INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES($1,'reintroduced')").bind(f.ids[0]).execute(pool).await.unwrap_err();
            assert_eq!(code(&error),"23514");
        }
        let retained_authority=f.authority().await;
        // Only these four literal relation names enter SQL. Target IDs are
        // bound, and neither fixture data nor user input supplies identifiers.
        for table in ["post_secrets.deletion","post_secrets.anonymous_posts","post_secrets.posting_history","staff_identity.discussion_posts"] {
            let query=format!("INSERT INTO {table} SELECT (jsonb_populate_record(NULL::{table},to_jsonb(a)||jsonb_build_object('post_id',$1::bigint))).* FROM {table} a WHERE post_id=$2");
            let error=sqlx::query(sqlx::AssertSqlSafe(query.as_str())).bind(f.ids[1]).bind(f.ids[3]).execute(&f.owner).await.unwrap_err();
            assert_eq!(code(&error),"23514","Cannot recreate target-specific authority in {table}");
            let query=format!("UPDATE {table} SET post_id=$1 WHERE post_id=$2");
            let error=sqlx::query(sqlx::AssertSqlSafe(query.as_str())).bind(f.ids[1]).bind(f.ids[3]).execute(&f.owner).await.unwrap_err();
            assert_eq!(code(&error),"23514","Cannot reassign live authority into erased target in {table}");
        }
        assert_eq!(f.authority().await,retained_authority,"Denied resurrection does not damage unrelated author authority");
        sqlx::query("UPDATE content.posts SET comment='' WHERE id=$1").bind(f.ids[3]).execute(&f.owner).await.unwrap();
        assert_eq!(f.posts().await[3]["subject"],"Fresh erasure sentinel","Live subject-only OP remains valid under0032");
        let error=sqlx::query("UPDATE content.posts SET comment='',subject='' WHERE id=$1").bind(f.ids[3]).execute(&f.owner).await.unwrap_err();
        assert_eq!(code(&error),"23514","Live text-only OP still requires a subject or comment");
        let live_reply=support::create_post(&f.public,&f.board,f.ids[3],&post()).await.unwrap();
        let error=sqlx::query("UPDATE content.posts SET comment='' WHERE id=$1").bind(live_reply).execute(&f.owner).await.unwrap_err();
        assert_eq!(code(&error),"23514","A live subject-bearing reply still needs comment or attachment");
        assert!(!f.posts().await[3]["content_erased"].as_bool().unwrap());
        // Repeated deletion cannot erase the independent remaining thread.
        sqlx::query("UPDATE content.posts SET deleted=true WHERE id=$1").bind(f.ids[1]).execute(&f.public).await.unwrap();
        f.assert_erased(&f.ids[..3]).await;
            f.assert_thread_erased(f.ids[0]).await;
    }).await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn retained_archive_preserves_payload_but_rollover_and_expiry_erase_descendants() {
    for retention in [0, 3600] {
        let f = Fixture::new().await;
        let work = f.clone();
        let result=tokio::spawn(async move {
            let f=work;
            let before=f.posts().await;
            // Protect the unrelated control thread from capacity selection.
            sqlx::query("UPDATE content.threads SET sticky=true WHERE id=$1").bind(f.ids[3]).execute(&f.owner).await.unwrap();
            sqlx::query("UPDATE content.boards SET thread_limit=1,archive_retention_seconds=$2 WHERE slug=$1").bind(&f.board).bind(retention).execute(&f.owner).await.unwrap();
            support::create_post(&f.public,&f.board,0,&post()).await.unwrap();
            if retention>0 {
                assert_eq!(&f.posts().await[..4],&before[..],"Retained archives preserve saved payload and identities");
                sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp()-interval '2 hours',archive_expires_at=clock_timestamp()-interval '1 hour' WHERE id=$1").bind(f.ids[0]).execute(&f.owner).await.unwrap();
                support::create_post(&f.public,&f.board,0,&post()).await.unwrap();
            }
            f.assert_erased(&f.ids[..3]).await;
            f.assert_thread_erased(f.ids[0]).await;
            assert_eq!(f.posts().await[3],before[3]);
        }).await;
        f.cleanup().await;
        result.unwrap();
    }
}

#[tokio::test]
async fn queued_secret_insert_observes_committed_deletion_and_fixed_snapshots_fail_closed() {
    for repeatable in [false, true] {
        let f = Fixture::new().await;
        let work = f.clone();
        let result=tokio::spawn(async move {
            let f=work;
            sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id=$1").bind(f.ids[1]).execute(&f.owner).await.unwrap();
            let mut writer=f.public.begin().await.unwrap();
            if repeatable {sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ").execute(&mut *writer).await.unwrap();}
            let waiter:i32=sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *writer).await.unwrap();
            let live:bool=sqlx::query_scalar("SELECT NOT deleted FROM content.posts WHERE id=$1").bind(f.ids[1]).fetch_one(&mut *writer).await.unwrap();
            assert!(live);
            let mut deletion=f.public.begin().await.unwrap();
            let blocker:i32=sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *deletion).await.unwrap();
            board_lock(&mut deletion,&f.board).await;
            sqlx::query("UPDATE content.posts SET deleted=true WHERE id=$1").bind(f.ids[1]).execute(&mut *deletion).await.unwrap();
            let id=f.ids[1];
            let queued=tokio::spawn(async move {
                let error=sqlx::query("INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES($1,'queued-authority')").bind(id).execute(&mut *writer).await.unwrap_err();
                writer.rollback().await.unwrap();
                error
            });
            wait_behind(&f.owner,waiter,blocker).await;
            deletion.commit().await.unwrap();
            let error=tokio::time::timeout(Duration::from_secs(5),queued).await.unwrap().unwrap();
            if repeatable {
                assert!(matches!(code(&error).as_str(), "22023" | "40001"), "Fixed snapshots must fail closed: {error}");
            } else { assert_eq!(code(&error), "23514"); }
            f.assert_erased(&[id]).await;
        }).await;
        f.cleanup().await;
        result.unwrap();
    }
}

#[tokio::test]
async fn actual_runtime_logins_satisfy_shared_content_erasure_readiness() {
    for variable in ["TEST_PUBLIC_DATABASE_URL", "STAFF_DATABASE_URL"] {
        let pool = PgPool::connect(&std::env::var(variable).unwrap())
            .await
            .unwrap();
        let ready: bool = sqlx::query_scalar(board_store::content_erasure::READINESS_SQL)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(ready, "{variable}");
        for function in [
            "SELECT post_secrets.guard_post_erasure()",
            "SELECT post_secrets.guard_thread_erasure()",
            "SELECT post_secrets.erase_thread_descendants()",
            "SELECT post_secrets.guard_erased_author_link()",
        ] {
            let error = sqlx::query(function).execute(&pool).await.unwrap_err();
            assert_eq!(
                code(&error),
                "42501",
                "Runtime cannot invoke private erasure function {function}"
            );
        }
        let error = sqlx::query("SET ROLE board_content_erasure_owner")
            .execute(&pool)
            .await
            .unwrap_err();
        assert_eq!(code(&error), "42501", "Runtime cannot become erasure owner");
        let narrow:bool=sqlx::query_scalar("SELECT count(*)=5 AND bool_and(CASE WHEN forbidden.column_name IS NULL THEN NOT has_table_privilege('board_content_erasure_owner',c.oid,'DELETE') ELSE NOT has_column_privilege('board_content_erasure_owner',c.oid,a.attnum,'SELECT') END) FROM (VALUES ('content','posts','comment'),('post_secrets','deletion','password_hash'),('post_secrets','anonymous_posts','password_proof'),('content','post_media',NULL),('post_secrets','anonymous_sessions',NULL)) forbidden(schema_name,table_name,column_name) JOIN pg_catalog.pg_namespace n ON n.nspname=forbidden.schema_name JOIN pg_catalog.pg_class c ON c.relnamespace=n.oid AND c.relname=forbidden.table_name LEFT JOIN pg_catalog.pg_attribute a ON a.attrelid=c.oid AND a.attname=forbidden.column_name AND NOT a.attisdropped").fetch_one(&pool).await.unwrap();
        assert!(
            narrow,
            "Erasure role needs target identifiers and removal rights, not payload or shared-state authority"
        );
    }
}

#[tokio::test]
async fn deletion_rejects_fixed_snapshots_and_secret_rotation_cannot_join_a_board_lock_cycle() {
    let f = Fixture::new().await;
    let work = f.clone();
    let result=tokio::spawn(async move {
        let f=work;
        let before=f.posts().await;
        let authority=f.authority().await;
        for isolation in ["SET TRANSACTION ISOLATION LEVEL REPEATABLE READ","SET TRANSACTION ISOLATION LEVEL SERIALIZABLE"] {
            for statement in ["UPDATE content.posts SET deleted=true WHERE id=$1","UPDATE content.threads SET deleted=true WHERE id=$1"] {
                let mut tx=f.public.begin().await.unwrap();
                sqlx::query(isolation).execute(&mut *tx).await.unwrap();
                board_lock(&mut tx,&f.board).await;
                let error=sqlx::query(statement).bind(f.ids[0]).execute(&mut *tx).await.unwrap_err();
                assert_eq!(code(&error),"22023");
                tx.rollback().await.unwrap();
            }
        }
        assert_eq!(f.posts().await,before);
        assert_eq!(f.authority().await,authority);
        // Lock inversion must fail immediately, not deadlock erasure's
        // board -> post -> author-row order against rotation's author tuple.
        let mut rotation=f.owner.begin().await.unwrap();
        let rotator:i32=sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *rotation).await.unwrap();
        sqlx::query("SELECT post_id FROM post_secrets.deletion WHERE post_id=$1 FOR UPDATE").bind(f.ids[1]).execute(&mut *rotation).await.unwrap();
        let mut deletion=f.public.begin().await.unwrap();
        let eraser:i32=sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *deletion).await.unwrap();
        board_lock(&mut deletion,&f.board).await;
        let id=f.ids[1];
        let queued=tokio::spawn(async move {
            sqlx::query("UPDATE content.posts SET deleted=true WHERE id=$1").bind(id).execute(&mut *deletion).await.unwrap();
            deletion.commit().await.unwrap();
        });
        wait_behind(&f.owner,eraser,rotator).await;
        let error=tokio::time::timeout(Duration::from_secs(3),sqlx::query("UPDATE post_secrets.deletion SET password_hash='rotation-after-deletion' WHERE post_id=$1").bind(id).execute(&mut *rotation)).await.expect("Rotation must not wait into a deadlock").unwrap_err();
        assert_eq!(code(&error),"55P03");
        rotation.rollback().await.unwrap();
        tokio::time::timeout(Duration::from_secs(5),queued).await.unwrap().unwrap();
        f.assert_erased(&[id]).await;
    }).await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn queued_reply_cannot_repopulate_a_thread_erased_while_waiting() {
    let f = Fixture::new().await;
    let work = f.clone();
    let result = tokio::spawn(async move {
        let f = work;
        let one = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let waiter: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&one)
            .await
            .unwrap();
        let mut deletion = f.public.begin().await.unwrap();
        let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *deletion)
            .await
            .unwrap();
        board_lock(&mut deletion, &f.board).await;
        sqlx::query("UPDATE content.threads SET deleted=true WHERE id=$1")
            .bind(f.ids[0])
            .execute(&mut *deletion)
            .await
            .unwrap();
        let board = f.board.clone();
        let parent = f.ids[0];
        let queued = tokio::spawn(async move {
            let result = support::create_post(&one, &board, parent, &post()).await;
            one.close().await;
            result
        });
        wait_behind(&f.owner, waiter, blocker).await;
        deletion.commit().await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(5), queued)
                .await
                .unwrap()
                .unwrap()
                .is_err()
        );
        f.assert_erased(&f.ids[..3]).await;
        f.assert_thread_erased(f.ids[0]).await;
        assert_eq!(
            f.posts().await.len(),
            4,
            "Rejected queued posting leaves no new payload or tombstone"
        );
    })
    .await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn queued_author_link_revalidates_post_board_mapping_after_its_lock_wait() {
    let source = Fixture::new().await;
    let destination = Fixture::new().await;
    let a = source.clone();
    let b = destination.clone();
    let result = tokio::spawn(async move {
        // The independent op_replies foreign keys permit this direct runtime
        // insertion; erasure's guard must bind its decision to the board it
        // actually locked, including after a concurrent migrator reassignment.
        sqlx::query("INSERT INTO post_secrets.op_peers(thread_id,peer) VALUES($1,'192.0.2.201') ON CONFLICT(thread_id) DO NOTHING")
            .bind(a.ids[0]).execute(&a.owner).await.unwrap();
        for move_target in [false, true] {
            // A legitimate bare import has no posting_history composite FK.
            // Do not disable guards or erase existing author links to arrange
            // this race. The migrator's ordinary insertion path stays enabled.
            let reply:i64=sqlx::query_scalar("INSERT INTO content.posts(board,thread_id,name,subject,comment) VALUES($1,$2,'Anonymous','Owned mapping fixture','Mapping race sentinel') RETURNING id")
                .bind(&a.board).bind(a.ids[0]).fetch_one(&a.owner).await.unwrap();
            let history:i64=sqlx::query_scalar("SELECT count(*) FROM post_secrets.posting_history WHERE post_id=$1")
                .bind(reply).fetch_one(&a.owner).await.unwrap();
            assert_eq!(history,0,"Bare imports do not manufacture author identity");
            let mut mover=a.owner.begin().await.unwrap();
            let blocker:i32=sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *mover).await.unwrap();
            sqlx::query("SELECT slug FROM content.boards WHERE slug=ANY($1) ORDER BY slug FOR UPDATE")
                .bind([a.board.clone(),b.board.clone()].as_slice()).fetch_all(&mut *mover).await.unwrap();
            if move_target {
                sqlx::query("UPDATE content.posts SET board=$2,thread_id=$3 WHERE id=$1")
                    .bind(reply).bind(&b.board).bind(b.ids[0]).execute(&mut *mover).await.unwrap();
            }
            let mut writer=a.public.begin().await.unwrap();
            let waiter:i32=sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *writer).await.unwrap();
            let source_thread=a.ids[0];
            let queued=tokio::spawn(async move {
                let inserted=sqlx::query("INSERT INTO post_secrets.op_replies(post_id,thread_id) VALUES($1,$2)")
                    .bind(reply).bind(source_thread).execute(&mut *writer).await;
                match inserted {
                    Ok(_) => {writer.commit().await.unwrap(); Ok(())}
                    Err(error) => {writer.rollback().await.unwrap(); Err(error)}
                }
            });
            wait_behind(&a.owner,waiter,blocker).await;
            mover.commit().await.unwrap();
            let inserted=tokio::time::timeout(Duration::from_secs(5),queued).await.unwrap().unwrap();
            if move_target {
                assert_eq!(code(&inserted.unwrap_err()),"23514","A lock on the old board cannot authorize the moved target");
            } else {
                inserted.unwrap();
            }
            let saved:Option<i64>=sqlx::query_scalar("SELECT thread_id FROM post_secrets.op_replies WHERE post_id=$1")
                .bind(reply).fetch_optional(&a.owner).await.unwrap();
            assert_eq!(saved,if move_target {None} else {Some(source_thread)},"Unchanged mapping succeeds; stale mapping leaves no authority row");
            let mapping:(String,i64,bool)=sqlx::query_as("SELECT board,thread_id,content_erased FROM content.posts WHERE id=$1")
                .bind(reply).fetch_one(&a.owner).await.unwrap();
            let expected=if move_target {(&b.board,b.ids[0])} else {(&a.board,a.ids[0])};
            assert_eq!(mapping,(expected.0.clone(),expected.1,false),"Rejected authority registration does not alter live content");
        }
    }).await;
    destination.cleanup().await;
    source.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn migrator_cannot_reassign_content_identity_but_unchanged_identity_updates_work() {
    let source = Fixture::new().await;
    let destination = Fixture::new().await;
    let a = source.clone();
    let b = destination.clone();
    let result=tokio::spawn(async move {
        // An empty thread and bare imported post ensure these are guard
        // failures, not foreign-key accidents on existing dependent rows.
        let thread:i64=sqlx::query_scalar("INSERT INTO content.threads(board) VALUES($1) RETURNING id")
            .bind(&a.board).fetch_one(&a.owner).await.unwrap();
        let reply:i64=sqlx::query_scalar("INSERT INTO content.posts(board,thread_id,name,subject,comment) VALUES($1,$2,'Anonymous','','Immutable identity sentinel') RETURNING id")
            .bind(&a.board).bind(a.ids[0]).fetch_one(&a.owner).await.unwrap();
        let unused:i64=sqlx::query_scalar("SELECT nextval('content.post_number')").fetch_one(&a.owner).await.unwrap();
        let error=sqlx::query("UPDATE content.threads SET board=$2 WHERE id=$1")
            .bind(thread).bind(&b.board).execute(&a.owner).await.unwrap_err();
        assert_eq!(code(&error),"23514");
        assert_eq!(error.as_database_error().unwrap().message(),"Thread identity cannot be reassigned.");
        let error=sqlx::query("UPDATE content.threads SET id=$2 WHERE id=$1")
            .bind(thread).bind(unused).execute(&a.owner).await.unwrap_err();
        assert_eq!(code(&error),"23514");
        assert_eq!(error.as_database_error().unwrap().message(),"Thread identity cannot be reassigned.");
        let error=sqlx::query("UPDATE content.posts SET id=$2 WHERE id=$1")
            .bind(reply).bind(unused).execute(&a.owner).await.unwrap_err();
        assert_eq!(code(&error),"23514");
        // UPDATE(id) grants are also used for row-lock privileges. A no-op
        // identity update remains allowed and must leave content live.
        let mut tx=a.owner.begin().await.unwrap();
        board_lock(&mut tx,&a.board).await;
        sqlx::query("SELECT id FROM content.threads WHERE id=$1 FOR UPDATE")
            .bind(thread).fetch_one(&mut *tx).await.unwrap();
        assert_eq!(sqlx::query("UPDATE content.threads SET id=id WHERE id=$1").bind(thread).execute(&mut *tx).await.unwrap().rows_affected(),1);
        assert_eq!(sqlx::query("UPDATE content.posts SET id=id WHERE id=$1").bind(reply).execute(&mut *tx).await.unwrap().rows_affected(),1);
        tx.commit().await.unwrap();
        let thread_mapping:(String,bool)=sqlx::query_as("SELECT board,content_erased FROM content.threads WHERE id=$1")
            .bind(thread).fetch_one(&a.owner).await.unwrap();
        assert_eq!(thread_mapping,(a.board.clone(),false));
        let post_mapping:(String,i64,String,bool)=sqlx::query_as("SELECT board,thread_id,comment,content_erased FROM content.posts WHERE id=$1")
            .bind(reply).fetch_one(&a.owner).await.unwrap();
        assert_eq!(post_mapping,(a.board.clone(),a.ids[0],"Immutable identity sentinel".into(),false));
    }).await;
    destination.cleanup().await;
    source.cleanup().await;
    result.unwrap();
}
