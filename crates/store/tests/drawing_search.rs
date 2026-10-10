#![cfg(feature = "database-tests")]
mod support;

use board_domain::drawing_annotation::DrawingTime;
use sqlx::PgPool;

const DRAWING_SEARCH: &str = "content.drawing_search_text(integer,bigint)";

async fn owner_and_public() -> (PgPool, PgPool) {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    (owner, public)
}

/// Tests the read projection alone. It never requires an image, source lookup,
/// upload receipt, or replay because the function receives only typed numbers.
async fn projected(
    public: &PgPool,
    seconds: Option<i32>,
    source_post: Option<i64>,
) -> Option<String> {
    sqlx::query_scalar("SELECT content.drawing_search_text($1::integer,$2::bigint)")
        .bind(seconds)
        .bind(source_post)
        .fetch_one(public)
        .await
        .unwrap()
}

#[tokio::test]
async fn numeric_projection_matches_domain_rounding_and_public_only_execution() {
    let (owner, public) = owner_and_public().await;
    let user: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&public)
        .await
        .unwrap();
    assert_eq!(user, "board_public");

    let function_contract: (bool, bool, bool, bool) = sqlx::query_as(
        r#"SELECT p.provolatile='i',p.proparallel='s',NOT p.prosecdef,
                  p.proowner='board_migrator'::regrole
           FROM pg_proc p WHERE p.oid=$1::regprocedure"#,
    )
    .bind(DRAWING_SEARCH)
    .fetch_one(&owner)
    .await
    .unwrap();
    assert_eq!(function_contract, (true, true, true, true));

    let permitted: (bool, bool, bool, bool, bool) = sqlx::query_as(
        r#"SELECT has_function_privilege('board_public',$1,'EXECUTE'),
                  has_function_privilege('board_staff',$1,'EXECUTE'),
                  has_function_privilege('board_auth',$1,'EXECUTE'),
                  has_function_privilege('board_media',$1,'EXECUTE'),
                  has_function_privilege('board_migrator',$1,'EXECUTE')"#,
    )
    .bind(DRAWING_SEARCH)
    .fetch_one(&owner)
    .await
    .unwrap();
    assert_eq!(permitted, (true, false, false, false, true));

    // Both branches deliberately round minutes independently. In particular,
    // 3599 seconds is "60m", while 7199 seconds is "1h 60m".
    for seconds in [1, 29, 59, 60, 89, 90, 3599, 3600, 3659, 7199, 5_184_000] {
        let time = DrawingTime::from_seconds(seconds.into()).unwrap();
        let without_source = format!("Oekaki Post (Time: {time})");
        let with_source = format!("Oekaki Post (Time: {time}, Source: >>42)");
        assert_eq!(
            projected(&public, Some(seconds), None).await,
            Some(without_source),
            "{seconds}s time-only"
        );
        assert_eq!(
            projected(&public, Some(seconds), Some(42)).await,
            Some(with_source),
            "{seconds}s with source"
        );
    }
    assert_eq!(DrawingTime::from_seconds(3599).unwrap().to_string(), "60m");
    assert_eq!(
        DrawingTime::from_seconds(7199).unwrap().to_string(),
        "1h 60m"
    );

    for seconds in [None, Some(0), Some(-1), Some(5_184_001), Some(i32::MAX)] {
        assert_eq!(
            projected(&public, seconds, Some(42)).await,
            None,
            "invalid time must never expose source text"
        );
    }
    for source in [Some(0), Some(-1)] {
        assert_eq!(
            projected(&public, Some(60), source).await,
            Some("Oekaki Post (Time: 1m)".into())
        );
    }
    assert_eq!(
        projected(&public, Some(60), Some(i64::MAX)).await,
        Some(format!("Oekaki Post (Time: 1m, Source: >>{})", i64::MAX))
    );

    public.close().await;
    owner.close().await;
}

async fn seed_thread(owner: &PgPool, slug: &str, deleted: bool) -> i64 {
    sqlx::query_scalar("INSERT INTO content.threads(board,deleted) VALUES($1,$2) RETURNING id")
        .bind(slug)
        .bind(deleted)
        .fetch_one(owner)
        .await
        .unwrap()
}

async fn seed_post(
    owner: &PgPool,
    slug: &str,
    thread_id: i64,
    op: bool,
    annotation: (i32, Option<i64>),
    deleted: bool,
    comment: &str,
) -> i64 {
    let (seconds, source) = annotation;
    let post_id = if op {
        thread_id
    } else {
        sqlx::query_scalar("SELECT nextval('content.post_number')")
            .fetch_one(owner)
            .await
            .unwrap()
    };
    // The migrator seeds typed columns directly to exercise the read path.
    // This deliberately does not claim media/attachment or posting authority.
    let inserted: i64 = sqlx::query_scalar(
        r#"INSERT INTO content.posts(id,board,thread_id,name,subject,comment,
                  drawing_time_seconds,drawing_source_post_id,deleted)
           VALUES($1,$2,$3,'Anonymous','',$4,$5,$6,$7) RETURNING id"#,
    )
    .bind(post_id)
    .bind(slug)
    .bind(thread_id)
    .bind(comment)
    .bind(seconds)
    .bind(source)
    .bind(deleted)
    .fetch_one(owner)
    .await
    .unwrap();
    assert_eq!(inserted, post_id);
    post_id
}

async fn cleanup(owner: &PgPool, slugs: &[String]) {
    let mut tx = support::begin_cleanup(owner, slugs).await;
    for query in [
        "DELETE FROM content.reports WHERE board=ANY($1)",
        "DELETE FROM content.post_media WHERE post_id IN (SELECT id FROM content.posts WHERE board=ANY($1))",
        "DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=ANY($1))",
        "DELETE FROM content.posts WHERE board=ANY($1)",
        "DELETE FROM content.threads WHERE board=ANY($1)",
        "DELETE FROM content.boards WHERE slug=ANY($1)",
    ] {
        sqlx::query(query)
            .bind(slugs)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn annotation_only_op_and_reply_hits_obey_deleted_and_private_visibility() {
    let (owner, public) = owner_and_public().await;
    let nonce: String =
        sqlx::query_scalar("SELECT substr(replace(gen_random_uuid()::text,'-',''),1,9)")
            .fetch_one(&owner)
            .await
            .unwrap();
    let public_slug = format!("d{nonce}");
    let private_slug = format!("p{nonce}");
    let slugs = vec![public_slug.clone(), private_slug.clone()];
    for (slug, staff_only) in [(&public_slug, false), (&private_slug, true)] {
        sqlx::query(
            r#"INSERT INTO content.boards(slug,title,description,max_comment_chars,
                   reply_limit,bump_limit,thread_limit,threads_per_page,staff_only)
               VALUES($1,'Drawing search test','Only owned typed fixtures',16000,100,100,100,10,$2)"#,
        )
        .bind(slug)
        .bind(staff_only)
        .execute(&owner)
        .await
        .unwrap();
    }

    let test_owner = owner.clone();
    let test_public = public.clone();
    let public_board = public_slug.clone();
    let private_board = private_slug.clone();
    let outcome = tokio::spawn(async move {
        let thread = seed_thread(&test_owner, &public_board, false).await;
        let op = seed_post(
            &test_owner,
            &public_board,
            thread,
            true,
            (59, None),
            false,
            "Unrelated OP text",
        )
        .await;
        let reply = seed_post(
            &test_owner,
            &public_board,
            thread,
            false,
            (7199, Some(op)),
            false,
            "Unrelated reply text",
        )
        .await;
        let _deleted_reply = seed_post(
            &test_owner,
            &public_board,
            thread,
            false,
            (11, None),
            true,
            "Deleted drawing reply",
        )
        .await;
        let removed_thread = seed_thread(&test_owner, &public_board, true).await;
        seed_post(
            &test_owner,
            &public_board,
            removed_thread,
            true,
            (12, None),
            false,
            "Post in deleted thread",
        )
        .await;

        let private_thread = seed_thread(&test_owner, &private_board, false).await;
        let private_op = seed_post(
            &test_owner,
            &private_board,
            private_thread,
            true,
            (13, None),
            false,
            "Private OP text",
        )
        .await;
        seed_post(
            &test_owner,
            &private_board,
            private_thread,
            false,
            (14, Some(private_op)),
            false,
            "Private reply text",
        )
        .await;

        // Candidate detection finds an OP whose subject and comment do not
        // contain the annotation. The matching-reply filter must omit the
        // unrelated reply when only the OP's duration matches.
        let op_hit = board_store::search(&test_public, "Time: 59s", Some(&public_board), 0)
            .await
            .unwrap();
        assert_eq!(op_hit.nhits, 1);
        assert_eq!(op_hit.threads.len(), 1);
        assert_eq!(op_hit.threads[0].thread.id, thread);
        assert_eq!(
            op_hit.threads[0]
                .posts
                .iter()
                .map(|p| p.id)
                .collect::<Vec<_>>(),
            vec![op]
        );

        // This reply is discovered solely through the rounded annotation,
        // then returned alongside the OP in the bounded matching-reply set.
        let reply_hit = board_store::search(&test_public, "Time: 1h 60m", Some(&public_board), 0)
            .await
            .unwrap();
        assert_eq!(reply_hit.nhits, 1);
        assert_eq!(
            reply_hit.threads[0]
                .posts
                .iter()
                .map(|p| p.id)
                .collect::<Vec<_>>(),
            vec![op, reply]
        );
        let source_query = format!("Source: >>{op}");
        let source_hit = board_store::search(&test_public, &source_query, Some(&public_board), 0)
            .await
            .unwrap();
        assert_eq!(source_hit.nhits, 1);
        assert_eq!(
            source_hit.threads[0]
                .posts
                .iter()
                .map(|p| p.id)
                .collect::<Vec<_>>(),
            vec![op, reply]
        );
        let time_only = board_store::search(
            &test_public,
            "Oekaki Post (Time: 59s)",
            Some(&public_board),
            0,
        )
        .await
        .unwrap();
        assert_eq!(time_only.nhits, 1);

        let all_drawing = board_store::search(&test_public, "Oekaki Post", Some(&public_board), 0)
            .await
            .unwrap();
        assert_eq!(
            all_drawing.nhits, 1,
            "all visible annotations group by thread"
        );
        // Every hidden fixture's time renders literally as Ns (<60). Search
        // globally for private posts so a faulty visibility gate is exposed.
        for (seconds, scope) in [
            (11, Some(public_board.as_str())),
            (12, Some(public_board.as_str())),
            (13, None),
            (14, None),
        ] {
            let query = format!("Time: {seconds}s");
            assert_eq!(
                board_store::search(&test_public, &query, scope, 0)
                    .await
                    .unwrap()
                    .nhits,
                0,
                "{query} must not reveal deleted or private posts"
            );
        }
        assert_eq!(
            board_store::search(&test_public, &format!("Source: >>{private_op}"), None, 0)
                .await
                .unwrap()
                .nhits,
            0,
            "private source IDs are not searchable globally"
        );
        assert_eq!(
            board_store::search(&test_public, "Oekaki Post", Some(&private_board), 0)
                .await
                .unwrap()
                .nhits,
            0,
            "explicit private-board scope cannot bypass visibility"
        );
        let public_private_rows: i64 =
            sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=$1")
                .bind(&private_board)
                .fetch_one(&test_public)
                .await
                .unwrap();
        assert_eq!(public_private_rows, 0);

        // The read projection must not modify persisted user text or the
        // posting-time wordfilter search cache.
        let stored: (String, Option<String>, i32, Option<i64>) = sqlx::query_as(
            r#"SELECT comment,wordfilter_search,drawing_time_seconds,drawing_source_post_id
               FROM content.posts WHERE id=$1"#,
        )
        .bind(reply)
        .fetch_one(&test_owner)
        .await
        .unwrap();
        assert_eq!(
            stored,
            ("Unrelated reply text".into(), None, 7199, Some(op))
        );
    })
    .await;

    cleanup(&owner, &slugs).await;
    public.close().await;
    owner.close().await;
    outcome.unwrap();
}
