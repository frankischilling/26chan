#![cfg(feature = "database-tests")]
use board_store::{StoreError, verification::VerificationStore};
use sqlx::PgPool;

/// Run against both sides of the 0169 -> 0170 upgrade. Current-schema test
/// runs take the new-contract branch; upgrade qualification also runs the old.
#[tokio::test]
async fn verification_startup_requires_credit_lifetime_contract() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let shape:String=sqlx::query_scalar("SELECT pg_get_function_result('content.verification_credit_receipt(bytea,bytea,bytea,bytea,bytea,bytea,text,bigint,text,bytea,bigint)'::regprocedure)").fetch_one(&owner).await.unwrap();
    let result =
        VerificationStore::connect(&std::env::var("TEST_VERIFICATION_DATABASE_URL").unwrap()).await;
    match shape.as_str() {
        "TABLE(status text, remaining smallint)" => assert!(
            matches!(result, Err(StoreError::UnsafeRole)),
            "Old credit result must fail readiness before serving requests"
        ),
        "TABLE(status text, remaining smallint, ttl_seconds integer)" => {
            result.unwrap().close().await
        }
        _ => panic!("Unexpected credit response contract"),
    }
    owner.close().await;
}
