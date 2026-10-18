#![cfg(feature = "database-tests")]
use board_domain::anonymous_session::State;
use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool};

const TOKEN: [u8; 32] = [231; 32];
const HASH: [u8; 32] = [232; 32];

fn number(value: &Value, field: &str) -> i64 {
    value[field].as_i64().unwrap()
}

async fn seed(connection: &mut PgConnection, state: State, now: i64) {
    sqlx::query("INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,created_at,network_at,address_at,environment_at,activity_at,action_at,expires_at,verified_level,posts,images,threads,reports,pending,change_score) VALUES($1,$2,$2,$2,$3,$4,$3,$3,$5,$5,$6,$7,$8,0,0,$9,$10,$11)")
        .bind(TOKEN.as_slice()).bind(HASH.as_slice()).bind(state.created_at as i64)
        .bind(state.network_at as i64).bind(now).bind(now+3600)
        .bind(i16::from(state.verified_level)).bind(i16::from(state.posts))
        .bind(i16::from(state.reports)).bind(i16::from(state.pending)).bind(i16::from(state.change_score))
        .execute(connection).await.unwrap();
}

async fn owner() -> PgPool {
    assert!(
        std::env::var("BOARD_TEST_CLUSTER").is_ok_and(|path| path
            .strip_prefix("/tmp/board-postgres.")
            .is_some_and(|tag| tag.len() == 8 && tag.bytes().all(|b| b.is_ascii_alphanumeric()))),
        "Network oracle requires an explicit disposable cluster"
    );
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let name: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&owner)
        .await
        .unwrap();
    assert_eq!(name, "board_migrator");
    owner
}

#[tokio::test]
async fn sql_report_policy_matches_independent_source_report_vectors() {
    let owner = owner().await;
    let corpus: Value = serde_json::from_str(include_str!(
        "../../../fixtures/network-admission-reference.json"
    ))
    .unwrap();
    let mut checked = 0;
    for case in corpus["cases"].as_array().unwrap().iter().filter(|case| {
        case["is_report"] == true && case["is_op"] == false && case["has_img"] == false
    }) {
        let mut tx = owner.begin().await.unwrap();
        let now: i64 = sqlx::query_scalar("SELECT extract(epoch FROM clock_timestamp())::bigint")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        let offset = now - number(case, "now");
        let clock = |value: i64| if value == 0 { 0 } else { value + offset };
        let board = case["board"].as_str().unwrap();
        assert_eq!(board, "g");
        sqlx::query("UPDATE content.boards SET worksafe=$2 WHERE slug=$1")
            .bind(board)
            .bind(case["is_sfw"].as_bool().unwrap())
            .execute(&mut *tx)
            .await
            .unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM admission.network_rules")
                .fetch_one(&mut *tx)
                .await
                .unwrap(),
            0
        );
        for rule in case["rules"].as_array().unwrap() {
            sqlx::query("INSERT INTO admission.network_rules(id,range_start,range_end,asn,active,expires_at,created_at,updated_at,boards,ops_only,images_only,reports_only,leniency,signatures) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)")
                .bind(number(rule,"id")).bind(number(rule,"range_start")).bind(number(rule,"range_end"))
                .bind((number(rule,"asn")!=0).then(||number(rule,"asn")))
                .bind(number(rule,"active")!=0).bind(clock(number(rule,"expires_on")))
                .bind(clock(number(rule,"created_on"))).bind(clock(number(rule,"updated_on")))
                .bind(rule["boards"].as_str().unwrap()).bind(number(rule,"ops_only")!=0)
                .bind(number(rule,"img_only")!=0).bind(number(rule,"report_only")!=0)
                .bind(number(rule,"lenient") as i16).bind(rule["ua_ids"].as_str().unwrap())
                .execute(&mut *tx).await.unwrap();
        }
        let actor = &case["actor"];
        if !actor.is_null() {
            let mut state = State::new((now - number(actor, "password_age")) as u64);
            state.network_at = (now - number(actor, "network_age")) as u64;
            state.posts = number(actor, "posts") as u8;
            state.reports = number(actor, "reports") as u8;
            state.pending = number(actor, "pending") as u8;
            state.verified_level = number(actor, "verified") as u8;
            state.change_score = number(actor, "score") as u8;
            seed(&mut tx, state, now).await;
        }
        let ctx = json!({"board":board,"post":1,"actor":"11".repeat(32),"request_at":now,
            "peer_known":true,"ipv4":case["address"],"asn_known":true,
            "asn":(number(case,"asn")!=0).then(||number(case,"asn")),
            "browser":case["browser_id"].as_str().unwrap_or(""),
            "request":case["req_sig"].as_str().unwrap_or("")});
        sqlx::query("SELECT set_config('board.report_network_context',$1,true)")
            .bind(ctx.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("SET LOCAL ROLE board_report_admission_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("SELECT content.lock_report_network_policy()")
            .execute(&mut *tx)
            .await
            .unwrap();
        let result=sqlx::query("SELECT post_secrets.assert_report_network($1,1,decode(repeat('11',32),'hex'),$2,$3,$3,$3,false,$4)")
            .bind(board).bind((!actor.is_null()).then_some(TOKEN.as_slice())).bind(HASH.as_slice()).bind(now)
            .execute(&mut *tx).await;
        let rejected = match result {
            Ok(_) => false,
            Err(error) => {
                assert_eq!(
                    error.as_database_error().and_then(|e| e.code()).as_deref(),
                    Some("P179B"),
                    "case {checked}: {error}"
                );
                true
            }
        };
        assert_eq!(
            rejected,
            !case["expected_id"].is_null(),
            "source report vector {checked}"
        );
        tx.rollback().await.unwrap();
        checked += 1;
    }
    assert_eq!(checked, 344);
}

#[tokio::test]
async fn report_provider_availability_scope_clocks_and_overflow_fail_closed() {
    let owner = owner().await;
    for case in 0..15 {
        let mut tx = owner.begin().await.unwrap();
        let now: i64 = sqlx::query_scalar("SELECT extract(epoch FROM clock_timestamp())::bigint")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        sqlx::query("SELECT slug FROM content.boards WHERE slug='g' FOR UPDATE")
            .execute(&mut *tx)
            .await
            .unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM admission.network_rules")
                .fetch_one(&mut *tx)
                .await
                .unwrap(),
            0
        );
        let id:i64=sqlx::query_scalar("INSERT INTO admission.network_rules(boards,range_start,range_end,signatures) VALUES('g',3221225994,3221225994,'hit') RETURNING id").fetch_one(&mut *tx).await.unwrap();
        let mut ctx = json!({"board":"g","post":1,"actor":"11".repeat(32),"request_at":now,
            "peer_known":true,"ipv4":3221225994_u64,"asn_known":false,"asn":null,"browser":"hit","request":null});
        let mut update = None;
        let expected = match case {
            0 => Some("P179B"),
            1 => {
                ctx["browser"] = Value::Null;
                ctx["request"] = json!("hit");
                Some("P179B")
            }
            2 => {
                ctx["browser"] = json!("miss");
                Some("P0179")
            }
            3 => {
                ctx["browser"] = json!("miss");
                ctx["request"] = json!("miss");
                None
            }
            4 => {
                update = Some(
                    "UPDATE admission.network_rules SET range_start=NULL,range_end=NULL,asn=15169,signatures='' WHERE id=$1",
                );
                Some("P0179")
            }
            5 => {
                update = Some(
                    "UPDATE admission.network_rules SET range_start=NULL,range_end=NULL,asn=15169,signatures='' WHERE id=$1",
                );
                ctx["ipv4"] = Value::Null;
                ctx["asn_known"] = json!(true);
                ctx["asn"] = json!(15169);
                Some("P179B")
            }
            6 => {
                update = Some("UPDATE admission.network_rules SET ops_only=true WHERE id=$1");
                ctx["browser"] = Value::Null;
                None
            }
            7 => {
                update = Some("UPDATE admission.network_rules SET signatures='0' WHERE id=$1");
                ctx["browser"] = Value::Null;
                Some("P179B")
            }
            8 | 9 => {
                sqlx::query(
                    "UPDATE admission.network_rules SET expires_at=$2,signatures='' WHERE id=$1",
                )
                .bind(id)
                .bind(now - 5)
                .execute(&mut *tx)
                .await
                .unwrap();
                if case == 8 {
                    ctx["request_at"] = json!(now - 10);
                    Some("P179B")
                } else {
                    None
                }
            }
            10 => {
                ctx["peer_known"] = json!(false);
                ctx["ipv4"] = Value::Null;
                Some("P0179")
            }
            11 => {
                ctx = json!([]);
                Some("P0179")
            }
            12 => {
                ctx["board"] = json!("other");
                Some("P0179")
            }
            13 => {
                sqlx::query("INSERT INTO admission.network_rules(boards,range_start,range_end) SELECT 'g',0,4294967295 FROM generate_series(1,128)").execute(&mut *tx).await.unwrap();
                Some("P0179")
            }
            14 => {
                ctx["browser"] = json!("x".repeat(2049));
                Some("P0179")
            }
            _ => unreachable!(),
        };
        if let Some(update) = update {
            sqlx::query(update)
                .bind(id)
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        sqlx::query("SELECT set_config('board.report_network_context',$1,true)")
            .bind(ctx.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("SET LOCAL ROLE board_report_admission_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("SELECT content.lock_report_network_policy()")
            .execute(&mut *tx)
            .await
            .unwrap();
        let result=sqlx::query("SELECT post_secrets.assert_report_network('g',1,decode(repeat('11',32),'hex'),NULL,NULL,NULL,NULL,NULL,$1)").bind(now).execute(&mut *tx).await;
        match expected {
            None => {
                result.unwrap();
            }
            Some(expected) => {
                let error = result.unwrap_err();
                assert_eq!(
                    error.as_database_error().and_then(|e| e.code()).as_deref(),
                    Some(expected),
                    "provider case {case}: {error}"
                );
            }
        }
        tx.rollback().await.unwrap();
    }
}
