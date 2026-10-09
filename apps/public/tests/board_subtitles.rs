#![cfg(feature = "database-tests")]

#[path = "support/posting.rs"]
mod posting;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use board_store::{BoardSubtitle, NewPost};
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;

const ORIGIN: &str = "http://127.0.0.1:3000";
const FICTION: &str = "<div class=\"boardSubtitle\">The stories and information posted here are artistic works of fiction and falsehood.<br>Only a fool would take anything posted here as fact.</div>";
const WORKSAFE_GIF: &str = "<div class=\"boardSubtitle\">Worksafe Board: /<a href=\"/wsg/\" title=\"Worksafe GIF\">wsg</a>/</div>";

#[derive(Clone)]
struct Fixture {
    source: &'static str,
    slug: String,
    profile: BoardSubtitle,
}

fn profile(slug: &str) -> BoardSubtitle {
    match slug {
        "b" | "trash" => BoardSubtitle::Fiction,
        "gif" => BoardSubtitle::WorksafeGif,
        _ => BoardSubtitle::None,
    }
}

fn description(slug: &str) -> String {
    format!("Owned SEO {slug} <script>ownedSubtitleDescription()</script> & description")
}

async fn get(app: &Router, path: &str) -> String {
    let response = app
        .clone()
        .oneshot(
            Request::get(path)
                .header("origin", ORIGIN)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = String::from_utf8(
        to_bytes(response.into_body(), 2_000_000)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert_eq!(status, StatusCode::OK, "{path}: {body}");
    body
}

fn assert_subtitle(html: &str, expected: BoardSubtitle, path: &str) {
    assert_eq!(
        html.matches("class=\"boardSubtitle\"").count(),
        usize::from(expected != BoardSubtitle::None),
        "{path}: subtitle count"
    );
    match expected {
        BoardSubtitle::None => {}
        BoardSubtitle::Fiction => assert!(html.contains(FICTION), "{path}: fiction subtitle"),
        BoardSubtitle::WorksafeGif => {
            assert!(
                html.contains(WORKSAFE_GIF),
                "{path}: fixed local worksafe link"
            );
        }
    }
    assert!(
        !html.contains("Owned SEO"),
        "{path}: description became a subtitle"
    );
    assert!(
        !html.contains("ownedSubtitleDescription()"),
        "{path}: description leaked into board chrome"
    );
}

async fn installed_profiles(owner: &PgPool) {
    let reference: Value =
        serde_json::from_str(include_str!("../../../fixtures/board-reference.json")).unwrap();
    let boards = reference["boards"].as_array().unwrap();
    assert_eq!(boards.len(), 82);
    for expected in boards {
        let slug = expected["slug"].as_str().unwrap();
        let saved = board_store::board(owner, slug).await.unwrap();
        assert_eq!(
            saved.board_subtitle,
            profile(slug),
            "/{slug}/: installed profile"
        );
        assert_eq!(saved.description, expected["description"].as_str().unwrap());
    }
}

async fn clone_boards(owner: &PgPool, fixtures: &[Fixture]) {
    let mut tx = owner.begin().await.unwrap();
    for fixture in fixtures {
        // Copy source policy to unique slugs so HTTP assertions cannot pass
        // through a board-name special case. Only fixture capacity is relaxed.
        let inserted = sqlx::query(
            "INSERT INTO content.boards SELECT (jsonb_populate_record(NULL::content.boards, \
             to_jsonb(b) || jsonb_build_object( \
             'slug',$1::text,'title','Owned subtitle ' || $1::text,'description',$3::text, \
             'posting_reply_seconds',0,'posting_image_seconds',0,'posting_thread_seconds',0, \
             'archive_retention_seconds',3600,'archive_limit',100,'thread_limit',100, \
             'threads_per_page',10))).* FROM content.boards b WHERE b.slug=$2",
        )
        .bind(&fixture.slug)
        .bind(fixture.source)
        .bind(description(&fixture.slug))
        .execute(&mut *tx)
        .await
        .unwrap();
        assert_eq!(inserted.rows_affected(), 1);
    }
    tx.commit().await.unwrap();
}

async fn rejected_profiles(owner: &PgPool, slug: &str) {
    for (saved, expected) in [
        ("none", BoardSubtitle::None),
        ("fiction", BoardSubtitle::Fiction),
        ("worksafe_gif", BoardSubtitle::WorksafeGif),
    ] {
        assert_eq!(
            sqlx::query_scalar::<_, BoardSubtitle>("SELECT $1::text")
                .bind(saved)
                .fetch_one(owner)
                .await
                .unwrap(),
            expected
        );
    }
    for invalid in [
        "",
        "Fiction",
        "fiction ",
        "<script>ownedSubtitleDescription()</script>",
        "<a href=\"javascript:alert(1)\">Worksafe</a>",
        "https://untrusted.example/subtitle",
    ] {
        let error = sqlx::query("UPDATE content.boards SET board_subtitle=$2 WHERE slug=$1")
            .bind(slug)
            .bind(invalid)
            .execute(owner)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514"),
            "Accepted invalid subtitle profile {invalid:?}"
        );
        // Even an unexpected database projection must fail typed decoding.
        assert!(matches!(
            sqlx::query_scalar::<_, BoardSubtitle>("SELECT $1::text")
                .bind(invalid)
                .fetch_one(owner)
                .await,
            Err(sqlx::Error::ColumnDecode { .. })
        ));
    }
    let error = sqlx::query("UPDATE content.boards SET board_subtitle=NULL WHERE slug=$1")
        .bind(slug)
        .execute(owner)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23502")
    );
    assert_eq!(
        board_store::board(owner, slug)
            .await
            .unwrap()
            .board_subtitle,
        BoardSubtitle::None
    );
}

async fn runtime_cannot_change_profiles(owner: &PgPool, slug: &str) {
    for (variable, role) in [
        ("TEST_PUBLIC_DATABASE_URL", "board_public"),
        ("STAFF_DATABASE_URL", "board_staff"),
        ("AUTH_DATABASE_URL", "board_auth"),
    ] {
        let runtime = PgPool::connect(&std::env::var(variable).unwrap())
            .await
            .unwrap();
        let identity: (String, String) =
            sqlx::query_as("SELECT current_user::text,session_user::text")
                .fetch_one(&runtime)
                .await
                .unwrap();
        assert_eq!(identity, (role.into(), role.into()), "{variable}");
        for privilege in ["INSERT", "UPDATE"] {
            assert!(
                !sqlx::query_scalar::<_, bool>(
                    "SELECT has_column_privilege($1,'content.boards','board_subtitle',$2)",
                )
                .bind(role)
                .bind(privilege)
                .fetch_one(owner)
                .await
                .unwrap(),
                "{role} has {privilege} subtitle authority"
            );
        }
        let mut tx = runtime.begin().await.unwrap();
        let result =
            sqlx::query("UPDATE content.boards SET board_subtitle='fiction' WHERE slug=$1")
                .bind(slug)
                .execute(&mut *tx)
                .await;
        tx.rollback().await.unwrap();
        runtime.close().await;
        let error = result.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501"),
            "{role} changed the subtitle"
        );
    }
}

async fn exercise(owner: PgPool, public: PgPool, fixtures: Vec<Fixture>) {
    installed_profiles(&owner).await;
    clone_boards(&owner, &fixtures).await;
    rejected_profiles(&owner, &fixtures[0].slug).await;
    runtime_cannot_change_profiles(&owner, &fixtures[0].slug).await;

    let (web, api) = posting::routers(public.clone(), &fixtures[0].slug, ORIGIN.into(), false);
    for fixture in &fixtures {
        let slug = &fixture.slug;
        assert_eq!(
            board_store::board(&public, slug)
                .await
                .unwrap()
                .board_subtitle,
            fixture.profile
        );
        let post = posting::create_post(
            &public,
            slug,
            0,
            &NewPost {
                name: "Anonymous".into(),
                subject: "Owned subtitle thread".into(),
                comment: "Owned persisted subtitle body".into(),
                deletion_hash: "owned-subtitle-fixture".into(),
                sage: false,
            },
        )
        .await
        .unwrap();
        for state in ["live", "closed", "archived"] {
            sqlx::query(
                "UPDATE content.threads SET closed=($3='closed'), \
                 archived_at=CASE WHEN $3='archived' THEN now() ELSE NULL END, \
                 archive_expires_at=CASE WHEN $3='archived' THEN now()+interval '1 hour' ELSE NULL END \
                 WHERE board=$1 AND id=$2",
            )
            .bind(slug)
            .bind(post)
            .bind(state)
            .execute(&owner)
            .await
            .unwrap();
            for text_only in [false, true] {
                sqlx::query("UPDATE content.boards SET text_only=$2 WHERE slug=$1")
                    .bind(slug)
                    .bind(text_only)
                    .execute(&owner)
                    .await
                    .unwrap();
                for path in [
                    format!("/{slug}/"),
                    format!("/{slug}/catalog"),
                    format!("/{slug}/thread/{post}"),
                    format!("/{slug}/archive"),
                ] {
                    let html = get(&web, &path).await;
                    assert_subtitle(&html, fixture.profile, &path);
                    let attributes = html
                        .split_once("<body")
                        .unwrap()
                        .1
                        .split_once('>')
                        .unwrap()
                        .0;
                    assert_eq!(attributes.contains(" text_only"), text_only, "{path}");
                    if path.contains("/thread/") {
                        assert!(html.contains("Owned persisted subtitle body"));
                        assert_eq!(html.contains("id=\"reply\""), state == "live");
                        assert_eq!(html.contains("archived and read-only"), state == "archived");
                    }
                    if state == "archived" && path.ends_with("/archive") {
                        assert!(html.contains(&format!("/{slug}/thread/{post}")));
                    }
                }
            }
        }
    }

    // Descriptions retain their existing directory/API role without supplying
    // the subtitle. The public JSON contract does not expose template profiles.
    for app in [&web, &api] {
        let directory: Value = serde_json::from_str(&get(app, "/boards.json").await).unwrap();
        for fixture in &fixtures {
            let board = directory["boards"]
                .as_array()
                .unwrap()
                .iter()
                .find(|board| board["board"] == fixture.slug)
                .unwrap();
            assert_eq!(board["meta_description"], description(&fixture.slug));
            assert!(board.get("board_subtitle").is_none());
            assert!(board.get("subtitle").is_none());
        }
    }
    let home = get(&web, "/").await;
    for fixture in &fixtures {
        assert!(home.contains(&format!("<p>Owned SEO {} &#60;script&#62;ownedSubtitleDescription()&#60;/script&#62; &#38; description</p>", fixture.slug)));
        let feed = get(&web, &format!("/{}/index.rss", fixture.slug)).await;
        let document = roxmltree::Document::parse(&feed).unwrap();
        let channel = document
            .root_element()
            .children()
            .find(|node| node.has_tag_name("channel"))
            .unwrap();
        let description = channel
            .children()
            .find(|node| node.has_tag_name("description"))
            .unwrap()
            .text()
            .unwrap();
        assert_eq!(
            description,
            format!(
                "Threads on /{}/ - Owned subtitle {} at 4chan.org.",
                fixture.slug, fixture.slug
            )
        );
        assert!(!feed.contains("boardSubtitle"));
    }
    assert!(!home.contains("<script>ownedSubtitleDescription()"));

    // Re-read persisted policy on the same arbitrary slug after an operator
    // update, including the default/absent transition on text-only pages.
    let slug = &fixtures[0].slug;
    for (saved, expected) in [
        ("fiction", BoardSubtitle::Fiction),
        ("worksafe_gif", BoardSubtitle::WorksafeGif),
    ] {
        sqlx::query("UPDATE content.boards SET board_subtitle=$2 WHERE slug=$1")
            .bind(slug)
            .bind(saved)
            .execute(&owner)
            .await
            .unwrap();
        for path in [
            format!("/{slug}/"),
            format!("/{slug}/catalog"),
            format!("/{slug}/archive"),
        ] {
            assert_subtitle(&get(&web, &path).await, expected, &path);
        }
    }
    sqlx::query("UPDATE content.boards SET board_subtitle=DEFAULT WHERE slug=$1")
        .bind(slug)
        .execute(&owner)
        .await
        .unwrap();
    assert_subtitle(
        &get(&web, &format!("/{slug}/")).await,
        BoardSubtitle::None,
        slug,
    );
    installed_profiles(&owner).await;
}

#[tokio::test]
async fn persisted_source_subtitles_render_on_board_routes_with_operator_only_policy() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let identity: (String, String) = sqlx::query_as("SELECT current_user::text,session_user::text")
        .fetch_one(&owner)
        .await
        .unwrap();
    assert_eq!(identity, ("board_migrator".into(), "board_migrator".into()));
    let suffix: String =
        sqlx::query_scalar("SELECT substr(replace(gen_random_uuid()::text,'-',''),1,7)")
            .fetch_one(&owner)
            .await
            .unwrap();
    let fixtures: Vec<_> = ["g", "b", "trash", "gif"]
        .into_iter()
        .enumerate()
        .map(|(index, source)| Fixture {
            source,
            slug: format!("bs{index}{suffix}"),
            profile: profile(source),
        })
        .collect();
    let result = tokio::spawn(exercise(owner.clone(), public.clone(), fixtures.clone())).await;
    let mut cleanup_ok = true;
    for fixture in &fixtures {
        // A failed seed (including a slug collision) must not clean up a board
        // that this fixture did not create.
        let owned = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM content.boards WHERE slug=$1 AND title=$2 AND description=$3)",
        )
        .bind(&fixture.slug)
        .bind(format!("Owned subtitle {}", fixture.slug))
        .bind(description(&fixture.slug))
        .fetch_one(&owner)
        .await;
        match owned {
            Ok(true) => {}
            Ok(false) => continue,
            Err(_) => {
                cleanup_ok = false;
                continue;
            }
        }
        cleanup_ok &= tokio::spawn({
            let owner = owner.clone();
            let slug = fixture.slug.clone();
            async move { posting::cleanup_posting(&owner, &slug).await }
        })
        .await
        .is_ok();
        for query in [
            "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
            "DELETE FROM content.posts WHERE board=$1",
            "DELETE FROM content.threads WHERE board=$1",
            "DELETE FROM content.boards WHERE slug=$1",
        ] {
            cleanup_ok &= sqlx::query(query)
                .bind(&fixture.slug)
                .execute(&owner)
                .await
                .is_ok();
        }
    }
    public.close().await;
    owner.close().await;
    assert!(cleanup_ok, "Owned subtitle fixture cleanup failed");
    result.unwrap();
}
