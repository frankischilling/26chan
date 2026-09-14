#![cfg(feature = "database-tests")]

use board_store::NewPost;
use rand_core::{OsRng, RngCore};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::time::Duration;

fn post() -> NewPost {
    NewPost {
        name: "Anonymous".into(),
        subject: "Owned markup policy".into(),
        comment: "[spoiler]Owned text[/spoiler]".into(),
        deletion_hash: "owned-fixture-hash".into(),
        sage: false,
    }
}

async fn set_policy(owner: &PgPool, slug: &str, mask: i16) {
    sqlx::query("UPDATE content.boards SET comment_spoiler_cleanup=$2,comment_code_spacing=$3,comment_sjis_spacing=$4 WHERE slug=$1")
        .bind(slug).bind(mask & 1 != 0).bind(mask & 2 != 0).bind(mask & 4 != 0)
        .execute(owner).await.unwrap();
}

async fn exercise(owner: PgPool, public: PgPool, slug: String) {
    let mut ids = Vec::new();
    for mask in 0i16..8 {
        set_policy(&owner, &slug, mask).await;
        let id = board_store::create_post(&public, &slug, 0, &post())
            .await
            .unwrap();
        let saved = board_store::find_post(&public, &slug, id).await.unwrap();
        assert_eq!(saved.comment_format, 8 + mask);
        assert_eq!(saved.comment, post().comment);
        ids.push(id);
    }
    let before: Vec<String> = sqlx::query_scalar(
        "SELECT to_jsonb(p)::text FROM content.posts p WHERE board=$1 ORDER BY id",
    )
    .bind(&slug)
    .fetch_all(&owner)
    .await
    .unwrap();
    set_policy(&owner, &slug, 0).await;
    let after: Vec<String> = sqlx::query_scalar(
        "SELECT to_jsonb(p)::text FROM content.posts p WHERE board=$1 ORDER BY id",
    )
    .bind(&slug)
    .fetch_all(&owner)
    .await
    .unwrap();
    assert_eq!(before, after);

    for statement in [
        "UPDATE content.posts SET comment_format=15 WHERE board=$1",
        "UPDATE content.boards SET comment_code_spacing=true WHERE slug=$1",
        "INSERT INTO content.posts(id,board,thread_id,name,subject,comment,comment_format) VALUES ($2,$1,$2,'Anonymous','','forged',0)",
    ] {
        let error = if statement.starts_with("INSERT") {
            sqlx::query(statement)
                .bind(&slug)
                .bind(ids[0])
                .execute(&public)
                .await
                .unwrap_err()
        } else {
            sqlx::query(statement)
                .bind(&slug)
                .execute(&public)
                .await
                .unwrap_err()
        };
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501")
        );
    }
    let trigger_safe: bool = sqlx::query_scalar("SELECT NOT p.prosecdef AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp'] AND NOT EXISTS (SELECT 1 FROM aclexplode(p.proacl) a WHERE a.grantee=0) FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname='content' AND p.proname='stamp_comment_format'")
        .fetch_one(&owner).await.unwrap();
    assert!(trigger_safe);

    // A deletion changes only its established fields, not the format stamp.
    sqlx::query("UPDATE content.posts SET deleted=true WHERE id=$1")
        .bind(ids[7])
        .execute(&public)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i16>("SELECT comment_format FROM content.posts WHERE id=$1")
            .bind(ids[7])
            .fetch_one(&public)
            .await
            .unwrap(),
        15
    );

    // The trigger runs with the narrowly privileged attachment owner too.
    // This authorized direct insertion is rolled back, not a substitute for
    // the existing approved-capability attachment integration test.
    set_policy(&owner, &slug, 7).await;
    let mut attachment = owner.begin().await.unwrap();
    let id: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&mut *attachment)
        .await
        .unwrap();
    sqlx::query("SET LOCAL ROLE board_attachment_owner")
        .execute(&mut *attachment)
        .await
        .unwrap();
    sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES ($1,$2,$3,'Anonymous','','Owned attachment-role stamp')")
        .bind(id).bind(&slug).bind(ids[0]).execute(&mut *attachment).await.unwrap();
    sqlx::query("RESET ROLE")
        .execute(&mut *attachment)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i16>("SELECT comment_format FROM content.posts WHERE id=$1")
            .bind(id)
            .fetch_one(&mut *attachment)
            .await
            .unwrap(),
        15
    );
    attachment.rollback().await.unwrap();
    assert!(
        !sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM content.posts WHERE id=$1)")
            .bind(id)
            .fetch_one(&owner)
            .await
            .unwrap()
    );

    for (before, after) in [(0, 7), (7, 0)] {
        set_policy(&owner, &slug, before).await;
        let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&public)
            .await
            .unwrap();
        let mut held = owner.begin().await.unwrap();
        sqlx::query("UPDATE content.boards SET comment_spoiler_cleanup=$2,comment_code_spacing=$3,comment_sjis_spacing=$4 WHERE slug=$1")
            .bind(&slug).bind(after & 1 != 0).bind(after & 2 != 0).bind(after & 4 != 0)
            .execute(&mut *held).await.unwrap();
        let task_pool = public.clone();
        let task_slug = slug.clone();
        let insertion = tokio::spawn(async move {
            board_store::create_post(&task_pool, &task_slug, 0, &post()).await
        });
        let mut witnessed = false;
        for _ in 0..100 {
            witnessed = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_locks WHERE pid=$1 AND NOT granted)",
            )
            .bind(pid)
            .fetch_one(&owner)
            .await
            .unwrap();
            if witnessed {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        if !witnessed {
            held.rollback().await.unwrap();
            insertion.await.unwrap().unwrap();
            panic!("posting did not witness the held board-policy lock");
        }
        held.commit().await.unwrap();
        let id = insertion.await.unwrap().unwrap();
        assert_eq!(
            board_store::find_post(&public, &slug, id)
                .await
                .unwrap()
                .comment_format,
            8 + after
        );
    }
}

#[tokio::test]
async fn new_posts_stamp_locked_markup_policy_without_changing_earlier_posts() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    // One connection makes the witnessed backend PID belong to the insertion.
    let public = PgPoolOptions::new()
        .max_connections(1)
        .connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT current_user::text")
            .fetch_one(&public)
            .await
            .unwrap(),
        "board_public"
    );
    let mut random = [0u8; 5];
    OsRng.fill_bytes(&mut random);
    let slug: String = random.iter().map(|b| format!("{b:02x}")).collect();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Markup policy','Owned fixture',1000,100,100,100,10)")
        .bind(&slug).execute(&owner).await.unwrap();
    let result = tokio::spawn(exercise(owner.clone(), public.clone(), slug.clone())).await;
    public.close().await;
    sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)").bind(&slug).execute(&owner).await.unwrap();
    for statement in [
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(statement)
            .bind(&slug)
            .execute(&owner)
            .await
            .unwrap();
    }
    owner.close().await;
    result.unwrap();
}
