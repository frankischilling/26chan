//! Owned test DDL must not masquerade as production catalog drift. Keep the
//! real readiness guard active and require exact restoration after injection.
use sqlx::PgPool;

pub async fn snapshot(owner: &PgPool) -> Vec<u8> {
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let ready: bool = sqlx::query_scalar("SELECT content.restrictions_ready()")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert!(ready, "Owned fixture requires intact restriction authority");
    let hash =
        sqlx::query_scalar("SELECT definition_hash FROM restrictions.catalog WHERE singleton")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    tx.rollback().await.unwrap();
    hash
}

pub async fn apply(owner: &PgPool, ddl: &str, original: Option<&[u8]>) {
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SELECT singleton FROM restrictions.catalog WHERE singleton FOR UPDATE")
        .execute(&mut *tx)
        .await
        .unwrap();
    let ready: bool = sqlx::query_scalar("SELECT content.restrictions_ready()")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert!(ready, "Do not attest pre-existing drift as owned test DDL");
    sqlx::query("RESET ROLE").execute(&mut *tx).await.unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(ddl.to_owned()))
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let hash: Vec<u8> = sqlx::query_scalar("SELECT restrictions.catalog_hash()")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    if let Some(expected) = original {
        assert_eq!(
            hash, expected,
            "Owned fixture removal must restore the original catalog exactly"
        );
    } else {
        let ready: bool = sqlx::query_scalar("SELECT content.restrictions_ready()")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert!(
            !ready,
            "Unattested trigger injection must invalidate production readiness"
        );
    }
    sqlx::query("UPDATE restrictions.catalog SET definition_hash=$1 WHERE singleton")
        .bind(&hash)
        .execute(&mut *tx)
        .await
        .unwrap();
    let ready: bool = sqlx::query_scalar("SELECT content.restrictions_ready()")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert!(ready, "Only the owned temporary schema is attested");
    tx.commit().await.unwrap();
}
