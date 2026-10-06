#![cfg(feature = "database-tests")]

#[path = "support/posting.rs"]
mod posting_fixture;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use board_domain::wordfiltered_comment::PreparedComment;
use sqlx::PgPool;
use tower::ServiceExt;

async fn get(app: &Router, path: &str) -> String {
    let response = app
        .clone()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK, "{path}");
    String::from_utf8(
        to_bytes(response.into_body(), 1_048_576)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}
async fn fixture() -> (PgPool, PgPool, String) {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let slug: String =
        sqlx::query_scalar("SELECT 'wf'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(posting_reply_seconds,posting_image_seconds,posting_thread_seconds,slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,comment_code_spacing,comment_spoiler_cleanup,comment_sjis_spacing,op_markup,rss_enabled,word_filter_enabled) VALUES(0,0,0,$1,'Owned wordfilters','Owned wordfilter HTTP fixture',16000,100,100,100,10,true,true,true,true,true,true)").bind(&slug).execute(&owner).await.unwrap();
    (owner, public, slug)
}
async fn cleanup(owner: &PgPool, slug: &str) {
    for query in [
        "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(query).bind(slug).execute(owner).await.unwrap();
    }
}

#[tokio::test]
async fn caller_cleanup_precedes_filters_and_markup_without_repeating_the_early_pass() {
    let (owner, public, slug) = fixture().await;
    // This cleanup-order matrix retains twenty same-actor OPs. Increase only
    // its owned board's capacity, preserving history and all filter assertions.
    sqlx::query("UPDATE content.boards SET user_thread_limit=100 WHERE slug=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    let (a, p, b) = (owner.clone(), public.clone(), slug.clone());
    let outcome = tokio::spawn(async move {
        let reference: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/wordfilter-posting-reference.json"
        ))
        .unwrap();
        let app = posting_fixture::router(p.clone(), &b, "http://127.0.0.1:3000".into(), false);
        for (profile, source) in [(0, "global"), (1, "ck"), (2, "asp"), (3, "v"), (4, "test")] {
            sqlx::query("UPDATE content.boards SET word_filter_profile=$2 WHERE slug=$1")
                .bind(&b)
                .bind(profile as i16)
                .execute(&a)
                .await
                .unwrap();
            for input in [
                "fa~?rep?~m so~?erep?~y CU~?rep?~CK",
                "[co~?rep?~de]soy fam CUCK[/code]",
                "~?re~?rep?~p?~fam",
                "s\u{1c82}y \u{1c89}soy soy\u{1c89}\u{1c89} \u{1ccf0}soy",
            ] {
                let fields = url::form_urlencoded::Serializer::new(String::new())
                    .append_pair("com", input)
                    .append_pair("password", "owned-caller-order-password")
                    .finish();
                let response = app
                    .clone()
                    .oneshot(
                        Request::post(format!("/{b}/imgboard.php"))
                            .header("origin", "http://127.0.0.1:3000")
                            .header("accept", "application/json")
                            .header("content-type", "application/x-www-form-urlencoded")
                            .body(Body::from(fields))
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::OK);
                let posted: serde_json::Value =
                    serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap())
                        .unwrap();
                assert!(posted.get("error").is_none(), "{source}: {input}: {posted}");
                let saved = board_store::find_post(&p, &b, posted["pid"].as_i64().unwrap())
                    .await
                    .unwrap();
                let typed =
                    PreparedComment::decode(saved.wordfilter_payload.as_deref().unwrap()).unwrap();
                let rolls = typed
                    .rolls()
                    .map(|rolls| serde_json::json!(rolls.choices()))
                    .unwrap_or(serde_json::json!([null, null]));
                let expected = reference["profiles"][source]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|case| case["input"] == input && case["rolls"] == rolls)
                    .unwrap();
                assert_eq!(
                    saved.comment,
                    expected["final"].as_str().unwrap(),
                    "{source}: {input}"
                );
            }
        }
        let before: i64 = sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=$1")
            .bind(&b)
            .fetch_one(&a)
            .await
            .unwrap();
        let fields = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("com", "~?rep?~[spoiler] [/spoiler]~?erep?~")
            .append_pair("password", "owned-caller-order-password")
            .finish();
        let response = app
            .oneshot(
                Request::post(format!("/{b}/post"))
                    .header("origin", "http://127.0.0.1:3000")
                    .header("accept", "application/json")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(fields))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let rejected: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
        assert_eq!(
            rejected,
            serde_json::json!({"error": "Error: New threads require a subject or comment."})
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM content.posts WHERE board=$1")
                .bind(&b)
                .fetch_one(&a)
                .await
                .unwrap(),
            before
        );
    })
    .await;
    cleanup(&owner, &slug).await;
    outcome.unwrap();
}

#[tokio::test]
async fn original_test_board_keeps_its_source_filter_while_generic_fixtures_stay_unfiltered() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let source_board = board_store::board(&public, "test").await.unwrap();
    assert!(source_board.word_filter_enabled);
    assert_eq!(source_board.word_filter_profile, 4);
    assert!(
        !board_store::board(&public, "fixture")
            .await
            .unwrap()
            .word_filter_enabled
    );
    let app = posting_fixture::router(
        public.clone(),
        "test",
        "http://127.0.0.1:3000".into(),
        false,
    );
    let input = "ordinary text fam soy CUCK finna pcfat";
    let fields = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("com", input)
        .append_pair("password", "owned-original-wordfilter-password")
        .finish();
    let response = app
        .oneshot(
            Request::post("/test/imgboard.php")
                .header("origin", "http://127.0.0.1:3000")
                .header("accept", "application/json")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(fields))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let result: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    let id = result["pid"].as_i64().unwrap();
    let saved = board_store::find_post(&public, "test", id).await.unwrap();
    for query in [
        "DELETE FROM post_secrets.deletion WHERE post_id=$1",
        "DELETE FROM content.posts WHERE id=$1 AND board='test'",
        "DELETE FROM content.threads WHERE id=$1 AND board='test'",
    ] {
        sqlx::query(query).bind(id).execute(&owner).await.unwrap();
    }
    posting_fixture::cleanup_actor_posting(
        &owner,
        "test",
        &posting_fixture::key("test"),
        posting_fixture::peer(),
    )
    .await;
    let typed = PreparedComment::decode(saved.wordfilter_payload.as_deref().unwrap()).unwrap();
    let rolls = typed.rolls().unwrap().choices();
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/wordfilter-posting-reference.json"
    ))
    .unwrap();
    let expected = fixture["profiles"]["test"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["input"] == input && case["rolls"] == serde_json::json!(rolls))
        .unwrap();
    assert_eq!(saved.comment, expected["final"].as_str().unwrap());
}

#[tokio::test]
async fn negotiated_posts_render_the_same_source_result_on_pages_json_updater_catalog_rss_and_search()
 {
    let (owner, public, slug) = fixture().await;
    let (a, p, b) = (owner.clone(), public.clone(), slug.clone());
    let outcome = tokio::spawn(async move {
        let fixture: serde_json::Value = serde_json::from_str(include_str!("../../../fixtures/wordfilter-posting-reference.json")).unwrap();
        let (app, api) = posting_fixture::routers(p.clone(), &b, "http://127.0.0.1:3000".into(), false);
        let input = "[code]soy fam CUCK[/code]";
        for (profile, source) in [(0,"global"),(1,"ck"),(2,"asp"),(3,"v"),(4,"test")] {
            sqlx::query("UPDATE content.boards SET word_filter_profile=$2 WHERE slug=$1").bind(&b).bind(profile as i16).execute(&a).await.unwrap();
            let fields = url::form_urlencoded::Serializer::new(String::new()).append_pair("name","soy fam CUCK").append_pair("sub","soy fam CUCK").append_pair("com",input).append_pair("password","owned-wordfilter-http-password").finish();
            let response = app.clone().oneshot(Request::post(format!("/{b}/post")).header("origin","http://127.0.0.1:3000").header("accept","application/json").header("content-type","application/x-www-form-urlencoded").body(Body::from(fields)).unwrap()).await.unwrap();
            assert_eq!(response.status(),StatusCode::OK);
            let posted: serde_json::Value = serde_json::from_slice(&to_bytes(response.into_body(),65536).await.unwrap()).unwrap();
            let id = posted["pid"].as_i64().unwrap();
            let saved = board_store::find_post(&p,&b,id).await.unwrap();
            let typed = PreparedComment::decode(saved.wordfilter_payload.as_deref().unwrap()).unwrap();
            let rolls = typed.rolls().map(|rolls| rolls.choices());
            let case = fixture["profiles"][source].as_array().unwrap().iter().find(|case| case["input"]==input && match rolls {
                None => case["rolls"]==serde_json::json!([null,null]),
                Some(rolls) => case["rolls"]==serde_json::json!(rolls),
            }).unwrap();
            let expected = case["final"].as_str().unwrap();
            assert_eq!(saved.comment,expected);
            assert_eq!(saved.name,"soy fam CUCK"); assert_eq!(saved.subject,"soy fam CUCK");
            let page = get(&app,&format!("/{b}/thread/{id}")).await;
            assert!(page.contains(&format!("id=\"m{id}\">{expected}</blockquote>")),"{page}");
            for router in [&app,&api] {
                let json: serde_json::Value=serde_json::from_str(&get(router,&format!("/{b}/thread/{id}.json")).await).unwrap();
                assert_eq!(json["posts"][0]["com"],expected);
                for private in ["wordfilter_payload","wordfilter_search","word_filter_profile","comment_format"] { assert!(json["posts"][0].get(private).is_none()); }
            }
            let snapshot: serde_json::Value=serde_json::from_str(&get(&app,&format!("/_watch/{b}/thread/{id}/posts")).await).unwrap();
            assert!(snapshot["posts"][0]["html"].as_str().unwrap().contains(&format!("id=\"m{id}\">{expected}</blockquote>")));
            let catalog: serde_json::Value=serde_json::from_str(&get(&api,&format!("/{b}/catalog.json")).await).unwrap();
            let entry=catalog.as_array().unwrap().iter().flat_map(|page| page["threads"].as_array().unwrap()).find(|post| post["no"]==id).unwrap();
            assert_eq!(entry["com"],expected);
            let feed=get(&app,&format!("/{b}/index.rss")).await;
            let xml=roxmltree::Document::parse(&feed).unwrap();
            let item=xml.descendants().find(|node| node.has_tag_name("item") && node.children().any(|child| child.has_tag_name("guid") && child.text().is_some_and(|text| text.ends_with(&format!("/{id}"))))).unwrap();
            let description=item.children().find(|node| node.has_tag_name("description")).unwrap().text().unwrap();
            assert!(description.contains(expected),"{description}");
            let search=url::form_urlencoded::Serializer::new(String::new()).append_pair("q",&board_domain::formatting::plain_text(&saved.formatted_lines())).append_pair("b",&b).finish();
            let hits: serde_json::Value=serde_json::from_str(&get(&app,&format!("/search/api?{search}")).await).unwrap();
            let search_id=id.to_string();
            assert!(hits["threads"].as_array().unwrap().iter().any(|thread| thread["thread"].as_str()==Some(search_id.as_str()) && thread["posts"].as_array().unwrap().iter().any(|post| post["html"].as_str().unwrap().contains(expected))));
            sqlx::query("UPDATE content.boards SET word_filter_enabled=false,comment_code_spacing=false WHERE slug=$1").bind(&b).execute(&a).await.unwrap();
            assert!(get(&app,&format!("/{b}/thread/{id}")).await.contains(&format!("id=\"m{id}\">{expected}</blockquote>")));
            assert_eq!(board_store::find_post(&p,&b,id).await.unwrap().wordfilter_payload,saved.wordfilter_payload);
            sqlx::query("UPDATE content.boards SET word_filter_enabled=true,comment_code_spacing=true WHERE slug=$1").bind(&b).execute(&a).await.unwrap();
        }
    }).await;
    cleanup(&owner, &slug).await;
    outcome.unwrap();
}

#[tokio::test]
async fn late_normalized_search_matches_and_literal_script_text_stay_bounded_and_inert() {
    let (owner, public, slug) = fixture().await;
    let (a, p, b) = (owner.clone(), public.clone(), slug.clone());
    let outcome=tokio::spawn(async move {
        let app=posting_fixture::router(p.clone(), &b,"http://127.0.0.1:3000".into(),false);
        let comment=format!("{} [code]{} https://boards.4chan.org/g/thread/42 target~?rep?~needle <script>literal</script> {}[/code]", "prefix ".repeat(230),"code prefix ".repeat(150),"tail ".repeat(200));
        let id=posting_fixture::create_post(&p,&b,0,&board_store::NewPost{name:"Anonymous".into(),subject:"Owned late search".into(),comment,deletion_hash:"owned".into(),sage:false}).await.unwrap();
        let text: String=sqlx::query_scalar("SELECT wordfilter_search FROM content.posts WHERE id=$1").bind(id).fetch_one(&a).await.unwrap();
        assert!(text.contains(">>>/g/42 targetneedle <script>literal</script>"));
        for query in [">>>/g/42","targetneedle","<script>literal</script>"] {
            let params=url::form_urlencoded::Serializer::new(String::new()).append_pair("q",query).append_pair("b",&b).finish();
            let hits: serde_json::Value=serde_json::from_str(&get(&app,&format!("/search/api?{params}")).await).unwrap();
            assert_eq!(hits["threads"].as_array().unwrap().len(),1);
            let html=hits["threads"][0]["posts"][0]["html"].as_str().unwrap();
            assert!(html.len()<8000,"excerpt must stay bounded");
            assert!(html.contains("targetneedle")); assert!(html.contains("&#60;script&#62;literal&#60;/script&#62;"));
            assert!(!html.contains("<script>literal")); assert!(html.contains("<pre class=\"prettyprint\">")); assert!(html.contains("</pre>"));
        }
    }).await;
    cleanup(&owner, &slug).await;
    outcome.unwrap();
}
