#![cfg(feature = "database-tests")]

#[tokio::test]
async fn still_manifest_is_bounded_immutable_and_lease_fenced() {
    let pool = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut tx = pool.begin().await.unwrap();
    let result = sqlx::raw_sql(include_str!("fixtures/still_manifest.sql"))
        .execute(&mut *tx)
        .await;
    tx.rollback().await.unwrap();
    pool.close().await;
    result.unwrap();
}
