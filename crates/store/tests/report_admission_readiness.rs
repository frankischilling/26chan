#![cfg(feature = "database-tests")]

#[tokio::test]
async fn both_actual_runtime_logins_satisfy_shared_report_readiness() {
    for variable in ["TEST_PUBLIC_DATABASE_URL", "STAFF_DATABASE_URL"] {
        let pool = sqlx::PgPool::connect(&std::env::var(variable).unwrap())
            .await
            .unwrap();
        let ready: bool = sqlx::query_scalar(board_store::report_admission::READINESS_SQL)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(ready, "{variable}");
        pool.close().await;
    }
}
