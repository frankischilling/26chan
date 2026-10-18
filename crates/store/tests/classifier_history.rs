#![cfg(feature = "database-tests")]
use sqlx::PgPool;

#[tokio::test]
async fn private_history_is_bounded_scoped_and_uses_a_strict_cutoff() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = PgPool::connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(r#"DO $owned$
DECLARE token bytea:=sha256(convert_to(gen_random_uuid()::text,'UTF8'));
 other bytea:=sha256(convert_to(gen_random_uuid()::text,'UTF8'));
 n bigint; h record; original bigint; state_before jsonb; state_after jsonb;
BEGIN
 n:=floor(extract(epoch FROM clock_timestamp()))::bigint;
 INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,
 created_at,network_at,address_at,environment_at,expires_at)
 VALUES(token,token,token,token,n-60,n-60,n-60,n-60,n+3600);
 SELECT to_jsonb(s) INTO state_before FROM post_secrets.anonymous_sessions s WHERE token_hash=token;
 SELECT * INTO h FROM post_secrets.read_classifier_history(token,false);
 IF h.edge_blocked OR h.country_blocked THEN RAISE EXCEPTION 'Unexpected history'; END IF;
 IF NOT post_secrets.record_classifier_history(token,'block_bm_bot') THEN RAISE EXCEPTION 'First event missing'; END IF;
 SELECT created_at INTO original FROM post_secrets.classifier_history WHERE token_hash=token;
 IF post_secrets.record_classifier_history(token,'block_bm_bot') THEN RAISE EXCEPTION 'Cooldown was extended'; END IF;
 IF (SELECT created_at FROM post_secrets.classifier_history WHERE token_hash=token)<>original THEN RAISE EXCEPTION 'Timestamp changed'; END IF;
 SELECT * INTO h FROM post_secrets.read_classifier_history(token,false);
 IF NOT h.edge_blocked OR h.country_blocked THEN RAISE EXCEPTION 'Kind scope lost'; END IF;
 IF NOT post_secrets.record_classifier_history(token,'country_changed') THEN RAISE EXCEPTION 'Country event missing'; END IF;
 IF (SELECT count(*) FROM post_secrets.classifier_history WHERE token_hash=token)<>2 THEN RAISE EXCEPTION 'Bad bounded cardinality'; END IF;
 SELECT * INTO h FROM post_secrets.read_classifier_history(other,true);
 IF h.edge_blocked OR h.country_blocked THEN RAISE EXCEPTION 'Session scope lost'; END IF;
 BEGIN PERFORM post_secrets.read_classifier_history(other,false); RAISE EXCEPTION 'Missing session accepted';
 EXCEPTION WHEN SQLSTATE '28000' THEN NULL; END;
 BEGIN PERFORM post_secrets.read_classifier_history(token,true); RAISE EXCEPTION 'Minted collision accepted';
 EXCEPTION WHEN SQLSTATE '28000' THEN NULL; END;
 BEGIN PERFORM post_secrets.record_classifier_history(token,'freeform'); RAISE EXCEPTION 'Unknown kind accepted';
 EXCEPTION WHEN check_violation THEN NULL; END;
 -- Retry if this tight boundary check crosses a whole second. This tests the
 -- reader's own database clock instead of accepting a caller-controlled clock.
 FOR attempt IN 1..20 LOOP
  n:=floor(extract(epoch FROM clock_timestamp()))::bigint;
  UPDATE post_secrets.classifier_history SET created_at=n-86400 WHERE token_hash=token;
  SELECT * INTO h FROM post_secrets.read_classifier_history(token,false);
  IF h.edge_blocked OR h.country_blocked THEN RAISE EXCEPTION 'Inclusive cutoff'; END IF;
  UPDATE post_secrets.classifier_history SET created_at=n-86399 WHERE token_hash=token;
  SELECT * INTO h FROM post_secrets.read_classifier_history(token,false);
  IF floor(extract(epoch FROM clock_timestamp()))::bigint=n THEN
   IF NOT h.edge_blocked OR NOT h.country_blocked THEN RAISE EXCEPTION 'Inside cutoff lost'; END IF;
   EXIT;
  END IF;
  IF attempt=20 THEN RAISE EXCEPTION 'Boundary clock did not stabilize'; END IF;
 END LOOP;
 UPDATE post_secrets.classifier_history SET created_at=n+100 WHERE token_hash=token;
 SELECT * INTO h FROM post_secrets.read_classifier_history(token,false);
 IF NOT h.edge_blocked OR NOT h.country_blocked THEN RAISE EXCEPTION 'Invented upper cutoff'; END IF;
 SELECT to_jsonb(s) INTO state_after FROM post_secrets.anonymous_sessions s WHERE token_hash=token;
 IF state_before IS DISTINCT FROM state_after THEN RAISE EXCEPTION 'History advanced activity'; END IF;
 BEGIN PERFORM post_secrets.read_classifier_history(decode('00','hex'),true); RAISE EXCEPTION 'Short token accepted';
 EXCEPTION WHEN check_violation THEN NULL; END;
 BEGIN PERFORM post_secrets.read_classifier_history(token,NULL); RAISE EXCEPTION 'Missing provenance accepted';
 EXCEPTION WHEN check_violation THEN NULL; END;
 UPDATE post_secrets.anonymous_sessions SET expires_at=n-1 WHERE token_hash=token;
 BEGIN PERFORM post_secrets.read_classifier_history(token,false); RAISE EXCEPTION 'Expired session accepted';
 EXCEPTION WHEN SQLSTATE '28000' THEN NULL; END;
 DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=token;
 IF EXISTS(SELECT 1 FROM post_secrets.classifier_history WHERE token_hash=token) THEN RAISE EXCEPTION 'Session erasure left history'; END IF;
END $owned$"#).execute(&mut *tx).await.unwrap();
    tx.rollback().await.unwrap();
    for query in [
        "SELECT * FROM post_secrets.classifier_history",
        "SELECT * FROM post_secrets.read_classifier_history(decode(repeat('01',32),'hex'),true)",
        "SELECT post_secrets.record_classifier_history(decode(repeat('01',32),'hex'),'block_bm_bot')",
        "SELECT post_secrets.classifier_history_ready()",
    ] {
        let error = sqlx::query(query).execute(&public).await.unwrap_err();
        assert_eq!(
            error.as_database_error().and_then(|e| e.code()).as_deref(),
            Some("42501")
        );
    }
    public.close().await;
    owner.close().await;
    private_history_serializes_same_session_writers().await;
    private_history_catalog_rejects_shape_permission_and_function_drift().await;
}

async fn private_history_catalog_rejects_shape_permission_and_function_drift() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    for change in [
        "ALTER TABLE post_secrets.classifier_history ADD COLUMN unexpected boolean",
        "GRANT CREATE ON SCHEMA post_secrets TO board_anonymous_owner",
        "GRANT SELECT ON post_secrets.classifier_history TO board_public",
        "ALTER TABLE post_secrets.classifier_history DROP CONSTRAINT classifier_history_kind_check",
    ] {
        let mut tx = owner.begin().await.unwrap();
        sqlx::query(change).execute(&mut *tx).await.unwrap();
        sqlx::query("SET LOCAL ROLE board_anonymous_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        assert!(
            !sqlx::query_scalar::<_, bool>("SELECT post_secrets.classifier_history_ready()")
                .fetch_one(&mut *tx)
                .await
                .unwrap()
        );
        let error = sqlx::query("SELECT * FROM post_secrets.read_classifier_history(decode(repeat('00',32),'hex'),true)").execute(&mut *tx).await.unwrap_err();
        assert_eq!(
            error.as_database_error().and_then(|e| e.code()).as_deref(),
            Some("P197U")
        );
        tx.rollback().await.unwrap();
    }
    for change in [
        "ALTER FUNCTION post_secrets.read_classifier_history(bytea,boolean) COST 99",
        "ALTER FUNCTION post_secrets.read_classifier_history(bytea,boolean) SET search_path=public,pg_temp",
        "GRANT EXECUTE ON FUNCTION post_secrets.read_classifier_history(bytea,boolean) TO board_staff",
    ] {
        let mut tx = owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_anonymous_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query(change).execute(&mut *tx).await.unwrap();
        assert!(
            !sqlx::query_scalar::<_, bool>("SELECT post_secrets.classifier_history_ready()")
                .fetch_one(&mut *tx)
                .await
                .unwrap()
        );
        tx.rollback().await.unwrap();
    }
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    assert!(
        sqlx::query_scalar::<_, bool>("SELECT post_secrets.classifier_history_ready()")
            .fetch_one(&mut *tx)
            .await
            .unwrap()
    );
    tx.rollback().await.unwrap();
    for role in [
        "board_public",
        "board_staff",
        "board_auth",
        "board_verifier",
        "board_mailer",
        "board_media",
        "board_media_read",
        "board_media_intake",
        "board_monitor",
        "board_moderation_retention",
    ] {
        let allowed: bool = sqlx::query_scalar("SELECT has_table_privilege($1,'post_secrets.classifier_history','SELECT,INSERT,UPDATE,DELETE') OR has_table_privilege($1,'post_secrets.classifier_history_catalog','SELECT,INSERT,UPDATE,DELETE') OR has_function_privilege($1,'post_secrets.classifier_history_ready()','EXECUTE') OR has_function_privilege($1,'post_secrets.classifier_history_catalog_hash()','EXECUTE') OR has_function_privilege($1,'post_secrets.read_classifier_history(bytea,boolean)','EXECUTE') OR has_function_privilege($1,'post_secrets.record_classifier_history(bytea,text)','EXECUTE')")
            .bind(role).fetch_one(&owner).await.unwrap();
        assert!(!allowed, "Unexpected runtime authority for {role}");
    }
    owner.close().await;
}

async fn private_history_serializes_same_session_writers() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let token: Vec<u8> =
        sqlx::query_scalar("SELECT sha256(convert_to(gen_random_uuid()::text,'UTF8'))")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,created_at,network_at,address_at,environment_at,expires_at) SELECT $1,$1,$1,$1,n,n,n,n,n+3600 FROM (SELECT floor(extract(epoch FROM clock_timestamp()))::bigint n) t")
        .bind(&token).execute(&owner).await.unwrap();
    let mut held = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *held)
        .await
        .unwrap();
    sqlx::query("SELECT * FROM post_secrets.read_classifier_history($1,false)")
        .bind(&token)
        .execute(&mut *held)
        .await
        .unwrap();
    let mut second = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *second)
        .await
        .unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *second)
        .await
        .unwrap();
    let second_token = token.clone();
    let worker = tokio::spawn(async move {
        let created: bool =
            sqlx::query_scalar("SELECT post_secrets.record_classifier_history($1,'block_bm_bot')")
                .bind(second_token)
                .fetch_one(&mut *second)
                .await
                .unwrap();
        second.commit().await.unwrap();
        created
    });
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut blocked = false;
    while tokio::time::Instant::now() < deadline {
        blocked = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE pid=$1 AND wait_event_type='Lock')",
        )
        .bind(pid)
        .fetch_one(&owner)
        .await
        .unwrap();
        if blocked {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let first: bool =
        sqlx::query_scalar("SELECT post_secrets.record_classifier_history($1,'block_bm_bot')")
            .bind(&token)
            .fetch_one(&mut *held)
            .await
            .unwrap();
    held.commit().await.unwrap();
    let second_created = tokio::time::timeout(std::time::Duration::from_secs(5), worker)
        .await
        .unwrap()
        .unwrap();
    let rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM post_secrets.classifier_history WHERE token_hash=$1",
    )
    .bind(&token)
    .fetch_one(&owner)
    .await
    .unwrap();
    sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
        .bind(&token)
        .execute(&owner)
        .await
        .unwrap();
    owner.close().await;
    assert!(blocked, "Second issuer never reached the session lock wait");
    assert!(first);
    assert!(
        !second_created,
        "The waiter missed the preceding committed event"
    );
    assert_eq!(rows, 1);
}
