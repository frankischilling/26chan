#![cfg(feature = "database-tests")]

use board_staff::{
    access::Permissions,
    auth::Session,
    report_queue::{self, ReportFilter},
};
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use tokio::sync::Notify;

// Metadata-only approved attachment, following file_digest.rs. This test proves
// the staff projection's snapshot, not decoder/publication or real file bytes.
struct Fixture {
    owner: PgPool,
    staff: PgPool,
    board: String,
    posts: [i64; 2],
    reports: [i64; 2],
    revision: i64,
    asset: String,
}

impl Fixture {
    async fn new() -> Self {
        assert!(std::env::var("BOARD_TEST_CLUSTER").is_ok_and(|path| {
            path.strip_prefix("/tmp/board-postgres.")
                .is_some_and(|tag| tag.len() == 8 && tag.bytes().all(|b| b.is_ascii_alphanumeric()))
        }));
        let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let staff = PgPool::connect(&std::env::var("STAFF_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let board = format!("gs{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
        sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Snapshot fixture','',1000,100,100,100,10)")
            .bind(&board).execute(&owner).await.unwrap();
        let mut posts = [0; 2];
        for post in &mut posts {
            *post =
                sqlx::query_scalar("INSERT INTO content.threads(board) VALUES($1) RETURNING id")
                    .bind(&board)
                    .fetch_one(&owner)
                    .await
                    .unwrap();
            sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous','','BeforePostSnapshot')")
                .bind(*post).bind(&board).execute(&owner).await.unwrap();
        }
        let categories: Vec<_> = (1..=3)
            .map(|id| {
                serde_json::json!({
                    "id":id,"board":"","op_only":false,"reply_only":false,
                    "image_only":false,"exclude_boards":null,"title":"Catalog title",
                    "weight":0.5,"filtered":0
                })
            })
            .collect();
        let revision = sqlx::query_scalar("SELECT content.import_report_catalog($1::text::jsonb)")
            .bind(serde_json::json!({"version":1,"categories":categories}).to_string())
            .fetch_one(&owner)
            .await
            .unwrap();
        let mut reports = [0; 2];
        for (report, state) in reports.iter_mut().zip(["open", "resolved"]) {
            *report = sqlx::query_scalar("INSERT INTO content.reports(board,post_id,state,reason,category_revision,category_id,category_kind,category_base_weight) VALUES($1,$2,$3,'BeforeCapturedCategory',$4,1,1,0.5) RETURNING id")
                .bind(&board).bind(posts[0]).bind(state).bind(revision)
                .fetch_one(&owner).await.unwrap();
        }
        let asset = uuid::Uuid::new_v4().simple().to_string();
        sqlx::query("INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at,md5,thumbnail_sha256,thumbnail_bytes,thumbnail_width,thumbnail_height,output_format) VALUES($1,$1,$1,repeat('a',64),100,500,300,'approved',clock_timestamp(),repeat('b',32),repeat('c',64),50,250,150,'png')")
            .bind(&asset).execute(&owner).await.unwrap();
        sqlx::query("INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler) VALUES($1,$2,$2,'before-snapshot.png',100,500,300,false)")
            .bind(posts[0]).bind(&asset).execute(&owner).await.unwrap();
        Self {
            owner,
            staff,
            board,
            posts,
            reports,
            revision,
            asset,
        }
    }

    async fn cleanup(&self) {
        let mut tx = self.owner.begin().await.unwrap();
        sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
            .bind(&self.board)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("SELECT id FROM content.threads WHERE board=$1 ORDER BY id FOR UPDATE")
            .bind(&self.board)
            .execute(&mut *tx)
            .await
            .unwrap();
        for sql in [
            "DELETE FROM content.reports WHERE board=$1",
            "DELETE FROM content.post_media WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
            "DELETE FROM content.posts WHERE board=$1",
            "DELETE FROM content.threads WHERE board=$1",
            "DELETE FROM content.boards WHERE slug=$1",
        ] {
            sqlx::query(sql)
                .bind(&self.board)
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        sqlx::query("DELETE FROM media.assets WHERE id=$1")
            .bind(&self.asset)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("SET LOCAL ROLE board_report_admission_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        for sql in [
            "DELETE FROM post_secrets.report_catalog_rows WHERE revision=$1",
            "DELETE FROM post_secrets.report_catalog_versions WHERE revision=$1",
        ] {
            sqlx::query(sql)
                .bind(self.revision)
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        tx.commit().await.unwrap();
        self.staff.close().await;
        self.owner.close().await;
    }
}

#[tokio::test]
async fn selected_groups_previews_categories_and_media_share_one_snapshot() {
    let f = Arc::new(Fixture::new().await);
    let exercise = f.clone();
    let result = tokio::spawn(async move {
        // A joined future owns both sides: a panic or timeout drops the paused
        // transaction instead of leaving a detached task blocking cleanup.
        tokio::time::timeout(Duration::from_secs(20), async move {
            let f = exercise;
            let selected = Notify::new();
            let resume = Notify::new();
            let session = Session {
                account_id: 0,
                role: "moderator".into(),
                csrf_hash: vec![],
                recent: true,
                permissions: Permissions {
                    allow_boards: vec![f.board.clone()],
                    ..Default::default()
                },
            };
            let read = report_queue::load_with_selection_checkpoint(
                &f.staff, &session, ReportFilter::All, &selected, &resume,
            );
            let write = async {
                tokio::time::timeout(Duration::from_secs(5), selected.notified())
                    .await.expect("group selection established the read snapshot");
                let mut tx = f.owner.begin().await.unwrap();
                // Only owned rows change. No relation lock, schema replacement,
                // global test hook or runtime timeout override is involved.
                sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
                    .bind(&f.board).execute(&mut *tx).await.unwrap();
                sqlx::query("UPDATE content.posts SET comment='AfterPostSnapshot' WHERE id=$1")
                    .bind(f.posts[0]).execute(&mut *tx).await.unwrap();
                sqlx::query("UPDATE content.reports SET reporter_cleared_at=clock_timestamp() WHERE id=$1")
                    .bind(f.reports[1]).execute(&mut *tx).await.unwrap();
                // Synthetic retained admissions, intentionally without private
                // membership/weight evidence: this is an inventory read test.
                let mut new_reports = Vec::new();
                for (post, state, id, title) in [
                    (f.posts[0], "open", 2_i64, "AfterCapturedCategoryTwo"),
                    (f.posts[0], "resolved", 3_i64, "AfterCapturedCategoryThree"),
                    (f.posts[1], "open", 2_i64, "NewGroupCapturedCategory"),
                ] {
                    let report: i64 = sqlx::query_scalar("INSERT INTO content.reports(board,post_id,state,reason,category_revision,category_id,category_kind,category_base_weight) VALUES($1,$2,$3,$4,$5,$6,1,0.5) RETURNING id")
                        .bind(&f.board).bind(post).bind(state).bind(title).bind(f.revision).bind(id)
                        .fetch_one(&mut *tx).await.unwrap();
                    new_reports.push(report);
                }
                // Legitimate approval retirement, matching file_digest.rs.
                // guard_asset_mutation preserves the immutable representation
                // metadata; the staff view stops joining this retired asset.
                sqlx::query("UPDATE media.assets SET state='deleting',approved_at=NULL WHERE id=$1")
                    .bind(&f.asset).execute(&mut *tx).await.unwrap();
                tx.commit().await.unwrap();
                resume.notify_one();
                new_reports
            };
            let (before, new_reports) = tokio::join!(read, write);
            let before = before.unwrap();
            assert_eq!(before.groups.len(), 1);
            assert!(!before.has_more);
            let group = &before.groups[0];
            assert_eq!(group.representative.id, f.reports[0]);
            assert_eq!(group.representative.comment, "BeforePostSnapshot");
            let summary = group.moderator_summary.as_ref().unwrap();
            assert_eq!(summary.report_count, 2);
            assert_eq!(summary.categories.len(), 1);
            assert_eq!(summary.categories[0].title, "BeforeCapturedCategory");
            let media = group.representative.attachment.as_ref().unwrap();
            assert_eq!(media.filename, "before-snapshot.png");
            assert!(!media.spoiler);
            assert!(media.available);
            assert_eq!(media.thumbnail_width, Some(250));
            assert_eq!(media.thumbnail_height, Some(150));
            assert_eq!(media.md5.as_deref(), Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"));

            let after = report_queue::load(&f.staff, &session, ReportFilter::All).await.unwrap();
            assert_eq!(after.groups.len(), 2);
            assert_eq!(after.groups[0].representative.id, new_reports[2]);
            let group = &after.groups[1];
            assert_eq!(group.representative.id, new_reports[0]);
            assert_eq!(group.representative.comment, "AfterPostSnapshot");
            let summary = group.moderator_summary.as_ref().unwrap();
            assert_eq!(summary.report_count, 3);
            assert_eq!(summary.categories.iter().map(|c| (c.id, c.title.as_str())).collect::<Vec<_>>(), vec![
                (1, "BeforeCapturedCategory"),
                (2, "AfterCapturedCategoryTwo"),
                (3, "AfterCapturedCategoryThree"),
            ]);
            assert!(summary.categories.iter().all(|c| c.revision == f.revision && c.kind == 1));
            assert!(!summary.categories_truncated);
            let media = group.representative.attachment.as_ref().unwrap();
            assert_eq!(media.filename, "before-snapshot.png");
            assert!(!media.spoiler);
            assert!(!media.available);
            assert_eq!(media.thumbnail_width, None);
            assert_eq!(media.thumbnail_height, None);
            assert_eq!(media.md5, None);
            let asset: (String, Option<i32>, Option<i32>, Option<String>) =
                sqlx::query_as("SELECT state,thumbnail_width,thumbnail_height,md5 FROM media.assets WHERE id=$1")
                    .bind(&f.asset).fetch_one(&f.owner).await.unwrap();
            assert_eq!(asset, ("deleting".into(), Some(250), Some(150), Some("b".repeat(32))));
        }).await.expect("bounded snapshot test completed");
    }).await;
    // Cleanup runs after assertion panics and after timeout drops both futures.
    f.cleanup().await;
    result.unwrap();
}
