#![cfg(feature = "database-tests")]
#[path = "support/posting.rs"]
mod posting;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use board_public::views::{BoardPage, ThreadView};
use board_store::{Board, Thread};
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;

const ORIGIN: &str = "http://127.0.0.1:3000";
const MEDIA_ORIGIN: &str = "http://127.0.0.1:4001";

async fn directory(api: &Router) -> Value {
    let response = api
        .clone()
        .oneshot(Request::get("/boards.json").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    serde_json::from_slice(&to_bytes(response.into_body(), 4_194_304).await.unwrap()).unwrap()
}

fn page(board: Board, media_enabled: bool) -> BoardPage {
    BoardPage {
        public_origin: String::new(),
        blotter: Vec::new(),
        spoiler_thumbnail: "/static/spoiler.png".into(),
        navigation_boards: vec![],
        quote: String::new(),
        catalog_hidden: vec![],
        board,
        threads: vec![],
        page_number: 1,
        parent: 0,
        previous: String::new(),
        next: String::new(),
        catalog: false,
        catalog_options: board_public::catalog::Options::default(),
        media_origin: if media_enabled {
            MEDIA_ORIGIN.into()
        } else {
            String::new()
        },
    }
}

fn live_thread(board: &Board) -> ThreadView {
    let now = chrono::Utc::now();
    ThreadView {
        catalog_position: None,
        catalog_last_reply: None,
        tail_size: 50,
        latest_reply_id: None,
        thread: Thread {
            id: 1,
            board: board.slug.clone(),
            created_at: now,
            bumped_at: now,
            modified_at: now,
            http_modified_at: now,
            reply_count: 0,
            sticky: false,
            permasage: false,
            permaage: false,
            undead: false,
            closed: false,
            deleted: false,
            archived_at: None,
            archive_expires_at: None,
        },
        posts: vec![],
        omitted: 0,
        image_replies: 0,
    }
}

fn check_page_gates(board: &Board) {
    let mut page = page(board.clone(), true);
    assert!(page.drawing_allowed());
    page.media_origin.clear();
    assert!(!page.drawing_allowed(), "drawing cannot enable media");
    page.media_origin = MEDIA_ORIGIN.into();
    page.catalog = true;
    assert!(
        !page.drawing_allowed(),
        "catalogs have no ordinary post form"
    );
    page.catalog = false;
    page.parent = 1;
    assert!(
        !page.drawing_allowed(),
        "missing threads cannot own a drawing"
    );
    let now = chrono::Utc::now();
    page.threads.push(live_thread(board));
    assert!(page.drawing_allowed());
    page.threads[0].thread.closed = true;
    assert!(!page.drawing_allowed());
    page.threads[0].thread.closed = false;
    page.threads[0].thread.archived_at = Some(now);
    assert!(!page.drawing_allowed());
    page.threads[0].thread.archived_at = None;
    for boundary in [
        "disabled",
        "replays",
        "text_only",
        "image_limit",
        "staff_only",
    ] {
        page.board = board.clone();
        match boundary {
            "disabled" => page.board.oekaki = false,
            "replays" => page.board.oekaki_replays = true,
            "text_only" => page.board.text_only = true,
            "image_limit" => page.board.image_limit = 0,
            "staff_only" => page.board.staff_only = true,
            _ => unreachable!(),
        }
        assert!(!page.drawing_allowed(), "{boundary}");
    }
}

fn check_image_edit_gates(board: &Board) {
    // Source /i/ enables replay policy, but importing an existing image opens
    // Tegaki with saveReplay=false. This is independent of ordinary Draw.
    let mut source = board.clone();
    source.slug = "i".into();
    source.oekaki = true;
    source.oekaki_replays = true;
    source.oekaki_width = 400;
    source.oekaki_height = 400;
    source.text_only = false;
    source.staff_only = false;
    source.image_limit = 100;
    let mut view = page(source.clone(), true);
    assert!(
        !view.drawing_allowed(),
        "replay policy keeps ordinary /i/ Draw disabled"
    );
    assert!(!view.drawing_edit_allowed(), "/i/ index has no post Edit");
    assert!(!view.drawing_controls_allowed());

    view.parent = 1;
    assert!(
        !view.drawing_edit_allowed(),
        "a missing thread cannot enable Edit"
    );
    view.threads.push(live_thread(&source));
    assert!(
        view.drawing_edit_allowed(),
        "a live /i/ thread enables imported Edit"
    );
    assert!(
        !view.drawing_allowed(),
        "imported Edit does not enable blank Draw"
    );
    assert!(view.drawing_controls_allowed());

    view.catalog = true;
    assert!(
        !view.drawing_edit_allowed(),
        "/i/ catalog cannot offer Edit"
    );
    view.catalog = false;
    view.threads[0].thread.closed = true;
    assert!(
        !view.drawing_edit_allowed(),
        "a closed /i/ thread cannot offer Edit"
    );
    view.threads[0].thread.closed = false;
    view.threads[0].thread.archived_at = Some(chrono::Utc::now());
    assert!(
        !view.drawing_edit_allowed(),
        "an archived /i/ thread cannot offer Edit"
    );
    view.threads[0].thread.archived_at = None;
    view.media_origin.clear();
    assert!(
        !view.drawing_edit_allowed(),
        "no media service means no imported Edit"
    );
    view.media_origin = MEDIA_ORIGIN.into();

    for (name, change) in [
        ("painter disabled", "oekaki"),
        ("replay policy disabled", "replays"),
        ("text-only", "text_only"),
        ("image limit disabled", "image_limit"),
        ("staff-only", "staff_only"),
        ("zero width", "width_zero"),
        ("oversized width", "width_large"),
        ("zero height", "height_zero"),
        ("oversized height", "height_large"),
    ] {
        view.board = source.clone();
        match change {
            "oekaki" => view.board.oekaki = false,
            "replays" => view.board.oekaki_replays = false,
            "text_only" => view.board.text_only = true,
            "image_limit" => view.board.image_limit = 0,
            "staff_only" => view.board.staff_only = true,
            "width_zero" => view.board.oekaki_width = 0,
            "width_large" => view.board.oekaki_width = 1025,
            "height_zero" => view.board.oekaki_height = 0,
            "height_large" => view.board.oekaki_height = 1025,
            _ => unreachable!(),
        }
        assert!(!view.drawing_edit_allowed(), "{name} must block /i/ Edit");
    }
    for other in ["qst", "vip", "test"] {
        view.board = source.clone();
        view.board.slug = other.into();
        assert!(
            !view.drawing_edit_allowed(),
            "/{other}/ never exposes imported Edit"
        );
    }
    for ordinary in ["qst", "vip"] {
        view.board = source.clone();
        view.board.slug = ordinary.into();
        view.board.oekaki_replays = false;
        assert!(view.drawing_allowed(), "/{ordinary}/ keeps ordinary Draw");
        assert!(
            !view.drawing_edit_allowed(),
            "/{ordinary}/ cannot inherit /i/ Edit"
        );
    }
}

async fn exercise(owner: &PgPool, public: &PgPool, slug: &str) {
    let reference: Value =
        serde_json::from_str(include_str!("../../../fixtures/board-reference.json")).unwrap();
    for expected in reference["boards"].as_array().unwrap() {
        let source = expected["slug"].as_str().unwrap();
        let saved: (bool, bool, i32, i32) = sqlx::query_as(
            "SELECT oekaki,oekaki_replays,oekaki_width,oekaki_height FROM content.boards WHERE slug=$1",
        )
        .bind(source)
        .fetch_one(owner)
        .await
        .unwrap();
        assert_eq!(
            saved,
            (
                matches!(source, "i" | "qst" | "vip"),
                source == "i",
                400,
                400
            )
        );
    }
    let board = board_store::board(public, slug).await.unwrap();
    check_page_gates(&board);
    check_image_edit_gates(&board);
    for media_enabled in [false, true] {
        let media = media_enabled.then(|| {
            board_config::PublicMediaSettings::development(
                "127.0.0.1:4000",
                &"a".repeat(64),
                MEDIA_ORIGIN,
            )
            .unwrap()
        });
        let (web, api) =
            posting::routers_with_media(public.clone(), slug, ORIGIN.into(), false, media);
        let listed = directory(&api).await;
        for expected in reference["boards"].as_array().unwrap() {
            let source = expected["slug"].as_str().unwrap();
            let saved = board_store::board(owner, source).await.unwrap();
            let actual = listed["boards"]
                .as_array()
                .unwrap()
                .iter()
                .find(|b| b["board"] == source);
            if saved.staff_only || !saved.json_enabled {
                assert!(actual.is_none(), "private or JSON-disabled /{source}/");
                continue;
            }
            let actual = actual.unwrap();
            assert_eq!(actual.get("oekaki").is_some(), saved.oekaki, "/{source}/");
            if actual.get("oekaki").is_some() {
                assert_eq!(actual["oekaki"], 1);
            }
            for unsupported in ["oekaki_replays", "oekaki_width", "oekaki_height"] {
                assert!(
                    actual.get(unsupported).is_none(),
                    "no invented source API fields"
                );
            }
        }
        let hidden = web
            .oneshot(Request::get("/j/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(hidden.status(), StatusCode::NOT_FOUND);
        for (enabled, replay, text_only, image_limit) in [
            (true, false, false, 100),
            (false, false, false, 100),
            (true, true, false, 100),
            (true, false, true, 100),
            (true, false, false, 0),
        ] {
            sqlx::query("UPDATE content.boards SET oekaki=$2,oekaki_replays=$3,text_only=$4,image_limit=$5 WHERE slug=$1")
                .bind(slug).bind(enabled).bind(replay).bind(text_only).bind(image_limit)
                .execute(owner).await.unwrap();
            let listed = directory(&api).await;
            let own = listed["boards"]
                .as_array()
                .unwrap()
                .iter()
                .find(|b| b["board"] == slug)
                .unwrap();
            assert_eq!(own.get("oekaki").is_some(), enabled);
        }
    }
    assert!(matches!(
        board_store::board(public, "j").await,
        Err(board_store::StoreError::NotFound)
    ));
    for (field, sql) in [
        (
            "oekaki",
            "UPDATE content.boards SET oekaki=oekaki WHERE slug=$1",
        ),
        (
            "oekaki_replays",
            "UPDATE content.boards SET oekaki_replays=oekaki_replays WHERE slug=$1",
        ),
        (
            "oekaki_width",
            "UPDATE content.boards SET oekaki_width=oekaki_width WHERE slug=$1",
        ),
        (
            "oekaki_height",
            "UPDATE content.boards SET oekaki_height=oekaki_height WHERE slug=$1",
        ),
    ] {
        assert!(
            sqlx::query(sql).bind(slug).execute(public).await.is_err(),
            "public cannot change {field}"
        );
    }
}

#[tokio::test]
async fn imported_policy_and_effective_drawing_keep_media_and_visibility_boundaries() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let slug: String =
        sqlx::query_scalar("SELECT 'oe'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,image_limit,oekaki) VALUES($1,'Owned drawing','Synthetic policy fixture',2000,100,100,100,10,100,true)")
        .bind(&slug).execute(&owner).await.unwrap();
    let owned = owner.clone();
    let runtime = public.clone();
    let board = slug.clone();
    let result = tokio::spawn(async move { exercise(&owned, &runtime, &board).await }).await;
    sqlx::query("DELETE FROM content.boards WHERE slug=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    public.close().await;
    owner.close().await;
    result.unwrap();
}
