#![cfg(feature = "database-tests")]
use board_domain::content_admission::{
    AdmissionError,
    files::history::{Counts, Snapshot, Upload, evaluate},
    network::ProviderValue,
};
use serde_json::{Value, json};
use sqlx::{PgPool, types::Json};

#[tokio::test]
async fn private_sql_decisions_match_bounded_history_evidence() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut cases = Vec::new();
    for ak in 0..=3 {
        for am in 0..=ak {
            for au in 0..=3 {
                for bk in 0..=3 {
                    for bm in 0..=bk {
                        for bu in 0..=3 {
                            for worksafe in [false, true] {
                                for known in [false, true] {
                                    if !known && (am != 0 || bm != 0) {
                                        continue;
                                    }
                                    let digest = known.then_some([1; 16]);
                                    let history = Snapshot {
                                        digest,
                                        template_226: Counts {
                                            matching: am,
                                            known: ak,
                                            unknown: au,
                                        },
                                        template_6: Counts {
                                            matching: bm,
                                            known: bk,
                                            unknown: bu,
                                        },
                                    };
                                    let expected = match evaluate(
                                        "g",
                                        worksafe,
                                        if known {
                                            Upload::PresentKnown([1; 16])
                                        } else {
                                            Upload::PresentUnknown
                                        },
                                        ProviderValue::Available(history),
                                    ) {
                                        Ok(value) => Some(value),
                                        Err(AdmissionError::Unavailable) => None,
                                        Err(error) => {
                                            panic!("Valid bounded fixture failed: {error}")
                                        }
                                    };
                                    cases.push(json!({"known":known,"safe":worksafe,"am":am,"ak":ak,"au":au,"bm":bm,"bk":bk,"bu":bu,"expected":expected}));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(cases.len(), 3712);
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let mismatches: Vec<(Json<Value>,Option<bool>)> = sqlx::query_as("SELECT to_jsonb(c),restrictions.prior_upload_decision(known,safe,am,ak,au,bm,bk,bu) FROM jsonb_to_recordset($1) c(known boolean,safe boolean,am integer,ak integer,au integer,bm integer,bk integer,bu integer,expected boolean) WHERE restrictions.prior_upload_decision(known,safe,am,ak,au,bm,bk,bu) IS DISTINCT FROM expected")
        .bind(Json(cases)).fetch_all(&mut *tx).await.unwrap();
    assert!(
        mismatches.is_empty(),
        "SQL history decisions differ: {mismatches:?}"
    );
    for counts in [
        vec![Some(1), Some(0), Some(0), Some(0), Some(0), Some(0)],
        vec![Some(0), Some(4), Some(0), Some(0), Some(0), Some(0)],
        vec![Some(0), Some(0), Some(4), Some(0), Some(0), Some(0)],
        vec![None, Some(0), Some(0), Some(0), Some(0), Some(0)],
    ] {
        sqlx::query("SAVEPOINT invalid_history")
            .execute(&mut *tx)
            .await
            .unwrap();
        let error = sqlx::query("SELECT restrictions.prior_upload_decision(true,true,($1::integer[])[1],($1::integer[])[2],($1::integer[])[3],($1::integer[])[4],($1::integer[])[5],($1::integer[])[6])")
            .bind(counts).execute(&mut *tx).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
        sqlx::query("ROLLBACK TO SAVEPOINT invalid_history")
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    tx.rollback().await.unwrap();
    for variable in [
        "TEST_PUBLIC_DATABASE_URL",
        "STAFF_DATABASE_URL",
        "AUTH_DATABASE_URL",
    ] {
        let runtime = PgPool::connect(&std::env::var(variable).unwrap())
            .await
            .unwrap();
        for statement in [
            "SELECT restrictions.prior_upload_decision(true,true,0,0,0,0,0,0)",
            "SELECT restrictions.prior_upload_history(NULL,true)",
            "SELECT content.prior_ban_upload_digest('','','g',0,false,false)",
            "SELECT content.assert_prior_upload_history()",
        ] {
            let error = sqlx::query(statement).execute(&runtime).await.unwrap_err();
            assert_eq!(
                error.as_database_error().unwrap().code().as_deref(),
                Some("42501"),
                "{variable}: {statement}"
            );
        }
        runtime.close().await;
    }
    owner.close().await;
}
