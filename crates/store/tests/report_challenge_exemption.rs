#![cfg(feature = "database-tests")]
mod support;
use board_domain::{anonymous_session::Capability, poster_id::PublicReportRateIdentity};
use board_store::{anonymous_session::PostingSession, verification::VerificationStore, *};
use sqlx::PgPool;

#[tokio::test]
async fn optional_report_exemption_is_rechecked_and_reserved_inside_admission() {
    for categorical in [false, true] {
        scenario(categorical).await;
    }
}

async fn submit(
    public: &PgPool,
    board: &str,
    target: i64,
    revision: Option<i64>,
    actor: &PublicReportRateIdentity,
    session: PostingSession,
    source_cookies_absent: bool,
) -> Result<(), StoreError> {
    let context = ProtectedReportingContext {
        source_cookies_absent,
        session,
        challenge: None,
    };
    match revision {
        Some(revision) => {
            report_categorical_with_challenge(public, board, target, 1, revision, actor, context)
                .await
        }
        None => {
            report_with_challenge(
                public,
                board,
                target,
                "Owned exemption report",
                actor,
                context,
            )
            .await
        }
    }
}
fn needs_proof(result: Result<(), StoreError>) {
    assert!(
        matches!(result,Err(StoreError::Database(ref error)) if error.as_database_error().and_then(|e|e.code()).as_deref()==Some("P0166")),
        "{result:?}"
    );
}
async fn scenario(categorical: bool) {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let verifier =
        VerificationStore::connect(&std::env::var("TEST_VERIFICATION_DATABASE_URL").unwrap())
            .await
            .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT 're'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,captcha_allow_bypass) VALUES($1,'Report exemption','Owned fixture',2000,100,100,100,10,false)").bind(&board).execute(&owner).await.unwrap();
    let thread: i64 =
        sqlx::query_scalar("INSERT INTO content.threads(board) VALUES($1) RETURNING id")
            .bind(&board)
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous','Owned target','Synthetic')").bind(thread).bind(&board).execute(&owner).await.unwrap();
    let second:i64=sqlx::query_scalar("INSERT INTO content.posts(board,thread_id,name,subject,comment) VALUES($1,$2,'Anonymous','','Synthetic') RETURNING id").bind(&board).bind(thread).fetch_one(&owner).await.unwrap();
    let text:i64=sqlx::query_scalar("INSERT INTO content.posts(board,thread_id,name,subject,comment) VALUES($1,$2,'Anonymous','','Text only') RETURNING id").bind(&board).bind(thread).fetch_one(&owner).await.unwrap();
    for target in [thread, second] {
        sqlx::query("INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler,file_deleted) VALUES($1,replace(gen_random_uuid()::text,'-',''),replace(gen_random_uuid()::text,'-',''),'owned.webm',100,1,1,false,true)").bind(target).execute(&owner).await.unwrap();
    }
    let original:(Option<Vec<u8>>,bool,chrono::DateTime<chrono::Utc>,i32)=sqlx::query_as("SELECT report_history_epoch,report_history_epoch_tainted,report_history_complete_after,report_history_limit FROM verification_private.policy WHERE singleton").fetch_one(&owner).await.unwrap();
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_report_admission_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let original_catalog: Option<i64> = sqlx::query_scalar(
        "SELECT active_catalog_revision FROM post_secrets.report_admission_gate WHERE singleton",
    )
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.rollback().await.unwrap();
    let revision: Option<i64> = if categorical {
        Some(sqlx::query_scalar("SELECT content.import_report_catalog($1::text::jsonb)").bind(serde_json::json!({"version":1,"categories":[{"id":1,"board":"","op_only":false,"reply_only":false,"image_only":false,"exclude_boards":null,"title":"Owned category","weight":1.0,"filtered":0}]}).to_string()).fetch_one(&owner).await.unwrap())
    } else {
        None
    };
    sqlx::query("SELECT content.set_report_catalog_active($1)")
        .bind(revision)
        .execute(&owner)
        .await
        .unwrap();
    let key = support::fresh_key();
    let peer = "192.0.2.171".parse().unwrap();
    let actor = key.public_report_rate_identity(peer);
    let fp = Capability::generate()
        .unwrap()
        .fingerprints(Some(peer), *b"US");
    verifier.ensure_session(&fp, true).await.unwrap();
    sqlx::query("UPDATE post_secrets.anonymous_sessions SET created_at=extract(epoch FROM statement_timestamp())::bigint-700000,network_at=extract(epoch FROM statement_timestamp())::bigint-700000,address_at=extract(epoch FROM statement_timestamp())::bigint-700000,activity_at=extract(epoch FROM statement_timestamp())::bigint WHERE token_hash=$1").bind(fp.token.as_slice()).execute(&owner).await.unwrap();
    sqlx::query("UPDATE verification_private.policy SET report_history_epoch=$1,report_history_epoch_tainted=false,report_history_complete_after=clock_timestamp()-interval '1 second' WHERE singleton").bind(actor.report_history_epoch().as_slice()).execute(&owner).await.unwrap();
    let mut restriction_tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *restriction_tx)
        .await
        .unwrap();
    let restriction: i64 = sqlx::query_scalar("INSERT INTO restrictions.bans(board,post_id,actor_hash,global,warning,starts_at,ends_at,public_reason,private_reason,account_id,charged_bytes,active,retired_at,retirement) VALUES($1,$2,$3,false,true,statement_timestamp()-interval '31 days',statement_timestamp()-interval '31 days','Owned retired warning','',1,512,false,clock_timestamp(),'warning-viewed') RETURNING id").bind(&board).bind(thread).bind(actor.restriction_identity().as_bytes().as_slice()).fetch_one(&mut *restriction_tx).await.unwrap();
    restriction_tx.commit().await.unwrap();
    let capacity_actor = Capability::generate().unwrap().storage_hash();
    let work = {
        let owner = owner.clone();
        let public = public.clone();
        let board = board.clone();
        async move {
            let session = || PostingSession {
                fingerprints: fp,
                minted: false,
                now: chrono::Utc::now(),
            };
            let activity_before: serde_json::Value = sqlx::query_scalar(
                "SELECT to_jsonb(s) FROM post_secrets.anonymous_sessions s WHERE token_hash=$1",
            )
            .bind(fp.token.as_slice())
            .fetch_one(&owner)
            .await
            .unwrap();
            let mut fresh = session();
            fresh.minted = true;
            assert!(
                !board_store::report_challenge::advice(
                    &public, &board, thread, &actor, fresh, true
                )
                .await
                .unwrap()
            );
            for _ in 0..3 {
                assert!(
                    board_store::report_challenge::advice(
                        &public,
                        &board,
                        thread,
                        &actor,
                        session(),
                        true
                    )
                    .await
                    .unwrap()
                );
            }
            assert!(
                !board_store::report_challenge::advice(
                    &public,
                    &board,
                    thread,
                    &actor,
                    session(),
                    false
                )
                .await
                .unwrap()
            );
            let activity_after: serde_json::Value = sqlx::query_scalar(
                "SELECT to_jsonb(s) FROM post_secrets.anonymous_sessions s WHERE token_hash=$1",
            )
            .bind(fp.token.as_slice())
            .fetch_one(&owner)
            .await
            .unwrap();
            assert_eq!(activity_before, activity_after);
            let recorded: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM verification_private.report_history WHERE actor_hash=$1",
            )
            .bind(actor.as_bytes().as_slice())
            .fetch_one(&owner)
            .await
            .unwrap();
            assert_eq!(recorded, 0);
            needs_proof(submit(&public, &board, thread, revision, &actor, session(), false).await);
            needs_proof(submit(&public, &board, text, revision, &actor, session(), true).await);
            let ipv6 = key.public_report_rate_identity("2001:db8::171".parse().unwrap());
            needs_proof(submit(&public, &board, thread, revision, &ipv6, session(), true).await);
            let mut changed = session();
            changed.fingerprints.address = [42; 32];
            needs_proof(submit(&public, &board, thread, revision, &actor, changed, true).await);
            for field in ["address_at", "activity_at"] {
                let value = if field == "address_at" {
                    chrono::Utc::now().timestamp() - 604799
                } else {
                    chrono::Utc::now().timestamp() - 604800
                };
                let query = match field {
                    "address_at" => {
                        "UPDATE post_secrets.anonymous_sessions SET address_at=$1 WHERE token_hash=$2"
                    }
                    _ => {
                        "UPDATE post_secrets.anonymous_sessions SET activity_at=$1 WHERE token_hash=$2"
                    }
                };
                sqlx::query(query)
                    .bind(value)
                    .bind(fp.token.as_slice())
                    .execute(&owner)
                    .await
                    .unwrap();
                needs_proof(
                    submit(&public, &board, thread, revision, &actor, session(), true).await,
                );
                sqlx::query("UPDATE post_secrets.anonymous_sessions SET address_at=extract(epoch FROM statement_timestamp())::bigint-700000,activity_at=extract(epoch FROM statement_timestamp())::bigint WHERE token_hash=$1").bind(fp.token.as_slice()).execute(&owner).await.unwrap();
            }
            sqlx::query("UPDATE verification_private.policy SET report_history_complete_after=clock_timestamp()+interval '1 hour' WHERE singleton").execute(&owner).await.unwrap();
            needs_proof(submit(&public, &board, thread, revision, &actor, session(), true).await);
            sqlx::query("UPDATE verification_private.policy SET report_history_complete_after=clock_timestamp()-interval '1 second',report_history_epoch_tainted=true WHERE singleton").execute(&owner).await.unwrap();
            needs_proof(submit(&public, &board, thread, revision, &actor, session(), true).await);
            sqlx::query("UPDATE verification_private.policy SET report_history_epoch_tainted=false WHERE singleton").execute(&owner).await.unwrap();
            let mut tx = owner.begin().await.unwrap();
            sqlx::query("SET LOCAL ROLE board_restriction_owner")
                .execute(&mut *tx)
                .await
                .unwrap();
            sqlx::query("UPDATE restrictions.bans SET starts_at=statement_timestamp()-interval '1 day',ends_at=statement_timestamp()-interval '1 day' WHERE id=$1").bind(restriction).execute(&mut *tx).await.unwrap();
            tx.commit().await.unwrap();
            needs_proof(submit(&public, &board, thread, revision, &actor, session(), true).await);
            let mut tx = owner.begin().await.unwrap();
            sqlx::query("SET LOCAL ROLE board_restriction_owner")
                .execute(&mut *tx)
                .await
                .unwrap();
            sqlx::query("UPDATE restrictions.bans SET starts_at=statement_timestamp()-interval '31 days',ends_at=statement_timestamp()-interval '31 days' WHERE id=$1").bind(restriction).execute(&mut *tx).await.unwrap();
            tx.commit().await.unwrap();
            sqlx::query("INSERT INTO verification_private.report_history(actor_hash,last_reports,expires_at) VALUES($1,ARRAY[clock_timestamp()],clock_timestamp()+interval '1 hour')").bind(capacity_actor.as_slice()).execute(&owner).await.unwrap();
            sqlx::query("UPDATE verification_private.policy SET report_history_limit=(SELECT count(*) FROM verification_private.report_history) WHERE singleton").execute(&owner).await.unwrap();
            needs_proof(submit(&public, &board, thread, revision, &actor, session(), true).await);
            sqlx::query("UPDATE verification_private.policy SET report_history_limit=100000 WHERE singleton").execute(&owner).await.unwrap();
            sqlx::query("INSERT INTO verification_private.report_history(actor_hash,last_reports,last_cleared_at,expires_at) VALUES($1,ARRAY[clock_timestamp()-interval '3 minutes',clock_timestamp()-interval '2 minutes',clock_timestamp()-interval '1 minute'],NULL,clock_timestamp()+interval '1 hour')").bind(actor.as_bytes().as_slice()).execute(&owner).await.unwrap();
            needs_proof(submit(&public, &board, thread, revision, &actor, session(), true).await);
            sqlx::query("UPDATE verification_private.report_history SET last_reports='{}',last_cleared_at=clock_timestamp()-interval '71 hours',expires_at=clock_timestamp()+interval '1 hour' WHERE actor_hash=$1").bind(actor.as_bytes().as_slice()).execute(&owner).await.unwrap();
            needs_proof(submit(&public, &board, thread, revision, &actor, session(), true).await);
            sqlx::query("UPDATE verification_private.report_history SET last_reports=ARRAY[clock_timestamp()-interval '3 minutes',clock_timestamp()-interval '2 minutes'],last_cleared_at=clock_timestamp()-interval '73 hours',expires_at=clock_timestamp()+interval '1 hour' WHERE actor_hash=$1").bind(actor.as_bytes().as_slice()).execute(&owner).await.unwrap();
            // Force a late failure after the exempt report/history write.
            assert!(board.starts_with("re") && board[2..].bytes().all(|b| b.is_ascii_hexdigit()));
            let token_hex: String = fp.token.iter().map(|b| format!("{b:02x}")).collect();
            sqlx::query(sqlx::AssertSqlSafe(format!("ALTER TABLE post_secrets.anonymous_reports ADD CONSTRAINT {board}_late_exempt CHECK(token_hash<>decode('{token_hex}','hex')) NOT VALID"))).execute(&owner).await.unwrap();
            let failed = submit(&public, &board, thread, revision, &actor, session(), true).await;
            assert!(
                matches!(failed,Err(StoreError::Database(ref e)) if e.as_database_error().and_then(|e|e.code()).as_deref()==Some("23514")),
                "{failed:?}"
            );
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "ALTER TABLE post_secrets.anonymous_reports DROP CONSTRAINT {board}_late_exempt"
            )))
            .execute(&owner)
            .await
            .unwrap();
            let count:i32=sqlx::query_scalar("SELECT cardinality(last_reports) FROM verification_private.report_history WHERE actor_hash=$1").bind(actor.as_bytes().as_slice()).fetch_one(&owner).await.unwrap();
            assert_eq!(count, 2);
            // Environment changes alone do not reset exact-address age. Both
            // targets retain positive fsize after file deletion; credits are off.
            let mut first = session();
            first.fingerprints.environment = [43; 32];
            let mut other = session();
            other.fingerprints.environment = [43; 32];
            let (a, b) = tokio::join!(
                submit(&public, &board, thread, revision, &actor, first, true),
                submit(&public, &board, second, revision, &actor, other, true)
            );
            assert_eq!(
                usize::from(a.is_ok()) + usize::from(b.is_ok()),
                1,
                "{a:?} / {b:?}"
            );
            needs_proof(if a.is_err() { a } else { b });
            assert!(
                !board_store::report_challenge::advice(
                    &public,
                    &board,
                    thread,
                    &actor,
                    session(),
                    true
                )
                .await
                .unwrap()
            );
            let count:i32=sqlx::query_scalar("SELECT cardinality(last_reports) FROM verification_private.report_history WHERE actor_hash=$1").bind(actor.as_bytes().as_slice()).fetch_one(&owner).await.unwrap();
            assert_eq!(count, 3);
        }
    };
    let outcome = tokio::spawn(work).await;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "ALTER TABLE post_secrets.anonymous_reports DROP CONSTRAINT IF EXISTS {board}_late_exempt"
    )))
    .execute(&owner)
    .await
    .unwrap();
    sqlx::query("SELECT content.set_report_catalog_active($1)")
        .bind(original_catalog)
        .execute(&owner)
        .await
        .unwrap();
    let mut restriction_tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *restriction_tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM restrictions.bans WHERE id=$1")
        .bind(restriction)
        .execute(&mut *restriction_tx)
        .await
        .unwrap();
    restriction_tx.commit().await.unwrap();
    sqlx::query("DELETE FROM verification_private.report_history WHERE actor_hash=$1")
        .bind(capacity_actor.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    let mut tx =
        support::begin_cleanup_with_sessions(&owner, std::slice::from_ref(&board), &[fp.token])
            .await;
    for query in [
        "DELETE FROM content.reports WHERE board=$1",
        "DELETE FROM content.post_media WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(query)
            .bind(&board)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    tx.commit().await.unwrap();
    sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
        .bind(fp.token.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM verification_private.report_history WHERE actor_hash=$1")
        .bind(actor.as_bytes().as_slice())
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("UPDATE verification_private.policy SET report_history_epoch=$1,report_history_epoch_tainted=$2,report_history_complete_after=$3,report_history_limit=$4 WHERE singleton").bind(original.0).bind(original.1).bind(original.2).bind(original.3).execute(&owner).await.unwrap();
    public.close().await;
    owner.close().await;
    outcome.unwrap();
}
