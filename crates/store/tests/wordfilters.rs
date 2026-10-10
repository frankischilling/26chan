#![cfg(feature = "database-tests")]
mod support;

use board_domain::wordfiltered_comment::PreparedComment;
use board_store::{NewPost, PostIdentityKeys, PostMetadata, PostingContext, StoreError};
use chrono::Utc;
use sqlx::PgPool;

fn post(comment: &str) -> NewPost {
    NewPost {
        name: "soy fam CUCK".into(),
        subject: "soy fam CUCK".into(),
        comment: comment.into(),
        deletion_hash: "owned-wordfilter-deletion-hash".into(),
        sage: false,
    }
}

async fn fixture() -> (PgPool, PgPool, String) {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let slug: String =
        sqlx::query_scalar("SELECT substr(replace(gen_random_uuid()::text,'-',''),1,10)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,comment_spoiler_cleanup,comment_code_spacing,comment_sjis_spacing,op_markup,posting_reply_seconds,posting_image_seconds,posting_thread_seconds) VALUES($1,'Owned wordfilters','Synthetic',2000,100,100,100,10,true,true,true,true,0,0,0)").bind(&slug).execute(&owner).await.unwrap();
    (owner, public, slug)
}

async fn cleanup(owner: &PgPool, slug: &str) {
    for query in [
        "DELETE FROM content.post_media WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(query).bind(slug).execute(owner).await.unwrap();
    }
}

async fn counts(owner: &PgPool, slug: &str) -> (i64, i64, i64) {
    sqlx::query_as("SELECT (SELECT count(*) FROM content.posts WHERE board=$1),(SELECT count(*) FROM content.threads WHERE board=$1),(SELECT count(*) FROM post_secrets.deletion d JOIN content.posts p ON p.id=d.post_id WHERE p.board=$1)").bind(slug).fetch_one(owner).await.unwrap()
}

#[tokio::test]
async fn board_reads_bound_saved_expansion_and_only_count_selected_post_slots() {
    use board_store::BoardSelection::{All, Page};
    let (owner, public, slug) = fixture().await;
    let (a, p, b) = (owner.clone(), public.clone(), slug.clone());
    let outcome = tokio::spawn(async move {
        sqlx::query("UPDATE content.boards SET word_filter_enabled=true,max_comment_chars=16000,threads_per_page=1 WHERE slug=$1")
            .bind(&b).execute(&a).await.unwrap();
        let large = support::create_post(&p, &b, 0, &post(&"𠮷".repeat(16_000))).await.unwrap();
        assert!(matches!(board_store::board_snapshot(&p, &b, All, Some(0)).await, Err(StoreError::ReadLimit)));
        assert_eq!(board_store::board_snapshot(&p, &b, All, Some(5)).await.unwrap().threads[0].posts[0].id, large);
        let small = support::create_post(&p, &b, 0, &post("owned small thread")).await.unwrap();
        let before = counts(&a, &b).await;
        assert_eq!(board_store::board_snapshot(&p, &b, Page(1), Some(0)).await.unwrap().threads[0].posts[0].id, small);
        assert!(matches!(board_store::board_page_snapshot(&p, &b, Page(2), Some(0)).await, Err(StoreError::ReadLimit)));
        assert!(matches!(board_store::board_snapshot(&p, &b, All, Some(0)).await, Err(StoreError::ReadLimit)));
        let metadata = board_store::board_snapshot(&p, &b, All, None).await.unwrap();
        assert_eq!(metadata.threads.len(), 2);
        assert!(metadata.threads.iter().all(|thread| thread.posts.is_empty()));
        assert_eq!(counts(&a, &b).await, before);
    }).await;
    cleanup(&owner, &slug).await;
    outcome.unwrap();
}

#[tokio::test]
async fn selected_thread_body_budgets_are_checked_before_rows_transfer() {
    let (owner, public, slug) = fixture().await;
    let (a, p, b) = (owner.clone(), public.clone(), slug.clone());
    let outcome = tokio::spawn(async move {
        sqlx::query("UPDATE content.boards SET word_filter_enabled=true,json_tail_size=1 WHERE slug=$1")
            .bind(&b).execute(&a).await.unwrap();
        let id = support::create_post(&p, &b, 0, &post("soy fam CUCK")).await.unwrap();
        for _ in 0..2 {
            support::create_post(&p, &b, id, &post(&"owned ".repeat(100))).await.unwrap();
        }
        let before = counts(&a, &b).await;
        let bytes: i64 = sqlx::query_scalar("SELECT sum(octet_length(comment)+octet_length(wordfilter_payload)+octet_length(wordfilter_search))::bigint FROM content.posts WHERE board=$1 AND thread_id=$2")
            .bind(&b).bind(id).fetch_one(&a).await.unwrap();
        assert!(matches!(board_store::thread_snapshot_selection_bounded(&p, &b, id, false, bytes as usize - 1).await,
            Err(StoreError::ReadLimit)));
        let full = board_store::thread_snapshot_selection_bounded(&p, &b, id, false, bytes as usize).await.unwrap();
        assert_eq!(full.posts.len(), 3);
        let tail = board_store::thread_snapshot_selection(&p, &b, id, true).await.unwrap();
        let tail_bytes: usize = tail.posts.iter().map(|post| post.comment.len() + post.wordfilter_payload.as_ref().unwrap().len()
            + board_domain::formatting::plain_text(&post.formatted_lines()).len()).sum();
        assert!(tail_bytes < bytes as usize);
        assert!(matches!(board_store::thread_snapshot_selection_bounded(&p, &b, id, true, tail_bytes - 1).await,
            Err(StoreError::ReadLimit)));
        assert_eq!(board_store::thread_snapshot_selection_bounded(&p, &b, id, true, tail_bytes).await.unwrap().posts.len(), 2);
        for limit in [0, board_store::MAX_THREAD_READ_BYTES + 1] {
            assert!(matches!(board_store::thread_snapshot_selection_bounded(&p, &b, id, false, limit).await,
                Err(StoreError::Invalid(_))));
        }
        assert_eq!(counts(&a, &b).await, before);
    }).await;
    cleanup(&owner, &slug).await;
    outcome.unwrap();
}

#[tokio::test]
async fn source_profiles_field_scope_once_only_results_and_atomic_failure_are_persisted() {
    let (owner, public, slug) = fixture().await;
    // This profile matrix retains eight same-actor OPs to check saved results.
    // Increase capacity only on its owned board, preserving posting history.
    sqlx::query("UPDATE content.boards SET user_thread_limit=100 WHERE slug=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    let (a, p, b) = (owner.clone(), public.clone(), slug.clone());
    let outcome=tokio::spawn(async move {
        let inventory: (i64,i64)=sqlx::query_as("SELECT count(*) FILTER (WHERE word_filter_enabled),count(*) FILTER (WHERE NOT word_filter_enabled) FROM content.boards WHERE source_order<1000").fetch_one(&a).await.unwrap();
        assert_eq!(inventory,(79,3));
        let profiles: Vec<(String,i16)>=sqlx::query_as("SELECT slug,word_filter_profile FROM content.boards WHERE slug=ANY($1) ORDER BY slug").bind(vec!["ck","int","asp","v","test"]).fetch_all(&a).await.unwrap();
        assert_eq!(profiles,vec![("asp".into(),2),("ck".into(),1),("int".into(),1),("test".into(),4),("v".into(),3)]);
        let input="ordinary text fam soy CUCK finna pcfat";
        let id=support::create_post(&p,&b,0,&post(input)).await.unwrap();
        let disabled=board_store::find_post(&p,&b,id).await.unwrap();
        assert_eq!(disabled.comment,input); assert!(disabled.wordfilter_payload.is_none());
        // Expected strings were extracted from the hash-pinned PHP files.
        for (profile,expected) in [
            (0,"ordinary text senpai onions KEK finna pcfat"),
            (1,"ordinary text senpai soy KEK finna pcfat"),
            (2,"ordinary text senpai onions KEK ding-dong diddly pcfat"),
            (3,"ordinary text senpai onions KEK finna pcbro"),
        ] {
            sqlx::query("UPDATE content.boards SET word_filter_enabled=true,word_filter_profile=$2 WHERE slug=$1").bind(&b).bind(profile as i16).execute(&a).await.unwrap();
            let id=support::create_post(&p,&b,0,&post(input)).await.unwrap();
            let saved=board_store::find_post(&p,&b,id).await.unwrap();
            assert_eq!(saved.comment,expected); assert_eq!(saved.name,"soy fam CUCK"); assert_eq!(saved.subject,"soy fam CUCK");
            let typed=PreparedComment::decode(saved.wordfilter_payload.as_deref().unwrap()).unwrap();
            assert!(typed.rolls().is_none());
            assert_eq!(board_domain::filtered_formatting::source_projection(&saved.formatted_lines()),expected);
        }
        sqlx::query("UPDATE content.boards SET word_filter_profile=4 WHERE slug=$1").bind(&b).execute(&a).await.unwrap();
        let id=support::create_post(&p,&b,0,&post("[code]soy fam CUCK[/code] <script>& \"")).await.unwrap();
        let saved=board_store::find_post(&p,&b,id).await.unwrap();
        let typed=PreparedComment::decode(saved.wordfilter_payload.as_deref().unwrap()).unwrap();
        assert!(typed.rolls().unwrap().choices().iter().all(|choice| *choice<=5));
        let projection=board_domain::filtered_formatting::source_projection(&saved.formatted_lines());
        assert_eq!(projection,saved.comment);
        for _ in 0..16 {
            let current=board_store::find_post(&p,&b,id).await.unwrap();
            assert_eq!(current.wordfilter_payload,saved.wordfilter_payload); assert_eq!(current.comment,projection);
        }
        sqlx::query("UPDATE content.boards SET word_filter_enabled=false,word_filter_profile=0,comment_code_spacing=false,comment_spoiler_cleanup=false,op_markup=false WHERE slug=$1").bind(&b).execute(&a).await.unwrap();
        let unchanged=board_store::find_post(&p,&b,id).await.unwrap();
        assert_eq!(board_domain::filtered_formatting::source_projection(&unchanged.formatted_lines()),projection);
        let next=support::create_post(&p,&b,0,&post(input)).await.unwrap();
        assert_eq!(board_store::find_post(&p,&b,next).await.unwrap().comment,input);
        sqlx::query("UPDATE content.boards SET word_filter_enabled=true,word_filter_profile=2 WHERE slug=$1").bind(&b).execute(&a).await.unwrap();
        let expanded=support::create_post(&p,&b,0,&post(&"finna".repeat(400))).await.unwrap();
        let expanded=board_store::find_post(&p,&b,expanded).await.unwrap();
        assert_eq!(expanded.comment,"ding-dong diddly".repeat(400));
        let before=counts(&a,&b).await;
        assert!(matches!(support::create_post(&p,&b,0,&post(&"finna".repeat(401))).await,Err(StoreError::Invalid(_))));
        assert_eq!(counts(&a,&b).await,before);
        let mut tx=p.begin().await.unwrap();
        sqlx::query("SELECT set_config('board.wordfilter_payload','bad',true)").execute(&mut *tx).await.unwrap();
        let id: i64=sqlx::query_scalar("SELECT nextval('content.post_number')").fetch_one(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO content.threads(id,board) VALUES($1,$2)").bind(id).bind(&b).execute(&mut *tx).await.unwrap();
        let error=sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous','Owned','Missing typed result')").bind(id).bind(&b).execute(&mut *tx).await.unwrap_err();
        assert_eq!(error.as_database_error().unwrap().code().as_deref(),Some("23514"));
        tx.rollback().await.unwrap(); assert_eq!(counts(&a,&b).await,before);
        for query in ["UPDATE content.boards SET word_filter_enabled=false WHERE false","UPDATE content.posts SET wordfilter_payload=NULL WHERE false","UPDATE content.posts SET wordfilter_search=NULL WHERE false","INSERT INTO content.posts(id,board,thread_id,name,subject,comment,wordfilter_payload) VALUES(0,'invalid',0,'','','',NULL)","SELECT staff_identity.issue_wordfiltered_post_authority(NULL,NULL,NULL,60,false,0,'',0,'','','',clock_timestamp(),NULL,NULL)"] {
            let error=sqlx::query(query).execute(&p).await.unwrap_err();
            assert_eq!(error.as_database_error().unwrap().code().as_deref(),Some("42501"),"{query}");
        }
    }).await;
    cleanup(&owner, &slug).await;
    outcome.unwrap();
}

#[tokio::test]
async fn locked_policy_changes_and_filtered_robot_duplicates_have_atomic_ordering() {
    let (owner, public, slug) = fixture().await;
    let (a, p, b) = (owner.clone(), public.clone(), slug.clone());
    let outcome=tokio::spawn(async move {
        sqlx::query("UPDATE content.boards SET word_filter_enabled=true,word_filter_profile=0 WHERE slug=$1").bind(&b).execute(&a).await.unwrap();
        let mut tx=a.begin().await.unwrap();
        sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE").bind(&b).fetch_one(&mut *tx).await.unwrap();
        let (sender,receiver)=tokio::sync::oneshot::channel();
        let (blocked_pool,blocked_board)=(p.clone(),b.clone());
        let waiting=tokio::spawn(async move { sender.send(()).unwrap(); support::create_post(&blocked_pool,&blocked_board,0,&post("soy fam CUCK")).await });
        receiver.await.unwrap(); tokio::time::sleep(std::time::Duration::from_millis(100)).await; assert!(!waiting.is_finished());
        sqlx::query("UPDATE content.boards SET word_filter_profile=1 WHERE slug=$1").bind(&b).execute(&mut *tx).await.unwrap(); tx.commit().await.unwrap();
        let id=waiting.await.unwrap().unwrap(); assert_eq!(board_store::find_post(&p,&b,id).await.unwrap().comment,"soy senpai KEK");
        sqlx::query("UPDATE content.boards SET word_filter_profile=0,robot9000=true,thread_limit=1 WHERE slug=$1").bind(&b).execute(&a).await.unwrap();
        let key=support::key(&b);
        for (index,comment) in ["CUCK original comment","KEK original comment"].into_iter().enumerate() {
            let result=support::create_post_with_metadata(&p,&b,0,&post(comment),None,PostingContext { request_start:Utc::now(), peer:Some("192.0.2.23".parse().unwrap()),op_password_proof:None },PostMetadata { drawing: None, spoiler: false,keys:PostIdentityKeys {tripcode:None,poster_id:Some(&key)},country_database:None,flag:"",options:""}).await;
            if index==0 { assert!(result.is_ok()); }
            else { assert!(matches!(result,Err(StoreError::Robot9000Rejected(ref message)) if message=="You have been muted for 2 seconds, because your comment was not original.")); }
        }
        let state:(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM content.posts WHERE board=$1),(SELECT count(*) FROM post_secrets.robot9000_texts WHERE board=$1),(SELECT count(*) FROM post_secrets.robot9000_mutes WHERE board=$1)").bind(&b).fetch_one(&a).await.unwrap();
        assert_eq!(state,(2,1,1));
        let visible:i64=sqlx::query_scalar("SELECT count(*) FROM content.visible_threads WHERE board=$1").bind(&b).fetch_one(&p).await.unwrap(); assert_eq!(visible,1);
        let result=board_store::search(&p,"KEK original",Some(&b),0).await.unwrap();
        assert_eq!(result.threads.len(),1);
        assert_eq!(board_domain::filtered_formatting::source_projection(&result.threads[0].posts[0].formatted_lines()),"KEK original comment");
    }).await;
    cleanup(&owner, &slug).await;
    outcome.unwrap();
}
