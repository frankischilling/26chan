#![cfg(feature = "database-tests")]
mod support;

use board_domain::{
    BoardSlug,
    anonymous_session::Capability,
    report_category::{Catalog, Category, CategoryId, Target},
};
use board_store::anonymous_session::PostingSession;
use chrono::Utc;
use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool};

// SQL contract fixtures run as the explicitly permitted migrator and roll back.
// They do not prove public-login authorization. The guarded committed race uses
// actual runtime logins and retains exactly one revision until cluster teardown.
// Run the database-test binaries serially against the owned test database.
static TEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn pool(variable: &str, role: &str) -> PgPool {
    let pool = PgPool::connect(&std::env::var(variable).expect("owned test database URL required"))
        .await
        .unwrap();
    let actual: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(actual, role, "test must use the actual configured login");
    pool
}
async fn owner() -> PgPool {
    pool("MIGRATION_DATABASE_URL", "board_migrator").await
}
async fn reset(c: &mut PgConnection) {
    sqlx::query("RESET ROLE").execute(c).await.unwrap();
}
// Restore the permitted migrator after inspecting private diagnostic state.
async fn migrator_contract(c: &mut PgConnection) {
    reset(c).await;
}
async fn save(c: &mut PgConnection) {
    sqlx::query("SAVEPOINT expected_failure")
        .execute(c)
        .await
        .unwrap();
}
async fn recover(c: &mut PgConnection) {
    sqlx::query("ROLLBACK TO SAVEPOINT expected_failure")
        .execute(c)
        .await
        .unwrap();
}
fn code(e: &sqlx::Error) -> String {
    e.as_database_error().unwrap().code().unwrap().into_owned()
}
fn message(e: &sqlx::Error) -> &str {
    e.as_database_error().unwrap().message()
}
fn row(id: i64) -> Value {
    json!({"id":id,"board":"","op_only":false,"reply_only":false,"image_only":false,
        "exclude_boards":null,"title":"Synthetic category","weight":1.0,"filtered":0})
}
async fn import(c: &mut PgConnection, rows: &[Value]) -> i64 {
    sqlx::query_scalar("SELECT content.import_report_catalog($1::text::jsonb)")
        .bind(json!({"version":1,"categories":rows}).to_string())
        .fetch_one(c)
        .await
        .unwrap()
}
async fn activate(c: &mut PgConnection, revision: Option<i64>) {
    sqlx::query("SELECT content.set_report_catalog_active($1)")
        .bind(revision)
        .execute(c)
        .await
        .unwrap();
}
async fn fixture(c: &mut PgConnection, zero: bool) -> (String, i64, i64) {
    let op: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&mut *c)
        .await
        .unwrap();
    let board = if zero {
        "0".into()
    } else {
        format!("cr{op:x}")
    };
    // The reserved '0' fixture must not already exist; never modify another board.
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,archive_retention_seconds,posting_reply_seconds,posting_image_seconds,posting_thread_seconds) VALUES($1,'Owned categorical fixture','Synthetic',2000,100,100,100,10,3600,0,0,0)")
        .bind(&board).execute(&mut *c).await.unwrap();
    sqlx::query("INSERT INTO content.threads(id,board) VALUES($1,$2)")
        .bind(op)
        .bind(&board)
        .execute(&mut *c)
        .await
        .unwrap();
    sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous','','Synthetic OP')")
        .bind(op).bind(&board).execute(&mut *c).await.unwrap();
    let reply = sqlx::query_scalar("INSERT INTO content.posts(board,thread_id,name,subject,comment) VALUES($1,$2,'Anonymous','','Synthetic reply') RETURNING id")
        .bind(&board).bind(op).fetch_one(c).await.unwrap();
    (board, op, reply)
}
async fn form(c: &mut PgConnection, board: &str, post: i64) -> Value {
    let raw: String = sqlx::query_scalar("SELECT content.report_category_form($1,$2)::text")
        .bind(board)
        .bind(post)
        .fetch_one(c)
        .await
        .unwrap();
    serde_json::from_str(&raw).unwrap()
}
fn session(cap: &Capability, minted: bool) -> PostingSession {
    PostingSession {
        fingerprints: cap.fingerprints(Some("198.51.100.7".parse().unwrap()), *b"US"),
        minted,
        now: Utc::now(),
    }
}
async fn admit(
    c: &mut PgConnection,
    board: &str,
    post: i64,
    category: i64,
    revision: Option<i64>,
    actor: &[u8],
    session: PostingSession,
) -> Result<i64, sqlx::Error> {
    let f = session.fingerprints;
    sqlx::query_scalar(
        "SELECT content.admit_categorical_report($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
    )
    .bind(board)
    .bind(post)
    .bind(category)
    .bind(revision)
    .bind(actor)
    .bind(f.token.as_slice())
    .bind(f.network.as_slice())
    .bind(f.address.as_slice())
    .bind(f.environment.as_slice())
    .bind(session.minted)
    .bind(session.now.timestamp())
    .fetch_one(c)
    .await
}
async fn snapshot(c: &mut PgConnection, board: &str, token: &[u8]) -> Value {
    reset(c).await;
    let raw: String = sqlx::query_scalar("SELECT jsonb_build_object('reports',(SELECT coalesce(jsonb_agg(to_jsonb(r) ORDER BY id),'[]') FROM content.reports r WHERE board=$1),'session',(SELECT to_jsonb(s) FROM post_secrets.anonymous_sessions s WHERE token_hash=$2),'links',(SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY report_id),'[]') FROM post_secrets.anonymous_reports a WHERE token_hash=$2))::text")
        .bind(board).bind(token).fetch_one(&mut *c).await.unwrap();
    let mut result: Value = serde_json::from_str(&raw).unwrap();
    sqlx::query("SET LOCAL ROLE board_report_admission_owner")
        .execute(&mut *c)
        .await
        .unwrap();
    let members: String = sqlx::query_scalar("SELECT coalesce(jsonb_agg(to_jsonb(m) ORDER BY report_id),'[]')::text FROM post_secrets.report_membership m WHERE board=$1")
        .bind(board).fetch_one(&mut *c).await.unwrap();
    result["members"] = serde_json::from_str(&members).unwrap();
    reset(c).await;
    result
}

#[tokio::test]
async fn sql_form_matches_domain_order_scope_post_image_and_exact_exclusions() {
    let _serial = TEST.lock().await;
    let owner = owner().await;
    let mut tx = owner.begin().await.unwrap();
    for zero in [false, true] {
        let (board, op, reply) = fixture(&mut tx, zero).await;
        let mut rows: Vec<Value> = [
            90, 2, 70, 8, 6, 31, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30,
        ]
        .into_iter()
        .map(row)
        .collect();
        rows[0]["board"] = Value::Null;
        rows[2]["board"] = json!("_ws_");
        rows[3]["board"] = json!("_nws_");
        rows[4]["board"] = json!(board);
        rows[5]["board"] = Value::Null;
        rows[5]["op_only"] = json!(true);
        rows[5]["reply_only"] = json!(true);
        rows[5]["image_only"] = json!(true);
        rows[5]["exclude_boards"] = json!(board);
        rows[6]["op_only"] = json!(true);
        rows[7]["reply_only"] = json!(true);
        rows[8]["image_only"] = json!(true);
        rows[9]["op_only"] = json!(true);
        rows[9]["reply_only"] = json!(true);
        rows[10]["exclude_boards"] = json!(format!("before,{board},after"));
        rows[11]["exclude_boards"] = json!(format!(" {board}"));
        rows[12]["exclude_boards"] = json!(format!("{board} "));
        rows[13]["exclude_boards"] = json!(board.to_uppercase());
        rows[14]["board"] = json!("0");
        rows[14]["exclude_boards"] = json!("0");
        rows[15]["exclude_boards"] = json!("");
        rows[16]["exclude_boards"] = json!(",0,");
        let revision = import(&mut tx, &rows).await;
        activate(&mut tx, Some(revision)).await;
        let categories: Vec<Category<'_>> = rows
            .iter()
            .map(|r| Category {
                id: CategoryId::new(r["id"].as_i64().unwrap()).unwrap(),
                board: r["board"].as_str(),
                op_only: r["op_only"].as_bool().unwrap(),
                reply_only: r["reply_only"].as_bool().unwrap(),
                image_only: r["image_only"].as_bool().unwrap(),
                exclude_boards: r["exclude_boards"].as_str(),
                title: r["title"].as_str().unwrap(),
                weight: r["weight"].as_f64().unwrap(),
                filtered: 0,
            })
            .collect();
        let catalog = Catalog::new(&categories).unwrap();
        let slug = BoardSlug::parse(&board).unwrap();
        for worksafe in [false, true] {
            sqlx::query("UPDATE content.boards SET worksafe=$2 WHERE slug=$1")
                .bind(&board)
                .bind(worksafe)
                .execute(&mut *tx)
                .await
                .unwrap();
            for post in [op, reply] {
                sqlx::query("INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler) VALUES($1,replace(gen_random_uuid()::text,'-',''),replace(gen_random_uuid()::text,'-',''),'owned.png',1,1,1,false) ON CONFLICT(post_id) DO NOTHING")
                    .bind(post).execute(&mut *tx).await.unwrap();
                for deleted in [false, true] {
                    sqlx::query("UPDATE content.post_media SET file_deleted=$2 WHERE post_id=$1")
                        .bind(post)
                        .bind(deleted)
                        .execute(&mut *tx)
                        .await
                        .unwrap();
                    let selected = catalog.select(Target {
                        board: &slug,
                        is_worksafe: worksafe,
                        resto: if post == op { 0 } else { op as u64 },
                        fsize: 1,
                        filedeleted: deleted,
                    });
                    let expected: Vec<Value> = selected
                        .rules()
                        .iter()
                        .map(|r| json!({"id":r.id.get(),"title":r.title,"kind":"rule"}))
                        .chain(
                            selected
                                .illegal()
                                .map(|r| json!({"id":r.id.get(),"title":r.title,"kind":"illegal"})),
                        )
                        .collect();
                    migrator_contract(&mut tx).await;
                    assert_eq!(
                        form(&mut tx, &board, post).await,
                        json!({"revision":revision,"categories":expected})
                    );
                    reset(&mut tx).await;
                }
                sqlx::query("DELETE FROM content.post_media WHERE post_id=$1")
                    .bind(post)
                    .execute(&mut *tx)
                    .await
                    .unwrap();
                migrator_contract(&mut tx).await;
                let without_image = form(&mut tx, &board, post).await;
                assert!(
                    !without_image["categories"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|r| r["id"] == 22)
                );
                reset(&mut tx).await;
            }
        }
    }
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn labels_and_derived_metadata_are_atomic_with_membership_and_session_activity() {
    let _serial = TEST.lock().await;
    let owner = owner().await;
    let mut tx = owner.begin().await.unwrap();
    let (board, op, _) = fixture(&mut tx, false).await;
    let labels = [
        "".to_owned(),
        "é".repeat(2048),
        "<script>alert('raw configuration')</script> & <b>label</b>".into(),
    ];
    let mut rows: Vec<Value> = [1, 2, 31].into_iter().map(row).collect();
    for (r, label) in rows.iter_mut().zip(&labels) {
        r["title"] = json!(label);
        r["weight"] = json!(-17.25);
        r["filtered"] = json!(i64::MAX);
    }
    let revision = import(&mut tx, &rows).await;
    activate(&mut tx, Some(revision)).await;
    for (index, label) in labels.iter().enumerate() {
        let category = [1, 2, 31][index];
        let cap = Capability::generate().unwrap();
        let actor =
            support::fresh_key().public_report_rate_identity("198.51.100.7".parse().unwrap());
        migrator_contract(&mut tx).await;
        let report = admit(
            &mut tx,
            &board,
            op,
            category,
            Some(revision),
            actor.as_bytes(),
            session(&cap, true),
        )
        .await
        .unwrap();
        let snap = snapshot(&mut tx, &board, &cap.storage_hash()).await;
        let stored = snap["reports"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == report)
            .unwrap();
        assert_eq!(stored["reason"], json!(label));
        assert_eq!(stored["category_revision"], revision);
        assert_eq!(stored["category_id"], category);
        assert_eq!(stored["category_kind"], if category == 31 { 2 } else { 1 });
        assert_eq!(stored["category_base_weight"], json!(-17.25));
        assert!(stored.get("filtered").is_none());
        let member = snap["members"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["report_id"] == report)
            .unwrap();
        assert_eq!(
            member["automatic_identity"],
            snap["session"]["automatic_identity"]
        );
        assert!(!member["automatic_identity"].is_null());
        assert_eq!(snap["links"].as_array().unwrap().len(), 1);
        assert_eq!(snap["links"][0]["report_id"], report);
        assert!(snap["session"]["activity_at"].as_i64().unwrap() > 0);
        // Even a table owner cannot leave partially populated category metadata.
        save(&mut tx).await;
        let error = sqlx::query("UPDATE content.reports SET category_id=NULL WHERE id=$1")
            .bind(report)
            .execute(&mut *tx)
            .await
            .unwrap_err();
        assert_eq!(code(&error), "23514");
        recover(&mut tx).await;
        save(&mut tx).await;
        let error = sqlx::query("UPDATE content.reports SET category_kind=$2 WHERE id=$1")
            .bind(report)
            .bind(if category == 31 { 1_i16 } else { 2_i16 })
            .execute(&mut *tx)
            .await
            .unwrap_err();
        assert_eq!(code(&error), "23514");
        recover(&mut tx).await;
    }
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn stale_missing_or_ineligible_categories_and_registration_failure_leave_no_activity() {
    let _serial = TEST.lock().await;
    let owner = owner().await;
    let mut tx = owner.begin().await.unwrap();
    let (board, op, _) = fixture(&mut tx, false).await;
    let mut hidden = row(2);
    hidden["reply_only"] = json!(true);
    let revision = import(&mut tx, &[row(1), hidden]).await;
    activate(&mut tx, Some(revision)).await;
    let cap = Capability::generate().unwrap();
    let actor = support::fresh_key().public_report_rate_identity("198.51.100.7".parse().unwrap());
    let before = snapshot(&mut tx, &board, &cap.storage_hash()).await;
    for (category, expected_revision, expected) in [
        (
            1,
            None,
            "Report categories changed. Please reload the report form.",
        ),
        (
            1,
            Some(revision + 1),
            "Report categories changed. Please reload the report form.",
        ),
        (2, Some(revision), "Invalid category selected."),
        (31, Some(revision), "Invalid category selected."),
        (0, Some(revision), "Invalid category selected."),
    ] {
        migrator_contract(&mut tx).await;
        save(&mut tx).await;
        let error = admit(
            &mut tx,
            &board,
            op,
            category,
            expected_revision,
            actor.as_bytes(),
            session(&cap, true),
        )
        .await
        .unwrap_err();
        assert_eq!(code(&error), "P0001");
        assert_eq!(message(&error), expected);
        recover(&mut tx).await;
        assert_eq!(snapshot(&mut tx, &board, &cap.storage_hash()).await, before);
    }
    migrator_contract(&mut tx).await;
    save(&mut tx).await;
    let f = session(&cap, true).fingerprints;
    let error = sqlx::query(
        "SELECT content.admit_categorical_report($1,$2,1,$3,$4,$5,''::bytea,$6,$7,true,$8)",
    )
    .bind(&board)
    .bind(op)
    .bind(revision)
    .bind(actor.as_bytes().as_slice())
    .bind(f.token.as_slice())
    .bind(f.address.as_slice())
    .bind(f.environment.as_slice())
    .bind(Utc::now().timestamp())
    .execute(&mut *tx)
    .await
    .unwrap_err();
    assert_eq!(code(&error), "23514");
    recover(&mut tx).await;
    assert_eq!(snapshot(&mut tx, &board, &cap.storage_hash()).await, before);
    migrator_contract(&mut tx).await;
    admit(
        &mut tx,
        &board,
        op,
        1,
        Some(revision),
        actor.as_bytes(),
        session(&cap, true),
    )
    .await
    .unwrap();
    let after = snapshot(&mut tx, &board, &cap.storage_hash()).await;
    assert_eq!(after["reports"].as_array().unwrap().len(), 1);
    assert_eq!(after["members"].as_array().unwrap().len(), 1);
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn categorical_duplicate_uses_ip_or_captured_identity_without_touching_session() {
    let _serial = TEST.lock().await;
    let owner = owner().await;
    let mut tx = owner.begin().await.unwrap();
    let (board, op, reply) = fixture(&mut tx, false).await;
    let revision = import(&mut tx, &[row(1)]).await;
    activate(&mut tx, Some(revision)).await;
    let cap = Capability::generate().unwrap();
    let key = support::fresh_key();
    let original = key.public_report_rate_identity("198.51.100.7".parse().unwrap());
    let changed = key.public_report_rate_identity("203.0.113.7".parse().unwrap());
    migrator_contract(&mut tx).await;
    admit(
        &mut tx,
        &board,
        op,
        1,
        Some(revision),
        original.as_bytes(),
        session(&cap, true),
    )
    .await
    .unwrap();
    let before = snapshot(&mut tx, &board, &cap.storage_hash()).await;
    for target in [op, reply] {
        migrator_contract(&mut tx).await;
        save(&mut tx).await;
        let mut changed_session = session(&cap, false);
        changed_session.fingerprints =
            cap.fingerprints(Some("203.0.113.7".parse().unwrap()), *b"CA");
        let error = admit(
            &mut tx,
            &board,
            target,
            1,
            Some(revision),
            changed.as_bytes(),
            changed_session,
        )
        .await
        .unwrap_err();
        assert_eq!(code(&error), "P0001");
        assert_eq!(
            message(&error),
            if target == op {
                "You have already reported this post."
            } else {
                "You have to wait a while before reporting another post."
            }
        );
        recover(&mut tx).await;
        assert_eq!(snapshot(&mut tx, &board, &cap.storage_hash()).await, before);
    }
    let other_cap = Capability::generate().unwrap();
    migrator_contract(&mut tx).await;
    save(&mut tx).await;
    let error = admit(
        &mut tx,
        &board,
        op,
        1,
        Some(revision),
        original.as_bytes(),
        session(&other_cap, true),
    )
    .await
    .unwrap_err();
    assert_eq!(message(&error), "You have already reported this post.");
    recover(&mut tx).await;
    let unchanged = snapshot(&mut tx, &board, &other_cap.storage_hash()).await;
    assert!(unchanged["session"].is_null());
    assert!(unchanged["links"].as_array().unwrap().is_empty());
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn inactive_category_rejects_and_migrator_cannot_execute_legacy_overloads() {
    let _serial = TEST.lock().await;
    let owner = owner().await;
    let mut tx = owner.begin().await.unwrap();
    let (board, op, reply) = fixture(&mut tx, false).await;
    activate(&mut tx, None).await;
    let revision = import(&mut tx, &[row(1)]).await;
    migrator_contract(&mut tx).await;
    assert_eq!(
        form(&mut tx, &board, op).await,
        json!({"revision":null,"categories":[]})
    );
    let cap = Capability::generate().unwrap();
    let actor = support::fresh_key().public_report_rate_identity("198.51.100.7".parse().unwrap());
    save(&mut tx).await;
    let error = admit(
        &mut tx,
        &board,
        op,
        1,
        Some(revision),
        actor.as_bytes(),
        session(&cap, true),
    )
    .await
    .unwrap_err();
    assert_eq!(message(&error), "Categorical reporting is not active.");
    recover(&mut tx).await;
    reset(&mut tx).await;
    activate(&mut tx, Some(revision)).await;
    let before = snapshot(&mut tx, &board, &cap.storage_hash()).await;
    for active in [true, false] {
        if !active {
            activate(&mut tx, None).await;
        }
        migrator_contract(&mut tx).await;
        save(&mut tx).await;
        let f = session(&cap, true).fingerprints;
        let result: Result<i64, _> = sqlx::query_scalar(
            "SELECT content.admit_report($1,$2,'Legacy reason',$3,$4,$5,$6,$7,true,$8)",
        )
        .bind(&board)
        .bind(op)
        .bind(actor.as_bytes().as_slice())
        .bind(f.token.as_slice())
        .bind(f.network.as_slice())
        .bind(f.address.as_slice())
        .bind(f.environment.as_slice())
        .bind(Utc::now().timestamp())
        .fetch_one(&mut *tx)
        .await;
        assert_eq!(code(&result.unwrap_err()), "42501");
        recover(&mut tx).await;
        reset(&mut tx).await;
        save(&mut tx).await;
        let staff_actor =
            support::fresh_key().public_report_rate_identity("192.0.2.7".parse().unwrap());
        let result: Result<i64, _> =
            sqlx::query_scalar("SELECT content.admit_report($1,$2,'Legacy staff reason',$3)")
                .bind(&board)
                .bind(reply)
                .bind(staff_actor.as_bytes().as_slice())
                .fetch_one(&mut *tx)
                .await;
        assert_eq!(code(&result.unwrap_err()), "42501");
        recover(&mut tx).await;
        reset(&mut tx).await;
        assert_eq!(snapshot(&mut tx, &board, &cap.storage_hash()).await, before);
    }
    // Actual public/staff active and inactive behavior is asserted in the
    // committed race below. Migrator authority never substitutes for theirs.
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn actual_logins_have_only_the_categorical_api_grants_and_pass_readiness() {
    let _serial = TEST.lock().await;
    for (variable, role) in [
        ("TEST_PUBLIC_DATABASE_URL", "board_public"),
        ("STAFF_DATABASE_URL", "board_staff"),
        ("AUTH_DATABASE_URL", "board_auth"),
        ("MIGRATION_DATABASE_URL", "board_migrator"),
    ] {
        let login = pool(variable, role).await;
        for (function, allowed) in [
            ("report_category_form", role != "board_auth"),
            (
                "admit_categorical_report",
                matches!(role, "board_public" | "board_migrator"),
            ),
            ("set_report_catalog_active", role == "board_migrator"),
        ] {
            let privileges: Vec<bool> = sqlx::query_scalar("SELECT has_function_privilege(current_user,p.oid,'EXECUTE') FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname='content' AND p.proname=$1")
                .bind(function).fetch_all(&login).await.unwrap();
            assert_eq!(privileges, vec![allowed], "{role}: {function}");
        }
        // OID-based ACL inspection works even without private-schema USAGE.
        let selector: bool = sqlx::query_scalar("SELECT has_function_privilege(current_user,p.oid,'EXECUTE') FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname='post_secrets' AND p.proname='eligible_report_categories'")
            .fetch_one(&login).await.unwrap();
        assert!(!selector);
        let writable: bool = sqlx::query_scalar("SELECT has_column_privilege(current_user,c.oid,'category_id','INSERT') OR has_column_privilege(current_user,c.oid,'category_base_weight','UPDATE') FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='content' AND c.relname='reports'")
            .fetch_one(&login).await.unwrap();
        if role != "board_migrator" {
            assert!(!writable);
        }
        if role != "board_migrator" {
            assert_eq!(
                code(
                    &sqlx::query("SELECT content.set_report_catalog_active(NULL)")
                        .execute(&login)
                        .await
                        .unwrap_err()
                ),
                "42501"
            );
        }
        if matches!(role, "board_staff" | "board_auth") {
            assert_eq!(code(&sqlx::query("SELECT content.admit_categorical_report('missing',1,1,1,NULL,NULL,NULL,NULL,NULL,true,1)").execute(&login).await.unwrap_err()),"42501");
        }
        if role == "board_auth" {
            assert_eq!(
                code(
                    &sqlx::query("SELECT content.report_category_form('missing',1)")
                        .execute(&login)
                        .await
                        .unwrap_err()
                ),
                "42501"
            );
        }
        if matches!(role, "board_public" | "board_staff") {
            let ready: bool = sqlx::query_scalar(board_store::report_admission::READINESS_SQL)
                .fetch_one(&login)
                .await
                .unwrap();
            assert!(ready, "{role}");
        }
        login.close().await;
    }
}

// ACL/constraint readiness fault injection is exercised by the dedicated
// superuser-owned upgrade shell test, without granting runtime role membership.

#[tokio::test]
async fn activation_validation_and_target_visibility_fail_closed() {
    let _serial = TEST.lock().await;
    let owner = owner().await;
    let mut tx = owner.begin().await.unwrap();
    let (board, op, _) = fixture(&mut tx, false).await;
    let empty = import(&mut tx, &[]).await;
    for invalid in [0, -1, empty, i64::MAX] {
        save(&mut tx).await;
        let error = sqlx::query("SELECT content.set_report_catalog_active($1)")
            .bind(invalid)
            .execute(&mut *tx)
            .await
            .unwrap_err();
        assert_eq!(code(&error), "22023");
        recover(&mut tx).await;
    }
    let revision = import(&mut tx, &[row(31)]).await;
    activate(&mut tx, Some(revision)).await;
    // Deleted target policy also applies to explicitly authorized migrators.
    sqlx::query("UPDATE content.posts SET deleted=true WHERE id=$1")
        .bind(op)
        .execute(&mut *tx)
        .await
        .unwrap();
    migrator_contract(&mut tx).await;
    save(&mut tx).await;
    let error = sqlx::query("SELECT content.report_category_form($1,$2)")
        .bind(&board)
        .bind(op)
        .execute(&mut *tx)
        .await
        .unwrap_err();
    assert_eq!(code(&error), "P0002");
    recover(&mut tx).await;
    tx.rollback().await.unwrap();
    for isolation in [
        "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ",
        "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE",
    ] {
        let mut tx = owner.begin().await.unwrap();
        sqlx::query(isolation).execute(&mut *tx).await.unwrap();
        assert_eq!(
            code(
                &sqlx::query("SELECT content.set_report_catalog_active(NULL)")
                    .execute(&mut *tx)
                    .await
                    .unwrap_err()
            ),
            "22023"
        );
        tx.rollback().await.unwrap();
    }
}

async fn legacy_public(
    c: &mut PgConnection,
    board: &str,
    post: i64,
    cap: &Capability,
) -> Result<i64, sqlx::Error> {
    let actor = support::fresh_key().public_report_rate_identity("198.51.100.7".parse().unwrap());
    let f = session(cap, false).fingerprints;
    sqlx::query_scalar(
        "SELECT content.admit_report($1,$2,'Actual public free text',$3,$4,$5,$6,$7,false,$8)",
    )
    .bind(board)
    .bind(post)
    .bind(actor.as_bytes().as_slice())
    .bind(f.token.as_slice())
    .bind(f.network.as_slice())
    .bind(f.address.as_slice())
    .bind(f.environment.as_slice())
    .bind(Utc::now().timestamp())
    .fetch_one(c)
    .await
}
async fn legacy_staff(pool: &PgPool, board: &str, post: i64) -> Result<i64, sqlx::Error> {
    let actor = support::fresh_key().public_report_rate_identity("192.0.2.7".parse().unwrap());
    sqlx::query_scalar("SELECT content.admit_report($1,$2,'Actual staff free text',$3)")
        .bind(board)
        .bind(post)
        .bind(actor.as_bytes().as_slice())
        .fetch_one(pool)
        .await
}

async fn wait_for_blocker(observer: &PgPool, waiter: i32, blocker: i32) {
    tokio::time::timeout(std::time::Duration::from_secs(5),async {
        loop {
            let blocked: bool = sqlx::query_scalar("SELECT $2=ANY(pg_blocking_pids($1)) AND EXISTS(SELECT 1 FROM pg_locks WHERE pid=$1 AND NOT granted AND locktype='transactionid')")
                .bind(waiter).bind(blocker).fetch_one(observer).await.unwrap();
            if blocked { break; }
            tokio::task::yield_now().await;
        }
    }).await.expect("Expected real row-lock dependency was not observed");
}

#[tokio::test]
async fn committed_activation_after_gate_wait_uses_new_snapshot_and_archive_expiry_is_fresh() {
    let _serial = TEST.lock().await;
    assert!(
        std::env::var("BOARD_TEST_CLUSTER").is_ok_and(|path| path
            .strip_prefix("/tmp/board-postgres.")
            .is_some_and(|tag| tag.len() == 8 && tag.bytes().all(|b| b.is_ascii_alphanumeric()))),
        "Committed configuration race requires the explicit fresh disposable cluster marker"
    );
    let owner = owner().await;
    let mut setup = owner.begin().await.unwrap();
    let (board, op, _) = fixture(&mut setup, false).await;
    let (other, other_op, other_reply) = fixture(&mut setup, false).await;
    assert!(
        form(&mut setup, &board, op).await["revision"].is_null(),
        "Requires initially inactive mode"
    );
    let mut category = row(31);
    category["title"] = json!("Committed synthetic race label");
    let revision = import(&mut setup, &[category]).await;
    setup.commit().await.unwrap();
    // Exactly one immutable synthetic revision remains until this disposable
    // cluster is destroyed. All mutable configuration and owned content are
    // restored even if the spawned assertion body panics.
    let cap = Capability::generate().unwrap();
    let token = cap.storage_hash();
    let test_owner = owner.clone();
    let test_board = board.clone();
    let test_other = other.clone();
    let result = tokio::spawn(async move {
        let owner = test_owner;
        let public = sqlx::postgres::PgPoolOptions::new().max_connections(1).after_connect(|c,_| Box::pin(async move {
            sqlx::query("SET lock_timeout='10s'").execute(&mut *c).await?;
            sqlx::query("SET statement_timeout='15s'").execute(c).await?; Ok(())
        })).connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap()).await.unwrap();
        let waiter: i32 = sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&public).await.unwrap();
        let mut activation = owner.begin().await.unwrap();
        let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *activation).await.unwrap();
        activate(&mut activation,Some(revision)).await;
        let mut c = public.acquire().await.unwrap();
        let target_board = test_other.clone();
        let actor = support::fresh_key().public_report_rate_identity("198.51.100.7".parse().unwrap());
        let admission_session = session(&cap,true);
        let admission = tokio::spawn(async move { admit(&mut c,&target_board,other_reply,31,Some(revision),actor.as_bytes(),admission_session).await });
        wait_for_blocker(&owner,waiter,blocker).await;
        activation.commit().await.unwrap();
        let report = tokio::time::timeout(std::time::Duration::from_secs(5),admission).await.unwrap().unwrap().unwrap();
        let stored: (String,i64,i64,i16,f64) = sqlx::query_as("SELECT reason,category_revision,category_id,category_kind,category_base_weight FROM content.reports WHERE id=$1")
            .bind(report).fetch_one(&owner).await.unwrap();
        assert_eq!(stored,("Committed synthetic race label".into(),revision,31,2,1.0));
        // These assertions use real runtime logins, never SET ROLE from a
        // migrator. Private visibility and free-text bypass prevention remain
        // enforced even for illegal category 31.
        let staff = pool("STAFF_DATABASE_URL","board_staff").await;
        let public_role: String = sqlx::query_scalar("SELECT current_user::text").fetch_one(&public).await.unwrap();
        assert_eq!(public_role,"board_public");
        let mut c = public.acquire().await.unwrap();
        let error = legacy_public(&mut c,&test_other,other_op,&cap).await.unwrap_err();
        assert_eq!(message(&error),"Free-text reporting is not active.");
        drop(c);
        assert_eq!(message(&legacy_staff(&staff,&test_other,other_op).await.unwrap_err()),"Free-text reporting is not active.");
        sqlx::query("UPDATE content.boards SET staff_only=true WHERE slug=$1").bind(&test_other).execute(&owner).await.unwrap();
        let error = sqlx::query("SELECT content.report_category_form($1,$2)").bind(&test_other).bind(other_op).execute(&public).await.unwrap_err();
        assert_eq!(code(&error),"P0002");
        let mut c = public.acquire().await.unwrap();
        let actor = support::fresh_key().public_report_rate_identity("198.51.100.7".parse().unwrap());
        let error = admit(&mut c,&test_other,other_op,31,Some(revision),actor.as_bytes(),session(&cap,false)).await.unwrap_err();
        assert_eq!(code(&error),"P0002");
        drop(c);
        let mut c = staff.acquire().await.unwrap();
        assert_eq!(form(&mut c,&test_other,other_op).await["categories"][0]["id"],31);
        drop(c);
        sqlx::query("UPDATE content.boards SET staff_only=false WHERE slug=$1").bind(&test_other).execute(&owner).await.unwrap();
        // Retire the established report's quotas without deleting its session.
        sqlx::query("UPDATE content.posts SET deleted=true WHERE id=$1").bind(other_reply).execute(&owner).await.unwrap();
        sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1")
            .bind(op).execute(&owner).await.unwrap();
        let before: String = sqlx::query_scalar("SELECT to_jsonb(s)::text FROM post_secrets.anonymous_sessions s WHERE token_hash=$1")
            .bind(token.as_slice()).fetch_one(&owner).await.unwrap();
        let mut lock = owner.begin().await.unwrap();
        let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *lock).await.unwrap();
        sqlx::query("SELECT token_hash FROM post_secrets.anonymous_sessions WHERE token_hash=$1 FOR UPDATE")
            .bind(token.as_slice()).fetch_one(&mut *lock).await.unwrap();
        let mut c = public.acquire().await.unwrap();
        let target_board = test_board.clone();
        let actor = support::fresh_key().public_report_rate_identity("203.0.113.7".parse().unwrap());
        let admission_session = session(&cap,false);
        let admission = tokio::spawn(async move { admit(&mut c,&target_board,op,31,Some(revision),actor.as_bytes(),admission_session).await });
        wait_for_blocker(&owner,waiter,blocker).await;
        sqlx::query("UPDATE content.threads SET archive_expires_at=clock_timestamp()-interval '1 microsecond' WHERE id=$1")
            .bind(op).execute(&owner).await.unwrap();
        lock.commit().await.unwrap();
        let error = tokio::time::timeout(std::time::Duration::from_secs(5),admission).await.unwrap().unwrap().unwrap_err();
        assert_eq!(code(&error),"P0002");
        let after: String = sqlx::query_scalar("SELECT to_jsonb(s)::text FROM post_secrets.anonymous_sessions s WHERE token_hash=$1")
            .bind(token.as_slice()).fetch_one(&owner).await.unwrap();
        assert_eq!(before,after,"Expired target must not refresh session activity");
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM content.reports WHERE board=$1").bind(&test_board).fetch_one(&owner).await.unwrap();
        assert_eq!(count,0);
        // A second gate wait observes committed deactivation rather than a stale
        // active pointer from the outer SELECT statement's initial snapshot.
        let mut deactivation = owner.begin().await.unwrap();
        let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *deactivation).await.unwrap();
        activate(&mut deactivation,None).await;
        let fresh = Capability::generate().unwrap();
        let actor = support::fresh_key().public_report_rate_identity("192.0.2.7".parse().unwrap());
        let mut c = public.acquire().await.unwrap();
        let target_board = test_other.clone();
        let admission_session = session(&fresh,true);
        let admission = tokio::spawn(async move { admit(&mut c,&target_board,other_op,31,Some(revision),actor.as_bytes(),admission_session).await });
        wait_for_blocker(&owner,waiter,blocker).await;
        deactivation.commit().await.unwrap();
        let error = tokio::time::timeout(std::time::Duration::from_secs(5),admission).await.unwrap().unwrap().unwrap_err();
        assert_eq!(message(&error),"Categorical reporting is not active.");
        let mut c = public.acquire().await.unwrap();
        assert_eq!(form(&mut c,&test_other,other_op).await,json!({"revision":null,"categories":[]}));
        legacy_public(&mut c,&test_other,other_op,&cap).await.unwrap();
        drop(c);
        legacy_staff(&staff,&test_other,other_op).await.unwrap();
        let metadata_null: bool = sqlx::query_scalar("SELECT bool_and(category_revision IS NULL AND category_id IS NULL AND category_kind IS NULL AND category_base_weight IS NULL) FROM content.reports WHERE board=$1 AND post_id=$2")
            .bind(&test_other).bind(other_op).fetch_one(&owner).await.unwrap();
        assert!(metadata_null);
        staff.close().await;
        public.close().await;
    }).await;
    // Also executes after panics inside the race body. No foreign fixture rows
    // or catalog history are rewritten during cleanup.
    let mut cleanup = owner.begin().await.unwrap();
    activate(&mut cleanup, None).await;
    let boards = vec![board.clone(), other.clone()];
    sqlx::query("SELECT slug FROM content.boards WHERE slug=ANY($1) ORDER BY slug FOR UPDATE")
        .bind(&boards)
        .fetch_all(&mut *cleanup)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM content.threads WHERE board=ANY($1) ORDER BY board,id FOR UPDATE")
        .bind(&boards)
        .fetch_all(&mut *cleanup)
        .await
        .unwrap();
    // Lock the owned session before report/post cascades lock memberships.
    sqlx::query("SELECT token_hash FROM post_secrets.anonymous_sessions WHERE token_hash=$1 ORDER BY token_hash FOR UPDATE")
        .bind(token.as_slice())
        .fetch_all(&mut *cleanup)
        .await
        .unwrap();
    for slug in [&board, &other] {
        sqlx::query("DELETE FROM content.reports WHERE board=$1")
            .bind(slug)
            .execute(&mut *cleanup)
            .await
            .unwrap();
        sqlx::query("DELETE FROM content.posts WHERE board=$1")
            .bind(slug)
            .execute(&mut *cleanup)
            .await
            .unwrap();
        sqlx::query("DELETE FROM content.threads WHERE board=$1")
            .bind(slug)
            .execute(&mut *cleanup)
            .await
            .unwrap();
        sqlx::query("DELETE FROM content.boards WHERE slug=$1")
            .bind(slug)
            .execute(&mut *cleanup)
            .await
            .unwrap();
    }
    sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
        .bind(token.as_slice())
        .execute(&mut *cleanup)
        .await
        .unwrap();
    cleanup.commit().await.unwrap();
    result.unwrap();
}
