#![cfg(feature = "database-tests")]

use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool};

const SNAPSHOT_COLUMNS: &[&str] = &[
    "snapshot_version",
    "snapshot_name",
    "snapshot_trip",
    "snapshot_capcode",
    "snapshot_subject",
    "snapshot_comment",
    "snapshot_comment_format",
    "snapshot_staff_authorized_limits",
    "snapshot_wordfiltered",
    "snapshot_image_spoiler",
    "snapshot_filename",
    "snapshot_dice_result",
    "snapshot_fortune_text",
    "snapshot_fortune_color",
    "snapshot_drawing_time_seconds",
    "snapshot_drawing_source_post_id",
];

async fn pool(variable: &str, expected: &str) -> PgPool {
    let pool = PgPool::connect(&std::env::var(variable).expect("owned database URL required"))
        .await
        .unwrap();
    let role: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(role, expected);
    pool
}

fn snapshot(board: &str) -> Value {
    json!({
        "account_id": 42, "board": board, "target_id": 1, "action": "spoiler",
        "snapshot_version": 1, "snapshot_name": "", "snapshot_subject": "",
        "snapshot_comment": "Saved final comment", "snapshot_comment_format": 0,
        "snapshot_staff_authorized_limits": false, "snapshot_wordfiltered": false,
        "snapshot_image_spoiler": false
    })
}

fn drawing_snapshot(board: &str) -> Value {
    let mut value = snapshot(board);
    value["snapshot_version"] = json!(2);
    value
}

async fn insert(c: &mut PgConnection, value: &Value) -> Result<i64, sqlx::Error> {
    // The typed record deliberately excludes generated/default columns.
    sqlx::query_scalar(
        "INSERT INTO content.moderation_audit(account_id,board,target_id,action,before_mask,after_mask,snapshot_version,snapshot_name,snapshot_trip,snapshot_capcode,snapshot_subject,snapshot_comment,snapshot_comment_format,snapshot_staff_authorized_limits,snapshot_wordfiltered,snapshot_image_spoiler,snapshot_filename,snapshot_dice_result,snapshot_fortune_text,snapshot_fortune_color,snapshot_drawing_time_seconds,snapshot_drawing_source_post_id)
         SELECT account_id,board,target_id,action,before_mask,after_mask,snapshot_version,snapshot_name,snapshot_trip,snapshot_capcode,snapshot_subject,snapshot_comment,snapshot_comment_format,snapshot_staff_authorized_limits,snapshot_wordfiltered,snapshot_image_spoiler,snapshot_filename,snapshot_dice_result,snapshot_fortune_text,snapshot_fortune_color,snapshot_drawing_time_seconds,snapshot_drawing_source_post_id
         FROM jsonb_populate_record(NULL::content.moderation_audit,$1) RETURNING id"
    ).bind(value).fetch_one(c).await
}

async fn invalid(c: &mut PgConnection, value: &Value, label: &str) {
    sqlx::query("SAVEPOINT invalid_snapshot")
        .execute(&mut *c)
        .await
        .unwrap();
    let error = insert(c, value).await.expect_err(label);
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514"),
        "{label}"
    );
    sqlx::query("ROLLBACK TO SAVEPOINT invalid_snapshot")
        .execute(&mut *c)
        .await
        .unwrap();
    sqlx::query("RELEASE SAVEPOINT invalid_snapshot")
        .execute(c)
        .await
        .unwrap();
}

#[tokio::test]
async fn snapshot_shape_saved_bounds_and_legacy_nulls() {
    let owner = pool("MIGRATION_DATABASE_URL", "board_migrator").await;
    let mut tx = owner.begin().await.unwrap();
    let seed: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    let board = format!("ss{seed:x}");
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Snapshot fixture','Synthetic rollback-only fixture',2000,100,100,100,10)")
        .bind(&board).execute(&mut *tx).await.unwrap();
    let valid = snapshot(&board);
    let id = insert(&mut tx, &valid).await.unwrap();
    let saved: Value =
        sqlx::query_scalar("SELECT to_jsonb(a) FROM content.moderation_audit a WHERE id=$1")
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    for (column, value) in valid.as_object().unwrap() {
        assert_eq!(&saved[column], value);
    }
    assert!(saved["snapshot_drawing_time_seconds"].is_null());
    assert!(saved["snapshot_drawing_source_post_id"].is_null());

    // New writes use v2, including posts without metadata. Historical v1 and
    // unversioned rows remain readable without manufacturing annotation data.
    let v2 = drawing_snapshot(&board);
    insert(&mut tx, &v2).await.unwrap();
    for seconds in [1, 59, 60, 3599, 3600, 5_184_000] {
        let mut time_only = v2.clone();
        time_only["snapshot_drawing_time_seconds"] = json!(seconds);
        insert(&mut tx, &time_only).await.unwrap();
    }
    for source in [1_i64, i64::MAX] {
        let mut with_source = v2.clone();
        with_source["snapshot_drawing_time_seconds"] = json!(90);
        with_source["snapshot_drawing_source_post_id"] = json!(source);
        let id = insert(&mut tx, &with_source).await.unwrap();
        let saved: Value =
            sqlx::query_scalar("SELECT to_jsonb(a) FROM content.moderation_audit a WHERE id=$1")
                .bind(id)
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        assert_eq!(saved["snapshot_drawing_time_seconds"], json!(90));
        assert_eq!(saved["snapshot_drawing_source_post_id"], json!(source));
        assert_eq!(saved["snapshot_comment"], "Saved final comment");
    }
    for column in [
        "snapshot_drawing_time_seconds",
        "snapshot_drawing_source_post_id",
    ] {
        let mut old = valid.clone();
        old[column] = json!(1);
        invalid(
            &mut tx,
            &old,
            "version one cannot gain new drawing evidence",
        )
        .await;
    }
    for seconds in [-1, 0, 5_184_001] {
        let mut invalid_time = v2.clone();
        invalid_time["snapshot_drawing_time_seconds"] = json!(seconds);
        invalid(
            &mut tx,
            &invalid_time,
            "drawing time must be in source bounds",
        )
        .await;
    }
    for source in [-1_i64, 0] {
        let mut invalid_source = v2.clone();
        invalid_source["snapshot_drawing_time_seconds"] = json!(60);
        invalid_source["snapshot_drawing_source_post_id"] = json!(source);
        invalid(&mut tx, &invalid_source, "source post ID must be positive").await;
    }
    let mut source_without_time = v2.clone();
    source_without_time["snapshot_drawing_source_post_id"] = json!(42);
    invalid(
        &mut tx,
        &source_without_time,
        "source annotation requires a time",
    )
    .await;
    let mut partial_v2 = v2.clone();
    partial_v2["snapshot_comment"] = Value::Null;
    invalid(
        &mut tx,
        &partial_v2,
        "version two requires the original saved comment",
    )
    .await;
    let mut ineligible_v2 = v2.clone();
    ineligible_v2["action"] = json!("close");
    invalid(
        &mut tx,
        &ineligible_v2,
        "version two cannot widen snapshot actions",
    )
    .await;

    // The old binary's exact column list remains valid for both historical
    // isolated actions and the supported spoiler actions. There is no backfill.
    for action in ["close", "sticky", "staff-post", "spoiler", "unspoiler"] {
        let old: Value = sqlx::query_scalar("INSERT INTO content.moderation_audit(account_id,board,target_id,action) VALUES(42,$1,1,$2) RETURNING to_jsonb(moderation_audit)")
            .bind(&board).bind(action).fetch_one(&mut *tx).await.unwrap();
        for column in SNAPSHOT_COLUMNS {
            assert!(old[*column].is_null(), "legacy {action}: {column}");
        }
    }
    let mut grouped = valid.clone();
    grouped["action"] = json!("thread-options");
    grouped["before_mask"] = json!(0);
    grouped["after_mask"] = json!(31);
    insert(&mut tx, &grouped).await.unwrap();
    let mut legacy_grouped = grouped.clone();
    for column in SNAPSHOT_COLUMNS {
        legacy_grouped.as_object_mut().unwrap().remove(*column);
    }
    insert(&mut tx, &legacy_grouped).await.unwrap();
    for bad_mask in [Value::Null, json!(0), json!(32)] {
        let mut bad = grouped.clone();
        bad["after_mask"] = bad_mask;
        invalid(&mut tx, &bad, "0103 grouped mask constraint is preserved").await;
    }
    let mut isolated_mask = valid.clone();
    isolated_mask["before_mask"] = json!(0);
    isolated_mask["after_mask"] = json!(1);
    invalid(
        &mut tx,
        &isolated_mask,
        "isolated action cannot carry grouped masks",
    )
    .await;

    for action in ["spoiler", "unspoiler"] {
        let mut value = valid.clone();
        value["action"] = json!(action);
        insert(&mut tx, &value).await.unwrap();
    }
    for format in 0_i16..=127 {
        let supported = format == 0
            || (8..=15).contains(&format)
            || (24..=31).contains(&format)
            || (40..=47).contains(&format)
            || (56..=63).contains(&format)
            || (104..=111).contains(&format)
            || (120..=127).contains(&format);
        let mut value = valid.clone();
        value["snapshot_comment_format"] = json!(format);
        if supported {
            insert(&mut tx, &value).await.unwrap();
        } else {
            invalid(&mut tx, &value, "unsupported historical formatter stamp").await;
        }
    }
    for capcode in [
        "mod",
        "admin",
        "admin_highlight",
        "manager",
        "developer",
        "founder",
    ] {
        let mut value = valid.clone();
        value["snapshot_capcode"] = json!(capcode);
        insert(&mut tx, &value).await.unwrap();
    }
    for trip in ["!./01234567", "!!+/012345678"] {
        let mut value = valid.clone();
        value["snapshot_trip"] = json!(trip);
        insert(&mut tx, &value).await.unwrap();
    }

    // Saved maxima are byte limits, independent of today's posting branch:
    // false staff/wordfilter bits do not narrow historical evidence bounds.
    let mut maximum = valid.clone();
    for (column, bytes) in [
        ("snapshot_name", 255),
        ("snapshot_subject", 1020),
        ("snapshot_comment", 2_097_152),
        ("snapshot_filename", 255),
        ("snapshot_dice_result", 1024),
    ] {
        maximum[column] = json!("x".repeat(bytes));
    }
    maximum["snapshot_trip"] = json!("!!abcdefghijk");
    maximum["snapshot_capcode"] = json!("admin_highlight");
    maximum["snapshot_comment_format"] = json!(127);
    insert(&mut tx, &maximum).await.unwrap();
    maximum["snapshot_dice_result"] = Value::Null;
    maximum["snapshot_fortune_text"] = json!("x".repeat(256));
    maximum["snapshot_fortune_color"] = json!("#abcdef");
    insert(&mut tx, &maximum).await.unwrap();

    for (column, bytes) in [
        ("snapshot_name", 255),
        ("snapshot_subject", 1020),
        ("snapshot_comment", 2_097_152),
        ("snapshot_filename", 255),
        ("snapshot_dice_result", 1024),
        ("snapshot_fortune_text", 256),
    ] {
        let mut bad = valid.clone();
        // Character length is within the bound; UTF-8 byte length is not.
        bad[column] = json!(format!("{}é", "x".repeat(bytes - 1)));
        if column == "snapshot_fortune_text" {
            bad["snapshot_fortune_color"] = json!("#123abc");
        }
        invalid(&mut tx, &bad, column).await;
    }
    for column in [
        "snapshot_name",
        "snapshot_subject",
        "snapshot_comment",
        "snapshot_comment_format",
        "snapshot_staff_authorized_limits",
        "snapshot_wordfiltered",
        "snapshot_image_spoiler",
    ] {
        let mut bad = valid.clone();
        bad[column] = Value::Null;
        invalid(&mut tx, &bad, "partial version-one snapshot").await;
    }
    for (column, value) in [
        ("snapshot_version", json!(0)),
        ("snapshot_version", json!(3)),
        ("snapshot_version", Value::Null),
        ("action", json!("close")),
        ("snapshot_trip", json!("!short")),
        ("snapshot_trip", json!("!!abcdefghijkl")),
        ("snapshot_capcode", json!("moderator")),
        ("snapshot_comment_format", json!(-1)),
        ("snapshot_comment_format", json!(128)),
        ("snapshot_dice_result", json!("")),
        ("snapshot_dice_result", json!("1\n2")),
        ("snapshot_fortune_text", json!("lucky")),
        ("snapshot_fortune_color", json!("#abcdef")),
    ] {
        let mut bad = valid.clone();
        bad[column] = value;
        invalid(&mut tx, &bad, column).await;
    }
    // Every field is forbidden without a version, including optional fields.
    for column in SNAPSHOT_COLUMNS
        .iter()
        .filter(|c| **c != "snapshot_version")
    {
        let mut bad =
            json!({"account_id": 42, "board": board, "target_id": 1, "action": "spoiler"});
        bad[*column] = match *column {
            "snapshot_comment_format" => json!(0),
            "snapshot_staff_authorized_limits"
            | "snapshot_wordfiltered"
            | "snapshot_image_spoiler" => json!(false),
            "snapshot_trip" => json!("!!abcdefghijk"),
            "snapshot_capcode" => json!("mod"),
            "snapshot_fortune_color" => json!("#abcdef"),
            "snapshot_drawing_time_seconds" | "snapshot_drawing_source_post_id" => json!(1),
            _ => json!("x"),
        };
        invalid(&mut tx, &bad, "unversioned partial snapshot").await;
    }
    for (text, color, dice) in [
        ("", "#abcdef", None),
        ("bad\tfortune", "#abcdef", None),
        ("lucky", "#ABCDEF", None),
        ("lucky", "#12345", None),
        ("lucky", "#abcdef", Some("rolled 1")),
    ] {
        let mut bad = valid.clone();
        bad["snapshot_fortune_text"] = json!(text);
        bad["snapshot_fortune_color"] = json!(color);
        bad["snapshot_dice_result"] = json!(dice);
        invalid(&mut tx, &bad, "randomizer bounds, pair or exclusivity").await;
    }
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn snapshot_runtime_role_boundaries_remain_append_only_and_staff_private() {
    let owner = pool("MIGRATION_DATABASE_URL", "board_migrator").await;
    let staff = pool("STAFF_DATABASE_URL", "board_staff").await;
    let public = pool("TEST_PUBLIC_DATABASE_URL", "board_public").await;
    for role in [
        "board_staff",
        "board_public",
        "board_auth",
        "board_media",
        "board_media_read",
        "board_media_intake",
        "board_monitor",
        "board_staff_post_owner",
    ] {
        for column in SNAPSHOT_COLUMNS {
            for privilege in ["SELECT", "INSERT", "UPDATE"] {
                let allowed: bool = sqlx::query_scalar(
                    "SELECT has_column_privilege($1,'content.moderation_audit',$2,$3)",
                )
                .bind(role)
                .bind(column)
                .bind(privilege)
                .fetch_one(&owner)
                .await
                .unwrap();
                assert_eq!(
                    allowed,
                    role == "board_staff" && privilege != "UPDATE",
                    "{role} {privilege} {column}"
                );
            }
        }
        let allowed: bool = sqlx::query_scalar(
            "SELECT has_table_privilege($1,'content.moderation_audit','DELETE')",
        )
        .bind(role)
        .fetch_one(&owner)
        .await
        .unwrap();
        assert!(!allowed, "{role} cannot delete audit evidence");
    }
    // Real runtime connections prove actual checks, not just ACL inspection.
    sqlx::query("SELECT snapshot_comment,snapshot_drawing_time_seconds,snapshot_drawing_source_post_id FROM content.moderation_audit WHERE false")
        .execute(&staff)
        .await
        .unwrap();
    macro_rules! denied {
        ($pool:expr, $query:literal) => {{
            let error = sqlx::query($query).execute($pool).await.unwrap_err();
            assert_eq!(
                error.as_database_error().unwrap().code().as_deref(),
                Some("42501")
            );
        }};
    }
    denied!(
        &public,
        "SELECT snapshot_comment FROM content.moderation_audit WHERE false"
    );
    denied!(
        &public,
        "SELECT snapshot_drawing_time_seconds,snapshot_drawing_source_post_id FROM content.moderation_audit WHERE false"
    );
    denied!(
        &public,
        "INSERT INTO content.moderation_audit(snapshot_comment) SELECT 'x' WHERE false"
    );
    denied!(
        &public,
        "INSERT INTO content.moderation_audit(snapshot_drawing_time_seconds,snapshot_drawing_source_post_id) SELECT 90,42 WHERE false"
    );
    denied!(
        &public,
        "UPDATE content.moderation_audit SET snapshot_comment='changed' WHERE false"
    );
    denied!(
        &public,
        "UPDATE content.moderation_audit SET snapshot_drawing_time_seconds=90 WHERE false"
    );
    denied!(&public, "DELETE FROM content.moderation_audit WHERE false");
    denied!(
        &staff,
        "UPDATE content.moderation_audit SET snapshot_comment='changed' WHERE false"
    );
    denied!(
        &staff,
        "UPDATE content.moderation_audit SET snapshot_drawing_source_post_id=42 WHERE false"
    );
    denied!(&staff, "DELETE FROM content.moderation_audit WHERE false");
    // Real staff INSERT and SELECT retain exact typed saved values; the audit
    // rows are rolled back before the owned board is removed. A spawned test
    // body lets cleanup also run after an assertion failure.
    let seed: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&owner)
        .await
        .unwrap();
    let board = format!("sr{seed:x}");
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Snapshot role fixture','Synthetic',2000,100,100,100,10)")
        .bind(&board).execute(&owner).await.unwrap();
    let test_board = board.clone();
    let result = tokio::spawn(async move {
        let mut tx = staff.begin().await.unwrap();
        let mut value = drawing_snapshot(&test_board);
        value["snapshot_name"] = json!("Saved name");
        value["snapshot_trip"] = json!("!!abcdefghijk");
        value["snapshot_capcode"] = json!("founder");
        value["snapshot_subject"] = json!("Saved subject");
        value["snapshot_staff_authorized_limits"] = json!(true);
        value["snapshot_wordfiltered"] = json!(true);
        value["snapshot_image_spoiler"] = json!(true);
        value["snapshot_filename"] = json!("saved.png");
        value["snapshot_fortune_text"] = json!("Saved fortune");
        value["snapshot_fortune_color"] = json!("#123abc");
        value["snapshot_drawing_time_seconds"] = json!(5_184_000);
        value["snapshot_drawing_source_post_id"] = json!(i64::MAX);
        let id = insert(&mut tx, &value).await.unwrap();
        let saved: Value =
            sqlx::query_scalar("SELECT to_jsonb(a) FROM content.moderation_audit a WHERE id=$1")
                .bind(id)
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        for (column, expected) in value.as_object().unwrap() {
            assert_eq!(&saved[column], expected);
        }
        tx.rollback().await.unwrap();
    })
    .await;
    sqlx::query("DELETE FROM content.boards WHERE slug=$1")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    result.unwrap();
}
