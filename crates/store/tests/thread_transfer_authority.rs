#![cfg(feature = "database-tests")]
//! Synthetic, owned transfer fixtures. Runtime calls never expose private proof.
//! No grants, trigger changes, imported-data writes, retries, or global resets.
mod support;

use board_domain::anonymous_session::Capability;
use board_store::{
    AnonymousPostingContext, NewPost, PostIdentityKeys, PostMetadata, PostingContext,
    anonymous_session::PostingSession,
    thread_transfer::{self as transfer, Credentials, PreparedTransfer, Ticket, TransferError},
};
use chrono::Utc;
use sqlx::{PgPool, Postgres, Transaction};
use std::{
    future::Future,
    sync::{Arc, Mutex},
    time::Duration,
};

// The production pending limit is two globally, one per account. Deliberate
// contention is inside a test; normal cases must not race one another.
static TEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone)]
struct Fixture {
    owner: PgPool,
    public: PgPool,
    staff: PgPool,
    auth: PgPool,
    boards: [String; 2],
    account: i64,
    credential: Vec<u8>,
    session: [u8; 32],
    csrf: [u8; 32],
    tokens: Arc<Mutex<Vec<[u8; 32]>>>,
}

async fn pool(name: &str, expected: &str) -> PgPool {
    let pool = PgPool::connect(&std::env::var(name).expect("explicit test database URL required"))
        .await
        .unwrap();
    let role: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(role, expected);
    pool
}

impl Fixture {
    async fn new() -> Self {
        let owner = pool("MIGRATION_DATABASE_URL", "board_migrator").await;
        let seed: String = sqlx::query_scalar("SELECT replace(gen_random_uuid()::text,'-','')")
            .fetch_one(&owner)
            .await
            .unwrap();
        let boards = [format!("x{}", &seed[..9]), format!("y{}", &seed[..9])];
        for board in &boards {
            sqlx::query("INSERT INTO content.boards(captcha_enabled,slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,posting_reply_seconds,posting_image_seconds,posting_thread_seconds) VALUES(false,$1,'Owned transfer authority','Synthetic fixture',2000,100,100,100,10,0,0,0)")
                .bind(board).execute(&owner).await.unwrap();
        }
        let account = sqlx::query_scalar("INSERT INTO staff_identity.accounts(role,flags) VALUES('moderator',ARRAY['capcode','capcodename']) RETURNING id")
            .fetch_one(&owner).await.unwrap();
        let credential = seed.into_bytes();
        sqlx::query(
            "INSERT INTO staff_identity.credentials(id,account_id,credential) VALUES($1,$2,'{}')",
        )
        .bind(&credential)
        .bind(account)
        .execute(&owner)
        .await
        .unwrap();
        let (session, csrf): (Vec<u8>, Vec<u8>) = sqlx::query_as("SELECT sha256(convert_to(gen_random_uuid()::text,'UTF8')),sha256(convert_to(gen_random_uuid()::text,'UTF8'))")
            .fetch_one(&owner).await.unwrap();
        sqlx::query("INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id) VALUES($1,$2,$3,$4)")
            .bind(&session).bind(&csrf).bind(account).bind(&credential).execute(&owner).await.unwrap();
        Self {
            public: pool("TEST_PUBLIC_DATABASE_URL", "board_public").await,
            staff: pool("STAFF_DATABASE_URL", "board_staff").await,
            auth: pool("AUTH_DATABASE_URL", "board_auth").await,
            owner,
            boards,
            account,
            credential,
            session: session.try_into().unwrap(),
            csrf: csrf.try_into().unwrap(),
            tokens: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn credentials(&self) -> Credentials {
        Credentials {
            session_hash: self.session,
            csrf_hash: self.csrf,
            idle_seconds: 900,
        }
    }

    fn capability(&self) -> Capability {
        let cap = Capability::generate().unwrap();
        self.tokens.lock().unwrap().push(cap.storage_hash());
        cap
    }

    async fn post(&self, board: usize, parent: i64, cap: Option<&Capability>) -> i64 {
        let post = NewPost {
            name: "Anonymous".into(),
            subject: "Owned transfer".into(),
            comment: format!("Owned text >>{parent}"),
            deletion_hash: "synthetic-transfer-deletion-hash".into(),
            sage: false,
        };
        let context = PostingContext {
            request_start: Utc::now(),
            peer: Some(support::peer()),
            op_password_proof: None,
        };
        if let Some(cap) = cap {
            support::create_post_with_anonymous_session(
                &self.public,
                &self.boards[board],
                parent,
                &post,
                None,
                AnonymousPostingContext {
                    posting: context,
                    session: PostingSession {
                        fingerprints: cap.fingerprints(Some(support::peer()), *b"US"),
                        minted: true,
                        now: Utc::now(),
                    },
                },
                PostMetadata {
                    event_scores: false,
                    drawing: None,
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
            .unwrap()
        } else {
            support::create_post_with_context(
                &self.public,
                &self.boards[board],
                parent,
                &post,
                None,
                context,
            )
            .await
            .unwrap()
        }
    }

    async fn prepared(&self, source: i64) -> (Ticket, PreparedTransfer) {
        let ticket = Ticket::from_bytes(Capability::generate().unwrap().storage_hash());
        let prepared = transfer::prepare(
            &self.auth,
            &self.credentials(),
            &ticket,
            &self.boards[0],
            source,
            &self.boards[1],
        )
        .await
        .unwrap();
        (ticket, prepared)
    }

    async fn sealed(&self, source: i64) -> (Ticket, PreparedTransfer) {
        let (ticket, prepared) = self.prepared(source).await;
        transfer::seal(&self.auth, &self.credentials(), &prepared)
            .await
            .unwrap();
        (ticket, prepared)
    }

    async fn private(&self) -> Transaction<'static, Postgres> {
        let mut tx = self.owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_thread_transfer_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        tx
    }

    async fn snapshot(&self) -> serde_json::Value {
        sqlx::query_scalar("SELECT jsonb_build_object('posts',(SELECT coalesce(jsonb_agg(to_jsonb(p) ORDER BY id),'[]') FROM content.posts p WHERE board=ANY($1)),'threads',(SELECT coalesce(jsonb_agg(to_jsonb(t) ORDER BY id),'[]') FROM content.threads t WHERE board=ANY($1)),'history',(SELECT coalesce(jsonb_agg(to_jsonb(h) ORDER BY post_id),'[]') FROM post_secrets.posting_history h WHERE board=ANY($1)),'actions',(SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY board,actor_hash),'[]') FROM post_secrets.posting_thread_actions a WHERE board=ANY($1)),'audit',(SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY id),'[]') FROM content.moderation_audit a WHERE board=ANY($1)))")
            .bind(&self.boards[..]).fetch_one(&self.owner).await.unwrap()
    }

    async fn pending(&self) -> serde_json::Value {
        let mut tx = self.private().await;
        let result = sqlx::query_scalar("SELECT jsonb_build_object('intent',(SELECT coalesce(jsonb_agg(to_jsonb(i)),'[]') FROM post_secrets.thread_transfer_intents i WHERE account_id=$1),'map',(SELECT coalesce(jsonb_agg(to_jsonb(m) ORDER BY ordinal),'[]') FROM post_secrets.thread_transfer_map m JOIN post_secrets.thread_transfer_intents i USING(ticket_hash) WHERE i.account_id=$1),'sealed',(SELECT coalesce(jsonb_agg(to_jsonb(s) ORDER BY ordinal),'[]') FROM post_secrets.thread_transfer_sealed_rows s JOIN post_secrets.thread_transfer_intents i USING(ticket_hash) WHERE i.account_id=$1),'active',(SELECT count(*) FROM post_secrets.thread_transfer_active WHERE board=ANY($2)))")
            .bind(self.account).bind(&self.boards[..]).fetch_one(&mut *tx).await.unwrap();
        tx.rollback().await.unwrap();
        result
    }

    async fn cleanup(&self) {
        // Fault index is restricted to this random owned board and is never a
        // runtime grant or a trigger/readiness bypass.
        assert_owned_identifier(&self.boards[0]);
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP INDEX IF EXISTS content.{}_transfer_audit_refusal",
            self.boards[0]
        )))
        .execute(&self.owner)
        .await
        .unwrap();
        // Only this account's work is removed, under the production gate.
        let mut tx = self.private().await;
        sqlx::query("SELECT singleton FROM post_secrets.thread_transfer_gate FOR UPDATE")
            .fetch_all(&mut *tx)
            .await
            .unwrap();
        sqlx::query("DELETE FROM post_secrets.thread_transfer_intents WHERE account_id=$1")
            .bind(self.account)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("DELETE FROM post_secrets.thread_transfer_result_posts WHERE request_id IN(SELECT request_id FROM post_secrets.thread_transfer_results WHERE account_id=$1)").bind(self.account).execute(&mut *tx).await.unwrap();
        sqlx::query("DELETE FROM post_secrets.thread_transfer_results WHERE account_id=$1")
            .bind(self.account)
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let tokens = self.tokens.lock().unwrap().clone();
        let mut tx = support::begin_cleanup_with_sessions(&self.owner, &self.boards, &tokens).await;
        for query in [
            "DELETE FROM content.moderation_audit WHERE board=ANY($1)",
            "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=ANY($1))",
            "DELETE FROM content.posts WHERE board=ANY($1)",
            "DELETE FROM content.threads WHERE board=ANY($1)",
            "DELETE FROM post_secrets.posting_thread_actions WHERE board=ANY($1)",
            "DELETE FROM content.boards WHERE slug=ANY($1)",
        ] {
            sqlx::query(sqlx::AssertSqlSafe(query))
                .bind(&self.boards[..])
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
        sqlx::query("DELETE FROM verification_private.identities WHERE email_hash=$1")
            .bind(self.session.as_slice())
            .execute(&mut *tx)
            .await
            .unwrap();
        for query in [
            "DELETE FROM staff_identity.sessions WHERE account_id=$1",
            "DELETE FROM staff_identity.credentials WHERE account_id=$1",
            "DELETE FROM staff_identity.accounts WHERE id=$1",
        ] {
            sqlx::query(sqlx::AssertSqlSafe(query))
                .bind(self.account)
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        tx.commit().await.unwrap();
    }
}

async fn run<F, Fut>(test: F)
where
    F: FnOnce(Fixture) -> Fut,
    Fut: Future<Output = ()> + Send + 'static,
{
    let _serial = TEST.lock().await;
    let fixture = Fixture::new().await;
    let outcome = tokio::spawn(test(fixture.clone())).await;
    fixture.cleanup().await;
    outcome.unwrap();
}

// Dynamic SQL is limited to static table names or random owned fixture slugs.
// Values otherwise use binds; no SQL punctuation or quoted data is accepted.
fn assert_owned_identifier(identifier: &str) {
    assert!(!identifier.is_empty());
    assert!(
        identifier
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    );
}

fn denied(error: sqlx::Error) {
    assert_eq!(
        error.as_database_error().and_then(|e| e.code()).as_deref(),
        Some("42501")
    );
}

#[tokio::test]
async fn raw_runtime_roles_cannot_read_write_private_work_or_cross_issuer_boundary() {
    run(|f| async move {
        for role in [&f.public, &f.staff, &f.auth] {
            for table in ["thread_transfer_gate", "thread_transfer_intents", "thread_transfer_map", "thread_transfer_sealed_rows", "thread_transfer_active", "thread_transfer_results", "thread_transfer_result_posts", "posting_history", "comment_history", "anonymous_posts"] {
                assert_owned_identifier(table);
                denied(sqlx::query(sqlx::AssertSqlSafe(format!("SELECT * FROM post_secrets.{table} LIMIT 0"))).fetch_all(role).await.unwrap_err());
                denied(sqlx::query(sqlx::AssertSqlSafe(format!("DELETE FROM post_secrets.{table} WHERE false"))).execute(role).await.unwrap_err());
                denied(sqlx::query(sqlx::AssertSqlSafe(format!("INSERT INTO post_secrets.{table} DEFAULT VALUES"))).execute(role).await.unwrap_err());
            }
            denied(sqlx::query("SET ROLE board_thread_transfer_owner").execute(role).await.unwrap_err());
        }
        for role in [&f.public, &f.staff] {
            denied(sqlx::query("SELECT * FROM staff_identity.prepare_text_thread_transfer(NULL,NULL,NULL,900,'x',1,'y',1::smallint)").fetch_all(role).await.unwrap_err());
            denied(sqlx::query("SELECT staff_identity.seal_text_thread_transfer(NULL,NULL,NULL,NULL,1::smallint,NULL,NULL,NULL,NULL,NULL)").execute(role).await.unwrap_err());
            denied(sqlx::query("SELECT staff_identity.seal_text_thread_transfer_comparisons(NULL,NULL,NULL,NULL,1::smallint,NULL,NULL,NULL,NULL,NULL,NULL)").execute(role).await.unwrap_err());
        }
        for role in [&f.public, &f.auth] {
            denied(sqlx::query("SELECT * FROM content.consume_text_thread_transfer(NULL)").fetch_all(role).await.unwrap_err());
        }
    }).await;
}

#[tokio::test]
async fn successful_move_preserves_exact_private_evidence_and_old_registration_xid() {
    run(|f| async move {
        let cap = f.capability();
        let source = f.post(0, 0, Some(&cap)).await;
        let reply = f.post(0, source, None).await;
        sqlx::query("UPDATE content.posts SET since4pass=CASE WHEN id=$1 THEN 2020 ELSE 2008 END,event_perk=CASE WHEN id=$1 THEN 1 ELSE 231 END WHERE id=ANY($2)")
            .bind(source).bind(&[source,reply][..]).execute(&f.owner).await.unwrap();
        let mut membership=f.owner.begin().await.unwrap();sqlx::query("SET LOCAL ROLE board_anonymous_owner").execute(&mut *membership).await.unwrap();
        sqlx::query("INSERT INTO verification_private.pass_posts VALUES($1,$2),($3,$4)")
            .bind(source).bind(vec![0x41u8;32]).bind(reply).bind(vec![0x42u8;32]).execute(&mut *membership).await.unwrap();membership.commit().await.unwrap();
        // Synthetic retained evidence isolates transfer from capture. Runtime
        // capture, including badged staff, has a separate posting regression.
        sqlx::query("UPDATE post_secrets.posting_history SET automatic_browser='1deadbeef',automatic_prefix=decode(repeat('12',32),'hex') WHERE post_id=$1")
            .bind(source).execute(&f.owner).await.unwrap();
        sqlx::query("UPDATE post_secrets.posting_history SET automatic_browser='1cafebabe',automatic_prefix=decode(repeat('34',32),'hex') WHERE post_id=$1")
            .bind(reply).execute(&f.owner).await.unwrap();
        let before: serde_json::Value = sqlx::query_scalar("SELECT jsonb_build_object('deletion',(SELECT password_hash FROM post_secrets.deletion WHERE post_id=$1),'peer',(SELECT peer::text FROM post_secrets.op_peers WHERE thread_id=$1),'history',(SELECT to_jsonb(h)-'post_id'-'board'-'thread_id' FROM post_secrets.posting_history h WHERE post_id=$1),'anonymous',(SELECT to_jsonb(a)-'post_id' FROM post_secrets.anonymous_posts a WHERE post_id=$1),'session',(SELECT to_jsonb(s) FROM post_secrets.anonymous_sessions s WHERE token_hash=$2),'reply',(SELECT count(*) FROM post_secrets.op_replies WHERE post_id=$3 AND thread_id=$1))")
            .bind(source).bind(cap.storage_hash().as_slice()).bind(reply).fetch_one(&f.owner).await.unwrap();
        assert!(before["peer"].is_string());
        assert!(!before["history"]["registration_xid"].is_null());
        assert!(before["history"]["automatic_identity"].is_string());
        assert_eq!(before["reply"], 1);
        let (ticket, prepared) = f.sealed(source).await;
        assert_eq!(prepared.post_count(), 2);
        let result = transfer::consume(&f.staff, &ticket).await.unwrap();
        assert_eq!(result.posts[0].source_post_id, source);
        assert_eq!(result.posts[1].source_post_id, reply);
        let target = result.destination_thread;
        let moved_years:Vec<i32>=sqlx::query_scalar("SELECT since4pass FROM content.posts WHERE thread_id=$1 AND board=$2 ORDER BY id")
            .bind(target).bind(&f.boards[1]).fetch_all(&f.public).await.unwrap();
        assert_eq!(moved_years,vec![2020,2008]);
        let erased_years:Vec<i32>=sqlx::query_scalar("SELECT since4pass FROM content.posts WHERE id=ANY($1) ORDER BY id")
            .bind(&[source,reply][..]).fetch_all(&f.owner).await.unwrap();
        assert_eq!(erased_years,vec![0,0]);
        let moved_perks:Vec<Option<i16>>=sqlx::query_scalar("SELECT event_perk FROM content.posts WHERE thread_id=$1 AND board=$2 ORDER BY id").bind(target).bind(&f.boards[1]).fetch_all(&f.owner).await.unwrap();
        assert_eq!(moved_perks,vec![Some(1),Some(231)]);
        let erased_perks:Vec<Option<i16>>=sqlx::query_scalar("SELECT event_perk FROM content.posts WHERE id=ANY($1) ORDER BY id").bind(&[source,reply][..]).fetch_all(&f.owner).await.unwrap();
        assert_eq!(erased_perks,vec![None,None]);

        let mut membership=f.private().await;
        let moved:Vec<Vec<u8>>=sqlx::query_scalar("SELECT identity_hash FROM verification_private.pass_posts WHERE post_id=ANY($1) ORDER BY post_id")
            .bind(&[target,result.posts[1].destination_post_id][..]).fetch_all(&mut *membership).await.unwrap();
        assert_eq!(moved,vec![vec![0x41u8;32],vec![0x42u8;32]]);
        let old:i64=sqlx::query_scalar("SELECT count(*) FROM verification_private.pass_posts WHERE post_id=ANY($1)").bind(&[source,reply][..]).fetch_one(&mut *membership).await.unwrap();assert_eq!(old,0);membership.rollback().await.unwrap();
        let after: serde_json::Value = sqlx::query_scalar("SELECT jsonb_build_object('deletion',(SELECT password_hash FROM post_secrets.deletion WHERE post_id=$1),'peer',(SELECT peer::text FROM post_secrets.op_peers WHERE thread_id=$1),'history',(SELECT to_jsonb(h)-'post_id'-'board'-'thread_id' FROM post_secrets.posting_history h WHERE post_id=$1),'anonymous',(SELECT to_jsonb(a)-'post_id' FROM post_secrets.anonymous_posts a WHERE post_id=$1),'session',(SELECT to_jsonb(s) FROM post_secrets.anonymous_sessions s WHERE token_hash=$2),'reply',(SELECT count(*) FROM post_secrets.op_replies WHERE post_id=$3 AND thread_id=$1))")
            .bind(target).bind(cap.storage_hash().as_slice()).bind(result.posts[1].destination_post_id).fetch_one(&f.owner).await.unwrap();
        assert_eq!(after, before, "copy evidence exactly, never mover context or fresh activity");
        let moved_reply: (String, Vec<u8>, i64) = sqlx::query_as("SELECT automatic_browser,automatic_prefix,CASE WHEN post_id=thread_id THEN 0 ELSE thread_id END FROM post_secrets.posting_history WHERE post_id=$1")
            .bind(result.posts[1].destination_post_id).fetch_one(&f.owner).await.unwrap();
        assert_eq!(moved_reply, ("1cafebabe".into(), vec![0x34;32], target));

        let erased: bool = sqlx::query_scalar("SELECT content_erased AND deleted AND comment='' AND saved_presentation IS NULL FROM content.posts WHERE id=$1").bind(source).fetch_one(&f.owner).await.unwrap();
        assert!(erased);
        let source_secrets: i64 = sqlx::query_scalar("SELECT (SELECT count(*) FROM post_secrets.deletion WHERE post_id=$1)+(SELECT count(*) FROM post_secrets.posting_history WHERE post_id=$1)+(SELECT count(*) FROM post_secrets.anonymous_posts WHERE post_id=$1)").bind(source).fetch_one(&f.owner).await.unwrap();
        assert_eq!(source_secrets, 0);
        assert_eq!(transfer::consume(&f.staff, &ticket).await.unwrap_err(), TransferError::Expired);
        assert_eq!(transfer::result(&f.auth, &f.credentials(), prepared.request_id()).await.unwrap(), Some(result.clone()));
        // Reconciliation is account-bound, not dependent on a deleted issuer
        // session or on the canonically erased source payload.
        sqlx::query("DELETE FROM staff_identity.sessions WHERE token_hash=$1")
            .bind(f.session.as_slice()).execute(&f.owner).await.unwrap();
        let replacement = Credentials {
            session_hash: Capability::generate().unwrap().storage_hash(),
            csrf_hash: Capability::generate().unwrap().storage_hash(), idle_seconds: 900,
        };
        sqlx::query("INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id) VALUES($1,$2,$3,$4)")
            .bind(replacement.session_hash.as_slice()).bind(replacement.csrf_hash.as_slice()).bind(f.account).bind(&f.credential).execute(&f.owner).await.unwrap();
        assert_eq!(transfer::result(&f.auth, &replacement, prepared.request_id()).await.unwrap(), Some(result));
    }).await;
}

#[tokio::test]
async fn missing_history_is_not_repaired_and_copied_old_posts_cannot_be_adopted() {
    run(|f| async move {
        let source = f.post(0, 0, None).await;
        let reply = f.post(0, source, None).await;
        // Model legitimately absent historical evidence on owned rows only.
        sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id=$1").bind(reply).execute(&f.owner).await.unwrap();
        sqlx::query("DELETE FROM post_secrets.posting_history WHERE post_id=$1").bind(reply).execute(&f.owner).await.unwrap();
        sqlx::query("DELETE FROM post_secrets.op_replies WHERE post_id=$1").bind(reply).execute(&f.owner).await.unwrap();
        sqlx::query("DELETE FROM post_secrets.op_peers WHERE thread_id=$1").bind(source).execute(&f.owner).await.unwrap();
        let (ticket, _) = f.sealed(source).await;
        let result = transfer::consume(&f.staff, &ticket).await.unwrap();
        let missing: i64 = sqlx::query_scalar("SELECT (SELECT count(*) FROM post_secrets.posting_history WHERE post_id=$1)+(SELECT count(*) FROM post_secrets.op_replies WHERE post_id=$1)+(SELECT count(*) FROM post_secrets.op_peers WHERE thread_id=$2)+(SELECT count(*) FROM post_secrets.deletion WHERE post_id=$1)+(SELECT count(*) FROM post_secrets.anonymous_posts WHERE post_id=$1)")
            .bind(result.posts[1].destination_post_id).bind(result.destination_thread).fetch_one(&f.owner).await.unwrap();
        assert_eq!(missing, 0);
        // Even the OP with existing null automatic_identity must retain its old
        // registration xid. Forged request GUCs cannot make it a fresh post.
        let cap = f.capability();
        for post in result.posts {
            let mut tx = f.public.begin().await.unwrap();
            sqlx::query("SELECT set_config('board.registration_xid',pg_current_xact_id()::text,true),set_config('board.thread_transfer','true',true),set_config('board.posting_actor',repeat('11',32),true)").execute(&mut *tx).await.unwrap();
            let error = sqlx::query("SELECT content.register_anonymous_post($1,$1,$1,$1,true,$2,$3,extract(epoch FROM clock_timestamp())::bigint)")
                .bind(cap.storage_hash().as_slice()).bind(&f.boards[1]).bind(post.destination_post_id).execute(&mut *tx).await.unwrap_err();
            assert_eq!(error.as_database_error().and_then(|e| e.code()).as_deref(), Some("23514"));
            tx.rollback().await.unwrap();
        }
    }).await;
}

#[tokio::test]
async fn expired_or_rotated_anonymous_proof_is_never_resurrected_from_prepare() {
    run(|f| async move {
        let first = f.capability();
        let second = f.capability();
        let source = f.post(0, 0, Some(&first)).await;
        let reply = f.post(0, source, Some(&second)).await;
        let (ticket, prepared) = f.prepared(source).await;
        sqlx::query("UPDATE post_secrets.anonymous_sessions SET expires_at=1 WHERE token_hash=$1")
            .bind(first.storage_hash().as_slice())
            .execute(&f.owner)
            .await
            .unwrap();
        sqlx::query(
            "UPDATE post_secrets.deletion SET password_hash='rotated-owned-proof' WHERE post_id=$1",
        )
        .bind(reply)
        .execute(&f.owner)
        .await
        .unwrap();
        transfer::seal(&f.auth, &f.credentials(), &prepared)
            .await
            .unwrap();
        let result = transfer::consume(&f.staff, &ticket).await.unwrap();
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM post_secrets.anonymous_posts WHERE post_id=ANY($1)",
        )
        .bind(
            result
                .posts
                .iter()
                .map(|p| p.destination_post_id)
                .collect::<Vec<_>>(),
        )
        .fetch_one(&f.owner)
        .await
        .unwrap();
        assert_eq!(count, 0);
        let password: String =
            sqlx::query_scalar("SELECT password_hash FROM post_secrets.deletion WHERE post_id=$1")
                .bind(result.posts[1].destination_post_id)
                .fetch_one(&f.owner)
                .await
                .unwrap();
        assert_eq!(password, "rotated-owned-proof");
    })
    .await;
}

#[tokio::test]
async fn sealed_map_and_credential_binding_cannot_be_replaced() {
    run(|f| async move {
        let source = f.post(0, 0, None).await;
        let (ticket, prepared) = f.prepared(source).await;
        let before = f.pending().await;
        let mut tx = f.private().await;
        let (raw_ticket, fingerprint, destination): (Vec<u8>, Vec<u8>, i64) = sqlx::query_as("SELECT ticket_hash,source_fingerprint,destination_thread FROM post_secrets.thread_transfer_intents WHERE account_id=$1")
            .bind(f.account).fetch_one(&mut *tx).await.unwrap();
        tx.rollback().await.unwrap();
        // A trusted issuer still cannot change the allocated exact ID map.
        let error = sqlx::query("SELECT staff_identity.seal_text_thread_transfer($1,$2,$3,$4,1::smallint,ARRAY[$5]::bigint[],ARRAY[$6]::bigint[],ARRAY['forged'],ARRAY[decode('465030310000000000','hex')],ARRAY['forged'])")
            .bind(&raw_ticket).bind(f.session.as_slice()).bind(f.csrf.as_slice()).bind(fingerprint).bind(source).bind(destination + 1).execute(&f.auth).await.unwrap_err();
        assert_eq!(error.as_database_error().and_then(|e| e.code()).as_deref(), Some("P131I"));
        assert_eq!(f.pending().await, before);
        let mut wrong = f.credentials();
        wrong.csrf_hash = [7; 32];
        assert_eq!(transfer::seal(&f.auth, &wrong, &prepared).await.unwrap_err(), TransferError::Forbidden);
        transfer::seal(&f.auth, &f.credentials(), &prepared).await.unwrap();
        assert_eq!(transfer::seal(&f.auth, &f.credentials(), &prepared).await.unwrap_err(), TransferError::Invalid);
        let replacement = Capability::generate().unwrap().storage_hash();
        sqlx::query("INSERT INTO staff_identity.credentials(id,account_id,credential) VALUES($1,$2,'{}')")
            .bind(replacement.as_slice()).bind(f.account).execute(&f.owner).await.unwrap();
        sqlx::query("UPDATE staff_identity.sessions SET credential_id=$2 WHERE token_hash=$1")
            .bind(f.session.as_slice()).bind(replacement.as_slice()).execute(&f.owner).await.unwrap();
        let public = f.snapshot().await;
        let pending = f.pending().await;
        assert_eq!(transfer::consume(&f.staff, &ticket).await.unwrap_err(), TransferError::Forbidden);
        assert_eq!(f.snapshot().await, public);
        assert_eq!(f.pending().await, pending);
        // Restoring only our fixture binding establishes the successful control.
        sqlx::query("UPDATE staff_identity.sessions SET credential_id=$2 WHERE token_hash=$1")
            .bind(f.session.as_slice()).bind(&f.credential).execute(&f.owner).await.unwrap();
        transfer::consume(&f.staff, &ticket).await.unwrap();
    }).await;
}

#[tokio::test]
async fn prepared_or_sealed_work_and_forged_gucs_are_not_ordinary_insert_authority() {
    run(|f| async move {
        let source = f.post(0, 0, None).await;
        let (_ticket, prepared) = f.prepared(source).await;
        for sealed in [false, true] {
            if sealed { transfer::seal(&f.auth, &f.credentials(), &prepared).await.unwrap(); }
            let mut inspect = f.private().await;
            let target: i64 = sqlx::query_scalar("SELECT destination_thread FROM post_secrets.thread_transfer_intents WHERE account_id=$1")
                .bind(f.account).fetch_one(&mut *inspect).await.unwrap();
            inspect.rollback().await.unwrap();
            for role in [&f.public, &f.staff] {
                let mut tx = role.begin().await.unwrap();
                sqlx::query("SELECT set_config('board.thread_transfer','true',true),set_config('board.transfer_ticket',repeat('00',32),true),set_config('board.staff_capcode','admin',true),set_config('board.posting_actor',repeat('22',32),true)").execute(&mut *tx).await.unwrap();
                let allowed: bool = sqlx::query_scalar("SELECT content.is_thread_transfer_post($1,$2,$1)")
                    .bind(target).bind(&f.boards[1]).fetch_one(&mut *tx).await.unwrap();
                assert!(!allowed, "pending map is never active xid authority");
                // FP insertion must fail before a fabricated capcode or private
                // request context can enter durable copied content.
                let error = sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment,capcode,comment_format,saved_presentation,saved_presentation_search) VALUES($1,$2,$1,'Forged','Forged','forged','admin',127,decode('465030310000000000','hex'),'forged')")
                    .bind(target).bind(&f.boards[1]).execute(&mut *tx).await.unwrap_err();
                let code = error.as_database_error().and_then(|e| e.code());
                assert!(matches!(code.as_deref(), Some("42501" | "P131A" | "23514")));
                tx.rollback().await.unwrap();
            }
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=$1")
                .bind(&f.boards[1]).fetch_one(&f.owner).await.unwrap();
            assert_eq!(count, 0);
        }
    }).await;
}

#[tokio::test]
async fn source_and_policy_drift_refuse_without_partial_destination_or_audit() {
    run(|f| async move {
        let source = f.post(0, 0, None).await;
        let (ticket, _) = f.sealed(source).await;
        sqlx::query("UPDATE content.posts SET comment='source changed after seal' WHERE id=$1")
            .bind(source)
            .execute(&f.owner)
            .await
            .unwrap();
        let before = f.snapshot().await;
        let pending = f.pending().await;
        assert_eq!(
            transfer::consume(&f.staff, &ticket).await.unwrap_err(),
            TransferError::Drift
        );
        assert_eq!(f.snapshot().await, before);
        assert_eq!(f.pending().await, pending);
        transfer::discard(&f.auth, &f.credentials(), &ticket)
            .await
            .unwrap();
        let (year_ticket, _) = f.sealed(source).await;
        sqlx::query("UPDATE content.posts SET since4pass=2014 WHERE id=$1")
            .bind(source)
            .execute(&f.owner)
            .await
            .unwrap();
        let before_year = f.snapshot().await;
        assert_eq!(
            transfer::consume(&f.staff, &year_ticket).await.unwrap_err(),
            TransferError::Drift
        );
        assert_eq!(f.snapshot().await, before_year);
        transfer::discard(&f.auth, &f.credentials(), &year_ticket)
            .await
            .unwrap();
        let (membership_ticket, _) = f.sealed(source).await;
        let mut membership = f.owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_anonymous_owner")
            .execute(&mut *membership)
            .await
            .unwrap();
        sqlx::query("INSERT INTO verification_private.pass_posts VALUES($1,$2)")
            .bind(source)
            .bind(vec![0x43u8; 32])
            .execute(&mut *membership)
            .await
            .unwrap();
        membership.commit().await.unwrap();
        let before_membership = f.snapshot().await;
        assert_eq!(
            transfer::consume(&f.staff, &membership_ticket)
                .await
                .unwrap_err(),
            TransferError::Drift
        );
        assert_eq!(f.snapshot().await, before_membership);
        transfer::discard(&f.auth, &f.credentials(), &membership_ticket)
            .await
            .unwrap();
        let (ticket, _) = f.sealed(source).await;
        sqlx::query("UPDATE content.boards SET reply_limit=reply_limit+1 WHERE slug=$1")
            .bind(&f.boards[0])
            .execute(&f.owner)
            .await
            .unwrap();
        let before = f.snapshot().await;
        assert_eq!(
            transfer::consume(&f.staff, &ticket).await.unwrap_err(),
            TransferError::Drift
        );
        assert_eq!(f.snapshot().await, before);
    })
    .await;
}

#[tokio::test]
async fn nowait_lock_conflicts_abort_whole_call_without_retry_or_capability_loss() {
    run(|f| async move {
        let cap = f.capability();
        let source = f.post(0, 0, Some(&cap)).await;
        let (ticket, _) = f.sealed(source).await;
        // Mirror account-first operator changes, session-first logout, board-
        // first posting, source writers and anonymous-session cleanup.
        for query in [
            "SELECT id FROM staff_identity.accounts WHERE id=$1 FOR UPDATE",
            "SELECT token_hash FROM staff_identity.sessions WHERE account_id=$1 FOR UPDATE",
            "SELECT id FROM staff_identity.credentials WHERE account_id=$1 FOR UPDATE",
        ] {
            let before = f.snapshot().await;
            let mut blocker = f.owner.begin().await.unwrap();
            sqlx::query(sqlx::AssertSqlSafe(query))
                .bind(f.account)
                .fetch_all(&mut *blocker)
                .await
                .unwrap();
            let result =
                tokio::time::timeout(Duration::from_secs(2), transfer::consume(&f.staff, &ticket))
                    .await
                    .expect("NOWAIT must not block on another workflow");
            assert_eq!(result.unwrap_err(), TransferError::Busy);
            blocker.rollback().await.unwrap();
            assert_eq!(f.snapshot().await, before);
        }
        for query in [
            "SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE",
            "SELECT id FROM content.threads WHERE board=$1 FOR UPDATE",
            "SELECT id FROM content.posts WHERE board=$1 FOR UPDATE",
            "SELECT post_id FROM post_secrets.comment_history WHERE board=$1 FOR UPDATE",
        ] {
            let mut blocker = f.owner.begin().await.unwrap();
            sqlx::query(sqlx::AssertSqlSafe(query))
                .bind(&f.boards[0])
                .fetch_all(&mut *blocker)
                .await
                .unwrap();
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(2), transfer::consume(&f.staff, &ticket))
                    .await
                    .expect("NOWAIT source lock")
                    .unwrap_err(),
                TransferError::Busy
            );
            blocker.rollback().await.unwrap();
        }
        let mut blocker = f.owner.begin().await.unwrap();
        sqlx::query(
            "SELECT token_hash FROM post_secrets.anonymous_sessions WHERE token_hash=$1 FOR UPDATE",
        )
        .bind(cap.storage_hash().as_slice())
        .fetch_all(&mut *blocker)
        .await
        .unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), transfer::consume(&f.staff, &ticket))
                .await
                .expect("NOWAIT session lock")
                .unwrap_err(),
            TransferError::Busy
        );
        blocker.rollback().await.unwrap();
        let mut blocker = f.private().await;
        sqlx::query("SELECT singleton FROM post_secrets.thread_transfer_gate FOR UPDATE")
            .fetch_all(&mut *blocker)
            .await
            .unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), transfer::consume(&f.staff, &ticket))
                .await
                .expect("NOWAIT transfer gate")
                .unwrap_err(),
            TransferError::Busy
        );
        blocker.rollback().await.unwrap();
        transfer::consume(&f.staff, &ticket).await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn fixed_expiry_and_per_account_capacity_do_not_renew_or_leak_reservations() {
    run(|f| async move {
        let source = f.post(0, 0, None).await;
        let (ticket, prepared) = f.sealed(source).await;
        let second = Ticket::from_bytes(Capability::generate().unwrap().storage_hash());
        let result = transfer::prepare(&f.auth, &f.credentials(), &second, &f.boards[0], source, &f.boards[1]).await;
        assert!(matches!(result, Err(TransferError::Capacity)));
        let mut tx = f.private().await;
        sqlx::query("UPDATE post_secrets.thread_transfer_intents SET created_at=created_at-interval '61 seconds',expires_at=expires_at-interval '61 seconds' WHERE account_id=$1")
            .bind(f.account).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        let before = f.snapshot().await;
        assert_eq!(transfer::consume(&f.staff, &ticket).await.unwrap_err(), TransferError::Expired);
        assert_eq!(f.snapshot().await, before);
        assert!(transfer::result(&f.auth, &f.credentials(), prepared.request_id()).await.unwrap().is_none());
        let fresh = transfer::prepare(&f.auth, &f.credentials(), &second, &f.boards[0], source, &f.boards[1]).await.unwrap();
        assert_ne!(fresh.request_id(), prepared.request_id());
        transfer::seal(&f.auth, &f.credentials(), &fresh).await.unwrap();
        transfer::consume(&f.staff, &second).await.unwrap();
    }).await;
}

#[tokio::test]
async fn destination_capacity_refusal_and_savepoint_rollback_preserve_sealed_intent() {
    run(|f| async move {
        let source = f.post(0, 0, None).await;
        sqlx::query("UPDATE content.boards SET thread_limit=1 WHERE slug=$1")
            .bind(&f.boards[1])
            .execute(&f.owner)
            .await
            .unwrap();
        let (ticket, _) = f.sealed(source).await;
        let filler = f.post(1, 0, None).await;
        let before = f.snapshot().await;
        assert_eq!(
            transfer::consume(&f.staff, &ticket).await.unwrap_err(),
            TransferError::Capacity
        );
        assert_eq!(f.snapshot().await, before);
        sqlx::query("UPDATE content.threads SET deleted=true WHERE id=$1")
            .bind(filler)
            .execute(&f.owner)
            .await
            .unwrap();
        let before = f.snapshot().await;
        let pending = f.pending().await;
        let mut inspect = f.private().await;
        let raw_ticket: Vec<u8> = sqlx::query_scalar(
            "SELECT ticket_hash FROM post_secrets.thread_transfer_intents WHERE account_id=$1",
        )
        .bind(f.account)
        .fetch_one(&mut *inspect)
        .await
        .unwrap();
        inspect.rollback().await.unwrap();
        let mut tx = f.staff.begin().await.unwrap();
        sqlx::query("SAVEPOINT owned_transfer")
            .execute(&mut *tx)
            .await
            .unwrap();
        let rows = sqlx::query("SELECT * FROM content.consume_text_thread_transfer($1)")
            .bind(raw_ticket)
            .fetch_all(&mut *tx)
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        sqlx::query("ROLLBACK TO SAVEPOINT owned_transfer")
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        assert_eq!(
            f.snapshot().await,
            before,
            "destination, canonical source deletion and audit all roll back"
        );
        assert_eq!(
            f.pending().await,
            pending,
            "rollback restores sealed capability and removes active handoff"
        );
        transfer::consume(&f.staff, &ticket).await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn role_separation_and_preserved_registration_xid_prevent_copied_history_adoption() {
    run(|f| async move {
        let source = f.post(0, 0, None).await;
        let original_xid: String = sqlx::query_scalar("SELECT registration_xid::text FROM post_secrets.posting_history WHERE post_id=$1")
            .bind(source).fetch_one(&f.owner).await.unwrap();
        let (_ticket, _) = f.sealed(source).await;
        let mut inspect = f.private().await;
        let raw_ticket: Vec<u8> = sqlx::query_scalar("SELECT ticket_hash FROM post_secrets.thread_transfer_intents WHERE account_id=$1")
            .bind(f.account).fetch_one(&mut *inspect).await.unwrap();
        inspect.rollback().await.unwrap();
        // The actual consuming staff role cannot call anonymous registration,
        // even while its consume transaction is still current. No role grants
        // are broadened to manufacture a cross-role same-transaction caller.
        let mut tx = f.staff.begin().await.unwrap();
        let (target, consume_xid): (i64, String) = sqlx::query_as("SELECT destination_post_id,pg_current_xact_id()::text FROM content.consume_text_thread_transfer($1)")
            .bind(raw_ticket).fetch_one(&mut *tx).await.unwrap();
        sqlx::query("SAVEPOINT reject_adoption").execute(&mut *tx).await.unwrap();
        let cap = f.capability();
        let error = sqlx::query("SELECT content.register_anonymous_post($1,$1,$1,$1,true,$2,$3,extract(epoch FROM clock_timestamp())::bigint)")
            .bind(cap.storage_hash().as_slice()).bind(&f.boards[1]).bind(target).execute(&mut *tx).await.unwrap_err();
        denied(error);
        sqlx::query("ROLLBACK TO SAVEPOINT reject_adoption").execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        // Inspect only through the migrator's existing legacy-table grants.
        let (copied_xid, no_identity): (String, bool) = sqlx::query_as("SELECT registration_xid::text,automatic_identity IS NULL FROM post_secrets.posting_history WHERE post_id=$1")
            .bind(target).fetch_one(&f.owner).await.unwrap();
        assert_eq!(copied_xid, original_xid);
        assert_ne!(copied_xid, consume_xid);
        assert!(no_identity);
        // A separate real public caller does have registration EXECUTE, but
        // forged GUCs cannot turn this copied historical row into a new post.
        let mut public = f.public.begin().await.unwrap();
        sqlx::query("SELECT set_config('board.registration_xid',pg_current_xact_id()::text,true),set_config('board.thread_transfer','true',true)").execute(&mut *public).await.unwrap();
        let error = sqlx::query("SELECT content.register_anonymous_post($1,$1,$1,$1,true,$2,$3,extract(epoch FROM clock_timestamp())::bigint)")
            .bind(cap.storage_hash().as_slice()).bind(&f.boards[1]).bind(target).execute(&mut *public).await.unwrap_err();
        assert_eq!(error.as_database_error().and_then(|e| e.code()).as_deref(), Some("23514"));
        public.rollback().await.unwrap();
        let members: i64 = sqlx::query_scalar("SELECT count(*) FROM post_secrets.anonymous_posts WHERE post_id=$1")
            .bind(target).fetch_one(&f.owner).await.unwrap();
        assert_eq!(members, 0);
    }).await;
}

#[tokio::test]
async fn late_audit_refusal_rolls_back_copied_private_evidence_source_erasure_and_intent() {
    run(|f| async move {
        let cap = f.capability();
        let source = f.post(0, 0, Some(&cap)).await;
        // An owned partial index produces a genuine failure at the late audit
        // INSERT, without replacing functions/triggers or changing role ACLs.
        // The generated slug contains only ASCII alphanumerics (DB constrained).
        assert_owned_identifier(&f.boards[0]);
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE UNIQUE INDEX {}_transfer_audit_refusal ON content.moderation_audit(target_id) WHERE board='{}' AND action='text-thread-transfer'", f.boards[0], f.boards[0])))
            .execute(&f.owner).await.unwrap();
        sqlx::query("INSERT INTO content.moderation_audit(account_id,board,target_id,action) VALUES($1,$2,$3,'text-thread-transfer')")
            .bind(f.account).bind(&f.boards[0]).bind(source).execute(&f.owner).await.unwrap();
        assert!(transfer::ready(&f.auth).await.unwrap(), "fault must not be an early readiness rejection");
        assert!(transfer::ready(&f.staff).await.unwrap());
        let (ticket, _) = f.sealed(source).await;
        let public = f.snapshot().await;
        let pending = f.pending().await;
        let evidence: serde_json::Value = sqlx::query_scalar("SELECT jsonb_build_object('deletion',(SELECT to_jsonb(d) FROM post_secrets.deletion d WHERE post_id=$1),'member',(SELECT to_jsonb(a) FROM post_secrets.anonymous_posts a WHERE post_id=$1),'session',(SELECT to_jsonb(s) FROM post_secrets.anonymous_sessions s WHERE token_hash=$2))")
            .bind(source).bind(cap.storage_hash().as_slice()).fetch_one(&f.owner).await.unwrap();
        assert_eq!(transfer::consume(&f.staff, &ticket).await.unwrap_err(), TransferError::Drift);
        assert_eq!(f.snapshot().await, public);
        assert_eq!(f.pending().await, pending);
        let after: serde_json::Value = sqlx::query_scalar("SELECT jsonb_build_object('deletion',(SELECT to_jsonb(d) FROM post_secrets.deletion d WHERE post_id=$1),'member',(SELECT to_jsonb(a) FROM post_secrets.anonymous_posts a WHERE post_id=$1),'session',(SELECT to_jsonb(s) FROM post_secrets.anonymous_sessions s WHERE token_hash=$2))")
            .bind(source).bind(cap.storage_hash().as_slice()).fetch_one(&f.owner).await.unwrap();
        assert_eq!(after, evidence);
        sqlx::query("DELETE FROM content.moderation_audit WHERE board=$1 AND target_id=$2 AND action='text-thread-transfer'")
            .bind(&f.boards[0]).bind(source).execute(&f.owner).await.unwrap();
        transfer::consume(&f.staff, &ticket).await.unwrap();
    }).await;
}

#[tokio::test]
async fn global_two_pending_limit_is_shared_across_distinct_accounts() {
    let _serial = TEST.lock().await;
    let fixtures = [
        Fixture::new().await,
        Fixture::new().await,
        Fixture::new().await,
    ];
    let work = fixtures.clone();
    let outcome = tokio::spawn(async move {
        let source_a = work[0].post(0, 0, None).await;
        let source_b = work[1].post(0, 0, None).await;
        let source_c = work[2].post(0, 0, None).await;
        let (ticket_a, _) = work[0].prepared(source_a).await;
        let (ticket_b, _) = work[1].prepared(source_b).await;
        let third = Ticket::from_bytes(Capability::generate().unwrap().storage_hash());
        let error = transfer::prepare(
            &work[2].auth,
            &work[2].credentials(),
            &third,
            &work[2].boards[0],
            source_c,
            &work[2].boards[1],
        )
        .await;
        assert!(matches!(error, Err(TransferError::Capacity)));
        transfer::discard(&work[0].auth, &work[0].credentials(), &ticket_a)
            .await
            .unwrap();
        let prepared = transfer::prepare(
            &work[2].auth,
            &work[2].credentials(),
            &third,
            &work[2].boards[0],
            source_c,
            &work[2].boards[1],
        )
        .await
        .unwrap();
        transfer::seal(&work[2].auth, &work[2].credentials(), &prepared)
            .await
            .unwrap();
        transfer::consume(&work[2].staff, &third).await.unwrap();
        transfer::discard(&work[1].auth, &work[1].credentials(), &ticket_b)
            .await
            .unwrap();
    })
    .await;
    for fixture in &fixtures {
        fixture.cleanup().await;
    }
    outcome.unwrap();
}

#[tokio::test]
async fn verified_ownership_moves_without_a_live_browser_and_is_erased_with_content() {
    run(|f| async move {
        let cap=f.capability();
        let source=f.post(0,0,Some(&cap)).await;
        // Seed a private ownership row to isolate transfer/erasure semantics.
        // Receipt-backed linking and normal registration have separate tests.
        let identity:String=sqlx::query_scalar("INSERT INTO verification_private.identities(email_hash) VALUES($1) RETURNING id::text")
            .bind(f.session.as_slice()).fetch_one(&f.owner).await.unwrap();
        sqlx::query("INSERT INTO verification_private.post_ownership(post_id,identity_id,password_proof) SELECT post_id,$2::text::uuid,sha256(convert_to(password_hash,'UTF8')) FROM post_secrets.deletion WHERE post_id=$1")
            .bind(source).bind(&identity).execute(&f.owner).await.unwrap();
        sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
            .bind(cap.storage_hash().as_slice()).execute(&f.owner).await.unwrap();
        let (ticket,_)=f.sealed(source).await;
        let result=transfer::consume(&f.staff,&ticket).await.unwrap();
        let copied:(String,bool)=sqlx::query_as("SELECT o.identity_id::text,o.password_proof=sha256(convert_to(d.password_hash,'UTF8')) FROM verification_private.post_ownership o JOIN post_secrets.deletion d ON d.post_id=o.post_id WHERE o.post_id=$1")
            .bind(result.destination_thread).fetch_one(&f.owner).await.unwrap();
        assert_eq!(copied,(identity,true));
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM verification_private.post_ownership WHERE post_id=$1")
            .bind(source).fetch_one(&f.owner).await.unwrap(),0);
        let mut tx=f.owner.begin().await.unwrap();
        sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE").bind(&f.boards[1]).execute(&mut *tx).await.unwrap();
        sqlx::query("UPDATE content.threads SET deleted=true WHERE id=$1").bind(result.destination_thread).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM verification_private.post_ownership WHERE post_id=$1")
            .bind(result.destination_thread).fetch_one(&f.owner).await.unwrap(),0);
    }).await;
}

#[tokio::test]
async fn verified_ownership_schema_access_drift_fails_readiness() {
    run(|f| async move {
        let mut tx = f.owner.begin().await.unwrap();
        sqlx::query("REVOKE USAGE ON SCHEMA verification_private FROM board_thread_transfer_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("SET LOCAL ROLE board_thread_transfer_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        assert!(
            !sqlx::query_scalar::<_, bool>("SELECT content.text_thread_transfer_ready()")
                .fetch_one(&mut *tx)
                .await
                .unwrap()
        );
        tx.rollback().await.unwrap();
        assert!(
            sqlx::query_scalar::<_, bool>("SELECT content.text_thread_transfer_ready()")
                .fetch_one(&f.staff)
                .await
                .unwrap()
        );
    })
    .await;
}

#[tokio::test]
async fn retained_mobile_metadata_changes_invalidate_prepared_transfer() {
    run(|f| async move {
        let source = f.post(0, 0, None).await;
        let (ticket, _) = f.sealed(source).await;
        sqlx::query("UPDATE post_secrets.posting_history SET automatic_browser='1deadbeef',automatic_prefix=decode(repeat('12',32),'hex') WHERE post_id=$1")
            .bind(source).execute(&f.owner).await.unwrap();
        let before = f.snapshot().await;
        assert_eq!(transfer::consume(&f.staff, &ticket).await.unwrap_err(), TransferError::Drift);
        assert_eq!(f.snapshot().await, before);
        transfer::discard(&f.auth, &f.credentials(), &ticket).await.unwrap();
    }).await;
}

#[tokio::test]
async fn moved_comments_keep_actor_window_and_rewritten_quote_comparison() {
    run(|f| async move {
        let source = f.post(0, 0, None).await;
        let reply = f.post(0, source, None).await;
        let before: (Vec<u8>, Vec<u8>, i64) = sqlx::query_as(
            "SELECT actor_hash,projection_hash,request_at FROM post_secrets.comment_history WHERE post_id=$1",
        ).bind(reply).fetch_one(&f.owner).await.unwrap();
        let (ticket, _) = f.sealed(source).await;
        let result = transfer::consume(&f.staff, &ticket).await.unwrap();
        let moved_reply = result.posts[1].destination_post_id;
        let after: (Vec<u8>, Vec<u8>, i64) = sqlx::query_as(
            "SELECT actor_hash,projection_hash,request_at FROM post_secrets.comment_history WHERE post_id=$1",
        ).bind(moved_reply).fetch_one(&f.owner).await.unwrap();
        assert_eq!(after.0, before.0);
        assert_eq!(after.2, before.2);
        assert_ne!(after.1, before.1, "rewritten quote changes the comparison");
        let posted_at: i64 = sqlx::query_scalar("SELECT floor(extract(epoch FROM created_at))::bigint FROM content.posts WHERE id=$1")
            .bind(moved_reply).fetch_one(&f.owner).await.unwrap();
        for (request, expected) in [(posted_at + 299, Some(1_i64)), (posted_at + 300, None)] {
            let remaining: Option<i64> = sqlx::query_scalar("SELECT content.comment_duplicate_remaining($1,$2,$3,$4)")
                .bind(&after.0).bind(&f.boards[1]).bind(&after.1).bind(request)
                .fetch_one(&f.public).await.unwrap();
            assert_eq!(remaining, expected);
        }

        let old_count: i64 = sqlx::query_scalar("SELECT count(*) FROM post_secrets.comment_history WHERE board=$1")
            .bind(&f.boards[0]).fetch_one(&f.owner).await.unwrap();
        assert_eq!(old_count, 0, "source erasure retires source comparison rows");
        let source_key = support::key(&f.boards[0]);
        let mut post = NewPost {
            name: "Anonymous".into(), subject: String::new(),
            comment: format!("Owned text >>{}", result.destination_thread),
            deletion_hash: "synthetic-transfer-comparison".into(), sage: false,
        };
        let error = board_store::create_post_with_identity_keys(
            &f.public, &f.boards[1], result.destination_thread, &post, None,
            PostingContext { request_start: Utc::now(), peer: Some(support::peer()), op_password_proof: None },
            PostIdentityKeys { tripcode: None, poster_id: Some(&source_key) },
        ).await.unwrap_err();
        assert!(matches!(error, board_store::StoreError::ContentRejected(ref text) if text.contains("duplicate reply")));
        // The old quote spelling is different after the move; no broad reset
        // or copied source-board digest should reject this ordinary reply.
        post.comment = format!("Owned text >>{source}");
        board_store::create_post_with_identity_keys(
            &f.public, &f.boards[1], result.destination_thread, &post, None,
            PostingContext { request_start: Utc::now(), peer: Some(support::peer()), op_password_proof: None },
            PostIdentityKeys { tripcode: None, poster_id: Some(&source_key) },
        ).await.unwrap();
    }).await;
}

#[tokio::test]
async fn missing_comparison_stays_unknown_and_mismatched_proof_rolls_back_move() {
    run(|f| async move {
        let source = f.post(0, 0, None).await;
        let reply = f.post(0, source, None).await;
        sqlx::query("DELETE FROM post_secrets.comment_history WHERE post_id=$1")
            .bind(reply).execute(&f.owner).await.unwrap();
        let original: Vec<u8> = sqlx::query_scalar("SELECT projection_hash FROM post_secrets.comment_history WHERE post_id=$1")
            .bind(source).fetch_one(&f.owner).await.unwrap();
        let (ticket, prepared) = f.sealed(source).await;
        sqlx::query("UPDATE post_secrets.comment_history SET projection_hash=decode(repeat('00',32),'hex') WHERE post_id=$1")
            .bind(source).execute(&f.owner).await.unwrap();
        assert_eq!(transfer::consume(&f.staff, &ticket).await.unwrap_err(), TransferError::Format);
        let intact: bool = sqlx::query_scalar("SELECT NOT deleted AND NOT content_erased FROM content.threads WHERE id=$1")
            .bind(source).fetch_one(&f.owner).await.unwrap();
        assert!(intact);
        let destination_count: i64 = sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=$1")
            .bind(&f.boards[1]).fetch_one(&f.owner).await.unwrap();
        assert_eq!(destination_count, 0);
        assert!(transfer::result(&f.auth, &f.credentials(), prepared.request_id()).await.unwrap().is_none());
        sqlx::query("UPDATE post_secrets.comment_history SET projection_hash=$2 WHERE post_id=$1")
            .bind(source).bind(original).execute(&f.owner).await.unwrap();
        let moved = transfer::consume(&f.staff, &ticket).await.unwrap();
        let ids: Vec<i64> = sqlx::query_scalar("SELECT post_id FROM post_secrets.comment_history WHERE board=$1 ORDER BY post_id")
            .bind(&f.boards[1]).fetch_all(&f.owner).await.unwrap();
        assert_eq!(ids, vec![moved.destination_thread]);
    }).await;
}

#[tokio::test]
async fn repeated_moves_rewrite_known_comparisons_without_refreshing_post_time() {
    run(|f| async move {
        let source = f.post(0, 0, None).await;
        let reply = f.post(0, source, None).await;
        let original: (Vec<u8>, i64, chrono::DateTime<Utc>) = sqlx::query_as(
            "SELECT h.actor_hash,h.request_at,p.created_at FROM post_secrets.comment_history h JOIN content.posts p ON p.id=h.post_id WHERE h.post_id=$1",
        ).bind(reply).fetch_one(&f.owner).await.unwrap();
        let (ticket, _) = f.sealed(source).await;
        let first = transfer::consume(&f.staff, &ticket).await.unwrap();
        let second_ticket = Ticket::from_bytes(Capability::generate().unwrap().storage_hash());
        let prepared = transfer::prepare(&f.auth, &f.credentials(), &second_ticket,
            &f.boards[1], first.destination_thread, &f.boards[0]).await.unwrap();
        transfer::seal(&f.auth, &f.credentials(), &prepared).await.unwrap();
        let second = transfer::consume(&f.staff, &second_ticket).await.unwrap();
        let final_reply = second.posts[1].destination_post_id;
        let after: (Vec<u8>, i64, chrono::DateTime<Utc>) = sqlx::query_as(
            "SELECT h.actor_hash,h.request_at,p.created_at FROM post_secrets.comment_history h JOIN content.posts p ON p.id=h.post_id WHERE h.post_id=$1",
        ).bind(final_reply).fetch_one(&f.owner).await.unwrap();
        assert_eq!(after, original);
        let matched: bool = sqlx::query_scalar("SELECT h.projection_hash=sha256(convert_to('26chan:source-comment-projection:v1','UTF8')||decode('00','hex')||convert_to($2,'UTF8')) FROM post_secrets.comment_history h WHERE h.post_id=$1")
            .bind(final_reply).bind(format!("Owned text &gt;&gt;{}",second.destination_thread))
            .fetch_one(&f.owner).await.unwrap();
        assert!(matched);
        let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM post_secrets.comment_history WHERE board=ANY($1)")
            .bind(&f.boards[..]).fetch_one(&f.owner).await.unwrap();
        assert_eq!(rows, 2);
    }).await;
}
