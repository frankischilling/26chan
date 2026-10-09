#![cfg(feature = "database-tests")]
mod support;

use board_store::{BoardSelection, BoardSnapshot, NewPost};
use sqlx::PgPool;
use std::collections::BTreeMap;

fn post(comment: String) -> NewPost {
    NewPost {
        name: "Anonymous".into(),
        subject: String::new(),
        comment,
        deletion_hash: "owned-preview-fixture-hash".into(),
        sage: false,
    }
}

fn assert_selected(
    snapshot: &BoardSnapshot,
    saved: &BTreeMap<i64, Vec<i64>>,
    configured: i32,
    sticky: Option<bool>,
) {
    assert_eq!(snapshot.board.replies_shown, configured);
    assert_eq!(snapshot.threads.len(), saved.len());
    for preview in &snapshot.threads {
        if let Some(sticky) = sticky {
            assert_eq!(preview.thread.sticky, sticky);
        }
        let all = &saved[&preview.thread.id];
        let limit = if preview.thread.sticky {
            configured.min(1)
        } else {
            configured
        } as usize;
        let expected = std::iter::once(preview.thread.id)
            .chain(all.iter().skip(all.len().saturating_sub(limit)).copied())
            .collect::<Vec<_>>();
        assert_eq!(
            preview.posts.iter().map(|post| post.id).collect::<Vec<_>>(),
            expected
        );
        assert_eq!(preview.visible_posts, all.len() as i64 + 1);
        assert_eq!(preview.latest_reply_id, all.last().copied());
        assert_eq!(preview.visible_images, 0);
    }
    assert_eq!(snapshot.quote_targets.has_dependencies(), configured > 0);
}

#[tokio::test]
async fn persisted_source_policy_selects_each_thread_inside_the_board_snapshot() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let seed: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&owner)
        .await
        .unwrap();
    let board = format!("pv{seed:x}");
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,posting_reply_seconds,posting_image_seconds,posting_thread_seconds,user_thread_limit) VALUES($1,'Owned preview policy','Synthetic source policy fixture',16000,1000,1000,1000,10,0,0,0,100)")
        .bind(&board).execute(&owner).await.unwrap();
    let test_board = board.clone();
    let test_owner = owner.clone();
    let result = tokio::spawn(async move {
        assert_eq!(
            board_store::board(&public, &test_board)
                .await
                .unwrap()
                .replies_shown,
            5
        );
        for invalid in [-1, 6] {
            let error = sqlx::query("UPDATE content.boards SET replies_shown=$2 WHERE slug=$1")
                .bind(&test_board)
                .bind(invalid)
                .execute(&test_owner)
                .await
                .unwrap_err();
            assert_eq!(
                error.as_database_error().unwrap().code().as_deref(),
                Some("23514")
            );
        }
        let error = sqlx::query("UPDATE content.boards SET replies_shown=0 WHERE slug=$1")
            .bind(&test_board)
            .execute(&public)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501")
        );
        let mut saved = BTreeMap::new();
        for count in [0, 3, 8] {
            let op = support::create_post(
                &public,
                &test_board,
                0,
                &post(format!("Owned OP with {count} replies")),
            )
            .await
            .unwrap();
            let mut ids = Vec::new();
            for i in 0..count {
                ids.push(
                    support::create_post(
                        &public,
                        &test_board,
                        op,
                        &post(format!("Owned reply {i} >>{op}")),
                    )
                    .await
                    .unwrap(),
                );
            }
            if count == 8 {
                let deleted = ids.remove(7);
                sqlx::query("UPDATE content.posts SET deleted=true WHERE board=$1 AND id=$2")
                    .bind(&test_board)
                    .bind(deleted)
                    .execute(&test_owner)
                    .await
                    .unwrap();
            }
            saved.insert(op, ids);
        }
        for configured in [0, 1, 3, 5] {
            sqlx::query("UPDATE content.boards SET replies_shown=$2 WHERE slug=$1")
                .bind(&test_board)
                .bind(configured)
                .execute(&test_owner)
                .await
                .unwrap();
            for sticky in [false, true] {
                sqlx::query("UPDATE content.threads SET sticky=$2 WHERE board=$1")
                    .bind(&test_board)
                    .bind(sticky)
                    .execute(&test_owner)
                    .await
                    .unwrap();
                let ordinary = board_store::source_board_snapshot(
                    &public,
                    &test_board,
                    BoardSelection::Page(1),
                )
                .await
                .unwrap();
                assert_selected(&ordinary, &saved, configured, Some(sticky));
                let json = board_store::source_json_board_snapshot(
                    &public,
                    &test_board,
                    BoardSelection::All,
                )
                .await
                .unwrap();
                assert_selected(&json, &saved, configured, Some(sticky));
                let html = board_store::source_board_page_snapshot(
                    &public,
                    &test_board,
                    BoardSelection::Page(1),
                )
                .await
                .unwrap();
                assert_selected(&html.snapshot, &saved, configured, Some(sticky));
            }
        }
        // Different selected threads carry different limits in the paired SQL
        // arrays; a sticky OP must not accidentally apply its limit to a peer.
        let sticky_id = *saved.keys().next_back().unwrap();
        sqlx::query("UPDATE content.threads SET sticky=(id=$2) WHERE board=$1")
            .bind(&test_board)
            .bind(sticky_id)
            .execute(&test_owner)
            .await
            .unwrap();
        let mixed = board_store::source_board_snapshot(&public, &test_board, BoardSelection::All)
            .await
            .unwrap();
        assert_selected(&mixed, &saved, 5, None);
        // Explicit internal bounds retain their old semantics, including staff
        // consumers and OP-only catalog HTML; metadata does not load comments.
        let explicit =
            board_store::board_snapshot(&public, &test_board, BoardSelection::All, Some(5))
                .await
                .unwrap();
        for preview in &explicit.threads {
            assert_eq!(
                preview.posts.len(),
                1 + saved[&preview.thread.id].len().min(5)
            );
        }
        let metadata = board_store::board_snapshot(&public, &test_board, BoardSelection::All, None)
            .await
            .unwrap();
        assert!(
            metadata
                .threads
                .iter()
                .all(|preview| preview.posts.is_empty())
        );
        assert!(!metadata.quote_targets.has_dependencies());
        let op_only =
            board_store::board_page_snapshot(&public, &test_board, BoardSelection::All, Some(0))
                .await
                .unwrap();
        assert!(
            op_only
                .snapshot
                .threads
                .iter()
                .all(|preview| preview.posts.len() == 1)
        );
        assert!(!op_only.snapshot.quote_targets.has_dependencies());
        // A deleted OP with live replies is not a body-bearing public preview,
        // independent of whether the configured reply limit is zero or positive.
        sqlx::query("UPDATE content.posts SET deleted=true WHERE board=$1 AND id=$2")
            .bind(&test_board)
            .bind(sticky_id)
            .execute(&test_owner)
            .await
            .unwrap();
        saved.remove(&sticky_id);
        for configured in [0, 5] {
            sqlx::query("UPDATE content.boards SET replies_shown=$2 WHERE slug=$1")
                .bind(&test_board)
                .bind(configured)
                .execute(&test_owner)
                .await
                .unwrap();
            let visible =
                board_store::source_json_board_snapshot(&public, &test_board, BoardSelection::All)
                    .await
                    .unwrap();
            assert_selected(&visible, &saved, configured, None);
            let explicit = board_store::board_page_snapshot(
                &public,
                &test_board,
                BoardSelection::All,
                Some(i64::from(configured)),
            )
            .await
            .unwrap();
            assert!(
                explicit
                    .snapshot
                    .threads
                    .iter()
                    .all(|entry| entry.thread.id != sticky_id)
            );
        }
        let metadata = board_store::board_snapshot(&public, &test_board, BoardSelection::All, None)
            .await
            .unwrap();
        assert!(
            metadata
                .threads
                .iter()
                .any(|entry| entry.thread.id == sticky_id)
        );
    })
    .await;
    let mut cleanup = support::begin_cleanup(&owner, std::slice::from_ref(&board)).await;
    support::cleanup_posting(&mut *cleanup, &board).await;
    sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)")
        .bind(&board).execute(&mut *cleanup).await.unwrap();
    sqlx::query("DELETE FROM content.posts WHERE board=$1")
        .bind(&board)
        .execute(&mut *cleanup)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.threads WHERE board=$1")
        .bind(&board)
        .execute(&mut *cleanup)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.boards WHERE slug=$1")
        .bind(&board)
        .execute(&mut *cleanup)
        .await
        .unwrap();
    cleanup.commit().await.unwrap();
    result.unwrap();
}
