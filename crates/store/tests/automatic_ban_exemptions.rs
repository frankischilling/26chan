#![cfg(feature = "database-tests")]
use serde_json::Value;
use sqlx::PgPool;

#[tokio::test]
async fn private_exemptions_match_source_and_refuse_unsafe_configuration() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = PgPool::connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let staff = PgPool::connect(&std::env::var("STAFF_DATABASE_URL").unwrap())
        .await
        .unwrap();
    // All configuration is rolled back; no production exemptions are imported.
    let mut tx = owner.begin().await.unwrap();
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/ban-exemption-reference.json")).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 924);
    let mut previous = String::from("not-a-network");
    for case in cases {
        let network = case["network"].as_str().unwrap_or("");
        if network != previous {
            sqlx::query("DELETE FROM admission.automatic_ban_exemptions")
                .execute(&mut *tx)
                .await
                .unwrap();
            if !network.is_empty() {
                sqlx::query(
                    "INSERT INTO admission.automatic_ban_exemptions(network) VALUES($1::cidr)",
                )
                .bind(network)
                .execute(&mut *tx)
                .await
                .unwrap();
            }
            previous = network.into();
        }
        sqlx::query("SET LOCAL ROLE board_admission_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        let actual: bool = sqlx::query_scalar("SELECT admission.automatic_ban_exempt($1::inet)")
            .bind(case["peer"].as_str().unwrap())
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert_eq!(actual, case["exempt"].as_bool().unwrap(), "{case}");
        sqlx::query("RESET ROLE").execute(&mut *tx).await.unwrap();
    }
    sqlx::query("DELETE FROM admission.automatic_ban_exemptions")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO admission.automatic_ban_exemptions(network,active) VALUES('192.0.2.0/24',false)").execute(&mut *tx).await.unwrap();
    sqlx::query("SET LOCAL ROLE board_admission_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let inactive: bool = sqlx::query_scalar("SELECT admission.automatic_ban_exempt('192.0.2.1')")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert!(!inactive);
    let ipv6: bool = sqlx::query_scalar("SELECT admission.automatic_ban_exempt('2001:db8::1')")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert!(!ipv6);
    sqlx::query("RESET ROLE").execute(&mut *tx).await.unwrap();
    tx.rollback().await.unwrap();
    for pool in [&public, &staff] {
        for statement in [
            "SELECT * FROM admission.automatic_ban_exemptions",
            "SELECT admission.automatic_ban_exempt('192.0.2.1')",
        ] {
            assert!(sqlx::query(statement).execute(pool).await.is_err());
        }
    }
    for network in [
        "0.0.0.0/0",
        "0.0.0.0/8",
        "192.0.2.1/24",
        "192.0.2.0/33",
        "192.0.2.0/-1",
        "not-an-address/24",
        "2001:db8::/32",
    ] {
        let mut tx = owner.begin().await.unwrap();
        assert!(
            sqlx::query("INSERT INTO admission.automatic_ban_exemptions(network) VALUES($1::cidr)")
                .bind(network)
                .execute(&mut *tx)
                .await
                .is_err()
        );
        tx.rollback().await.unwrap();
    }
    for peer in [None, Some("192.0.2.0/24"), Some("2001:db8::/64")] {
        let mut tx = owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_admission_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        let error = sqlx::query("SELECT admission.automatic_ban_exempt($1::inet)")
            .bind(peer)
            .execute(&mut *tx)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
        tx.rollback().await.unwrap();
    }
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("INSERT INTO admission.automatic_ban_exemptions(network) SELECT ('192.0.2.'||n||'/32')::cidr FROM generate_series(1,129) n")
        .execute(&mut *tx).await.unwrap();
    sqlx::query("SET LOCAL ROLE board_admission_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let error = sqlx::query("SELECT admission.automatic_ban_exempt('198.51.100.1')")
        .execute(&mut *tx)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("55000")
    );
    tx.rollback().await.unwrap();
}
