#![cfg(feature = "database-tests")]

use board_store::NewPost;
use sqlx::PgPool;

fn post(subject: &str, comment: &str) -> NewPost {
    NewPost {
        name: "Anonymous".into(),
        subject: subject.into(),
        comment: comment.into(),
        deletion_hash: "search-fixture-not-a-password".into(),
        sage: false,
    }
}

#[tokio::test]
async fn search_groups_public_matches_and_never_crosses_board_visibility() {
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
    let public_slug = format!("s{:x}", seed).chars().take(10).collect::<String>();
    let private_slug = format!("p{:x}", seed).chars().take(10).collect::<String>();
    let marker = format!("owned-search-{seed:x}");

    for (slug, staff_only) in [(&public_slug, false), (&private_slug, true)] {
        sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,staff_only) VALUES($1,'Search fixture','Owned search data',16000,100,100,100,10,$2)")
            .bind(slug)
            .bind(staff_only)
            .execute(&owner)
            .await
            .unwrap();
    }

    let test_public = public.clone();
    let test_owner = owner.clone();
    let test_slug = public_slug.clone();
    let test_private = private_slug.clone();
    let test_marker = marker.clone();
    let outcome = tokio::spawn(async move {
        let first = board_store::create_post(
            &test_public,
            &test_slug,
            0,
            &post("Owned first", &format!("prefix {test_marker} one")),
        )
        .await
        .unwrap();
        board_store::create_post(
            &test_public,
            &test_slug,
            first,
            &post("", &format!("reply {test_marker} two")),
        )
        .await
        .unwrap();
        board_store::create_post(
            &test_public,
            &test_slug,
            first,
            &post("", &format!("reply {test_marker} three")),
        )
        .await
        .unwrap();
        let second = board_store::create_post(
            &test_public,
            &test_slug,
            0,
            &post(&format!("subject {test_marker}"), "second thread"),
        )
        .await
        .unwrap();

        let private_thread: i64 = sqlx::query_scalar(
            "INSERT INTO content.threads(board) VALUES($1) RETURNING id",
        )
        .bind(&test_private)
        .fetch_one(&test_owner)
        .await
        .unwrap();
        sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous','Private match',$3)")
            .bind(private_thread)
            .bind(&test_private)
            .bind(format!("hidden {test_marker}"))
            .execute(&test_owner)
            .await
            .unwrap();

        let result = board_store::search(&test_public, &test_marker, None, 0)
            .await
            .unwrap();
        assert_eq!(result.nhits, 2, "multiple matching posts group by visible thread");
        assert_eq!(result.threads.len(), 2);
        assert!(result.threads.iter().all(|thread| thread.board.slug == test_slug));
        let grouped = result
            .threads
            .iter()
            .find(|thread| thread.thread.id == first)
            .unwrap();
        assert_eq!(grouped.posts.len(), 3, "OP plus matching replies are retained");
        assert!(grouped.posts.iter().all(|post| post.comment.chars().count() <= board_store::SEARCH_COMMENT_CHARS as usize));
        assert!(result.threads.iter().any(|thread| thread.thread.id == second));

        let scoped = board_store::search(&test_public, &test_marker, Some(&test_slug), 0)
            .await
            .unwrap();
        assert_eq!(scoped.nhits, 2);
        assert!(board_store::search(&test_public, &test_marker, Some(&test_private), 0)
            .await
            .unwrap()
            .threads
            .is_empty());
        let visible_private: i64 = sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=$1")
            .bind(&test_private)
            .fetch_one(&test_public)
            .await
            .unwrap();
        assert_eq!(visible_private, 0);
    })
    .await;

    for slug in [&public_slug, &private_slug] {
        sqlx::query("DELETE FROM content.reports WHERE board=$1")
            .bind(slug)
            .execute(&owner)
            .await
            .unwrap();
        sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)")
            .bind(slug)
            .execute(&owner)
            .await
            .unwrap();
        sqlx::query("DELETE FROM content.posts WHERE board=$1")
            .bind(slug)
            .execute(&owner)
            .await
            .unwrap();
        sqlx::query("DELETE FROM content.threads WHERE board=$1")
            .bind(slug)
            .execute(&owner)
            .await
            .unwrap();
        sqlx::query("DELETE FROM content.boards WHERE slug=$1")
            .bind(slug)
            .execute(&owner)
            .await
            .unwrap();
    }
    public.close().await;
    owner.close().await;
    outcome.unwrap();
}
