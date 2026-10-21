#![cfg(feature = "database-tests")]
mod support;
use board_store::{NewPost, PostingContext, StoreError};
use sqlx::PgPool;

fn post(body: &str) -> NewPost {
    NewPost {
        name: "Anonymous".into(),
        subject: "Owned comment admission".into(),
        comment: body.into(),
        deletion_hash: "owned-fixture".into(),
        sage: false,
    }
}
async fn write(
    pool: &PgPool,
    board: &str,
    parent: i64,
    body: &str,
    peer: u8,
    time: i64,
) -> Result<i64, StoreError> {
    support::create_post_with_context(
        pool,
        board,
        parent,
        &post(body),
        None,
        PostingContext {
            request_start: chrono::DateTime::from_timestamp(time, 0).unwrap(),
            peer: Some([192, 0, 2, peer].into()),
            op_password_proof: None,
        },
    )
    .await
}
fn refused(result: Result<i64, StoreError>, seconds: i64) {
    match result {
        Err(StoreError::ContentRejected(message)) => {
            assert_eq!(message, board_domain::comment_duplicates::refusal(seconds))
        }
        other => panic!("Expected duplicate comment refusal: {other:?}"),
    }
}
#[tokio::test]
async fn source_boundaries_scope_concurrency_and_post_clock() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let staff = PgPool::connect(&std::env::var("STAFF_DATABASE_URL").unwrap())
        .await
        .unwrap();
    assert!(
        sqlx::query_scalar::<_, bool>(board_store::COMMENT_DUPLICATE_READINESS_SQL)
            .fetch_one(&staff)
            .await
            .unwrap(),
        "Staff readiness must not require private schema usage"
    );
    assert!(
        !sqlx::query_scalar::<_, bool>(
            "SELECT has_schema_privilege(current_user,'post_secrets','USAGE')"
        )
        .fetch_one(&staff)
        .await
        .unwrap(),
        "Readiness must not expand staff authority"
    );
    staff.close().await;
    let seed: String = sqlx::query_scalar("SELECT replace(gen_random_uuid()::text,'-','')")
        .fetch_one(&owner)
        .await
        .unwrap();
    let board = format!("d{}", &seed[..9]);
    sqlx::query("INSERT INTO content.boards(slug,title,description,captcha_enabled,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,posting_reply_seconds,posting_image_seconds,posting_thread_seconds) VALUES($1,'Owned duplicate fixture','Synthetic',false,2000,1000,1000,1000,10,0,0,0)").bind(&board).execute(&owner).await.unwrap();
    let now: i64 =
        sqlx::query_scalar("SELECT floor(extract(epoch FROM clock_timestamp()))::bigint")
            .fetch_one(&owner)
            .await
            .unwrap();
    let op = write(&public, &board, 0, "Owned OP", 1, now).await.unwrap();
    write(&public, &board, op, "Owned\r\nreply", 1, now)
        .await
        .unwrap();
    refused(
        write(&public, &board, op, "Owned\nreply", 1, now + 299).await,
        1,
    );
    write(&public, &board, op, "Owned\nreply", 1, now + 300)
        .await
        .unwrap();
    write(&public, &board, op, "Owned\nreply", 2, now + 300)
        .await
        .unwrap();
    let shifted = write(&public, &board, op, "Owned shifted clock", 3, now)
        .await
        .unwrap();
    sqlx::query("UPDATE content.posts SET created_at=to_timestamp($2) WHERE id=$1")
        .bind(shifted)
        .bind((now + 40) as f64)
        .execute(&owner)
        .await
        .unwrap();
    refused(
        write(&public, &board, op, "Owned shifted clock", 3, now + 300).await,
        40,
    );
    let (one, two) = tokio::join!(
        write(&public, &board, op, "Owned simultaneous", 4, now),
        write(&public, &board, op, "Owned simultaneous", 4, now)
    );
    assert_eq!(usize::from(one.is_ok()) + usize::from(two.is_ok()), 1);
    if one.is_err() {
        refused(one, 300);
    } else {
        refused(two, 300);
    }
    sqlx::query("UPDATE content.boards SET robot9000=true WHERE slug=$1")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    write(
        &public,
        &board,
        op,
        "Owned nested savepoint registration",
        5,
        now,
    )
    .await
    .unwrap();
    sqlx::query("UPDATE content.boards SET robot9000=false WHERE slug=$1")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    let key = support::key(&board);
    let actor = key.public_posting_rate_identity([192, 0, 2, 1].into());
    let mut registration = public.begin().await.unwrap();
    let refused_old = sqlx::query("SELECT content.record_comment_projection($1,$2,$3)")
        .bind(op)
        .bind(actor.as_bytes().as_slice())
        .bind([1_u8; 32].as_slice())
        .execute(&mut *registration)
        .await;
    assert!(
        refused_old.is_err(),
        "a later transaction must not invent historical comparison data"
    );
    registration.rollback().await.unwrap();
    assert!(
        sqlx::query("SELECT * FROM post_secrets.comment_history")
            .fetch_all(&public)
            .await
            .is_err()
    );
    sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 day' WHERE id=$1")
        .bind(op)
        .execute(&owner)
        .await
        .unwrap();
    refused(write(&public, &board, 0, "Owned OP", 1, now + 1).await, 299);
    sqlx::query("UPDATE content.threads SET deleted=true WHERE id=$1")
        .bind(op)
        .execute(&owner)
        .await
        .unwrap();
    write(&public, &board, 0, "Owned OP", 1, now + 2)
        .await
        .unwrap();
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
