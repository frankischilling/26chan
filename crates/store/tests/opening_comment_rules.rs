#![cfg(feature = "database-tests")]
mod support;
use board_store::{NewPost, PostingContext, StoreError};
use sqlx::PgPool;

async fn write(
    pool: &PgPool,
    board: &str,
    parent: i64,
    body: &str,
    peer: u8,
    now: i64,
) -> Result<i64, StoreError> {
    support::create_post_with_context(
        pool,
        board,
        parent,
        &NewPost {
            name: "Anonymous".into(),
            subject: "Owned final comment".into(),
            comment: body.into(),
            deletion_hash: "owned-fixture".into(),
            sage: false,
        },
        None,
        PostingContext {
            request_start: chrono::DateTime::from_timestamp(now, 0).unwrap(),
            peer: Some([192, 0, 2, peer].into()),
            op_password_proof: None,
        },
    )
    .await
}

#[tokio::test]
async fn imported_policies_and_actual_writer_preserve_final_rule_order() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let reference: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/opening-comment-source.json"
    ))
    .unwrap();
    for (slug, policy) in reference["boards"].as_object().unwrap() {
        let actual: (String,String) = sqlx::query_as("SELECT opening_comment_pattern,opening_comment_refusal FROM content.boards WHERE slug=$1")
            .bind(slug).fetch_one(&owner).await.unwrap();
        assert_eq!(actual.0, policy["pattern"].as_str().unwrap(), "{slug}");
        assert_eq!(actual.1, policy["refusal"].as_str().unwrap(), "{slug}");
    }
    let seed: String = sqlx::query_scalar("SELECT replace(gen_random_uuid()::text,'-','')")
        .fetch_one(&owner)
        .await
        .unwrap();
    let board = format!("u{}", &seed[..9]);
    sqlx::query("INSERT INTO content.boards(slug,title,description,captcha_enabled,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,posting_reply_seconds,posting_image_seconds,posting_thread_seconds,opening_comment_pattern,opening_comment_refusal) VALUES($1,'Owned final rule','Synthetic',false,2000,1000,1000,1000,10,0,0,60,'@https?://@','Owned URL required')")
        .bind(&board).execute(&owner).await.unwrap();
    let now: i64 =
        sqlx::query_scalar("SELECT floor(extract(epoch FROM clock_timestamp()))::bigint")
            .fetch_one(&owner)
            .await
            .unwrap();
    for (index, body) in ["body", "HTTP://example.test", "https:/ /example.test"]
        .iter()
        .enumerate()
    {
        assert!(
            matches!(write(&public,&board,0,body,index as u8+1,now).await, Err(StoreError::ContentRejected(message)) if message=="Owned URL required")
        );
    }
    let op = write(&public, &board, 0, "https://example.test/owned", 10, now)
        .await
        .unwrap();
    // The final pattern precedes the recent-thread cooldown for this same actor.
    assert!(
        matches!(write(&public,&board,0,"No link after previous OP",10,now).await, Err(StoreError::ContentRejected(message)) if message=="Owned URL required")
    );
    assert!(
        write(&public, &board, 0, "https://example.test/retry", 10, now)
            .await
            .is_err()
    );
    write(&public, &board, op, "Replies need no URL", 11, now)
        .await
        .unwrap();
    // A rule runs on the source formatter output: escaping changes the text.
    sqlx::query("UPDATE content.boards SET opening_comment_pattern='@&lt;tag&gt;@' WHERE slug=$1")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    write(&public, &board, 0, "<tag>", 12, now).await.unwrap();
    sqlx::query("UPDATE content.boards SET opening_comment_pattern='/broken' WHERE slug=$1")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    assert!(
        write(&public, &board, 0, "Invalid pattern cannot allow", 13, now)
            .await
            .is_err()
    );
    write(
        &public,
        &board,
        op,
        "Malformed OP policy does not affect replies",
        14,
        now,
    )
    .await
    .unwrap();
    sqlx::query("UPDATE content.boards SET opening_comment_pattern='0' WHERE slug=$1")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    write(&public, &board, 0, "PHP false disables the rule", 15, now)
        .await
        .unwrap();
    assert!(
        sqlx::query("UPDATE content.boards SET opening_comment_pattern='' WHERE slug=$1")
            .bind(&board)
            .execute(&public)
            .await
            .is_err()
    );
    // Observe an actual wait on the board policy lock, then verify the writer
    // sees the newly committed rule rather than its earlier connection snapshot.
    let mut held = owner.begin().await.unwrap();
    let holder: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *held)
        .await
        .unwrap();
    sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
        .bind(&board)
        .fetch_one(&mut *held)
        .await
        .unwrap();
    let waiting_pool = public.clone();
    let waiting_board = board.clone();
    let writer = tokio::spawn(async move {
        write(
            &waiting_pool,
            &waiting_board,
            0,
            "Concurrent body without URL",
            18,
            now,
        )
        .await
    });
    let mut observed = false;
    for _ in 0..100 {
        observed = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE usename='board_public' AND $1=ANY(pg_blocking_pids(pid)))").bind(holder).fetch_one(&owner).await.unwrap();
        if observed {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    sqlx::query("UPDATE content.boards SET opening_comment_pattern='@https?://@' WHERE slug=$1")
        .bind(&board)
        .execute(&mut *held)
        .await
        .unwrap();
    held.commit().await.unwrap();
    let result = writer.await.unwrap();
    assert!(observed, "Writer did not wait on the owned board lock");
    assert!(
        matches!(result,Err(StoreError::ContentRejected(message)) if message=="Owned URL required")
    );
    // A configured quiet filter exits from postfilter.php itself, before the
    // caller's COM_REGEX check. It is not the later $silent_reject flood flag.
    sqlx::query("UPDATE content.boards SET opening_comment_pattern='@https?://@' WHERE slug=$1")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    let rule: i64 = sqlx::query_scalar("INSERT INTO admission.rules(board,pattern,quiet) VALUES($1,'ownedquiet',true) RETURNING id").bind(&board).fetch_one(&owner).await.unwrap();
    assert!(matches!(
        write(&public, &board, 0, "ownedquiet without link", 16, now).await,
        Err(StoreError::ContentQuiet { .. })
    ));
    sqlx::query("UPDATE content.boards SET opening_comment_pattern='/broken' WHERE slug=$1")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    assert!(matches!(
        write(
            &public,
            &board,
            0,
            "ownedquiet invalid later policy",
            17,
            now
        )
        .await,
        Err(StoreError::ContentQuiet { .. })
    ));
    sqlx::query("DELETE FROM admission.hits WHERE board=$1")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM admission.rules WHERE id=$1")
        .bind(rule)
        .execute(&owner)
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=$1")
        .bind(&board)
        .fetch_one(&owner)
        .await
        .unwrap();
    assert_eq!(count, 5);
    let mut tx = support::begin_cleanup(&owner, std::slice::from_ref(&board)).await;
    support::cleanup_posting(&mut *tx, &board).await;
    for query in [
        "DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
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
    public.close().await;
    owner.close().await;
}
