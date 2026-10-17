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
