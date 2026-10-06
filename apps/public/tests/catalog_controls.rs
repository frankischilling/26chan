#![cfg(feature = "database-tests")]

use axum::{Router, body::Body, http::Request};
use http_body_util::BodyExt;
use rand_core::{OsRng, RngCore};
use sqlx::PgPool;
use tower::ServiceExt;

async fn read(app: &Router, path: &str) -> (u16, String) {
    let response = app
        .clone()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status().as_u16();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

fn ids(html: &str) -> Vec<i64> {
    html.split("<template id=\"catalogFiltered\">")
        .next()
        .unwrap()
        .split("id=\"thread-")
        .skip(1)
        .map(|suffix| suffix.split('"').next().unwrap().parse().unwrap())
        .collect()
}

fn bump_limited(html: &str, id: i64) -> bool {
    html.split(&format!("id=\"meta-{id}\""))
        .nth(1)
        .unwrap()
        .split("</div>")
        .next()
        .unwrap()
        .contains("<i>R: <b>")
}

fn latest_reply(html: &str, id: i64) -> Option<i64> {
    let value = html
        .split(&format!("id=\"thread-{id}\""))
        .nth(1)
        .unwrap()
        .split("data-latest-reply=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    if value.is_empty() {
        None
    } else {
        Some(value.parse().unwrap())
    }
}

fn preview(html: &str, id: i64) -> &str {
    html.split(&format!("id=\"thread-{id}\""))
        .nth(1)
        .unwrap()
        .split("<template class=\"catalogPreview\">")
        .nth(1)
        .unwrap()
        .split("</template>")
        .next()
        .unwrap()
}

async fn exercise(owner: PgPool, public: PgPool, slug: String) {
    let app = board_public::router(public.clone(), "http://127.0.0.1:3000".into(), false);
    let mut threads = Vec::new();
    for (index, subject) in ["Alpha [.*]", "Bravo", "Crane", "Sticky"]
        .iter()
        .enumerate()
    {
        let id: i64 = sqlx::query_scalar("INSERT INTO content.threads(board,bumped_at,sticky,reply_count) VALUES($1,'2026-01-05T00:00:00Z'::timestamptz - $2 * interval '1 day',$3,$4) RETURNING id")
            .bind(&slug).bind(index as i32).bind(index == 3).bind(100 - index as i32).fetch_one(&owner).await.unwrap();
        sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous',$3,'Synthetic <script>fold</script>')")
            .bind(id).bind(&slug).bind(subject).execute(&owner).await.unwrap();
        threads.push(id);
    }
    let [a, b, c, s] = threads.as_slice() else {
        unreachable!()
    };
    let mut replies = Vec::new();
    for thread in [c, c, c, a, b, b, c] {
        let id: i64 = sqlx::query_scalar("INSERT INTO content.posts(board,thread_id,name,subject,comment,deleted) VALUES($1,$2,'Anonymous','','A reply', $3) RETURNING id")
            .bind(&slug).bind(thread).bind(replies.len() == 6).fetch_one(&owner).await.unwrap();
        replies.push(id);
    }
    sqlx::query("UPDATE content.posts SET name='<script>reply & author</script>',created_at='2026-09-08T12:05:00Z' WHERE id=$1")
        .bind(replies[5]).execute(&owner).await.unwrap();
    sqlx::query("UPDATE content.posts SET name='Deleted private name' WHERE id=$1")
        .bind(replies[6])
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("UPDATE content.posts SET name='Visible reply name' WHERE id=$1")
        .bind(replies[3])
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("UPDATE content.posts SET name='<b>OP & author</b>' WHERE id=$1")
        .bind(b)
        .execute(&owner)
        .await
        .unwrap();
    for (query, expected) in [
        ("", vec![*s, *a, *b, *c]),
        ("?order=date", vec![*s, *c, *b, *a]),
        ("?order=absdate", vec![*s, *b, *a, *c]),
        ("?order=r", vec![*s, *c, *b, *a]),
    ] {
        let (status, page) = read(&app, &format!("/{slug}/catalog{query}")).await;
        assert_eq!(status, 200);
        assert_eq!(ids(&page), expected, "{query}");
        assert!(!page.contains("<script>fold</script>"));
        assert!(preview(&page, *b).contains("&#60;b&#62;OP &#38; author&#60;/b&#62;"));
        assert!(preview(&page, *b).contains("&#60;script&#62;reply &#38; author&#60;/script&#62;"));
        assert!(preview(&page, *b).contains("data-created-at=\"1788869100\""));
        assert!(!preview(&page, *s).contains("post-last"));
        assert!(!page.contains("Deleted private name"));
        for (id, latest) in [
            (*s, None),
            (*a, Some(replies[3])),
            (*b, Some(replies[5])),
            (*c, Some(replies[2])),
        ] {
            assert_eq!(
                latest_reply(&page, id),
                latest,
                "snapshot reply metadata {query} {id}"
            );
            if let Some(latest) = latest {
                assert!(preview(&page, id).contains(&format!("data-reply-id=\"{latest}\"")));
            }
        }
        for (id, limited) in [(*a, false), (*b, true), (*c, true), (*s, false)] {
            assert_eq!(bump_limited(&page, id), limited, "{query} {id}");
        }
    }
    for limit in [Some(0), Some(5), None] {
        let snapshot =
            board_store::board_snapshot(&public, &slug, board_store::BoardSelection::All, limit)
                .await
                .unwrap();
        for thread in snapshot.threads {
            assert_eq!(
                thread.catalog_last_reply.is_some(),
                limit == Some(0) && thread.latest_reply_id.is_some()
            );
            if let Some(last) = thread.catalog_last_reply {
                assert_eq!(Some(last.id), thread.latest_reply_id);
                assert_eq!(last.thread_id, thread.thread.id);
                assert_eq!(
                    thread.posts.len(),
                    1,
                    "hover fetch does not load reply bodies"
                );
            }
        }
    }
    sqlx::query("UPDATE content.boards SET text_only=true WHERE slug=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    for (query, expected) in [
        ("", vec![*a, *b, *c, *s]),
        ("?order=date", vec![*s, *c, *b, *a]),
        ("?order=absdate", vec![*b, *a, *c, *s]),
        ("?order=r", vec![*c, *b, *a, *s]),
        ("?q=Crane", vec![*c]),
        ("?q=absent", vec![]),
        ("?size=large&teaser=off", vec![*a, *b, *c, *s]),
    ] {
        let (status, page) = read(&app, &format!("/{slug}/catalog{query}")).await;
        assert_eq!(status, 200);
        assert_eq!(ids(&page), expected, "text catalog {query}");
        assert!(page.contains("class=\"catalog textCatalog\""));
        assert!(page.contains("data-text-only=\"true\""));
        assert!(page.contains("<th class=\"txt-sub\" scope=\"col\">Subject</th>"));
        assert!(!page.contains("<section class=\"thread\""));
        assert!(!page.contains("<img class=\"thumb"));
        assert_eq!(
            page.matches("<template class=\"catalogTeaser\">").count(),
            4
        );
        assert!(!page.contains("<script>fold</script>"));
        for id in &expected {
            let (_, json) = read(&app, &format!("/{slug}/thread/{id}.json")).await;
            let json: serde_json::Value = serde_json::from_str(&json).unwrap();
            let date = json["posts"][0]["now"].as_str().unwrap();
            assert!(page.contains(&format!("class=\"txt-date\" data-id=\"{id}\">{date}</td>")));
        }
    }
    let (_, page) = read(&app, &format!("/{slug}/catalog")).await;
    assert_eq!(page.matches("<td class=\"txt-rep\"><i>").count(), 2);
    sqlx::query("UPDATE content.boards SET text_only=false WHERE slug=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    for (query, expected) in [
        ("q=aLpHa", vec![*a]),
        ("q=%5B.*%5D", vec![*a]),
        ("q=%5EAlpha", vec![]),
        ("q=%5E%3Cb%3EAlpha", vec![*a]),
        ("q=%5EBravo%24%7C%5ECrane%24", vec![]),
        (
            "q=%5E%3Cb%3EBravo%3C%2Fb%3E%3A%7C%5E%3Cb%3ECrane%3C%2Fb%3E%3A",
            vec![*c, *b],
        ),
        ("q=%5B.*%5D%24", vec![]),
        ("q=%5B.*%5D%3C%2Fb%3E%3A", vec![*a]),
        ("q=Alpha%5E", vec![]),
        ("q=%5E%24", vec![]),
        (
            "q=%26lt%3Bscript%26gt%3B",
            threads.iter().copied().rev().collect::<Vec<_>>(),
        ),
        ("q=%3Cscript%3E", vec![]),
        ("q=absent", vec![]),
    ] {
        let (status, page) = read(&app, &format!("/{slug}/catalog?order=date&{query}")).await;
        assert_eq!(status, 200);
        assert_eq!(ids(&page), expected, "literal filter {query}");
        let inert = page
            .split("<template id=\"catalogFiltered\">")
            .nth(1)
            .unwrap();
        assert_eq!(
            ids(inert).len() + expected.len(),
            threads.len(),
            "complete public snapshot retained in inert markup"
        );
        assert!(!page.contains("value=\"<script>"));
        if query == "q=absent" {
            assert!(page.contains("No matching threads."));
            assert!(!page.contains("No threads yet."));
        }
    }
    let (_, large) = read(
        &app,
        &format!("/{slug}/catalog?size=large&teaser=off&order=r"),
    )
    .await;
    assert!(large.contains("class=\"catalog large\""));
    assert_eq!(large.matches("class=\"teaser\"").count(), threads.len());
    assert_eq!(
        large
            .matches("<template class=\"catalogTeaser\"><div class=\"teaser\">")
            .count(),
        threads.len()
    );
    assert_eq!(
        large.matches("<template class=\"catalogPreview\">").count(),
        threads.len()
    );
    assert_eq!(
        large.matches("</div></template>").count(),
        threads.len() * 2
    );
    assert!(!large.contains("<script>fold</script>"));
    assert!(large.contains("value=\"r\" selected"));
    assert!(large.contains("value=\"large\" selected"));
    assert!(large.contains("value=\"off\" selected"));
    sqlx::query("UPDATE content.posts SET deleted=true WHERE id=$1")
        .bind(replies[5])
        .execute(&owner)
        .await
        .unwrap();
    let (_, previous) = read(&app, &format!("/{slug}/catalog")).await;
    assert!(preview(&previous, *b).contains(&format!("data-reply-id=\"{}\"", replies[4])));
    assert!(!previous.contains("reply &#38; author"));
    sqlx::query("UPDATE content.posts SET deleted=true WHERE id=ANY($1)")
        .bind(&replies[4..6])
        .execute(&owner)
        .await
        .unwrap();
    let (_, page) = read(&app, &format!("/{slug}/catalog?order=absdate")).await;
    assert_eq!(ids(&page), vec![*s, *a, *c, *b]);
    assert_eq!(
        latest_reply(&page, *b),
        None,
        "deleted replies do not become client sort metadata"
    );
    assert!(!preview(&page, *b).contains("post-last"));
    assert!(!page.contains("reply &#38; author"));
    sqlx::query("UPDATE content.boards SET forced_anon=true WHERE slug=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    let (_, anonymous) = read(&app, &format!("/{slug}/catalog")).await;
    assert!(!anonymous.contains("OP &#38; author"));
    assert!(!anonymous.contains("Visible reply name"));
    assert!(preview(&anonymous, *b).contains("<span class=\"post-author\">Anonymous</span>"));
    assert!(
        preview(&anonymous, *a)
            .contains("Last reply by <span class=\"post-author\">Anonymous</span>")
    );
    sqlx::query("UPDATE content.boards SET forced_anon=false WHERE slug=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    let (_, page) = read(&app, &format!("/{slug}/catalog?order=r")).await;
    assert_eq!(ids(&page), vec![*s, *c, *a, *b]);
    assert!(
        !bump_limited(&page, *b),
        "deleted replies do not count toward the bump limit"
    );
    let (_, json) = read(&app, &format!("/{slug}/thread/{b}.json")).await;
    let json: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(json["posts"][0]["replies"], 0);
    assert!(json["posts"][0].get("bumplimit").is_none());
    for (limit, limited) in [(100, false), (0, true)] {
        sqlx::query("UPDATE content.boards SET bump_limit=$1 WHERE slug=$2")
            .bind(limit)
            .bind(&slug)
            .execute(&owner)
            .await
            .unwrap();
        let (_, page) = read(&app, &format!("/{slug}/catalog")).await;
        assert_eq!(bump_limited(&page, *b), limited);
        let (_, json) = read(&app, &format!("/{slug}/thread/{b}.json")).await;
        let json: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(json["posts"][0]["bumplimit"].as_i64(), limited.then_some(1));
    }
    sqlx::query("UPDATE content.threads SET deleted=true WHERE id=$1")
        .bind(a)
        .execute(&owner)
        .await
        .unwrap();
    let (_, page) = read(&app, &format!("/{slug}/catalog?q=Alpha")).await;
    assert!(ids(&page).is_empty());
    for query in [
        "order=unknown".to_owned(),
        "size=large&size=small".into(),
        "unknown=1".into(),
        "q=%0A".into(),
        format!("q={}", "x".repeat(129)),
        format!("q={}", "%61".repeat(700)),
    ] {
        let (status, body) = read(&app, &format!("/{slug}/catalog?{query}")).await;
        assert_eq!(status, 400, "{query}");
        assert!(body.contains("Invalid catalog options."));
    }
    public.close().await;
}

#[tokio::test]
async fn catalog_options_use_visible_persisted_data_and_escape_literal_filters() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut random = [0_u8; 5];
    OsRng.fill_bytes(&mut random);
    let slug: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Catalog control fixture','Owned synthetic data',1000,100,2,10,10)").bind(&slug).execute(&owner).await.unwrap();
    let result = tokio::spawn(exercise(owner.clone(), public.clone(), slug.clone())).await;
    public.close().await;
    for query in [
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
    owner.close().await;
    result.unwrap();
}

// These are saved public-header fixtures, not a substitute for posting-authority
// tests. Requests below run through the actual board_public database role.
async fn exercise_preview_identities(owner: PgPool, public: PgPool, slug: String) {
    let app = board_public::router(public, "http://127.0.0.1:3000".into(), false);
    // Source catalog.js deliberately uses a different OP label map and reply
    // capitalization. admin_highlight is absent from that OP map (undefined).
    // Verified exists in the source map but is not an allowed persisted capcode.
    let cases = [
        (None, "", ""),
        (Some("admin"), "Administrator", "Admin"),
        (Some("mod"), "Moderator", "Mod"),
        (Some("developer"), "Developer", "Developer"),
        (Some("manager"), "Manager", "Manager"),
        (Some("founder"), "Founder", "Founder"),
        (Some("admin_highlight"), "undefined", "Admin_highlight"),
    ];
    let mut saved = Vec::new();
    for (badge, op_label, reply_label) in cases {
        let thread: i64 =
            sqlx::query_scalar("INSERT INTO content.threads(board) VALUES($1) RETURNING id")
                .bind(&slug)
                .fetch_one(&owner)
                .await
                .unwrap();
        sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Initial','Preview identity','Owned preview body')")
            .bind(thread).bind(&slug).execute(&owner).await.unwrap();
        let reply: i64 = sqlx::query_scalar("INSERT INTO content.posts(board,thread_id,name,subject,comment) VALUES($1,$2,'Initial','','Owned latest body') RETURNING id")
            .bind(&slug).bind(thread).fetch_one(&owner).await.unwrap();
        for id in [thread, reply] {
            sqlx::query("UPDATE content.posts SET name='<b>Owned & author</b>',trip='!ozOtJW9BFA',capcode=$2,country='US',country_name='United States' WHERE id=$1")
                .bind(id).bind(badge).execute(&owner).await.unwrap();
        }
        saved.push((thread, reply, badge, op_label, reply_label));
    }
    // Historical storage permits an empty name without a trip. The source OP
    // falls back to Anonymous, but its latest-reply author is appended verbatim.
    let empty_thread: i64 =
        sqlx::query_scalar("INSERT INTO content.threads(board) VALUES($1) RETURNING id")
            .bind(&slug)
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Initial','Empty saved author','Owned empty author body')")
        .bind(empty_thread).bind(&slug).execute(&owner).await.unwrap();
    let empty_reply: i64 = sqlx::query_scalar("INSERT INTO content.posts(board,thread_id,name,subject,comment) VALUES($1,$2,'Initial','','Owned empty reply body') RETURNING id")
        .bind(&slug).bind(empty_thread).fetch_one(&owner).await.unwrap();
    sqlx::query("UPDATE content.posts SET name='',trip=NULL WHERE id=$1 OR id=$2")
        .bind(empty_thread)
        .bind(empty_reply)
        .execute(&owner)
        .await
        .unwrap();
    // Finish inserts while the fixture board has its default flag policy.
    // Enabling geography below correctly makes later inserts require verified
    // country context, which these saved-header fixtures do not supply.
    let deleted: i64 = sqlx::query_scalar("INSERT INTO content.posts(board,thread_id,name,subject,comment) VALUES($1,$2,'Deleted identity','','Deleted body') RETURNING id")
        .bind(&slug).bind(saved[0].0).fetch_one(&owner).await.unwrap();
    sqlx::query("UPDATE content.posts SET capcode='admin',deleted=true WHERE id=$1")
        .bind(deleted)
        .execute(&owner)
        .await
        .unwrap();
    for text_only in [false, true] {
        for (forced, meta) in [(false, false), (true, false), (false, true), (true, true)] {
            sqlx::query("UPDATE content.boards SET text_only=$2,forced_anon=$3,meta_board=$4,country_flags=true WHERE slug=$1")
                .bind(&slug).bind(text_only).bind(forced).bind(meta).execute(&owner).await.unwrap();
            let (status, page) = read(&app, &format!("/{slug}/catalog")).await;
            assert_eq!(status, 200);
            let (empty_op, empty_last) = preview(&page, empty_thread)
                .split_once("<div class=\"post-last\"")
                .unwrap();
            assert!(empty_op.contains("<span class=\"post-author\">Anonymous</span>"));
            let empty_author = if forced || meta { "Anonymous" } else { "" };
            assert!(empty_last.contains(&format!(
                "data-reply-id=\"{empty_reply}\">Last reply by <span class=\"post-author\">{empty_author}</span>"
            )));
            assert!(!empty_last.contains("post-tripcode"));
            for &(thread, reply, badge, op_label, reply_label) in &saved {
                let rendered = preview(&page, thread);
                let (op, last) = rendered.split_once("<div class=\"post-last\"").unwrap();
                let visible =
                    !(forced || meta) || matches!(badge, Some("admin" | "admin_highlight"));
                let name = if visible {
                    "&#60;b&#62;Owned &#38; author&#60;/b&#62; <span class=\"post-tripcode\">!ozOtJW9BFA</span>"
                } else {
                    "Anonymous"
                };
                let class = badge
                    .map(|badge| format!("{badge}-capcode "))
                    .unwrap_or_default();
                for (part, label) in [(op, op_label), (last, reply_label)] {
                    let label = if badge.is_some() {
                        format!(" ## {label}")
                    } else {
                        String::new()
                    };
                    assert!(
                        part.contains(&format!(
                            "<span class=\"{class}post-author\">{name}{label}</span>"
                        )),
                        "badge={badge:?}, text={text_only}, forced={forced}, meta={meta}: {part}"
                    );
                    assert!(!part.contains("<b>Owned & author</b>"));
                    assert!(!part.contains("postertrip"));
                    assert_eq!(part.contains("!ozOtJW9BFA"), visible);
                }
                assert!(last.contains(&format!("data-reply-id=\"{reply}\"")));
                assert_eq!(
                    op.contains("<div class=\"flag flag-us\"></div>"),
                    badge.is_none()
                );
                assert!(
                    !last.contains("flag-"),
                    "latest replies never display geographic flags"
                );
            }
        }
    }
    let (ordinary, ordinary_reply, ..) = saved[0];
    // A board with flag choices must still show geography when no board flag
    // was selected. A selected board flag is never rendered in catalog hover.
    for text_only in [false, true] {
        for country in [false, true] {
            for choices in [Vec::<String>::new(), vec!["AC".into()]] {
                sqlx::query("UPDATE content.boards SET text_only=$2,forced_anon=false,meta_board=false,country_flags=$3,board_flags=$4 WHERE slug=$1")
                    .bind(&slug).bind(text_only).bind(country).bind(&choices).execute(&owner).await.unwrap();
                let (status, page) = read(&app, &format!("/{slug}/catalog")).await;
                assert_eq!(status, 200);
                assert_eq!(preview(&page, ordinary).contains("flag-us"), country);
                sqlx::query("UPDATE content.posts SET country=NULL,country_name=NULL,board_flag='AC',flag_name='Anarcho-Capitalist' WHERE id=$1")
                    .bind(ordinary).execute(&owner).await.unwrap();
                let (status, page) = read(&app, &format!("/{slug}/catalog")).await;
                assert_eq!(status, 200);
                let rendered = preview(&page, ordinary);
                assert!(!rendered.contains("flag-us"));
                assert!(!rendered.contains("bfl"));
                assert!(!rendered.contains("Anarcho-Capitalist"));
                sqlx::query("UPDATE content.posts SET country='US',country_name='United States',board_flag=NULL,flag_name=NULL WHERE id=$1")
                    .bind(ordinary).execute(&owner).await.unwrap();
            }
        }
    }
    // A deleted newer badge must neither replace the last visible identity nor
    // leave a phantom identity after the last visible reply is deleted too.
    for text_only in [false, true] {
        sqlx::query("UPDATE content.boards SET text_only=$2 WHERE slug=$1")
            .bind(&slug)
            .bind(text_only)
            .execute(&owner)
            .await
            .unwrap();
        let (status, page) = read(&app, &format!("/{slug}/catalog")).await;
        assert_eq!(status, 200);
        assert_eq!(latest_reply(&page, ordinary), Some(ordinary_reply));
        assert!(!preview(&page, ordinary).contains("Deleted identity"));
        assert!(!preview(&page, ordinary).contains("admin-capcode"));
    }
    sqlx::query("UPDATE content.posts SET deleted=true WHERE id=$1")
        .bind(ordinary_reply)
        .execute(&owner)
        .await
        .unwrap();
    for text_only in [false, true] {
        sqlx::query("UPDATE content.boards SET text_only=$2 WHERE slug=$1")
            .bind(&slug)
            .bind(text_only)
            .execute(&owner)
            .await
            .unwrap();
        let (status, page) = read(&app, &format!("/{slug}/catalog")).await;
        assert_eq!(status, 200);
        assert_eq!(latest_reply(&page, ordinary), None);
        assert!(!preview(&page, ordinary).contains("post-last"));
    }
}

#[tokio::test]
async fn catalog_preview_saved_identities_follow_source_badges_suppression_and_flags() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut random = [0_u8; 5];
    OsRng.fill_bytes(&mut random);
    let slug: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Catalog identities','Owned saved public headers',1000,100,100,100,10)")
        .bind(&slug).execute(&owner).await.unwrap();
    let result = tokio::spawn(exercise_preview_identities(
        owner.clone(),
        public.clone(),
        slug.clone(),
    ))
    .await;
    public.close().await;
    for query in [
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
    owner.close().await;
    result.unwrap();
}
