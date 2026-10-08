use super::*;
use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use board_domain::anonymous_session::Capability;
use board_store::{PublicDeletionContext, anonymous_session::PostingSession};
use chrono::{Duration as ChronoDuration, Timelike, Utc};
use rand_core::{OsRng, RngCore};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{sync::Arc, time::Duration};

#[tokio::test]
async fn captured_batch_session_clock_survives_a_delayed_first_deletion() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = PgPoolOptions::new()
        .max_connections(1)
        .connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let seed: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&owner)
        .await
        .unwrap();
    let board = format!("lc{seed}");
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,deletion_known_min_seconds,deletion_unknown_min_seconds,deletion_max_seconds,posting_reply_seconds,posting_image_seconds,posting_thread_seconds) VALUES($1,'Batch clock','Owned fixture',200,100,100,100,10,60,600,86400,0,0,0)")
        .bind(&board).execute(&owner).await.unwrap();
    let capability = Capability::generate().unwrap();
    let fingerprints = capability.fingerprints(None, *b"XX");
    let token = fingerprints.token;
    let run_owner = owner.clone();
    let run_public = public.clone();
    let run_board = board.clone();
    let result = tokio::spawn(async move {
        let mut key_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut key_bytes);
        let key_text: String = key_bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        let key = Arc::new(board_domain::poster_id::PosterIdKey::parse(&key_text).unwrap());
        let peer = std::net::IpAddr::V6(std::net::Ipv6Addr::new(
            0x2001, 0xdb8, 9, 0, 0, 0, (seed >> 16) as u16, seed as u16,
        ));
        let password = "owned-batch-clock-password";
        let hash = Argon2::default().hash_password(password.as_bytes(),&SaltString::generate(&mut OsRng)).unwrap().to_string();
        let mut ids = Vec::new();
        for _ in 0..2 {
            ids.push(board_store::create_post_with_identity_keys(&run_public,&run_board,0,&board_store::NewPost {
                name: "Anonymous".into(),subject: "Owned clock fixture".into(),comment: "Keep the younger selection".into(),deletion_hash: hash.clone(),sage: false,
            }, None, board_store::PostingContext {
                request_start: Utc::now(), peer: Some(peer), op_password_proof: None,
            }, board_store::PostIdentityKeys { tripcode: None, poster_id: Some(&key) }).await.unwrap());
        }
        let now = Utc::now().with_nanosecond(0).unwrap();
        // Inject a trusted internal capture clock to model a 100-second first
        // operation delay without wall-clock sleeps or client-controlled fields.
        let captured = now-ChronoDuration::seconds(100);
        sqlx::query("INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,created_at,network_at,address_at,environment_at,activity_at,expires_at) VALUES($1,$2,$3,$4,$5-1000,$5-899,$5-1000,$5-1000,$5,$5+31536000)")
            .bind(token.as_slice()).bind(fingerprints.network.as_slice()).bind(fingerprints.address.as_slice()).bind(fingerprints.environment.as_slice()).bind(captured.timestamp()).execute(&run_owner).await.unwrap();
        for (id,age) in [(ids[0],650),(ids[1],300)] {
            sqlx::query("UPDATE content.posts SET created_at=$2 WHERE id=$1")
                .bind(id).bind(now-ChronoDuration::seconds(age)).execute(&run_owner).await.unwrap();
        }
        let context = PublicDeletionContext {
            request_start: captured,
            session: Some(PostingSession { fingerprints,minted:false,now:captured }),
        };
        let mut batch = board_store::PublicDeletionBatch::new(
            &run_board, context, key.public_deletion_rate_identity(peer),
        );
        let state = AppState {
            pool: run_public.clone(),origin:"http://127.0.0.1:3000".into(),production:false,
            limits: Arc::new(crate::security::Limits::new(board_config::PublicRequestLimits::default())),
            media:None,proxy_uid:None,poster_id_key:Some(key.clone()),tripcode_key:None,country_database:None,
        };
        let public_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&run_public).await.unwrap();
        let mut blocker = run_owner.begin().await.unwrap();
        sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
            .bind(&run_board).execute(&mut *blocker).await.unwrap();
        let delete_state = state.clone();
        let selected = ids.clone();
        let deleting = tokio::spawn(async move {
            delete_selection(&delete_state,&mut batch,Deletion {
                posts:selected,password:password.into(),file_only:false,
            }).await
        });
        tokio::time::timeout(Duration::from_secs(5),async {
            loop {
                let waiting: bool = sqlx::query_scalar("SELECT cardinality(pg_blocking_pids($1))>0")
                    .bind(public_pid).fetch_one(&run_owner).await.unwrap();
                if waiting { break; }
                assert!(!deleting.is_finished(),"First deletion must reach the held board lock");
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.expect("First deletion reaches the lock");
        blocker.commit().await.unwrap();
        let response = deleting.await.unwrap();
        assert_eq!(response.status(),StatusCode::FORBIDDEN);
        let body = axum::body::to_bytes(response.into_body(),65536).await.unwrap();
        assert!(std::str::from_utf8(&body).unwrap().contains("Error: You must wait longer before deleting this post."));
        let deleted: Vec<bool> = sqlx::query_scalar("SELECT deleted FROM content.posts WHERE id=ANY($1) ORDER BY id")
            .bind(&ids).fetch_all(&run_owner).await.unwrap();
        // The first succeeds using current lower-age time (650, not captured
        // 550 seconds). The second remains unknown despite wall network age999.
        assert_eq!(deleted,[true,false]);

        let refreshed = PublicDeletionContext {
            request_start:Utc::now(),
            session:Some(PostingSession { now:Utc::now(),..context.session.unwrap() }),
        };
        let mut next_batch = board_store::PublicDeletionBatch::new(
            &run_board, refreshed, key.public_deletion_rate_identity(peer),
        );
        assert!(handlers::delete_with_context(&state,&mut next_batch,DeleteForm {
            no:ids[1],password:password.into(),file_only:false,
        }).await.is_ok(),"A new request is now known; the batch must not refresh its capture clock");
    }).await;
    for statement in [
        "DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(statement)
            .bind(&board)
            .execute(&owner)
            .await
            .unwrap();
    }
    sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
        .bind(token.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    public.close().await;
    owner.close().await;
    result.unwrap();
}
