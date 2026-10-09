#![cfg(feature = "database-tests")]

use askama::Template;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use rand_core::{OsRng, RngCore};
use roxmltree::Document;
use sqlx::PgPool;
use tower::ServiceExt;

const ORIGIN: &str = "http://127.0.0.1:3000";
const HOSTILE_TITLE: &str =
    "Owned </title></div><script id=\"identity-breakout\">bad()</script> & \" ' &lt;";
const HOSTILE_SUBJECT: &str = "</title><script id=\"identity-breakout\">bad()</script> & \" '";

#[derive(Clone)]
struct Fixture {
    slug: String,
    title: &'static str,
    upload: bool,
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
    let content_type = response.headers()["content-type"]
        .to_str()
        .unwrap()
        .to_owned();
    let html = String::from_utf8(
        to_bytes(response.into_body(), 2_000_000)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert_eq!(status, StatusCode::OK, "{path}: {html}");
    assert!(
        content_type.starts_with("text/html"),
        "{path}: {content_type}"
    );
    html
}

fn fragment_text(html: &str, opening: &str, closing: &str, path: &str) -> String {
    assert_eq!(html.matches(opening).count(), 1, "{path}: {opening}");
    let start = html.find(opening).unwrap();
    let end = html[start..]
        .find(closing)
        .unwrap_or_else(|| panic!("{path}: missing {closing}"))
        + start
        + closing.len();
    // Both identity elements contain escaped text only. Parsing their actual
    // markup decodes entities and rejects nested elements or breakout syntax.
    let document = Document::parse(&html[start..end])
        .unwrap_or_else(|error| panic!("{path}: invalid identity markup: {error}"));
    let element = document.root_element();
    assert!(
        element.children().all(|node| node.is_text()),
        "{path}: identity contains markup"
    );
    element.children().filter_map(|node| node.text()).collect()
}

fn assert_identity(html: &str, title: &str, heading: &str, path: &str) {
    // Check the whole response as well as each fragment: extraction must not
    // hide a prematurely closed title or heading followed by injected markup.
    let lower = html.to_ascii_lowercase();
    for tag in ["<title", "</title", "<head>", "</head>", "<body", "</body>"] {
        assert_eq!(lower.matches(tag).count(), 1, "{path}: {tag} count");
    }
    assert!(
        !html.contains("id=\"identity-breakout\""),
        "{path}: hostile identity escaped into the document"
    );
    assert_eq!(
        fragment_text(html, "<title>", "</title>", path),
        title,
        "{path}: browser title"
    );
    assert_eq!(
        fragment_text(
            html,
            "<div class=\"boardTitle\" role=\"heading\" aria-level=\"1\">",
            "</div>",
            path,
        ),
        heading,
        "{path}: visible board heading"
    );
}

async fn board_routes(app: &Router, slug: &str, heading: &str) {
    for (suffix, title_suffix) in [
        ("", ""),
        ("0", ""),
        ("1", " - Page 2"),
        ("2", " - Page 3"),
        ("catalog", " - Catalog"),
        ("catalog?q=no-matching-identity", " - Catalog"),
        ("archive", " - Archive"),
    ] {
        let path = format!("/{slug}/{suffix}");
        assert_identity(
            &get(app, &path).await,
            &format!("{heading}{title_suffix} - 4chan"),
            heading,
            &path,
        );
    }
}

async fn seed_boards(owner: &PgPool, fixtures: &[Fixture], marker: &str) {
    let mut tx = owner.begin().await.unwrap();
    for fixture in fixtures {
        let inserted = sqlx::query(
            "INSERT INTO content.boards SELECT (jsonb_populate_record(NULL::content.boards, \
             to_jsonb(b) || jsonb_build_object( \
             'slug',$1::text,'title',$2::text,'description',$3::text, \
             'word_filter_enabled',false,'comment_spoiler_cleanup',true, \
             'comment_code_spacing',false,'comment_sjis_spacing',false, \
             'upload_board',$4::boolean,'archive_retention_seconds',3600, \
             'archive_limit',100,'thread_limit',100,'threads_per_page',10))).* \
             FROM content.boards b WHERE b.slug='g'",
        )
        .bind(&fixture.slug)
        .bind(fixture.title)
        .bind(marker)
        .bind(fixture.upload)
        .execute(&mut *tx)
        .await
        .unwrap();
        assert_eq!(inserted.rows_affected(), 1);
    }
    tx.commit().await.unwrap();
}

// Seed normalized saved text, as a historical import would. In particular,
// literal subject markers and entity spellings have no formatting authority.
async fn seed_thread(
    owner: &PgPool,
    slug: &str,
    marker: &str,
    subject: &str,
    comment: &str,
) -> i64 {
    let mut tx = owner.begin().await.unwrap();
    let id: i64 = sqlx::query_scalar("INSERT INTO content.threads(board) VALUES($1) RETURNING id")
        .bind(slug)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO content.posts(id,board,thread_id,name,subject,comment) \
         VALUES($1,$2,$1,$3,$4,$5)",
    )
    .bind(id)
    .bind(slug)
    .bind(marker)
    .bind(subject)
    .bind(comment)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    id
}

async fn thread_states(
    owner: &PgPool,
    app: &Router,
    slug: &str,
    id: i64,
    title: &str,
    heading: &str,
) {
    let path = format!("/{slug}/thread/{id}");
    for state in ["live", "closed", "archived"] {
        sqlx::query(
            "UPDATE content.threads SET closed=($3='closed'), \
             archived_at=CASE WHEN $3='archived' THEN now() ELSE NULL END, \
             archive_expires_at=CASE WHEN $3='archived' THEN now()+interval '1 hour' ELSE NULL END \
             WHERE board=$1 AND id=$2",
        )
        .bind(slug)
        .bind(id)
        .bind(state)
        .execute(owner)
        .await
        .unwrap();
        for suffix in ["", "/stale-subject"] {
            let route = format!("{path}{suffix}");
            let html = get(app, &route).await;
            assert_identity(&html, title, heading, &route);
            assert_eq!(
                html.contains("This thread is archived and read-only."),
                state == "archived",
                "{route}: thread state"
            );
        }
    }
}

async fn private_and_missing_op_views(owner: &PgPool, fixture: &Fixture) {
    let mut board = board_store::board(owner, &fixture.slug).await.unwrap();
    // Private-board routing remains protected by RLS. Exercise its presentation
    // branch with an in-memory copy, without changing the board's permissions.
    board.staff_only = true;
    let mut page = board_public::views::BoardPage {
        blotter: Vec::new(),
        spoiler_thumbnail: String::new(),
        navigation_boards: Vec::new(),
        quote: String::new(),
        catalog_hidden: Vec::new(),
        board,
        threads: Vec::new(),
        page_number: 1,
        parent: 123,
        previous: String::new(),
        next: String::new(),
        catalog: false,
        catalog_options: board_public::catalog::Options::default(),
        media_origin: String::new(),
    };
    let heading = format!("/{}/ - {}", fixture.slug, fixture.title);
    assert_identity(
        &page.render().unwrap(),
        &format!("{heading} - {} - 4chan", fixture.title),
        &heading,
        "private thread view",
    );
    page.board.staff_only = false;
    assert_identity(
        &page.render().unwrap(),
        &format!("/{}/ - No.123 - {} - 4chan", fixture.slug, fixture.title),
        &heading,
        "thread view without an OP",
    );
}

async fn exercise(owner: PgPool, public: PgPool, fixtures: Vec<Fixture>, marker: String) {
    seed_boards(&owner, &fixtures, &marker).await;
    private_and_missing_op_views(&owner, &fixtures[1]).await;
    let app = board_public::router(public, ORIGIN.into(), false);
    let long_subject = format!("Subject {}", "é".repeat(55));
    let unicode_comment = format!("{}🦀tail after the boundary", "é".repeat(49));
    let unicode_title = format!("{}🦀", "é".repeat(49));
    let cases = [
        ("Subject wins", "Ignored comment", Some("Subject wins")),
        (
            long_subject.as_str(),
            "Ignored comment",
            Some(long_subject.as_str()),
        ),
        ("!!!", "Must not replace punctuation", Some("!!!")),
        (
            "123|Ordinary subject",
            "Ignored comment",
            Some("123|Ordinary subject"),
        ),
        (
            "SPOILER<>literal &lt; &#60;",
            "Ignored comment",
            Some("SPOILER<>literal &lt; &#60;"),
        ),
        (HOSTILE_SUBJECT, "Ignored comment", Some(HOSTILE_SUBJECT)),
        ("", HOSTILE_SUBJECT, Some("bad() & \" '")),
        (
            "",
            "First line\nSecond line",
            Some("First line Second line"),
        ),
        ("", "a[spoiler]hidden[/spoiler]b", Some("ahiddenb")),
        (
            "",
            "<b>literal</b> &lt;tag&gt; &amp;",
            Some("literal &lt;tag&gt; &amp;"),
        ),
        ("", unicode_comment.as_str(), Some(unicode_title.as_str())),
        ("", "<b></b>", None),
    ];

    for fixture in &fixtures {
        let slug = &fixture.slug;
        let heading = format!("/{slug}/ - {}", fixture.title);
        for text_only in [false, true] {
            sqlx::query("UPDATE content.boards SET text_only=$2 WHERE slug=$1")
                .bind(slug)
                .bind(text_only)
                .execute(&owner)
                .await
                .unwrap();
            board_routes(&app, slug, &heading).await;
        }
        sqlx::query("UPDATE content.boards SET text_only=false WHERE slug=$1")
            .bind(slug)
            .execute(&owner)
            .await
            .unwrap();

        if fixture.upload {
            for (subject, comment, context) in [
                ("123|Upload subject", "Ignored comment", "Upload subject"),
                ("123|", "Upload comment", "Upload comment"),
                (
                    "Plain upload subject",
                    "Ignored comment",
                    "Plain upload subject",
                ),
            ] {
                let id = seed_thread(&owner, slug, &marker, subject, comment).await;
                thread_states(
                    &owner,
                    &app,
                    slug,
                    id,
                    &format!("/{slug}/ - {context} - {} - 4chan", fixture.title),
                    &heading,
                )
                .await;
            }
            continue;
        }

        for (subject, comment, expected) in cases {
            let id = seed_thread(&owner, slug, &marker, subject, comment).await;
            // A later reply must never become the thread's title source.
            sqlx::query(
                "INSERT INTO content.posts(board,thread_id,name,subject,comment) \
                 VALUES($1,$2,$3,'Reply title must not win','Reply body must not win')",
            )
            .bind(slug)
            .bind(id)
            .bind(&marker)
            .execute(&owner)
            .await
            .unwrap();
            let context = expected.map_or_else(|| format!("No.{id}"), str::to_owned);
            thread_states(
                &owner,
                &app,
                slug,
                id,
                &format!("/{slug}/ - {context} - {} - 4chan", fixture.title),
                &heading,
            )
            .await;
            let saved: (String, String) = sqlx::query_as(
                "SELECT subject,comment FROM content.posts WHERE board=$1 AND id=$2",
            )
            .bind(slug)
            .bind(id)
            .fetch_one(&owner)
            .await
            .unwrap();
            assert_eq!(
                saved,
                (subject.into(), comment.into()),
                "{slug}/{id}: saved text"
            );
        }
        // An archive with actual entries still displays the board name.
        board_routes(&app, slug, &heading).await;
    }

    // The special prefix belongs to the installed s4s identity. Only owned
    // thread rows are added; its source board settings remain untouched.
    let heading = "[s4s] - Sh*t 4chan Says";
    board_routes(&app, "s4s", heading).await;
    for (subject, comment, context) in [
        ("Owned s4s subject", "Ignored body", "Owned s4s subject"),
        ("", "Owned s4s fallback", "Owned s4s fallback"),
    ] {
        let id = seed_thread(&owner, "s4s", &marker, subject, comment).await;
        thread_states(
            &owner,
            &app,
            "s4s",
            id,
            &format!("[s4s] - {context} - Sh*t 4chan Says - 4chan"),
            heading,
        )
        .await;
    }
    board_routes(&app, "s4s", heading).await;
}

#[tokio::test]
async fn source_page_identity_survives_routes_archiving_and_hostile_saved_text() {
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
    let random = OsRng.next_u32();
    let marker = format!("Owned page identity {random:08x}");
    let fixtures: Vec<_> = [
        ("pi", "Owned identity", false),
        ("px", HOSTILE_TITLE, false),
        ("pu", "Owned uploads", true),
    ]
    .into_iter()
    .map(|(prefix, title, upload)| Fixture {
        slug: format!("{prefix}{random:08x}"),
        title,
        upload,
    })
    .collect();
    let result = tokio::spawn(exercise(
        owner.clone(),
        public.clone(),
        fixtures.clone(),
        marker.clone(),
    ))
    .await;

    let mut cleanup_ok = true;
    for fixture in &fixtures {
        let owned = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM content.boards WHERE slug=$1 AND title=$2 AND description=$3)",
        )
        .bind(&fixture.slug)
        .bind(fixture.title)
        .bind(&marker)
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
        for query in [
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
    // Delete only s4s threads whose OP still bears this invocation's marker.
    // A failed insert cannot lead to cleanup of an unrelated installed board.
    let cleanup = async {
        let mut tx = owner.begin().await?;
        let ids: Vec<i64> = sqlx::query_scalar(
            "SELECT id FROM content.posts WHERE board='s4s' AND id=thread_id AND name=$1",
        )
        .bind(&marker)
        .fetch_all(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM content.posts WHERE board='s4s' AND thread_id=ANY($1)")
            .bind(&ids)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM content.threads WHERE board='s4s' AND id=ANY($1)")
            .bind(&ids)
            .execute(&mut *tx)
            .await?;
        tx.commit().await
    }
    .await;
    cleanup_ok &= cleanup.is_ok();
    public.close().await;
    owner.close().await;
    assert!(cleanup_ok, "Owned page identity fixture cleanup failed");
    result.unwrap();
}
