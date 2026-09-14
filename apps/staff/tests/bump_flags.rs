#![cfg(feature = "database-tests")]

use board_staff::{AppError, auth::Session, store::moderate};
use sqlx::PgPool;

async fn exercise(owner: PgPool, staff: PgPool, slug: String, id: i64, reply: i64) {
    let mut session = Session {
        account_id: 42,
        role: "moderator".into(),
        csrf_hash: vec![],
        recent: true,
    };
    for action in ["permaage", "unpermaage"] {
        assert!(matches!(
            moderate(&staff, &session, &slug, id, action).await,
            Err(AppError::Forbidden)
        ));
    }
    let mut expected = Vec::new();
    for (action, role, permasage, permaage) in [
        ("permasage", "moderator", true, false),
        ("permaage", "admin", true, true),
        ("unpermasage", "moderator", false, true),
        ("unpermaage", "admin", false, false),
    ] {
        session.role = role.into();
        let before: (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>) =
            sqlx::query_as("SELECT bumped_at,modified_at FROM content.threads WHERE id=$1")
                .bind(id)
                .fetch_one(&owner)
                .await
                .unwrap();
        moderate(&staff, &session, &slug, id, action).await.unwrap();
        expected.push(action.to_string());
        let after: (
            bool,
            bool,
            chrono::DateTime<chrono::Utc>,
            chrono::DateTime<chrono::Utc>,
        ) = sqlx::query_as(
            "SELECT permasage,permaage,bumped_at,modified_at FROM content.threads WHERE id=$1",
        )
        .bind(id)
        .fetch_one(&owner)
        .await
        .unwrap();
        assert_eq!((after.0, after.1), (permasage, permaage));
        assert_eq!(after.2, before.0, "changing a flag does not itself bump");
        assert!(after.3 > before.1);
        let reports = board_staff::store::reports(&staff).await.unwrap();
        let report = reports.iter().find(|r| r.board == slug).unwrap();
        assert_eq!((report.permasage, report.permaage), (permasage, permaage));
        let audit: Vec<String> = sqlx::query_scalar(
            "SELECT action FROM content.moderation_audit WHERE board=$1 ORDER BY id",
        )
        .bind(&slug)
        .fetch_all(&owner)
        .await
        .unwrap();
        assert_eq!(audit, expected);
    }
    let snapshot: (bool, bool) =
        sqlx::query_as("SELECT permasage,permaage FROM content.threads WHERE id=$1")
            .bind(id)
            .fetch_one(&owner)
            .await
            .unwrap();
    for action in ["permasage", "unpermasage", "permaage", "unpermaage"] {
        session.role = "admin".into();
        session.recent = false;
        assert!(matches!(
            moderate(&staff, &session, &slug, id, action).await,
            Err(AppError::Recent)
        ));
        session.recent = true;
        session.role = "viewer".into();
        assert!(matches!(
            moderate(&staff, &session, &slug, id, action).await,
            Err(AppError::Forbidden)
        ));
        session.role = "admin".into();
        assert!(matches!(
            moderate(&staff, &session, &slug, reply, action).await,
            Err(AppError::NotFound)
        ));
        assert!(matches!(
            moderate(&staff, &session, "missing", id, action).await,
            Err(AppError::NotFound)
        ));
        assert!(matches!(
            moderate(&staff, &session, &slug, 0, action).await,
            Err(AppError::Invalid)
        ));
    }
    sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1").bind(id).execute(&owner).await.unwrap();
    for action in ["permasage", "unpermasage", "permaage", "unpermaage"] {
        assert!(matches!(
            moderate(&staff, &session, &slug, id, action).await,
            Err(AppError::Invalid)
        ));
    }
    sqlx::query("UPDATE content.threads SET deleted=true WHERE id=$1")
        .bind(id)
        .execute(&owner)
        .await
        .unwrap();
    for action in ["permasage", "unpermasage", "permaage", "unpermaage"] {
        assert!(matches!(
            moderate(&staff, &session, &slug, id, action).await,
            Err(AppError::NotFound)
        ));
    }
    let unchanged: (bool, bool) =
        sqlx::query_as("SELECT permasage,permaage FROM content.threads WHERE id=$1")
            .bind(id)
            .fetch_one(&owner)
            .await
            .unwrap();
    assert_eq!(unchanged, snapshot);
    let audit: Vec<(i64, i64, String)> = sqlx::query_as("SELECT account_id,target_id,action FROM content.moderation_audit WHERE board=$1 ORDER BY id").bind(&slug).fetch_all(&owner).await.unwrap();
    assert_eq!(
        audit,
        expected
            .into_iter()
            .map(|action| (42, id, action))
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn bump_flags_require_recent_scoped_authority_and_preserve_audit() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let staff = PgPool::connect(&std::env::var("STAFF_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let slug = format!("s{}", &uuid::Uuid::new_v4().simple().to_string()[..9]);
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,archive_retention_seconds) VALUES ($1,'Bump flags','Owned synthetic fixture',100,20,10,10,10,3600)").bind(&slug).execute(&owner).await.unwrap();
    let id: i64 = sqlx::query_scalar("INSERT INTO content.threads(board) VALUES ($1) RETURNING id")
        .bind(&slug)
        .fetch_one(&owner)
        .await
        .unwrap();
    sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES ($1,$2,$1,'Anonymous','','Owned root')").bind(id).bind(&slug).execute(&owner).await.unwrap();
    let reply: i64 = sqlx::query_scalar("INSERT INTO content.posts(board,thread_id,name,subject,comment) VALUES ($1,$2,'Anonymous','','Owned reply') RETURNING id").bind(&slug).bind(id).fetch_one(&owner).await.unwrap();
    sqlx::query(
        "INSERT INTO content.reports(board,post_id,reason) VALUES ($1,$2,'Owned flag report')",
    )
    .bind(&slug)
    .bind(reply)
    .execute(&owner)
    .await
    .unwrap();
    let result = tokio::spawn(exercise(
        owner.clone(),
        staff.clone(),
        slug.clone(),
        id,
        reply,
    ))
    .await;
    for query in [
        "DELETE FROM content.moderation_audit WHERE board=$1",
        "DELETE FROM content.reports WHERE board=$1",
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(query)
            .bind(&slug)
            .execute(&owner)
            .await
            .unwrap();
    }
    staff.close().await;
    owner.close().await;
    result.unwrap();
}
