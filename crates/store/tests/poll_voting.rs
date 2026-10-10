#![cfg(feature = "database-tests")]

use board_store::{
    POLL_READINESS_SQL, POLL_VOTE_READINESS_SQL, PollVoteOutcome, StoreError, cast_poll_vote,
    has_poll_vote, poll_catalogue, poll_snapshot,
};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::time::Duration;

async fn login(variable: &str, role: &str) -> PgPool {
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&std::env::var(variable).expect("owned database URL required"))
        .await
        .unwrap();
    let actual: (String, String) = sqlx::query_as("SELECT current_user::text,session_user::text")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(actual, (role.into(), role.into()));
    pool
}

async fn next_id(owner: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(owner)
        .await
        .unwrap()
}

fn voter(seed: u8) -> [u8; 32] {
    [seed; 32]
}

async fn seed(owner: &PgPool) -> [i64; 4] {
    let ids = [
        next_id(owner).await,
        next_id(owner).await,
        next_id(owner).await,
        next_id(owner).await,
    ];
    for (id, published, accepting) in [
        (ids[0], true, true),
        (ids[1], false, true),
        (ids[2], true, false),
        (ids[3], true, true),
    ] {
        sqlx::query(
            "INSERT INTO poll_private.polls(id,title,description,vote_count,published,accepting_votes,vote_capacity)
             VALUES($1,'Owned poll ballot','Historical aggregate',7,$2,$3,3)",
        )
        .bind(id)
        .bind(published)
        .bind(accepting)
        .execute(owner)
        .await
        .unwrap();
    }
    for (id, option, ordinal, score) in [
        (ids[0], 11_i64, 1_i32, 2_i64),
        (ids[0], 22, 2, 3),
        (ids[1], 31, 1, 1),
        (ids[2], 41, 1, 1),
        (ids[3], 99, 1, 0),
    ] {
        sqlx::query(
            "INSERT INTO poll_private.options(poll_id,id,ordinal,caption,score)
             VALUES($1,$2,$3,'Owned option',$4)",
        )
        .bind(id)
        .bind(option)
        .bind(ordinal)
        .bind(score)
        .execute(owner)
        .await
        .unwrap();
    }
    ids
}

async fn totals(owner: &PgPool, id: i64) -> (i64, i32, Vec<(i64, Option<i64>)>, i64) {
    let (votes, added): (i64, i32) =
        sqlx::query_as("SELECT vote_count,new_vote_count FROM poll_private.polls WHERE id=$1")
            .bind(id)
            .fetch_one(owner)
            .await
            .unwrap();
    let options: Vec<(i64, Option<i64>)> = sqlx::query_as(
        "SELECT id,score FROM poll_private.options WHERE poll_id=$1 ORDER BY ordinal",
    )
    .bind(id)
    .fetch_all(owner)
    .await
    .unwrap();
    let receipts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM poll_private.votes WHERE poll_id=$1")
            .bind(id)
            .fetch_one(owner)
            .await
            .unwrap();
    (votes, added, options, receipts)
}

#[tokio::test]
async fn votes_deduplicate_validate_scope_and_preserve_historical_tallies() {
    let owner = login("MIGRATION_DATABASE_URL", "board_migrator").await;
    let public = login("TEST_PUBLIC_DATABASE_URL", "board_public").await;
    let mut lock = owner.acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock(2250109)")
        .execute(&mut *lock)
        .await
        .unwrap();
    let ids = seed(&owner).await;
    let owner_task = owner.clone();
    let public_task = public.clone();
    let result = tokio::spawn(async move {
        let snapshot = poll_snapshot(&public_task, ids[0]).await.unwrap();
        assert!(snapshot.accepting_votes);
        assert!(
            !poll_snapshot(&public_task, ids[2])
                .await
                .unwrap()
                .accepting_votes
        );
        assert!(
            !poll_catalogue(&public_task)
                .await
                .unwrap()
                .iter()
                .any(|poll| ids.contains(&poll.id))
        );
        assert!(
            !has_poll_vote(&public_task, ids[0], &voter(1))
                .await
                .unwrap()
        );
        for hidden in [ids[1], i64::MAX, -1, 0] {
            assert!(matches!(
                cast_poll_vote(&public_task, hidden, 31, &voter(1)).await,
                Err(StoreError::NotFound)
            ));
            assert!(matches!(
                has_poll_vote(&public_task, hidden, &voter(1)).await,
                Err(StoreError::NotFound)
            ));
        }
        assert_eq!(
            cast_poll_vote(&public_task, ids[2], 41, &voter(2))
                .await
                .unwrap(),
            PollVoteOutcome::Closed
        );
        for foreign in [99, 31, -1, 0, i64::MAX] {
            assert_eq!(
                cast_poll_vote(&public_task, ids[0], foreign, &voter(3))
                    .await
                    .unwrap(),
                PollVoteOutcome::InvalidOption
            );
        }
        assert_eq!(totals(&owner_task, ids[0]).await.3, 0);
        assert_eq!(
            cast_poll_vote(&public_task, ids[0], 11, &voter(3))
                .await
                .unwrap(),
            PollVoteOutcome::Recorded
        );
        assert!(
            has_poll_vote(&public_task, ids[0], &voter(3))
                .await
                .unwrap()
        );
        // Replays remain idempotent even if the submitted option changes.
        assert_eq!(
            cast_poll_vote(&public_task, ids[0], 22, &voter(3))
                .await
                .unwrap(),
            PollVoteOutcome::AlreadyVoted
        );
        let (votes, added, options, receipts) = totals(&owner_task, ids[0]).await;
        assert_eq!((votes, added, receipts), (8, 1, 1));
        assert_eq!(options, vec![(11, Some(3)), (22, Some(3))]);
        assert_eq!(
            cast_poll_vote(&public_task, ids[0], 22, &voter(4))
                .await
                .unwrap(),
            PollVoteOutcome::Recorded
        );
        assert_eq!(
            cast_poll_vote(&public_task, ids[0], 11, &voter(5))
                .await
                .unwrap(),
            PollVoteOutcome::Recorded
        );
        assert_eq!(
            cast_poll_vote(&public_task, ids[0], 11, &voter(6))
                .await
                .unwrap(),
            PollVoteOutcome::CapacityReached
        );
        assert_eq!(
            cast_poll_vote(&public_task, ids[0], 11, &voter(3))
                .await
                .unwrap(),
            PollVoteOutcome::AlreadyVoted
        );
        assert_eq!(
            totals(&owner_task, ids[0]).await,
            (10, 3, vec![(11, Some(4)), (22, Some(4))], 3)
        );
        assert_eq!(totals(&owner_task, ids[1]).await.3, 0);
        assert_eq!(totals(&owner_task, ids[2]).await.3, 0);
    })
    .await;
    sqlx::query("DELETE FROM poll_private.polls WHERE id=ANY($1)")
        .bind(ids.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("SELECT pg_advisory_unlock(2250109)")
        .execute(&mut *lock)
        .await
        .unwrap();
    result.unwrap();
}

#[tokio::test]
async fn parallel_votes_close_and_option_changes_are_serialized() {
    let owner = login("MIGRATION_DATABASE_URL", "board_migrator").await;
    let public = login("TEST_PUBLIC_DATABASE_URL", "board_public").await;
    let mut lock = owner.acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock(2250109)")
        .execute(&mut *lock)
        .await
        .unwrap();
    let ids = seed(&owner).await;
    let public_task = public.clone();
    let owner_task = owner.clone();
    let result = tokio::spawn(async move {
        let same_voter = voter(10);
        let (first, second) = tokio::join!(
            cast_poll_vote(&public_task, ids[0], 11, &same_voter),
            cast_poll_vote(&public_task, ids[0], 22, &same_voter)
        );
        assert_eq!(
            [first.unwrap(), second.unwrap()]
                .iter()
                .filter(|outcome| **outcome == PollVoteOutcome::Recorded)
                .count(),
            1
        );
        assert_eq!(totals(&owner_task, ids[0]).await.3, 1);
        let third_voter = voter(11);
        let fourth_voter = voter(12);
        let (third, fourth) = tokio::join!(
            cast_poll_vote(&public_task, ids[0], 11, &third_voter),
            cast_poll_vote(&public_task, ids[0], 22, &fourth_voter)
        );
        assert_eq!(third.unwrap(), PollVoteOutcome::Recorded);
        assert_eq!(fourth.unwrap(), PollVoteOutcome::Recorded);
        assert_eq!(totals(&owner_task, ids[0]).await.3, 3);
        // A recorded receipt remains idempotent after closing. New visitors
        // cannot vote while closed.
        sqlx::query("UPDATE poll_private.polls SET accepting_votes=false WHERE id=$1")
            .bind(ids[0])
            .execute(&owner_task)
            .await
            .unwrap();
        assert_eq!(
            cast_poll_vote(&public_task, ids[0], 22, &voter(10))
                .await
                .unwrap(),
            PollVoteOutcome::AlreadyVoted
        );
        assert_eq!(
            cast_poll_vote(&public_task, ids[0], 11, &voter(16))
                .await
                .unwrap(),
            PollVoteOutcome::Closed
        );
        sqlx::query("UPDATE poll_private.polls SET published=false WHERE id=$1")
            .bind(ids[0])
            .execute(&owner_task)
            .await
            .unwrap();
        assert!(matches!(
            has_poll_vote(&public_task, ids[0], &voter(10)).await,
            Err(StoreError::NotFound)
        ));
        assert!(matches!(
            cast_poll_vote(&public_task, ids[0], 22, &voter(10)).await,
            Err(StoreError::NotFound)
        ));

        // A maintenance transaction owns the poll before the next vote begins.
        let mut close = owner_task.begin().await.unwrap();
        sqlx::query("UPDATE poll_private.polls SET accepting_votes=false WHERE id=$1")
            .bind(ids[3])
            .execute(&mut *close)
            .await
            .unwrap();
        let pending = tokio::spawn({
            let public_task = public_task.clone();
            async move { cast_poll_vote(&public_task, ids[3], 99, &voter(13)).await }
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            !pending.is_finished(),
            "vote ignored the operator's poll lock"
        );
        close.commit().await.unwrap();
        assert_eq!(pending.await.unwrap().unwrap(), PollVoteOutcome::Closed);
        assert_eq!(totals(&owner_task, ids[3]).await.3, 0);

        sqlx::query("UPDATE poll_private.polls SET accepting_votes=true WHERE id=$1")
            .bind(ids[3])
            .execute(&owner_task)
            .await
            .unwrap();
        let mut unpublish = owner_task.begin().await.unwrap();
        sqlx::query("UPDATE poll_private.polls SET published=false WHERE id=$1")
            .bind(ids[3])
            .execute(&mut *unpublish)
            .await
            .unwrap();
        let pending = tokio::spawn({
            let public_task = public_task.clone();
            async move { cast_poll_vote(&public_task, ids[3], 99, &voter(14)).await }
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!pending.is_finished());
        unpublish.commit().await.unwrap();
        assert!(matches!(pending.await.unwrap(), Err(StoreError::NotFound)));
        assert_eq!(totals(&owner_task, ids[3]).await.3, 0);

        // Existing option-row locks prevent a tally update from using a stale
        // score after operator maintenance commits.
        sqlx::query(
            "UPDATE poll_private.polls SET published=true,accepting_votes=true WHERE id=$1",
        )
        .bind(ids[3])
        .execute(&owner_task)
        .await
        .unwrap();
        let mut option = owner_task.begin().await.unwrap();
        sqlx::query("UPDATE poll_private.options SET score=1000000000 WHERE poll_id=$1")
            .bind(ids[3])
            .execute(&mut *option)
            .await
            .unwrap();
        let pending = tokio::spawn({
            let public_task = public_task.clone();
            async move { cast_poll_vote(&public_task, ids[3], 99, &voter(15)).await }
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!pending.is_finished());
        option.commit().await.unwrap();
        assert_eq!(
            pending.await.unwrap().unwrap(),
            PollVoteOutcome::CapacityReached
        );
        assert_eq!(totals(&owner_task, ids[3]).await.3, 0);
    })
    .await;
    sqlx::query("DELETE FROM poll_private.polls WHERE id=ANY($1)")
        .bind(ids.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("SELECT pg_advisory_unlock(2250109)")
        .execute(&mut *lock)
        .await
        .unwrap();
    result.unwrap();
}

#[tokio::test]
async fn uninitialized_scores_saturated_tallies_and_explicit_rollbacks_leave_no_ballots() {
    let owner = login("MIGRATION_DATABASE_URL", "board_migrator").await;
    let public = login("TEST_PUBLIC_DATABASE_URL", "board_public").await;
    let mut lock = owner.acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock(2250109)")
        .execute(&mut *lock)
        .await
        .unwrap();
    let ids = seed(&owner).await;
    let owner_task = owner.clone();
    let public_task = public.clone();
    let result = tokio::spawn(async move {
        sqlx::query("UPDATE poll_private.options SET score=NULL WHERE poll_id=$1 AND id=22")
            .bind(ids[0])
            .execute(&owner_task)
            .await
            .unwrap();
        assert_eq!(
            cast_poll_vote(&public_task, ids[0], 11, &voter(20))
                .await
                .unwrap(),
            PollVoteOutcome::InvalidOption
        );
        assert_eq!(totals(&owner_task, ids[0]).await.3, 0);
        sqlx::query("UPDATE poll_private.options SET score=3 WHERE poll_id=$1 AND id=22")
            .bind(ids[0])
            .execute(&owner_task)
            .await
            .unwrap();
        sqlx::query("UPDATE poll_private.options SET score=1000000000 WHERE poll_id=$1 AND id=11")
            .bind(ids[0])
            .execute(&owner_task)
            .await
            .unwrap();
        assert_eq!(
            cast_poll_vote(&public_task, ids[0], 11, &voter(20))
                .await
                .unwrap(),
            PollVoteOutcome::CapacityReached
        );
        assert_eq!(totals(&owner_task, ids[0]).await.3, 0);
        sqlx::query("UPDATE poll_private.options SET score=2 WHERE poll_id=$1 AND id=11")
            .bind(ids[0])
            .execute(&owner_task)
            .await
            .unwrap();
        sqlx::query("UPDATE poll_private.polls SET vote_count=1000000000 WHERE id=$1")
            .bind(ids[0])
            .execute(&owner_task)
            .await
            .unwrap();
        assert_eq!(
            cast_poll_vote(&public_task, ids[0], 11, &voter(20))
                .await
                .unwrap(),
            PollVoteOutcome::CapacityReached
        );
        assert_eq!(totals(&owner_task, ids[0]).await.3, 0);
        sqlx::query("UPDATE poll_private.polls SET vote_count=7 WHERE id=$1")
            .bind(ids[0])
            .execute(&owner_task)
            .await
            .unwrap();

        // Receipt and counters must share the surrounding SQL transaction:
        // even a result returned as Recorded cannot survive caller rollback.
        let mut rollback = public_task.begin().await.unwrap();
        let value: i16 = sqlx::query_scalar("SELECT content.cast_poll_vote($1,$2,$3)")
            .bind(ids[0])
            .bind(11_i64)
            .bind(voter(21).as_slice())
            .fetch_one(&mut *rollback)
            .await
            .unwrap();
        assert_eq!(value, 0);
        rollback.rollback().await.unwrap();
        assert_eq!(
            totals(&owner_task, ids[0]).await,
            (7, 0, vec![(11, Some(2)), (22, Some(3))], 0)
        );
        assert!(
            !has_poll_vote(&public_task, ids[0], &voter(21))
                .await
                .unwrap()
        );
        let malformed = sqlx::query("SELECT content.cast_poll_vote($1,$2,$3)")
            .bind(ids[0])
            .bind(11_i64)
            .bind(&[1_u8; 31][..])
            .execute(&public_task)
            .await
            .unwrap_err();
        assert_eq!(
            malformed
                .as_database_error()
                .unwrap()
                .code()
                .unwrap()
                .as_ref(),
            "23514"
        );
        assert_eq!(totals(&owner_task, ids[0]).await.3, 0);
    })
    .await;
    sqlx::query("DELETE FROM poll_private.polls WHERE id=ANY($1)")
        .bind(ids.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("SELECT pg_advisory_unlock(2250109)")
        .execute(&mut *lock)
        .await
        .unwrap();
    result.unwrap();
}

#[tokio::test]
async fn private_receipts_role_grants_and_readiness_drift_are_detected() {
    let owner = login("MIGRATION_DATABASE_URL", "board_migrator").await;
    let public = login("TEST_PUBLIC_DATABASE_URL", "board_public").await;
    let ready: bool = sqlx::query_scalar(POLL_READINESS_SQL)
        .fetch_one(&public)
        .await
        .unwrap();
    assert!(ready);
    let ready: bool = sqlx::query_scalar(POLL_VOTE_READINESS_SQL)
        .fetch_one(&public)
        .await
        .unwrap();
    assert!(ready);
    for denied in [
        "SELECT * FROM poll_private.votes LIMIT 1",
        "SELECT voter_hash FROM poll_private.votes LIMIT 1",
        "INSERT INTO poll_private.votes(poll_id,voter_hash) VALUES(1,decode(repeat('aa',32),'hex'))",
        "UPDATE poll_private.polls SET accepting_votes=true WHERE false",
        "UPDATE poll_private.options SET score=0 WHERE false",
    ] {
        let error = sqlx::query(denied).execute(&public).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().unwrap().as_ref(),
            "42501",
            "public role gained direct poll mutation/read: {denied}"
        );
    }
    let mut lock = owner.acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock(2250109)")
        .execute(&mut *lock)
        .await
        .unwrap();
    for mutation in [
        "GRANT SELECT(voter_hash) ON poll_private.votes TO board_public",
        "GRANT INSERT(voter_hash) ON poll_private.votes TO board_staff",
        "GRANT UPDATE(published) ON poll_private.polls TO board_poll_owner",
        "GRANT INSERT(title) ON poll_private.polls TO board_poll_owner",
        "ALTER TABLE poll_private.polls ALTER COLUMN accepting_votes DROP DEFAULT",
        "ALTER TABLE poll_private.polls DROP CONSTRAINT polls_vote_capacity_check",
        "ALTER TABLE poll_private.votes DROP CONSTRAINT votes_voter_hash_check",
    ] {
        let mut tx = owner.begin().await.unwrap();
        sqlx::query(mutation).execute(&mut *tx).await.unwrap();
        let ready: bool = sqlx::query_scalar(POLL_VOTE_READINESS_SQL)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert!(!ready, "readiness accepted {mutation}");
        tx.rollback().await.unwrap();
    }
    for mutation in [
        "ALTER FUNCTION content.cast_poll_vote(bigint,bigint,bytea) SECURITY INVOKER",
        "ALTER FUNCTION content.has_poll_vote(bigint,bytea) SET search_path=public",
        "REVOKE EXECUTE ON FUNCTION content.cast_poll_vote(bigint,bigint,bytea) FROM board_public",
        "GRANT EXECUTE ON FUNCTION content.has_poll_vote(bigint,bytea) TO PUBLIC",
    ] {
        let mut tx = owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_poll_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query(mutation).execute(&mut *tx).await.unwrap();
        sqlx::query("RESET ROLE").execute(&mut *tx).await.unwrap();
        let ready: bool = sqlx::query_scalar(POLL_VOTE_READINESS_SQL)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert!(!ready, "readiness accepted {mutation}");
        tx.rollback().await.unwrap();
    }
    // A replacement function with the same name, signature, owner and
    // volatility must also fail readiness if its decision body changes.
    let mut modified_body = owner.begin().await.unwrap();
    sqlx::query("GRANT CREATE ON SCHEMA content TO board_poll_owner")
        .execute(&mut *modified_body)
        .await
        .unwrap();
    sqlx::query("SET LOCAL ROLE board_poll_owner")
        .execute(&mut *modified_body)
        .await
        .unwrap();
    sqlx::query(
        "CREATE OR REPLACE FUNCTION content.has_poll_vote(p_poll bigint,p_voter bytea)
         RETURNS boolean LANGUAGE plpgsql STABLE SECURITY DEFINER
         SET search_path=pg_catalog,pg_temp AS $$
         BEGIN RETURN TRUE; END $$",
    )
    .execute(&mut *modified_body)
    .await
    .unwrap();
    sqlx::query("RESET ROLE")
        .execute(&mut *modified_body)
        .await
        .unwrap();
    sqlx::query("REVOKE CREATE ON SCHEMA content FROM board_poll_owner")
        .execute(&mut *modified_body)
        .await
        .unwrap();
    let ready: bool = sqlx::query_scalar(POLL_VOTE_READINESS_SQL)
        .fetch_one(&mut *modified_body)
        .await
        .unwrap();
    assert!(
        !ready,
        "readiness accepted a changed ballot identity decision"
    );
    modified_body.rollback().await.unwrap();
    sqlx::query("SELECT pg_advisory_unlock(2250109)")
        .execute(&mut *lock)
        .await
        .unwrap();
    let ready: bool = sqlx::query_scalar(POLL_VOTE_READINESS_SQL)
        .fetch_one(&public)
        .await
        .unwrap();
    assert!(ready);
}
