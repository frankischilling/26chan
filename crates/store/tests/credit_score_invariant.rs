#![cfg(feature = "database-tests")]
use board_domain::{anonymous_session::State, challenge_policy::credit_grant};
use serde_json::json;
use sqlx::PgPool;

#[tokio::test]
async fn private_credit_profile_implies_the_pre_post_numeric_score_bypass() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let now = 10_000_000;
    let mut cases = Vec::new();
    let mut granted = [false; 3];
    for password_age in [0, 21599, 21600, 259199, 259200, 1296000, 7776000] {
        for network_age in [0, 1799, 1800, 21599, 21600, 259200] {
            for posts in [4, 5, 9, 20, 100] {
                for change_score in [0, 9, 10] {
                    for pending in [0, 1] {
                        let state = State {
                            created_at: now - password_age,
                            network_at: now - network_age,
                            address_at: now - 60,
                            activity_at: now,
                            posts,
                            change_score,
                            pending,
                            ..State::new(now)
                        };
                        let grant = credit_grant(&state, now);
                        if let Some(grant) = grant {
                            granted[usize::from(grant.count) - 1] = true;
                        }
                        cases.push(
                            json!({"password_age":password_age,"network_age":network_age,
                            "posts":posts,"change_score":change_score,"pending":pending,
                            "credits":grant.map_or(0,|grant|grant.count),
                            "known_for_score":state.is_known_or_verified(now,360,0)}),
                        );
                    }
                }
            }
        }
    }
    assert_eq!(cases.len(), 1260);
    assert!(granted.into_iter().all(|value| value));
    let mut tx = owner.begin().await.unwrap();
    let token: Vec<u8> =
        sqlx::query_scalar("SELECT sha256(convert_to(gen_random_uuid()::text,'UTF8'))")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    sqlx::query("INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,created_at,network_at,address_at,environment_at,expires_at) SELECT $1,$1,$1,$1,n,n,n,n,n+3600 FROM (SELECT extract(epoch FROM clock_timestamp())::bigint n) t")
        .bind(&token).execute(&mut *tx).await.unwrap();
    sqlx::query("SELECT set_config('owned.credit_score_cases',$1,true),set_config('owned.credit_score_token',encode($2::bytea,'hex'),true)")
        .bind(serde_json::to_string(&cases).unwrap()).bind(&token).execute(&mut *tx).await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(r#"DO $owned$
DECLARE c record; profile record; n bigint;
 token bytea:=decode(current_setting('owned.credit_score_token'),'hex');
BEGIN
 FOR c IN SELECT * FROM jsonb_to_recordset(current_setting('owned.credit_score_cases')::jsonb)
  AS x(password_age bigint,network_age bigint,posts smallint,change_score smallint,pending smallint,credits smallint,known_for_score boolean) LOOP
  -- Rebase each case on database time so a slow runner cannot turn the
  -- profile's request-freshness check into the property being tested.
  n:=extract(epoch FROM clock_timestamp())::bigint;
  UPDATE post_secrets.anonymous_sessions SET created_at=n-c.password_age,network_at=n-c.network_age,
   address_at=n-60,environment_at=n,activity_at=n,action_at=0,verified_level=0,
   posts=c.posts,images=0,threads=0,reports=0,pending=c.pending,change_score=c.change_score,
   expires_at=n+3600 WHERE token_hash=token;
  SELECT * INTO profile FROM verification_private.credit_profile(token,token,token,token,n);
  IF profile.credit_count IS DISTINCT FROM c.credits THEN
   RAISE EXCEPTION 'Private credit profile differs from the qualified domain tiers'; END IF;
  IF profile.credit_count>0 AND c.known_for_score IS DISTINCT FROM true THEN
   RAISE EXCEPTION 'A courtesy grant required a non-bypassed numeric request score'; END IF;
 END LOOP;
END $owned$"#).execute(&mut *tx).await.unwrap();
    tx.rollback().await.unwrap();
    owner.close().await;
}
