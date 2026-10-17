use super::*;
use chrono::Timelike;
use sqlx::PgPool;

#[tokio::test]
async fn accepted_replay_preserves_originality_and_one_mute_decay() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = crate::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT 'rp'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,robot9000,captcha_enabled,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Owned replay','Synthetic',true,false,2000,100,100,100,10)")
        .bind(&board).execute(&owner).await.unwrap();
    let (o, p, b) = (owner.clone(), public.clone(), board.clone());
    let result = tokio::spawn(async move {
        let actor = [41_u8;32];
        let now = Utc::now().with_nanosecond(0).unwrap();
        sqlx::query("INSERT INTO post_secrets.robot9000_mutes(board,actor,timeout_power,mute_until,next_expire) VALUES($1,$2,3,$3::timestamptz-interval '2 days',$3::timestamptz-interval '1 day')")
            .bind(&b).bind(actor.as_slice()).bind(now).execute(&o).await.unwrap();
        let mut tx = p.begin().await.unwrap();
        sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
            .bind(&b).execute(&mut *tx).await.unwrap();
        sqlx::query("SAVEPOINT owned_post").execute(&mut *tx).await.unwrap();
        let prepared = board_domain::robot9000::prepare("A unique owned replay fixture").unwrap();
        let digest = prepared.digest;
        let checked = Checked::evaluate(&mut tx, &b, actor, prepared, now).await.unwrap();
        assert_eq!(checked.decision(), &Decision::Allow);
        sqlx::query("ROLLBACK TO SAVEPOINT owned_post").execute(&mut *tx).await.unwrap();
        checked.replay(&mut tx).await.unwrap();
        tx.commit().await.unwrap();
        let saved: DateTime<Utc> = sqlx::query_scalar("SELECT seen_at FROM post_secrets.robot9000_texts WHERE board=$1 AND digest=$2")
            .bind(&b).bind(digest.as_slice()).fetch_one(&o).await.unwrap();
        assert_eq!(saved, now);
        let mute: (i16, DateTime<Utc>) = sqlx::query_as("SELECT timeout_power,next_expire FROM post_secrets.robot9000_mutes WHERE board=$1 AND actor=$2")
            .bind(&b).bind(actor.as_slice()).fetch_one(&o).await.unwrap();
        assert_eq!(mute, (2, now + chrono::Duration::days(1)));
        let posts: i64 = sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=$1")
            .bind(&b).fetch_one(&o).await.unwrap();
        assert_eq!(posts, 0);

        // Replaying without first discarding the original check is not a
        // second successful allowance. Detect the changed decision and abort.
        let mut tx = p.begin().await.unwrap();
        let prepared = board_domain::robot9000::prepare("Another distinct owned replay fixture").unwrap();
        let digest = prepared.digest;
        let checked = Checked::evaluate(&mut tx, &b, actor, prepared, now).await.unwrap();
        assert_eq!(checked.decision(), &Decision::Allow);
        assert!(matches!(checked.replay(&mut tx).await,
            Err(StoreError::Database(sqlx::Error::Protocol(_)))));
        tx.rollback().await.unwrap();
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM post_secrets.robot9000_texts WHERE board=$1 AND digest=$2)")
            .bind(&b).bind(digest.as_slice()).fetch_one(&o).await.unwrap();
        assert!(!exists);
        let power: i16 = sqlx::query_scalar("SELECT timeout_power FROM post_secrets.robot9000_mutes WHERE board=$1 AND actor=$2")
            .bind(&b).bind(actor.as_slice()).fetch_one(&o).await.unwrap();
        assert_eq!(power, 2);
    }).await;
    sqlx::query("DELETE FROM content.boards WHERE slug=$1")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    result.unwrap();
}

#[tokio::test]
async fn country_refusal_and_replayed_originality_share_one_commit() {
    use crate::anonymous_session::PostingSession;
    use board_domain::anonymous_session::Fingerprints;
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = crate::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT 'rc'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    let token: Vec<u8> =
        sqlx::query_scalar("SELECT sha256(convert_to(gen_random_uuid()::text,'UTF8'))")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,robot9000,captcha_enabled,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Owned country replay','Synthetic',true,true,2000,100,100,100,10)")
        .bind(&board).execute(&owner).await.unwrap();
    let now = Utc::now().with_nanosecond(0).unwrap();
    sqlx::query("INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,created_at,network_at,address_at,environment_at,activity_at,expires_at) VALUES($1,$1,$1,$1,$2-3600,$2-3600,$2-3600,$2-3600,$2-60,$2+3600)")
        .bind(&token).bind(now.timestamp()).execute(&owner).await.unwrap();
    let (o, p, b, t) = (owner.clone(), public.clone(), board.clone(), token.clone());
    let result = tokio::spawn(async move {
        let hash: [u8;32] = t.clone().try_into().unwrap();
        let session = PostingSession { fingerprints: Fingerprints {
            token:hash, network:hash, address:hash, environment:[9;32],
        }, minted:false, now };
        // A rolled-back refusal must also discard the replayed originality.
        for commit in [false,true] {
            let mut tx = p.begin().await.unwrap();
            sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED").execute(&mut *tx).await.unwrap();
            sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE").bind(&b).execute(&mut *tx).await.unwrap();
            sqlx::query("SAVEPOINT owned_post").execute(&mut *tx).await.unwrap();
            let prepared = board_domain::robot9000::prepare("Owned combined country and originality refusal").unwrap();
            let checked = Checked::evaluate(&mut tx,&b,[42;32],prepared,now).await.unwrap();
            assert_eq!(checked.decision(), &Decision::Allow);
            sqlx::query("ROLLBACK TO SAVEPOINT owned_post").execute(&mut *tx).await.unwrap();
            checked.replay(&mut tx).await.unwrap();
            crate::posting_classifiers::record_country_refusal_in_transaction(&mut tx,&b,&session).await.unwrap();
            // The second connection must see neither uncommitted write.
            let visible: (i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM post_secrets.robot9000_texts WHERE board=$1),(SELECT count(*) FROM post_secrets.classifier_history WHERE token_hash=$2)")
                .bind(&b).bind(&t).fetch_one(&o).await.unwrap();
            assert_eq!(visible,(0,0));
            if commit { tx.commit().await.unwrap(); } else { tx.rollback().await.unwrap(); }
            let saved: (i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM post_secrets.robot9000_texts WHERE board=$1),(SELECT count(*) FROM post_secrets.classifier_history WHERE token_hash=$2)")
                .bind(&b).bind(&t).fetch_one(&o).await.unwrap();
            assert_eq!(saved,if commit {(1,1)} else {(0,0)});
            let state: (i64,i16,i16) = sqlx::query_as("SELECT activity_at,posts,pending FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
                .bind(&t).fetch_one(&o).await.unwrap();
            assert_eq!(state,(now.timestamp()-60,0,0));
        }
        let posts: i64 = sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=$1").bind(&b).fetch_one(&o).await.unwrap();
        assert_eq!(posts,0);
    }).await;
    sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
        .bind(&token)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.boards WHERE slug=$1")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    result.unwrap();
}
