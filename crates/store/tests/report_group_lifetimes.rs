#![cfg(feature = "database-tests")]
mod support;

use board_domain::anonymous_session::Capability;
use chrono::Utc;
use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool};
use std::time::Duration;

// Run database-test binaries serially. SQL contracts are owner-maintenance
// fixtures in rolled-back transactions; runtime authorization uses real logins.
static TEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn login(variable: &str, expected: &str) -> PgPool {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .after_connect(|c, _| {
            Box::pin(async move {
                sqlx::query("SET lock_timeout='10s'")
                    .execute(&mut *c)
                    .await?;
                sqlx::query("SET statement_timeout='15s'")
                    .execute(c)
                    .await?;
                Ok(())
            })
        })
        .connect(&std::env::var(variable).expect("owned database URL required"))
        .await
        .unwrap();
    let actual: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(actual, expected);
    pool
}
async fn reset(c: &mut PgConnection) {
    sqlx::query("RESET ROLE").execute(c).await.unwrap();
}
async fn private(c: &mut PgConnection) {
    sqlx::query("SET LOCAL ROLE board_report_admission_owner")
        .execute(c)
        .await
        .unwrap();
}
fn code(e: &sqlx::Error) -> String {
    e.as_database_error().unwrap().code().unwrap().into_owned()
}
async fn catalog(c: &mut PgConnection) -> i64 {
    let rows: Vec<Value> = [1, 31]
        .into_iter()
        .map(|id| {
            json!({
                "id":id,"board":"","op_only":false,"reply_only":false,"image_only":false,
                "exclude_boards":null,"title":"Owned lifetime category","weight":1.0,"filtered":0
            })
        })
        .collect();
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
async fn fixture(c: &mut PgConnection, replies: usize) -> (String, Vec<i64>) {
    let op: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&mut *c)
        .await
        .unwrap();
    let board = format!("gl{op:x}");
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,archive_retention_seconds,posting_reply_seconds,posting_image_seconds,posting_thread_seconds) VALUES($1,'Owned group lifetime','Synthetic',2000,100,100,100,10,3600,0,0,0)")
        .bind(&board).execute(&mut *c).await.unwrap();
    sqlx::query("INSERT INTO content.threads(id,board) VALUES($1,$2)")
        .bind(op)
        .bind(&board)
        .execute(&mut *c)
        .await
        .unwrap();
    sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous','','Synthetic OP')")
        .bind(op).bind(&board).execute(&mut *c).await.unwrap();
    let mut posts = vec![op];
    for _ in 0..replies {
        posts.push(sqlx::query_scalar("INSERT INTO content.posts(board,thread_id,name,subject,comment) VALUES($1,$2,'Anonymous','','Synthetic reply') RETURNING id")
            .bind(&board).bind(op).fetch_one(&mut *c).await.unwrap());
    }
    (board, posts)
}
async fn reports(
    c: &mut PgConnection,
    board: &str,
    post: i64,
    revision: i64,
    kinds: &[Option<i16>],
) -> Vec<i64> {
    let mut ids = Vec::new();
    for kind in kinds {
        ids.push(sqlx::query_scalar("INSERT INTO content.reports(board,post_id,reason,category_revision,category_id,category_kind,category_base_weight) VALUES($1,$2,'Owned report',$3,$4,$5,$6) RETURNING id")
            .bind(board).bind(post).bind(kind.map(|_|revision))
            .bind(kind.map(|k| if k==2 {31_i64} else {1_i64}))
            .bind(*kind).bind(kind.map(|_|1.0_f64)).fetch_one(&mut *c).await.unwrap());
    }
    ids
}
async fn insert_members(c: &mut PgConnection, board: &str, op: i64, post: i64, ids: &[i64]) -> u64 {
    private(c).await;
    let affected = sqlx::query("INSERT INTO post_secrets.report_membership(report_id,actor_hash,board,post_id,thread_id,reported_at) SELECT id,decode(repeat('ab',32),'hex'),$2,$3,$4,clock_timestamp() FROM unnest($1::bigint[]) id ON CONFLICT(report_id) DO NOTHING")
        .bind(ids).bind(board).bind(post).bind(op).execute(&mut *c).await.unwrap().rows_affected();
    reset(c).await;
    affected
}
async fn add(
    c: &mut PgConnection,
    board: &str,
    op: i64,
    post: i64,
    revision: i64,
    kinds: &[Option<i16>],
) -> Vec<i64> {
    let ids = reports(c, board, post, revision, kinds).await;
    assert_eq!(
        insert_members(c, board, op, post, &ids).await,
        ids.len() as u64
    );
    ids
}
async fn group(c: &mut PgConnection, board: &str, post: i64) -> Option<(i64, bool)> {
    private(c).await;
    let result = sqlx::query_as("SELECT illegal_count,incomplete FROM post_secrets.report_group WHERE board=$1 AND post_id=$2")
        .bind(board).bind(post).fetch_optional(&mut *c).await.unwrap();
    reset(c).await;
    result
}
async fn members(c: &mut PgConnection, board: &str, post: i64) -> i64 {
    private(c).await;
    let result = sqlx::query_scalar(
        "SELECT count(*) FROM post_secrets.report_membership WHERE board=$1 AND post_id=$2",
    )
    .bind(board)
    .bind(post)
    .fetch_one(&mut *c)
    .await
    .unwrap();
    reset(c).await;
    result
}
async fn purge(c: &mut PgConnection, ids: &[i64]) {
    private(c).await;
    sqlx::query("DELETE FROM post_secrets.report_membership WHERE report_id=ANY($1)")
        .bind(ids)
        .execute(&mut *c)
        .await
        .unwrap();
    reset(c).await;
}
async fn archive(c: &mut PgConnection, op: i64) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1")
        .bind(op).execute(c).await.map(|_|())
}
async fn history(c: &mut PgConnection, board: &str) -> String {
    sqlx::query_scalar("SELECT jsonb_build_object('reports',(SELECT coalesce(jsonb_agg(to_jsonb(r) ORDER BY id),'[]') FROM content.reports r WHERE board=$1),'audit',(SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY id),'[]') FROM content.moderation_audit a WHERE board=$1))::text")
        .bind(board).fetch_one(c).await.unwrap()
}

#[tokio::test]
async fn archive_uses_each_posts_lifetime_threshold_and_preserves_all_history() {
    let _serial = TEST.lock().await;
    let owner = login("MIGRATION_DATABASE_URL", "board_migrator").await;
    let mut tx = owner.begin().await.unwrap();
    let (board, posts) = fixture(&mut tx, 5).await;
    let revision = catalog(&mut tx).await;
    for (count, post) in posts[..4].iter().enumerate() {
        let mut kinds = vec![Some(1), Some(1)];
        kinds.extend(std::iter::repeat_n(Some(2), count));
        add(&mut tx, &board, posts[0], *post, revision, &kinds).await;
        assert_eq!(
            group(&mut tx, &board, *post).await,
            Some((count as i64, false))
        );
    }
    add(
        &mut tx,
        &board,
        posts[0],
        posts[4],
        revision,
        &[None, Some(1)],
    )
    .await;
    // Owner-only simulation of an untouched pre-migration membership.
    private(&mut tx).await;
    sqlx::query(
        "ALTER TABLE post_secrets.report_membership DISABLE TRIGGER report_membership_group_insert",
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    reset(&mut tx).await;
    add(&mut tx, &board, posts[0], posts[5], revision, &[Some(1)]).await;
    private(&mut tx).await;
    sqlx::query(
        "ALTER TABLE post_secrets.report_membership ENABLE TRIGGER report_membership_group_insert",
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    reset(&mut tx).await;
    sqlx::query("INSERT INTO content.moderation_audit(account_id,board,target_id,action) VALUES(1,$1,$2,'resolve')")
        .bind(&board).bind(posts[0]).execute(&mut *tx).await.unwrap();
    let before = history(&mut tx, &board).await;
    archive(&mut tx, posts[0]).await.unwrap();
    for post in &posts[..3] {
        assert_eq!(members(&mut tx, &board, *post).await, 0);
        assert_eq!(group(&mut tx, &board, *post).await, None);
    }
    assert_eq!(members(&mut tx, &board, posts[3]).await, 5);
    assert_eq!(members(&mut tx, &board, posts[4]).await, 2);
    assert_eq!(members(&mut tx, &board, posts[5]).await, 1);
    assert_eq!(group(&mut tx, &board, posts[5]).await, None);
    assert_eq!(history(&mut tx, &board).await, before);
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn partial_purge_preserves_count_and_taint_but_empty_resets_without_history_backfill() {
    let _serial = TEST.lock().await;
    let owner = login("MIGRATION_DATABASE_URL", "board_migrator").await;
    let mut tx = owner.begin().await.unwrap();
    let (board, posts) = fixture(&mut tx, 1).await;
    let revision = catalog(&mut tx).await;
    let ids = add(
        &mut tx,
        &board,
        posts[0],
        posts[0],
        revision,
        &[Some(2), Some(2), Some(2), Some(1)],
    )
    .await;
    let tainted = add(
        &mut tx,
        &board,
        posts[0],
        posts[1],
        revision,
        &[None, Some(1)],
    )
    .await;
    purge(&mut tx, &ids[..3]).await;
    purge(&mut tx, &tainted[..1]).await;
    assert_eq!(group(&mut tx, &board, posts[0]).await, Some((3, false)));
    assert_eq!(group(&mut tx, &board, posts[1]).await, Some((0, true)));
    archive(&mut tx, posts[0]).await.unwrap();
    assert_eq!(members(&mut tx, &board, posts[0]).await, 1);
    assert_eq!(members(&mut tx, &board, posts[1]).await, 1);
    purge(&mut tx, &ids).await;
    purge(&mut tx, &tainted).await;
    for post in &posts {
        assert_eq!(group(&mut tx, &board, *post).await, None);
    }
    let before = history(&mut tx, &board).await;
    add(&mut tx, &board, posts[0], posts[0], revision, &[Some(1)]).await;
    add(&mut tx, &board, posts[0], posts[1], revision, &[Some(2)]).await;
    assert_eq!(group(&mut tx, &board, posts[0]).await, Some((0, false)));
    assert_eq!(group(&mut tx, &board, posts[1]).await, Some((1, false)));
    assert_ne!(history(&mut tx, &board).await, before);
    // A repeated nonnull archive UPDATE must not retire post-archive members.
    archive(&mut tx, posts[0]).await.unwrap();
    assert_eq!(members(&mut tx, &board, posts[0]).await, 1);
    assert_eq!(members(&mut tx, &board, posts[1]).await, 1);
    sqlx::query("UPDATE content.threads SET archived_at=NULL,archive_expires_at=NULL WHERE id=$1")
        .bind(posts[0])
        .execute(&mut *tx)
        .await
        .unwrap();
    let before = history(&mut tx, &board).await;
    archive(&mut tx, posts[0]).await.unwrap();
    for post in &posts {
        assert_eq!(group(&mut tx, &board, *post).await, None);
    }
    assert_eq!(history(&mut tx, &board).await, before);
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn pre_counter_membership_taints_first_new_insert_even_when_all_kinds_are_known() {
    let _serial = TEST.lock().await;
    let owner = login("MIGRATION_DATABASE_URL", "board_migrator").await;
    let mut tx = owner.begin().await.unwrap();
    let (board, posts) = fixture(&mut tx, 0).await;
    let revision = catalog(&mut tx).await;
    private(&mut tx).await;
    sqlx::query(
        "ALTER TABLE post_secrets.report_membership DISABLE TRIGGER report_membership_group_insert",
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    reset(&mut tx).await;
    let old = add(
        &mut tx,
        &board,
        posts[0],
        posts[0],
        revision,
        &[Some(2), Some(1)],
    )
    .await;
    assert_eq!(group(&mut tx, &board, posts[0]).await, None);
    private(&mut tx).await;
    sqlx::query(
        "ALTER TABLE post_secrets.report_membership ENABLE TRIGGER report_membership_group_insert",
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    reset(&mut tx).await;
    let new = add(&mut tx, &board, posts[0], posts[0], revision, &[Some(2)]).await;
    assert_eq!(group(&mut tx, &board, posts[0]).await, Some((1, true)));
    purge(&mut tx, &old).await;
    assert_eq!(group(&mut tx, &board, posts[0]).await, Some((1, true)));
    archive(&mut tx, posts[0]).await.unwrap();
    assert_eq!(members(&mut tx, &board, posts[0]).await, 1);
    purge(&mut tx, &new).await;
    assert_eq!(group(&mut tx, &board, posts[0]).await, None);
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn bulk_conflicts_zero_rows_and_identity_updates_count_only_new_members() {
    let _serial = TEST.lock().await;
    let owner = login("MIGRATION_DATABASE_URL", "board_migrator").await;
    let mut tx = owner.begin().await.unwrap();
    let (board, posts) = fixture(&mut tx, 0).await;
    let revision = catalog(&mut tx).await;
    let ids = add(
        &mut tx,
        &board,
        posts[0],
        posts[0],
        revision,
        &[Some(2), Some(2), Some(1)],
    )
    .await;
    assert_eq!(
        insert_members(&mut tx, &board, posts[0], posts[0], &ids).await,
        0
    );
    assert_eq!(
        insert_members(&mut tx, &board, posts[0], posts[0], &[]).await,
        0
    );
    purge(&mut tx, &[]).await;
    assert_eq!(group(&mut tx, &board, posts[0]).await, Some((2, false)));
    let new = reports(&mut tx, &board, posts[0], revision, &[Some(2)]).await;
    let mixed = [ids[0], new[0]];
    assert_eq!(
        insert_members(&mut tx, &board, posts[0], posts[0], &mixed).await,
        1
    );
    private(&mut tx).await;
    sqlx::query("UPDATE post_secrets.report_membership SET automatic_identity=gen_random_uuid() WHERE report_id=ANY($1)").bind(&ids).execute(&mut *tx).await.unwrap();
    reset(&mut tx).await;
    assert_eq!(group(&mut tx, &board, posts[0]).await, Some((3, false)));
    tx.rollback().await.unwrap();
}

async fn admit(
    c: &mut PgConnection,
    board: &str,
    post: i64,
    category: i64,
    revision: i64,
    cap: &Capability,
    valid_network: bool,
) -> Result<i64, sqlx::Error> {
    let actor = support::fresh_key().public_report_rate_identity("198.51.100.7".parse().unwrap());
    let f = cap.fingerprints(Some("198.51.100.7".parse().unwrap()), *b"US");
    sqlx::query_scalar(
        "SELECT content.admit_categorical_report($1,$2,$3,$4,$5,$6,$7,$8,$9,true,$10)",
    )
    .bind(board)
    .bind(post)
    .bind(category)
    .bind(revision)
    .bind(actor.as_bytes().as_slice())
    .bind(f.token.as_slice())
    .bind(if valid_network {
        f.network.as_slice()
    } else {
        &[]
    })
    .bind(f.address.as_slice())
    .bind(f.environment.as_slice())
    .bind(Utc::now().timestamp())
    .fetch_one(c)
    .await
}

#[tokio::test]
async fn registration_failure_rolls_back_new_and_existing_group_counters() {
    let _serial = TEST.lock().await;
    let owner = login("MIGRATION_DATABASE_URL", "board_migrator").await;
    let mut tx = owner.begin().await.unwrap();
    let (board, posts) = fixture(&mut tx, 0).await;
    let revision = catalog(&mut tx).await;
    activate(&mut tx, Some(revision)).await;
    for expected in [None, Some((1, false))] {
        assert_eq!(group(&mut tx, &board, posts[0]).await, expected);
        let before = history(&mut tx, &board).await;
        let before_members = members(&mut tx, &board, posts[0]).await;
        let cap = Capability::generate().unwrap();
        sqlx::query("SAVEPOINT registration_failure")
            .execute(&mut *tx)
            .await
            .unwrap();
        let error = admit(&mut tx, &board, posts[0], 31, revision, &cap, false)
            .await
            .unwrap_err();
        assert_eq!(code(&error), "23514");
        sqlx::query("ROLLBACK TO SAVEPOINT registration_failure")
            .execute(&mut *tx)
            .await
            .unwrap();
        assert_eq!(group(&mut tx, &board, posts[0]).await, expected);
        assert_eq!(members(&mut tx, &board, posts[0]).await, before_members);
        assert_eq!(history(&mut tx, &board).await, before);
        let links: i64 = sqlx::query_scalar("SELECT (SELECT count(*) FROM post_secrets.anonymous_sessions WHERE token_hash=$1)+(SELECT count(*) FROM post_secrets.anonymous_reports WHERE token_hash=$1)")
            .bind(cap.storage_hash().as_slice()).fetch_one(&mut *tx).await.unwrap();
        assert_eq!(links, 0);
        if expected.is_none() {
            admit(&mut tx, &board, posts[0], 31, revision, &cap, true)
                .await
                .unwrap();
            // register_anonymous_report's automatic_identity UPDATE is not a
            // second contribution to the INSERT statement's counter.
            assert_eq!(group(&mut tx, &board, posts[0]).await, Some((1, false)));
        }
    }
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn counter_and_trigger_functions_are_private_for_actual_runtime_logins() {
    let _serial = TEST.lock().await;
    for (variable, role) in [
        ("TEST_PUBLIC_DATABASE_URL", "board_public"),
        ("STAFF_DATABASE_URL", "board_staff"),
        ("AUTH_DATABASE_URL", "board_auth"),
        ("MIGRATION_DATABASE_URL", "board_migrator"),
    ] {
        let pool = login(variable, role).await;
        // Resolve OIDs through pg_catalog, including for auth which does not
        // have schema USAGE. A schema-qualified regclass cast would mask ACLs.
        let acl: (bool,bool,bool,bool,bool,bool) = sqlx::query_as("SELECT has_table_privilege(current_user,c.oid,'SELECT'),has_table_privilege(current_user,c.oid,'INSERT'),has_table_privilege(current_user,c.oid,'UPDATE'),has_table_privilege(current_user,c.oid,'DELETE'),has_table_privilege(current_user,c.oid,'TRUNCATE'),has_any_column_privilege(current_user,c.oid,'SELECT,INSERT,UPDATE,REFERENCES') FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='post_secrets' AND c.relname='report_group'")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(acl, (false, false, false, false, false, false), "{role}");
        let functions: Vec<(String,bool)> = sqlx::query_as("SELECT p.proname,has_function_privilege(current_user,p.oid,'EXECUTE') FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname='post_secrets' AND p.proname IN ('increment_report_group','retire_empty_report_group','retire_archived_report_membership') ORDER BY p.proname")
            .fetch_all(&pool).await.unwrap();
        assert_eq!(functions.len(), 3);
        assert!(
            functions.iter().all(|(_, allowed)| !allowed),
            "{role}: {functions:?}"
        );
        if matches!(role, "board_public" | "board_staff") {
            let ready: bool = sqlx::query_scalar(board_store::report_admission::READINESS_SQL)
                .fetch_one(&pool)
                .await
                .unwrap();
            assert!(ready, "{role}");
        }
        pool.close().await;
    }
}

async fn wait_for_blocker(observer: &PgPool, waiter: i32, blocker: i32) {
    tokio::time::timeout(Duration::from_secs(5),async {
        loop {
            let blocked: bool = sqlx::query_scalar("SELECT $2=ANY(pg_blocking_pids($1)) AND EXISTS(SELECT 1 FROM pg_locks WHERE pid=$1 AND NOT granted AND locktype='transactionid')")
                .bind(waiter).bind(blocker).fetch_one(observer).await.unwrap();
            if blocked { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("real admission-to-archive transaction lock dependency");
}
async fn inspect(pool: &PgPool, board: &str, post: i64) -> Option<(i64, bool)> {
    let mut tx = pool.begin().await.unwrap();
    let result = group(&mut tx, board, post).await;
    tx.rollback().await.unwrap();
    result
}
async fn runtime_report(
    pool: &PgPool,
    board: &str,
    post: i64,
    category: i64,
    revision: i64,
) -> i64 {
    let mut c = pool.acquire().await.unwrap();
    admit(
        &mut c,
        board,
        post,
        category,
        revision,
        &Capability::generate().unwrap(),
        true,
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn actual_runtime_lifecycle_and_archive_lock_inversion_retry() {
    let _serial = TEST.lock().await;
    assert!(
        std::env::var("BOARD_TEST_CLUSTER").is_ok_and(|path| path
            .strip_prefix("/tmp/board-postgres.")
            .is_some_and(|tag| tag.len() == 8 && tag.bytes().all(|b| b.is_ascii_alphanumeric()))),
        "committed catalog fixture requires exact fresh disposable cluster marker"
    );
    let owner = login("MIGRATION_DATABASE_URL", "board_migrator").await;
    let mut setup = owner.begin().await.unwrap();
    let (board, posts) = fixture(&mut setup, 3).await;
    let (race_board, race_posts) = fixture(&mut setup, 0).await;
    let (operator_board, operator_posts) = fixture(&mut setup, 0).await;
    let second_thread: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&mut *setup)
        .await
        .unwrap();
    sqlx::query("INSERT INTO content.threads(id,board) VALUES($1,$2)")
        .bind(second_thread)
        .bind(&operator_board)
        .execute(&mut *setup)
        .await
        .unwrap();
    sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous','','Operator order fixture')")
        .bind(second_thread).bind(&operator_board).execute(&mut *setup).await.unwrap();
    let inactive: String = sqlx::query_scalar("SELECT content.report_category_form($1,$2)::text")
        .bind(&board)
        .bind(posts[0])
        .fetch_one(&mut *setup)
        .await
        .unwrap();
    assert!(
        serde_json::from_str::<Value>(&inactive).unwrap()["revision"].is_null(),
        "requires initially inactive catalog"
    );
    let revision = catalog(&mut setup).await;
    activate(&mut setup, Some(revision)).await;
    for post in &posts[1..3] {
        sqlx::query("INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler) VALUES($1,replace(gen_random_uuid()::text,'-',''),replace(gen_random_uuid()::text,'-',''),'owned.png',1,1,1,false)")
            .bind(post).execute(&mut *setup).await.unwrap();
    }
    setup.commit().await.unwrap();
    let body_owner = owner.clone();
    let body_board = board.clone();
    let body_race_board = race_board.clone();
    let body_operator_board = operator_board.clone();
    // Panic isolation ensures global mode is restored and only owned content
    // is cleaned. Exactly one immutable revision remains until cluster teardown.
    let result = tokio::spawn(async move {
        let owner = body_owner;
        let board = body_board;
        let race_board = body_race_board;
        let public = login("TEST_PUBLIC_DATABASE_URL", "board_public").await;
        let staff = login("STAFF_DATABASE_URL", "board_staff").await;
        let mut report_ids = Vec::new();
        for post in &posts {
            report_ids.push(runtime_report(&public, &board, *post, 31, revision).await);
            assert_eq!(inspect(&owner, &board, *post).await, Some((1, false)));
        }
        sqlx::query("SELECT content.delete_post_attachment($1,$2)")
            .bind(&board)
            .bind(posts[1])
            .execute(&public)
            .await
            .unwrap();
        assert_eq!(inspect(&owner, &board, posts[1]).await, Some((1, false)));
        let error = sqlx::query("SELECT content.staff_delete_post_attachment($1,$2)")
            .bind(&board)
            .bind(posts[1])
            .execute(&staff)
            .await
            .unwrap_err();
        assert_eq!(code(&error), "P0002");
        assert_eq!(inspect(&owner, &board, posts[1]).await, Some((1, false)));
        sqlx::query("SELECT content.staff_delete_post_attachment($1,$2)")
            .bind(&board)
            .bind(posts[2])
            .execute(&staff)
            .await
            .unwrap();
        assert_eq!(inspect(&owner, &board, posts[2]).await, None);
        report_ids.push(runtime_report(&public, &board, posts[2], 1, revision).await);
        let error = sqlx::query("SELECT content.staff_delete_post_attachment($1,$2)")
            .bind(&board)
            .bind(posts[2])
            .execute(&staff)
            .await
            .unwrap_err();
        assert_eq!(code(&error), "P0002");
        assert_eq!(inspect(&owner, &board, posts[2]).await, Some((0, false)));
        sqlx::query("UPDATE content.posts SET deleted=true WHERE id=$1")
            .bind(posts[3])
            .execute(&owner)
            .await
            .unwrap();
        assert_eq!(inspect(&owner, &board, posts[3]).await, None);
        assert_eq!(inspect(&owner, &board, posts[0]).await, Some((1, false)));
        let mut c = owner.acquire().await.unwrap();
        let before = history(&mut c, &board).await;
        archive(&mut c, posts[0]).await.unwrap();
        assert_eq!(history(&mut c, &board).await, before);
        drop(c);
        for post in &posts {
            assert_eq!(inspect(&owner, &board, *post).await, None);
        }
        report_ids.push(runtime_report(&public, &board, posts[0], 1, revision).await);
        let mut c = owner.acquire().await.unwrap();
        archive(&mut c, posts[0]).await.unwrap();
        drop(c);
        assert_eq!(inspect(&owner, &board, posts[0]).await, Some((0, false)));
        report_ids.push(runtime_report(&public, &board, posts[1], 31, revision).await);
        assert_eq!(inspect(&owner, &board, posts[1]).await, Some((1, false)));
        sqlx::query("UPDATE content.threads SET deleted=true WHERE id=$1")
            .bind(posts[0])
            .execute(&owner)
            .await
            .unwrap();
        assert_eq!(inspect(&owner, &board, posts[0]).await, None);
        assert_eq!(inspect(&owner, &board, posts[1]).await, None);
        let retained: i64 =
            sqlx::query_scalar("SELECT count(*) FROM content.reports WHERE id=ANY($1)")
                .bind(&report_ids)
                .fetch_one(&owner)
                .await
                .unwrap();
        assert_eq!(retained, report_ids.len() as i64);

        // Real inverted order: the raw archiver owns the thread row first;
        // admission owns the board then waits for its membership FK key-share
        // lock. The archive trigger must return 55P03, not deadlock or wait.
        let mut archiver = owner.begin().await.unwrap();
        let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *archiver)
            .await
            .unwrap();
        sqlx::query("SELECT id FROM content.threads WHERE id=$1 FOR UPDATE")
            .bind(race_posts[0])
            .execute(&mut *archiver)
            .await
            .unwrap();
        let mut admission = public.acquire().await.unwrap();
        let waiter: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *admission)
            .await
            .unwrap();
        let cap = Capability::generate().unwrap();
        let admit_future = admit(
            &mut admission,
            &race_board,
            race_posts[0],
            31,
            revision,
            &cap,
            true,
        );
        let archive_future = async {
            wait_for_blocker(&owner, waiter, blocker).await;
            let result = archive(&mut archiver, race_posts[0]).await;
            archiver.rollback().await.unwrap();
            assert_eq!(code(&result.unwrap_err()), "55P03");
        };
        let (report, ()) = tokio::join!(admit_future, archive_future);
        let report = report.unwrap();
        drop(admission);
        assert_eq!(
            inspect(&owner, &race_board, race_posts[0]).await,
            Some((1, false))
        );
        let mut retry = owner.begin().await.unwrap();
        sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
            .bind(&race_board)
            .execute(&mut *retry)
            .await
            .unwrap();
        archive(&mut retry, race_posts[0]).await.unwrap();
        retry.commit().await.unwrap();
        assert_eq!(inspect(&owner, &race_board, race_posts[0]).await, None);
        let retained: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM content.reports WHERE id=$1)")
                .bind(report)
                .fetch_one(&owner)
                .await
                .unwrap();
        assert!(retained);
        operator_board_first_serializes_opposite_thread_order(
            &owner,
            &body_operator_board,
            [operator_posts[0], second_thread],
            revision,
        )
        .await;
        public.close().await;
        staff.close().await;
    })
    .await;
    let mut cleanup = owner.begin().await.unwrap();
    activate(&mut cleanup, None).await;
    let boards = vec![board, race_board, operator_board];
    let tokens: Vec<Vec<u8>> = sqlx::query_scalar("SELECT a.token_hash FROM post_secrets.anonymous_reports a JOIN content.reports r ON r.id=a.report_id WHERE r.board=ANY($1)")
        .bind(&boards).fetch_all(&mut *cleanup).await.unwrap();
    for query in [
        "DELETE FROM content.reports WHERE board=ANY($1)",
        "DELETE FROM content.post_media WHERE post_id IN (SELECT id FROM content.posts WHERE board=ANY($1))",
        "DELETE FROM content.posts WHERE board=ANY($1)",
        "DELETE FROM content.threads WHERE board=ANY($1)",
        "DELETE FROM content.boards WHERE slug=ANY($1)",
    ] {
        sqlx::query(query)
            .bind(&boards)
            .execute(&mut *cleanup)
            .await
            .unwrap();
    }
    for token in tokens {
        sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
            .bind(token)
            .execute(&mut *cleanup)
            .await
            .unwrap();
    }
    cleanup.commit().await.unwrap();
    result.unwrap();
}

#[tokio::test]
async fn one_statement_groups_multiple_posts_and_boards_and_bulk_delete_retires_only_empty() {
    let _serial = TEST.lock().await;
    let owner = login("MIGRATION_DATABASE_URL", "board_migrator").await;
    let mut tx = owner.begin().await.unwrap();
    let (board, posts) = fixture(&mut tx, 1).await;
    let (other, other_posts) = fixture(&mut tx, 0).await;
    let revision = catalog(&mut tx).await;
    let a = reports(&mut tx, &board, posts[0], revision, &[Some(2), Some(1)]).await;
    let b = reports(&mut tx, &board, posts[1], revision, &[None, Some(2)]).await;
    let c = reports(
        &mut tx,
        &other,
        other_posts[0],
        revision,
        &[Some(2), Some(2)],
    )
    .await;
    // These newly inserted fixture boards are already owned by this transaction.
    // Explicitly demonstrate the supported operator boundary before bulk DML;
    // AFTER STATEMENT guards cannot prevent an earlier foreign-key lock wait.
    sqlx::query("SELECT slug FROM content.boards WHERE slug=ANY($1) ORDER BY slug FOR UPDATE")
        .bind(vec![board.clone(), other.clone()])
        .execute(&mut *tx)
        .await
        .unwrap();
    // Deliberately interleave input order: grouping remains target-independent.
    let ids = vec![c[0], a[0], b[0], c[1], a[1], b[1]];
    let boards = vec![
        other.clone(),
        board.clone(),
        board.clone(),
        other.clone(),
        board.clone(),
        board.clone(),
    ];
    let targets = vec![
        other_posts[0],
        posts[0],
        posts[1],
        other_posts[0],
        posts[0],
        posts[1],
    ];
    let threads = vec![
        other_posts[0],
        posts[0],
        posts[0],
        other_posts[0],
        posts[0],
        posts[0],
    ];
    private(&mut tx).await;
    let inserted = sqlx::query("INSERT INTO post_secrets.report_membership(report_id,actor_hash,board,post_id,thread_id,reported_at) SELECT id,decode(repeat('cd',32),'hex'),board,post,thread,clock_timestamp() FROM unnest($1::bigint[],$2::text[],$3::bigint[],$4::bigint[]) AS input(id,board,post,thread)")
        .bind(&ids).bind(&boards).bind(&targets).bind(&threads).execute(&mut *tx).await.unwrap().rows_affected();
    reset(&mut tx).await;
    assert_eq!(inserted, 6);
    assert_eq!(group(&mut tx, &board, posts[0]).await, Some((1, false)));
    assert_eq!(group(&mut tx, &board, posts[1]).await, Some((1, true)));
    assert_eq!(
        group(&mut tx, &other, other_posts[0]).await,
        Some((2, false))
    );
    purge(&mut tx, &[a[0], a[1], b[0], c[0]]).await;
    assert_eq!(group(&mut tx, &board, posts[0]).await, None);
    assert_eq!(group(&mut tx, &board, posts[1]).await, Some((1, true)));
    assert_eq!(
        group(&mut tx, &other, other_posts[0]).await,
        Some((2, false))
    );
    tx.rollback().await.unwrap();
}

// This assertion shares the committed test's exact disposable-cluster guard,
// single immutable revision, and panic-safe cleanup. It tests only supported
// operator ordering, not unguarded bulk DML or retries after deadlocks.
async fn operator_board_first_serializes_opposite_thread_order(
    owner: &PgPool,
    board: &str,
    threads: [i64; 2],
    revision: i64,
) {
    let mut operator = owner.begin().await.unwrap();
    let operator_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *operator)
        .await
        .unwrap();
    // Pre-lock every affected board before the first report/membership DML.
    sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 ORDER BY slug FOR UPDATE")
        .bind(board)
        .execute(&mut *operator)
        .await
        .unwrap();
    let a = reports(&mut operator, board, threads[0], revision, &[Some(2)]).await;
    let b = reports(&mut operator, board, threads[1], revision, &[Some(2)]).await;
    assert_eq!(
        insert_members(&mut operator, board, threads[0], threads[0], &a).await,
        1
    );
    assert_eq!(
        group(&mut operator, board, threads[0]).await,
        Some((1, false))
    );
    let retained_history = history(&mut operator, board).await;

    let mut archiver = owner.begin().await.unwrap();
    let archiver_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *archiver)
        .await
        .unwrap();
    let archive_future = async {
        // This must wait at the board before taking either thread row, even
        // though the requested thread order is opposite to the operator's.
        sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
            .bind(board)
            .execute(&mut *archiver)
            .await
            .expect("board-first archiver must wait without 40P01");
        for thread in [threads[1], threads[0]] {
            sqlx::query("SELECT id FROM content.threads WHERE id=$1 FOR UPDATE")
                .bind(thread)
                .execute(&mut *archiver)
                .await
                .expect("board-first B-to-A traversal must not deadlock");
            archive(&mut archiver, thread).await.unwrap();
            assert_eq!(group(&mut archiver, board, thread).await, None);
            assert_eq!(members(&mut archiver, board, thread).await, 0);
        }
        archiver.rollback().await.unwrap();
    };
    let operator_future = async {
        wait_for_blocker(owner, archiver_pid, operator_pid).await;
        // Prove the blocked archiver has not taken B before its board lock.
        // Release this diagnostic lock before the operator inserts B.
        let mut probe = owner.begin().await.unwrap();
        sqlx::query("SELECT id FROM content.threads WHERE id=$1 FOR UPDATE NOWAIT")
            .bind(threads[1])
            .execute(&mut *probe)
            .await
            .expect("archiver waiting at board must leave thread B unlocked");
        probe.rollback().await.unwrap();
        let inserted = tokio::time::timeout(
            Duration::from_secs(5),
            insert_members(&mut operator, board, threads[1], threads[1], &b),
        )
        .await
        .expect("operator must insert B while archiver still waits at board");
        assert_eq!(inserted, 1);
        assert_eq!(
            group(&mut operator, board, threads[1]).await,
            Some((1, false))
        );
        operator.commit().await.unwrap();
    };
    tokio::time::timeout(Duration::from_secs(15), async {
        tokio::join!(archive_future, operator_future);
    })
    .await
    .expect("supported operator/archive ordering must finish without retry");

    // The archive's rollback restores both memberships/counters and both
    // archive timestamps, including B which was inserted during contention.
    let mut verify = owner.begin().await.unwrap();
    for thread in threads {
        assert_eq!(group(&mut verify, board, thread).await, Some((1, false)));
        assert_eq!(members(&mut verify, board, thread).await, 1);
    }
    let unarchived: i64 = sqlx::query_scalar("SELECT count(*) FROM content.threads WHERE board=$1 AND archived_at IS NULL AND archive_expires_at IS NULL")
        .bind(board).fetch_one(&mut *verify).await.unwrap();
    assert_eq!(unarchived, 2);
    assert_eq!(history(&mut verify, board).await, retained_history);
    verify.rollback().await.unwrap();

    let mut final_archive = owner.begin().await.unwrap();
    sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
        .bind(board)
        .execute(&mut *final_archive)
        .await
        .unwrap();
    for thread in [threads[1], threads[0]] {
        archive(&mut final_archive, thread).await.unwrap();
    }
    final_archive.commit().await.unwrap();
    let mut verify = owner.begin().await.unwrap();
    for thread in threads {
        assert_eq!(group(&mut verify, board, thread).await, None);
        assert_eq!(members(&mut verify, board, thread).await, 0);
    }
    assert_eq!(history(&mut verify, board).await, retained_history);
    verify.rollback().await.unwrap();
}
