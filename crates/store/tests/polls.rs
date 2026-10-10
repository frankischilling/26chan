#![cfg(feature = "database-tests")]

use board_store::{POLL_READINESS_SQL, StoreError, poll_catalogue, poll_snapshot};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::time::Duration;

async fn pool(variable: &str, role: &str) -> PgPool {
    let pool = PgPoolOptions::new()
        .max_connections(3)
        .connect(&std::env::var(variable).expect("owned database URL required"))
        .await
        .unwrap();
    let actual: (String, String) = sqlx::query_as("SELECT current_user::text,session_user::text")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(actual, (role.into(), role.into()), "actual login required");
    pool
}

async fn next_id(owner: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(owner)
        .await
        .unwrap()
}

fn code(error: &sqlx::Error) -> String {
    error
        .as_database_error()
        .unwrap()
        .code()
        .unwrap()
        .into_owned()
}

#[tokio::test]
async fn actual_public_reads_are_ordered_bounded_private_and_immutable() {
    let owner = pool("MIGRATION_DATABASE_URL", "board_migrator").await;
    let public = pool("TEST_PUBLIC_DATABASE_URL", "board_public").await;
    // Shared with HTTP poll fixtures. Hold one dedicated connection, leaving
    // the pool free for fixture writes and cleanup after a failed assertion.
    let mut guard = owner.acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock(2250109)")
        .execute(&mut *guard)
        .await
        .unwrap();
    let baseline = poll_catalogue(&public).await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM poll_private.polls")
        .fetch_one(&owner)
        .await
        .unwrap();
    if count == 0 {
        assert!(baseline.is_empty(), "fresh migration must not seed polls");
    }
    let ids = [
        next_id(&owner).await,
        next_id(&owner).await,
        next_id(&owner).await,
    ];
    let ordinals: Vec<i32> = sqlx::query_scalar(
        "SELECT n FROM generate_series(1,200) n WHERE NOT EXISTS \
         (SELECT 1 FROM poll_private.polls p WHERE p.catalogue_ordinal=n) ORDER BY n DESC LIMIT 2",
    )
    .fetch_all(&owner)
    .await
    .unwrap();
    assert_eq!(
        ordinals.len(),
        2,
        "poll fixture needs two unused catalogue slots"
    );
    for (id, published, ordinal) in [
        (ids[0], true, Some(ordinals[0])),
        (ids[1], true, Some(ordinals[1])),
        (ids[2], false, None),
    ] {
        sqlx::query("INSERT INTO poll_private.polls(id,title,description,vote_count,published,catalogue_ordinal) VALUES($1,'Owned poll','Owned description',10,$2,$3)")
            .bind(id).bind(published).bind(ordinal).execute(&owner).await.unwrap();
        sqlx::query("INSERT INTO poll_private.options(poll_id,id,ordinal,caption,score) VALUES($1,90,2,'Second',0),($1,3,1,'First',NULL)")
            .bind(id).execute(&owner).await.unwrap();
    }
    let test_owner = owner.clone();
    let test_public = public.clone();
    let outcome = tokio::spawn(async move {
        let catalogue = poll_catalogue(&test_public).await.unwrap();
        let owned: Vec<i64> = catalogue.iter().filter(|row| ids.contains(&row.id)).map(|row| row.id).collect();
        assert_eq!(owned, vec![ids[1], ids[0]]);
        let snapshot = poll_snapshot(&test_public, ids[0]).await.unwrap();
        assert_eq!(snapshot.vote_count, 10);
        assert!(!snapshot.accepting_votes, "historical polls must stay closed after 0128");
        assert_eq!(snapshot.description, "Owned description");
        assert_eq!(snapshot.options.iter().map(|o| o.id).collect::<Vec<_>>(), vec![3,90]);
        assert_eq!(snapshot.options[0].score, None);
        assert_eq!(snapshot.options[1].score, Some(0));
        for id in [ids[2], 0, -1, i64::MAX] {
            assert!(matches!(poll_snapshot(&test_public,id).await, Err(StoreError::NotFound)));
        }
        let hidden: i64 = sqlx::query_scalar("SELECT count(*) FROM content.published_poll_options WHERE poll_id=$1")
            .bind(ids[2]).fetch_one(&test_public).await.unwrap();
        assert_eq!(hidden, 0);
        sqlx::query("UPDATE poll_private.polls SET catalogue_ordinal=NULL WHERE id=$1")
            .bind(ids[0]).execute(&test_owner).await.unwrap();
        assert!(poll_snapshot(&test_public,ids[0]).await.is_ok());
        assert!(!poll_catalogue(&test_public).await.unwrap().iter().any(|p| p.id==ids[0]));
        for statement in [
            "SELECT * FROM poll_private.polls", "SELECT * FROM poll_private.options",
            "UPDATE content.published_polls SET title='forbidden'",
            "DELETE FROM content.published_polls",
            "INSERT INTO content.published_polls(id,title,description,vote_count) VALUES(9223372036854775807,'x','',0)",
        ] {
            assert_eq!(code(&sqlx::query(statement).execute(&test_public).await.unwrap_err()), "42501");
        }
        let ready: bool = sqlx::query_scalar(POLL_READINESS_SQL).fetch_one(&test_public).await.unwrap();
        assert!(ready);
        for grant in [
            "GRANT UPDATE(title) ON content.published_polls TO board_public",
            "GRANT SELECT(id) ON poll_private.polls TO board_public",
            "GRANT UPDATE(score) ON content.published_poll_options TO board_public",
            "GRANT USAGE ON SCHEMA poll_private TO board_public",
            "GRANT USAGE ON SCHEMA poll_private TO board_staff",
            "GRANT USAGE ON SCHEMA poll_private TO board_auth",
            "GRANT SELECT ON poll_private.polls TO board_staff",
            "GRANT SELECT(caption) ON poll_private.options TO board_auth",
            "GRANT UPDATE(score) ON poll_private.options TO board_staff",
            "GRANT SELECT ON content.published_polls TO board_staff",
            "GRANT SELECT(caption) ON content.published_poll_options TO board_auth",
            "GRANT USAGE ON SCHEMA poll_private TO PUBLIC",
            "GRANT SELECT ON poll_private.polls TO PUBLIC",
            "GRANT SELECT(score) ON poll_private.options TO PUBLIC",
            "GRANT SELECT ON content.published_polls TO PUBLIC",
            "GRANT SELECT(caption) ON content.published_poll_options TO PUBLIC",
            "GRANT SELECT ON content.published_polls TO board_public WITH GRANT OPTION",
            "GRANT SELECT(caption) ON content.published_poll_options TO board_public WITH GRANT OPTION",
            "ALTER VIEW content.published_polls SET (security_barrier=false)",
            "CREATE OR REPLACE VIEW content.published_polls WITH (security_barrier=true) AS SELECT id,title,description,vote_count,catalogue_ordinal,accepting_votes FROM poll_private.polls",
            "CREATE OR REPLACE VIEW content.published_poll_options WITH (security_barrier=true) AS SELECT o.poll_id,o.id,o.ordinal,o.caption,o.score FROM poll_private.options o JOIN poll_private.polls p ON p.id=o.poll_id",
            "ALTER TABLE content.boards DROP CONSTRAINT boards_reserved_polls_route",
            "ALTER TABLE poll_private.polls DROP CONSTRAINT polls_title_check",
            "ALTER TABLE poll_private.options DROP CONSTRAINT options_ordinal_check",
            "ALTER TABLE poll_private.options DROP CONSTRAINT options_poll_id_ordinal_key",
        ] {
            let mut tx = test_owner.begin().await.unwrap();
            sqlx::query(grant).execute(&mut *tx).await.unwrap();
            let ready: bool = sqlx::query_scalar(POLL_READINESS_SQL).fetch_one(&mut *tx).await.unwrap();
            assert!(!ready, "readiness accepted {grant}");
            tx.rollback().await.unwrap();
        }
        // A maximum-length UTF-8 value uses bytes, not Unicode scalar count.
        sqlx::query("UPDATE poll_private.polls SET title=$2,description=$3,vote_count=1000000000 WHERE id=$1")
            .bind(ids[0]).bind("é".repeat(256)).bind("é".repeat(8192)).execute(&test_owner).await.unwrap();
        sqlx::query("DELETE FROM poll_private.options WHERE poll_id=$1")
            .bind(ids[0]).execute(&test_owner).await.unwrap();
        sqlx::query("INSERT INTO poll_private.options(poll_id,id,ordinal,caption,score) SELECT $1,n,n,repeat('é',512),1000000000 FROM generate_series(1,128) n")
            .bind(ids[0]).execute(&test_owner).await.unwrap();
        let bounded = poll_snapshot(&test_public,ids[0]).await.unwrap();
        assert_eq!(bounded.title.len(),512);
        assert_eq!(bounded.description.len(),16384);
        assert_eq!(bounded.options.len(),128);
        assert_eq!(bounded.options[127].caption.len(),1024);
        assert_eq!(bounded.options[127].score,Some(1_000_000_000));
        sqlx::query("DELETE FROM poll_private.options WHERE poll_id=$1")
            .bind(ids[1]).execute(&test_owner).await.unwrap();
        assert!(poll_snapshot(&test_public,ids[1]).await.unwrap().options.is_empty());
    }).await;
    sqlx::query("DELETE FROM poll_private.polls WHERE id=ANY($1)")
        .bind(ids.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("SELECT pg_advisory_unlock(2250109)")
        .execute(&mut *guard)
        .await
        .unwrap();
    outcome.unwrap();
}

#[tokio::test]
async fn operator_constraints_reject_unbounded_or_ambiguous_projection() {
    let owner = pool("MIGRATION_DATABASE_URL", "board_migrator").await;
    let mut guard = owner.acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock(2250109)")
        .execute(&mut *guard)
        .await
        .unwrap();
    let id = next_id(&owner).await;
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SAVEPOINT reserved_poll_route")
        .execute(&mut *tx)
        .await
        .unwrap();
    let error = sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES('polls','Owned conflict','',16000,100,100,100,10)")
        .execute(&mut *tx).await.unwrap_err();
    assert_eq!(code(&error), "23514");
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("boards_reserved_polls_route")
    );
    sqlx::query("ROLLBACK TO SAVEPOINT reserved_poll_route")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO poll_private.polls(id,title,description,vote_count) VALUES($1,'','',0)",
    )
    .bind(id)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query("INSERT INTO poll_private.options(poll_id,id,ordinal,caption) VALUES($1,1,1,'')")
        .bind(id)
        .execute(&mut *tx)
        .await
        .unwrap();
    for statement in [
        "UPDATE poll_private.polls SET id=0 WHERE id=$1",
        "UPDATE poll_private.polls SET title=repeat('é',257) WHERE id=$1",
        "UPDATE poll_private.polls SET description=repeat('é',8193) WHERE id=$1",
        "UPDATE poll_private.polls SET vote_count=-1 WHERE id=$1",
        "UPDATE poll_private.polls SET vote_count=1000000001 WHERE id=$1",
        "UPDATE poll_private.polls SET catalogue_ordinal=0 WHERE id=$1",
        "UPDATE poll_private.polls SET catalogue_ordinal=201 WHERE id=$1",
        "UPDATE poll_private.options SET caption=repeat('é',513) WHERE poll_id=$1",
        "UPDATE poll_private.options SET score=-1 WHERE poll_id=$1",
        "UPDATE poll_private.options SET score=1000000001 WHERE poll_id=$1",
        "UPDATE poll_private.options SET id=0 WHERE poll_id=$1",
        "UPDATE poll_private.options SET ordinal=0 WHERE poll_id=$1",
        "UPDATE poll_private.options SET ordinal=129 WHERE poll_id=$1",
    ] {
        sqlx::query("SAVEPOINT invalid_poll")
            .execute(&mut *tx)
            .await
            .unwrap();
        let error = sqlx::query(statement)
            .bind(id)
            .execute(&mut *tx)
            .await
            .unwrap_err();
        assert_eq!(code(&error), "23514", "{statement}");
        sqlx::query("ROLLBACK TO SAVEPOINT invalid_poll")
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    sqlx::query("SAVEPOINT duplicate_option")
        .execute(&mut *tx)
        .await
        .unwrap();
    let error = sqlx::query(
        "INSERT INTO poll_private.options(poll_id,id,ordinal,caption) VALUES($1,2,1,'duplicate')",
    )
    .bind(id)
    .execute(&mut *tx)
    .await
    .unwrap_err();
    assert_eq!(code(&error), "23505");
    tx.rollback().await.unwrap();
    sqlx::query("SELECT pg_advisory_unlock(2250109)")
        .execute(&mut *guard)
        .await
        .unwrap();
}

#[tokio::test]
async fn metadata_and_options_share_repeatable_read_during_operator_update() {
    let owner = pool("MIGRATION_DATABASE_URL", "board_migrator").await;
    let public = PgPoolOptions::new()
        .max_connections(1)
        .connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let actual: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&public)
        .await
        .unwrap();
    assert_eq!(actual, "board_public");
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&public)
        .await
        .unwrap();
    let mut guard = owner.acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock(2250109)")
        .execute(&mut *guard)
        .await
        .unwrap();
    let id = next_id(&owner).await;
    sqlx::query("INSERT INTO poll_private.polls(id,title,description,vote_count,published) VALUES($1,'Before','',1,true)")
        .bind(id).execute(&owner).await.unwrap();
    sqlx::query("INSERT INTO poll_private.options(poll_id,id,ordinal,caption,score) VALUES($1,1,1,'Before',1)")
        .bind(id).execute(&owner).await.unwrap();
    let test_owner = owner.clone();
    let outcome = tokio::spawn(async move {
        let mut writer = test_owner.begin().await.unwrap();
        // Only the second SELECT needs options. Wait for its blocked relation
        // lock before changing both projections in this writer transaction.
        sqlx::query("LOCK TABLE poll_private.options IN ACCESS EXCLUSIVE MODE").execute(&mut *writer).await.unwrap();
        let reader = tokio::spawn(async move { poll_snapshot(&public,id).await });
        let blocked = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_locks WHERE pid=$1 AND relation='poll_private.options'::regclass AND NOT granted)")
                    .bind(pid).fetch_one(&test_owner).await.unwrap();
                if waiting { break; }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await;
        if blocked.is_err() {
            reader.abort();
            writer.rollback().await.unwrap();
            panic!("reader never reached its option query");
        }
        sqlx::query("UPDATE poll_private.polls SET title='After',vote_count=2 WHERE id=$1")
            .bind(id).execute(&mut *writer).await.unwrap();
        sqlx::query("UPDATE poll_private.options SET caption='After',score=2 WHERE poll_id=$1")
            .bind(id).execute(&mut *writer).await.unwrap();
        writer.commit().await.unwrap();
        let snapshot = reader.await.unwrap().unwrap();
        assert_eq!(snapshot.title,"Before");
        assert_eq!(snapshot.vote_count,1);
        assert_eq!(snapshot.options[0].caption,"Before");
        assert_eq!(snapshot.options[0].score,Some(1));
    }).await;
    sqlx::query("DELETE FROM poll_private.polls WHERE id=$1")
        .bind(id)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("SELECT pg_advisory_unlock(2250109)")
        .execute(&mut *guard)
        .await
        .unwrap();
    outcome.unwrap();
}
