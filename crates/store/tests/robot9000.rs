#![cfg(feature = "database-tests")]

use board_domain::poster_id::PosterIdKey;
use board_store::{NewPost, PostIdentityKeys, PostMetadata, PostingContext, StoreError};
use chrono::Utc;
use sqlx::PgPool;
use std::net::{IpAddr, Ipv4Addr};

fn key() -> PosterIdKey {
    PosterIdKey::parse(&"12".repeat(32)).unwrap()
}

async fn create(
    pool: &PgPool,
    board: &str,
    parent: i64,
    comment: &str,
    actor: u8,
    options: &str,
    attachment: Option<&board_store::post_media::NewAttachment>,
) -> Result<i64, StoreError> {
    let key = key();
    board_store::create_post_with_metadata(
        pool,
        board,
        parent,
        &NewPost {
            name: "Anonymous".into(),
            subject: "Owned Robot9000".into(),
            comment: comment.into(),
            deletion_hash: "owned-not-a-real-password".into(),
            sage: false,
        },
        attachment,
        PostingContext {
            request_start: Utc::now(),
            peer: Some(IpAddr::V4(Ipv4Addr::new(192, 0, 2, actor))),
            op_password_proof: None,
        },
        PostMetadata {
            keys: PostIdentityKeys {
                tripcode: None,
                poster_id: Some(&key),
            },
            country_database: None,
            flag: "",
            options,
        },
    )
    .await
}

async fn fixture() -> (PgPool, PgPool, String) {
    let admin = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT substr(replace(gen_random_uuid()::text,'-',''),1,10)")
            .fetch_one(&admin)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Owned Robot9000','Synthetic',2000,100,100,100,10)")
        .bind(&board).execute(&admin).await.unwrap();
    (admin, public, board)
}

async fn cleanup(admin: &PgPool, board: &str) {
    sqlx::query("DELETE FROM content.post_media WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)")
        .bind(board).execute(admin).await.unwrap();
    sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)")
        .bind(board).execute(admin).await.unwrap();
    sqlx::query("DELETE FROM content.posts WHERE board=$1")
        .bind(board)
        .execute(admin)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.threads WHERE board=$1")
        .bind(board)
        .execute(admin)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.boards WHERE slug=$1")
        .bind(board)
        .execute(admin)
        .await
        .unwrap();
}

async fn state(admin: &PgPool, board: &str) -> (i64, i64, i64, i64) {
    sqlx::query_as("SELECT (SELECT count(*) FROM content.posts WHERE board=$1),(SELECT count(*) FROM content.threads WHERE board=$1),(SELECT count(*) FROM post_secrets.robot9000_texts WHERE board=$1),(SELECT count(*) FROM post_secrets.robot9000_mutes WHERE board=$1)")
        .bind(board).fetch_one(admin).await.unwrap()
}

fn rejection(result: Result<i64, StoreError>) -> String {
    match result {
        Err(StoreError::Robot9000Rejected(message)) => message,
        other => panic!("Expected Robot9000 rejection, got {other:?}"),
    }
}

#[tokio::test]
async fn originality_mutes_decay_and_rejections_are_persisted_atomically() {
    let (admin, public, board) = fixture().await;
    let (a, p, b) = (admin.clone(), public.clone(), board.clone());
    let outcome = tokio::spawn(async move {
        // Disabled boards ignore the plugin and its would-be bypass option.
        for _ in 0..2 {
            create(&p,&b,0,"ordinary original comment",1,"bypass_r9k",None).await.unwrap();
        }
        assert_eq!(state(&a,&b).await,(2,2,0,0));
        sqlx::query("UPDATE content.boards SET robot9000=true WHERE slug=$1")
            .bind(&b).execute(&a).await.unwrap();
        let first = create(&p,&b,0,"ordinary original comment",1,"",None).await.unwrap();
        assert_eq!(state(&a,&b).await,(3,3,1,0));
        let before: String = sqlx::query_scalar("SELECT to_jsonb(t)::text FROM content.threads t WHERE id=$1")
            .bind(first).fetch_one(&a).await.unwrap();
        let message = rejection(create(&p,&b,first,"ORDINARY, original comment!!!",1,"bypass_r9k",None).await);
        assert_eq!(message,"You have been muted for 2 seconds, because your comment was not original.");
        assert_eq!(state(&a,&b).await,(3,3,1,1));
        let after: String = sqlx::query_scalar("SELECT to_jsonb(t)::text FROM content.threads t WHERE id=$1")
            .bind(first).fetch_one(&a).await.unwrap();
        assert_eq!(before,after,"Rejected reply must not change counts, bumps or clocks");
        let fingerprint=key().robot9000_fingerprint(&b,"192.0.2.1".parse().unwrap()).unwrap();
        sqlx::query("UPDATE post_secrets.robot9000_mutes SET mute_until=date_trunc('second',clock_timestamp())+interval '60 seconds',next_expire=date_trunc('second',clock_timestamp())+interval '60 seconds' WHERE board=$1 AND actor=$2")
            .bind(&b).bind(fingerprint.as_slice()).execute(&a).await.unwrap();
        let unchanged: String=sqlx::query_scalar("SELECT to_jsonb(m)::text FROM post_secrets.robot9000_mutes m WHERE board=$1 AND actor=$2")
            .bind(&b).bind(fingerprint.as_slice()).fetch_one(&a).await.unwrap();
        let active = rejection(create(&p,&b,first,"an entirely different fresh comment",1,"",None).await);
        assert!(active.starts_with("You're muted! You cannot post until "));
        assert!(active.ends_with(" from now"));
        let current: String=sqlx::query_scalar("SELECT to_jsonb(m)::text FROM post_secrets.robot9000_mutes m WHERE board=$1 AND actor=$2")
            .bind(&b).bind(fingerprint.as_slice()).fetch_one(&a).await.unwrap();
        assert_eq!(current,unchanged,"Active mute must not extend or register new text");
        assert!(matches!(create(&p,&b,first,"café",1,"",None).await,Err(StoreError::Invalid("Non-ASCII text is not allowed."))));
        assert_eq!(state(&a,&b).await,(3,3,1,1));

        // Invalid targets and attachment capabilities cannot change history or mutes.
        assert!(matches!(create(&p,&b,i64::MAX-1,"ordinary original comment",2,"",None).await,Err(StoreError::NotFound)));
        let bad_attachment=board_store::post_media::NewAttachment {
            upload: board_store::media_intake::IntakeReservation { id:"0".repeat(32),capability:"0".repeat(64) },spoiler:false,
        };
        assert!(matches!(create(&p,&b,first,"ordinary original comment",2,"",Some(&bad_attachment)).await,Err(StoreError::Invalid(_) | StoreError::Conflict(_) | StoreError::NotFound)));
        assert_eq!(state(&a,&b).await,(3,3,1,1));

        // A further violation doubles the existing power even when decay is due.
        sqlx::query("UPDATE post_secrets.robot9000_mutes SET mute_until=clock_timestamp()-interval '1 second',next_expire=clock_timestamp()-interval '1 second' WHERE board=$1 AND actor=$2")
            .bind(&b).bind(fingerprint.as_slice()).execute(&a).await.unwrap();
        assert_eq!(rejection(create(&p,&b,first,"ordinary original comment",1,"",None).await),"You have been muted for 4 seconds, because your comment was not original.");
        sqlx::query("UPDATE post_secrets.robot9000_mutes SET mute_until=clock_timestamp()-interval '1 second',next_expire=clock_timestamp()-interval '10 days',timeout_power=6 WHERE board=$1 AND actor=$2")
            .bind(&b).bind(fingerprint.as_slice()).execute(&a).await.unwrap();
        create(&p,&b,first,"success after many days only decays once",1,"",None).await.unwrap();
        let (power, future):(i16,bool)=sqlx::query_as("SELECT timeout_power,next_expire>clock_timestamp()+interval '23 hours' FROM post_secrets.robot9000_mutes WHERE board=$1 AND actor=$2")
            .bind(&b).bind(fingerprint.as_slice()).fetch_one(&a).await.unwrap();
        assert_eq!((power,future),(5,true));
        create(&p,&b,first,"another unique successful comment",1,"",None).await.unwrap();
        let power:i16=sqlx::query_scalar("SELECT timeout_power FROM post_secrets.robot9000_mutes WHERE board=$1 AND actor=$2")
            .bind(&b).bind(fingerprint.as_slice()).fetch_one(&a).await.unwrap();
        assert_eq!(power,5);
        sqlx::query("UPDATE post_secrets.robot9000_mutes SET timeout_power=24,mute_until=clock_timestamp()-interval '1 second',next_expire=clock_timestamp()-interval '1 second' WHERE board=$1 AND actor=$2")
            .bind(&b).bind(fingerprint.as_slice()).execute(&a).await.unwrap();
        assert_eq!(rejection(create(&p,&b,first,"ordinary original comment",1,"",None).await),"You have been muted for 52 weeks 1 day, because your comment was not original.");
        let (power,seconds):(i16,i64)=sqlx::query_as("SELECT timeout_power,extract(epoch FROM mute_until-date_trunc('second',clock_timestamp()))::bigint FROM post_secrets.robot9000_mutes WHERE board=$1 AND actor=$2")
            .bind(&b).bind(fingerprint.as_slice()).fetch_one(&a).await.unwrap();
        assert_eq!(power,24);
        assert!((31_535_998..=31_536_000).contains(&seconds));
        assert_eq!(rejection(create(&p,&b,0,"!!!!!!!!!!!a",3,"",None).await),"You have been muted for 2 seconds, because your comment was too low in content (8.33% content).");
        assert_eq!(state(&a,&b).await,(5,3,3,2));

        // Concurrent identical posts have exactly one accepted transaction.
        let (one,two)=tokio::join!(
            create(&p,&b,0,"concurrent originality fixture",4,"",None),
            create(&p,&b,0,"concurrent originality fixture",5,"",None),
        );
        assert_eq!(usize::from(one.is_ok())+usize::from(two.is_ok()),1);
        let rejected=if one.is_err(){one}else{two};
        assert_eq!(rejection(rejected),"You have been muted for 2 seconds, because your comment was not original.");
        assert_eq!(state(&a,&b).await,(6,4,4,3));
        board_store::delete_post(&p,&b,first).await.unwrap();
        assert_eq!(rejection(create(&p,&b,0,"ordinary original comment",6,"",None).await),"You have been muted for 2 seconds, because your comment was not original.");
        assert_eq!(state(&a,&b).await.2,4,"Deleted posts retain originality history");
    }).await;
    cleanup(&admin, &board).await;
    outcome.unwrap();
}

#[tokio::test]
async fn private_state_capacity_and_missing_identity_fail_closed() {
    let (admin, public, board) = fixture().await;
    let (a, p, b) = (admin.clone(), public.clone(), board.clone());
    let outcome=tokio::spawn(async move {
        sqlx::query("UPDATE content.boards SET robot9000=true,robot9000_state_limit=1,thread_limit=1 WHERE slug=$1")
            .bind(&b).execute(&a).await.unwrap();
        let input=NewPost {name:"Anonymous".into(),subject:"Owned".into(),comment:"first original comment".into(),deletion_hash:"owned".into(),sage:false};
        assert!(matches!(board_store::create_post(&p,&b,0,&input).await,Err(StoreError::Invalid("Robot9000 identity is unavailable."))));
        assert_eq!(state(&a,&b).await,(0,0,0,0));
        let first=create(&p,&b,0,&input.comment,1,"",None).await.unwrap();
        assert!(matches!(create(&p,&b,0,"a different original text",2,"",None).await,Err(StoreError::Database(_))));
        assert_eq!(state(&a,&b).await,(1,1,1,0));
        rejection(create(&p,&b,0,&input.comment,2,"",None).await);
        board_store::find_post(&p,&b,first).await.expect("Rejected rollover must retain the original visible thread");
        assert!(matches!(create(&p,&b,first,&input.comment,3,"",None).await,Err(StoreError::Database(_))));
        assert_eq!(state(&a,&b).await,(1,1,1,1));
        for query in [
            "SELECT * FROM post_secrets.robot9000_texts",
            "SELECT * FROM post_secrets.robot9000_mutes",
            "DELETE FROM post_secrets.robot9000_mutes WHERE false",
            "UPDATE content.boards SET robot9000=false WHERE false",
            "SET ROLE board_robot9000_owner",
        ] {
            let error=sqlx::query(query).execute(&p).await.unwrap_err();
            assert_eq!(error.as_database_error().unwrap().code().as_deref(),Some("42501"),"{query}");
        }
        for actor in [vec![0_u8;31],vec![0_u8;33]] {
            let error=sqlx::query("SELECT * FROM content.check_robot9000($1,$2,$3,NULL,date_trunc('second',clock_timestamp()))")
                .bind(&b).bind(actor).bind(vec![1_u8;32]).execute(&p).await.unwrap_err();
            assert_eq!(error.as_database_error().unwrap().code().as_deref(),Some("23514"));
        }
        sqlx::query("UPDATE content.boards SET staff_only=true WHERE slug=$1")
            .bind(&b).execute(&a).await.unwrap();
        assert!(matches!(create(&p,&b,0,"private board remains invisible",4,"",None).await,Err(StoreError::NotFound)));
        let error=sqlx::query("SELECT * FROM content.check_robot9000($1,$2,$3,NULL,date_trunc('second',clock_timestamp()))")
            .bind(&b).bind(vec![2_u8;32]).bind(vec![3_u8;32]).execute(&p).await.unwrap_err();
        assert_eq!(error.as_database_error().unwrap().code().as_deref(),Some("23514"));
    }).await;
    cleanup(&admin, &board).await;
    outcome.unwrap();
}

#[tokio::test]
async fn rejected_posts_do_not_consume_an_approved_attachment() {
    let (admin, public, board) = fixture().await;
    let intake = board_store::media_intake::IntakeStore::connect(
        &std::env::var("INTAKE_DATABASE_URL").unwrap(),
    )
    .await
    .unwrap();
    let queue =
        board_store::media::MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
            .await
            .unwrap();
    let upload = intake.reserve("owned-robot9000.png").await.unwrap();
    let job = upload.id.clone();
    let (a, p, b) = (admin.clone(), public.clone(), board.clone());
    let outcome = tokio::spawn(async move {
        intake
            .begin_upload(&upload.id, &upload.capability)
            .await
            .unwrap();
        intake
            .finish_upload(&upload.id, &upload.capability, 100)
            .await
            .unwrap();
        let claim = queue.claim().await.unwrap().unwrap();
        assert_eq!(claim.id, upload.id, "Requires an idle owned media queue");
        let token = claim.lease_token.unwrap();
        let output = queue
            .prepare_output(
                &claim.id,
                &token,
                &board_store::media_assets::OutputMetadata {
                    sha256: "a".repeat(64),
                    bytes: 123,
                    width: 10,
                    height: 20,
                },
            )
            .await
            .unwrap();
        queue
            .approve_output(&claim.id, &token, &output.id)
            .await
            .unwrap();
        let attachment = board_store::post_media::NewAttachment {
            upload,
            spoiler: false,
        };
        sqlx::query("UPDATE content.boards SET robot9000=true,image_limit=100 WHERE slug=$1")
            .bind(&b)
            .execute(&a)
            .await
            .unwrap();
        let first = create(&p, &b, 0, "original attachment transaction", 1, "", None)
            .await
            .unwrap();
        rejection(
            create(
                &p,
                &b,
                first,
                "original attachment transaction",
                2,
                "",
                Some(&attachment),
            )
            .await,
        );
        assert_eq!(state(&a, &b).await, (1, 1, 1, 1));
        assert!(matches!(
            create(&p, &b, first, "", 3, "", Some(&attachment)).await,
            Err(StoreError::Invalid("Textless posts are not allowed."))
        ));
        board_store::post_media::check_upload(
            &p,
            &attachment.upload.id,
            &attachment.upload.capability,
        )
        .await
        .unwrap();
        let accepted = create(
            &p,
            &b,
            first,
            "new original attached reply",
            4,
            "",
            Some(&attachment),
        )
        .await
        .unwrap();
        assert_eq!(
            board_store::post_media::attachment(&p, accepted)
                .await
                .unwrap()
                .unwrap()
                .asset_id,
            output.id
        );
        assert_eq!(state(&a, &b).await, (2, 1, 2, 1));
    })
    .await;
    cleanup(&admin, &board).await;
    sqlx::query("DELETE FROM media.assets WHERE job_id=$1")
        .bind(&job)
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query("DELETE FROM media.jobs WHERE id=$1")
        .bind(&job)
        .execute(&admin)
        .await
        .unwrap();
    outcome.unwrap();
}
