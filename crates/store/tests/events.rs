#![cfg(feature = "database-tests")]
use board_domain::{
    poster_id::PosterIdKey,
    source_events::{
        credentials::Kind,
        emotes,
        market::{Order, Side},
    },
};
use board_store::events;
use serde_json::json;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::sync::{Arc, Mutex};

async fn login(name: &str, role: &str) -> PgPool {
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&std::env::var(name).expect("owned test database URL"))
        .await
        .unwrap();
    let users: (String, String) = sqlx::query_as("SELECT current_user::text,session_user::text")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(users, (role.into(), role.into()));
    pool
}
async fn now(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT floor(extract(epoch FROM clock_timestamp()))::bigint")
        .fetch_one(pool)
        .await
        .unwrap()
}
async fn next_id(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn private_event_accounts_keep_source_rules_atomic_and_scoped() {
    let owner = login("MIGRATION_DATABASE_URL", "board_migrator").await;
    let public = login("TEST_PUBLIC_DATABASE_URL", "board_public").await;
    let mut lock = owner.acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock(2240251)")
        .execute(&mut *lock)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM event_private.runs WHERE enabled")
            .fetch_one(&owner)
            .await
            .unwrap(),
        0,
        "Use a disposable database without active operator events"
    );
    let ids = [next_id(&owner).await, next_id(&owner).await];
    let timestamp = now(&owner).await;
    for (id, kind) in [(ids[0], 1_i16), (ids[1], 2_i16)] {
        sqlx::query("INSERT INTO event_private.runs(id,kind,enabled,starts_at,ends_at,account_capacity) VALUES($1,$2,true,$3,$4,2)")
   .bind(id).bind(kind).bind(timestamp-3600).bind(timestamp+3600).execute(&owner).await.unwrap();
    }
    sqlx::query("INSERT INTO event_private.quotes(run_id,stock,price) VALUES($1,0,100)")
        .bind(ids[0])
        .execute(&owner)
        .await
        .unwrap();
    let owned_ids = Arc::new(Mutex::new(ids.to_vec()));
    let result = tokio::spawn(check_accounts(
        owner.clone(),
        public.clone(),
        timestamp,
        owned_ids.clone(),
    ))
    .await;
    let cleanup_ids = owned_ids.lock().unwrap().clone();
    sqlx::query("DELETE FROM event_private.runs WHERE id=ANY($1)")
        .bind(&cleanup_ids)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("SELECT pg_advisory_unlock(2240251)")
        .execute(&mut *lock)
        .await
        .unwrap();
    result.unwrap();
}

async fn check_accounts(o: PgPool, p: PgPool, timestamp: i64, owned_ids: Arc<Mutex<Vec<i64>>>) {
    assert!(
        sqlx::query_scalar::<_, bool>("SELECT content.events_ready()")
            .fetch_one(&p)
            .await
            .unwrap()
    );
    let market = events::active_run(&p, Kind::Market).await.unwrap().unwrap();
    let emote = events::active_run(&p, Kind::Emotes).await.unwrap().unwrap();
    let key = PosterIdKey::parse(&"4".repeat(64)).unwrap().event_key();
    let alice = key
        .issue_for_peer(market, "192.0.2.1".parse().unwrap(), timestamp)
        .unwrap();
    let bob = key
        .issue_for_peer(market, "192.0.2.2".parse().unwrap(), timestamp)
        .unwrap();
    let third = key
        .issue_for_peer(market, "192.0.2.3".parse().unwrap(), timestamp)
        .unwrap();
    assert!(events::account(&p, market, &alice).await.unwrap().is_none());
    assert_eq!(
        events::open_account(&p, market, &alice, false)
            .await
            .unwrap()
            .unwrap(),
        json!({"balance":1000})
    );
    assert_eq!(
        events::open_account(&p, market, &bob, true)
            .await
            .unwrap()
            .unwrap(),
        json!({"balance":1000})
    );
    assert_eq!(
        events::open_account(&p, market, &third, false)
            .await
            .unwrap()
            .unwrap()
            .get("error")
            .unwrap(),
        "Event account capacity reached"
    );
    assert!(events::account(&p, market, &third).await.unwrap().is_none());
    assert_eq!(
        events::open_account(&p, market, &alice, false)
            .await
            .unwrap()
            .unwrap(),
        json!({"balance":1000})
    );
    assert!(events::account(&p, emote, &alice).await.is_err());
    let order = Order::parse(Side::Buy, "PEPE", "7", "100").unwrap();
    assert_eq!(
        events::market_trade(&p, market, &alice, order)
            .await
            .unwrap()
            .unwrap()["error"],
        "You can only make an order once every 10 seconds"
    );
    sqlx::query(
        "UPDATE event_private.accounts SET last_action_at=$3 WHERE run_id=$1 AND actor_hash=$2",
    )
    .bind(market.id())
    .bind(alice.storage_hash().as_slice())
    .bind(timestamp - 100)
    .execute(&o)
    .await
    .unwrap();
    let (first, second) = tokio::join!(
        events::market_trade(&p, market, &alice, order),
        events::market_trade(&p, market, &alice, order)
    );
    let results = [first.unwrap().unwrap(), second.unwrap().unwrap()];
    assert_eq!(
        results
            .iter()
            .filter(|value| value.get("error").is_none())
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|value| value["error"] == "Your account balance is too low")
            .count(),
        1
    );
    assert_eq!(
        events::account(&p, market, &alice).await.unwrap().unwrap(),
        json!({"balance":300,"PEPE":7})
    );
    assert_eq!(
        events::account(&p, market, &bob).await.unwrap().unwrap(),
        json!({"balance":1000})
    );
    assert_eq!(
        events::market_trade(
            &p,
            market,
            &bob,
            Order::parse(Side::Buy, "PEPE", "1", "101").unwrap()
        )
        .await
        .unwrap()
        .unwrap()["error"],
        "The price has changed."
    );
    let ea = key
        .issue_for_peer(emote, "192.0.2.1".parse().unwrap(), timestamp)
        .unwrap();
    let eb = key
        .issue_for_peer(emote, "192.0.2.2".parse().unwrap(), timestamp)
        .unwrap();
    assert_ne!(alice.storage_hash(), ea.storage_hash());
    assert_eq!(
        events::open_account(&p, emote, &ea, true)
            .await
            .unwrap()
            .unwrap()["balance"],
        100
    );
    assert_eq!(
        events::open_account(&p, emote, &eb, false)
            .await
            .unwrap()
            .unwrap()["balance"],
        0
    );
    let (first, second) = tokio::join!(
        events::buy_emote(&p, emote, &ea, "happy"),
        events::buy_emote(&p, emote, &ea, "happy")
    );
    let results = [first.unwrap().unwrap(), second.unwrap().unwrap()];
    assert_eq!(
        results
            .iter()
            .filter(|value| value["status"] == "success")
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|value| value["msg"] == "You already have that emote")
            .count(),
        1
    );
    let account = events::account(&p, emote, &ea).await.unwrap().unwrap();
    assert_eq!(account["balance"], 50);
    assert_eq!(account["owned"], json!([["happy", 1, "&#x1F600;"]]));
    let refused = events::roll_emotes(&p, emote, &eb, 3)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        refused,
        json!({"status":"error","msg":"Not enough points","pts":0})
    );
    sqlx::query(
        "UPDATE event_private.accounts SET created_at=$3 WHERE run_id=$1 AND actor_hash=$2",
    )
    .bind(emote.id())
    .bind(eb.storage_hash().as_slice())
    .bind(timestamp - 900)
    .execute(&o)
    .await
    .unwrap();
    let bought = events::buy_emote(&p, emote, &eb, "happy")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(bought["data"]["balance"], -50);
    let all_owned: Vec<i16> = (0..166).collect();
    sqlx::query("UPDATE event_private.accounts SET owned=$3,balance=100,created_at=$4 WHERE run_id=$1 AND actor_hash=$2")
   .bind(emote.id()).bind(ea.storage_hash().as_slice()).bind(&all_owned).bind(timestamp).execute(&o).await.unwrap();
    let rolled = events::roll_emotes(&p, emote, &ea, 10)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(rolled["status"], "success");
    let obtained = rolled["data"]["obtained"].as_array().unwrap();
    assert_eq!(obtained.len(), 10);
    let ids: Vec<&str> = obtained
        .iter()
        .map(|row| {
            if row[1] == 1 {
                row[0].as_str().unwrap()
            } else {
                row[2].as_str().unwrap()
            }
        })
        .collect();
    let names: Vec<String> = emotes::roster()
        .iter()
        .map(|emote| emote.name().to_owned())
        .collect();
    let model = emotes::Account::from_stored(timestamp, 100, &names)
        .unwrap()
        .apply_roll(&ids, timestamp)
        .unwrap();
    assert_eq!(rolled["data"]["balance"], model.account.balance());
    assert_eq!(
        events::account(&p, emote, &ea).await.unwrap().unwrap()["owned"]
            .as_array()
            .unwrap()
            .len(),
        166
    );
    assert!(events::roll_emotes(&p, emote, &ea, 255).await.is_err());
    // Runtime roles cannot inspect or alter balances/configuration directly.
    for query in [
        "SELECT * FROM event_private.accounts",
        "UPDATE event_private.runs SET enabled=false",
        "SET ROLE board_event_owner",
    ] {
        let error = sqlx::query(query).execute(&p).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501")
        );
    }
    let mut drift = o.begin().await.unwrap();
    sqlx::query("GRANT SELECT ON event_private.accounts TO board_public")
        .execute(&mut *drift)
        .await
        .unwrap();
    sqlx::query("SET LOCAL ROLE board_event_owner")
        .execute(&mut *drift)
        .await
        .unwrap();
    assert!(
        !sqlx::query_scalar::<_, bool>("SELECT content.events_ready()")
            .fetch_one(&mut *drift)
            .await
            .unwrap()
    );
    assert!(
        sqlx::query("SELECT content.event_account($1,$2)")
            .bind(market.id())
            .bind(alice.storage_hash().as_slice())
            .execute(&mut *drift)
            .await
            .is_err()
    );
    drift.rollback().await.unwrap();
    assert!(
        sqlx::query_scalar::<_, bool>("SELECT content.events_ready()")
            .fetch_one(&p)
            .await
            .unwrap()
    );
    // A close holding the run lock wins before a queued account mutation.
    let mut close = o.begin().await.unwrap();
    sqlx::query("SELECT id FROM event_private.runs WHERE id=$1 FOR UPDATE")
        .bind(market.id())
        .execute(&mut *close)
        .await
        .unwrap();
    let mut connection = p.acquire().await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *connection)
        .await
        .unwrap();
    let digest = *alice.storage_hash();
    let pending = tokio::spawn(async move {
        sqlx::query_scalar::<_, Option<serde_json::Value>>(
            "SELECT content.event_market_trade($1,$2,1::smallint,0::smallint,1,100)",
        )
        .bind(market.id())
        .bind(digest.as_slice())
        .fetch_one(&mut *connection)
        .await
    });
    tokio::time::timeout(std::time::Duration::from_millis(1500),async{
   loop{let waiting:bool=sqlx::query_scalar("SELECT coalesce((SELECT wait_event_type='Lock' FROM pg_stat_activity WHERE pid=$1),false)").bind(pid).fetch_one(&p).await.unwrap();if waiting{break;}tokio::time::sleep(std::time::Duration::from_millis(10)).await;}
  }).await.expect("Observe the real run-lock wait before closure");
    sqlx::query("UPDATE event_private.runs SET enabled=false WHERE id=$1")
        .bind(market.id())
        .execute(&mut *close)
        .await
        .unwrap();
    close.commit().await.unwrap();
    assert!(pending.await.unwrap().unwrap().is_none());
    assert!(
        events::active_run(&p, Kind::Market)
            .await
            .unwrap()
            .is_none()
    );
    assert!(events::account(&p, market, &alice).await.unwrap().is_none());
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT balance FROM event_private.accounts WHERE run_id=$1 AND actor_hash=$2"
        )
        .bind(market.id())
        .bind(alice.storage_hash().as_slice())
        .fetch_one(&o)
        .await
        .unwrap(),
        300
    );
    check_admin(&o, &p, timestamp, &owned_ids, alice.credential()).await;
}

async fn check_admin(
    owner: &PgPool,
    public: &PgPool,
    timestamp: i64,
    owned_ids: &Arc<Mutex<Vec<i64>>>,
    old_cookie: &str,
) {
    use events::admin;
    let prices: std::collections::BTreeMap<_, _> = board_domain::source_events::STOCKS
        .into_iter()
        .map(|stock| (stock, 100))
        .collect();
    let profile = json!({"version":1,"kind":"market","starts_at":timestamp-10,"ends_at":timestamp+7200,"account_capacity":1,"prices":prices});
    let input = admin::read_profile(profile.to_string().as_bytes()).unwrap();
    let mut unauthorized = public.acquire().await.unwrap();
    assert!(matches!(
        admin::start(&mut unauthorized, &input).await,
        Err(admin::Error::Role)
    ));
    drop(unauthorized);
    let mut connection = owner.acquire().await.unwrap();
    let id = admin::start(&mut connection, &input).await.unwrap();
    owned_ids.lock().unwrap().push(id);
    assert!(matches!(
        admin::start(&mut connection, &input).await,
        Err(admin::Error::Active)
    ));
    let run = events::active_run(public, Kind::Market)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.id(), id);
    let key = PosterIdKey::parse(&"4".repeat(64)).unwrap().event_key();
    assert!(key.parse(run, old_cookie, timestamp).is_none());
    let session = key
        .issue_for_peer(run, "192.0.2.1".parse().unwrap(), timestamp)
        .unwrap();
    assert_eq!(
        events::open_account(public, run, &session, false)
            .await
            .unwrap()
            .unwrap(),
        json!({"balance":1000})
    );
    assert_eq!(
        events::market_quotes(public, run).await.unwrap().unwrap(),
        profile["prices"]
    );
    let mut quotes = json!({"version":1,"run_id":id,"prices":profile["prices"]});
    quotes["prices"]["PEPE"] = json!(200);
    let quote_input = admin::read_quotes(quotes.to_string().as_bytes()).unwrap();
    admin::quote(&mut connection, &quote_input).await.unwrap();
    assert_eq!(
        events::market_quotes(public, run).await.unwrap().unwrap(),
        quotes["prices"]
    );
    assert_eq!(
        events::market_trade(
            public,
            run,
            &session,
            Order::parse(Side::Buy, "PEPE", "1", "100").unwrap()
        )
        .await
        .unwrap()
        .unwrap()["error"],
        "The price has changed."
    );
    assert!(admin::close(&mut connection, id).await.unwrap());
    assert!(!admin::close(&mut connection, id).await.unwrap());
    assert!(matches!(
        admin::quote(&mut connection, &quote_input).await,
        Err(admin::Error::Missing)
    ));
    assert!(events::market_quotes(public, run).await.unwrap().is_none());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM event_private.accounts WHERE run_id=$1")
            .bind(id)
            .fetch_one(owner)
            .await
            .unwrap(),
        1
    );
    let reset = admin::start(&mut connection, &input).await.unwrap();
    owned_ids.lock().unwrap().push(reset);
    assert!(reset > id);
    let new_run = events::active_run(public, Kind::Market)
        .await
        .unwrap()
        .unwrap();
    assert!(
        key.parse(new_run, session.credential(), timestamp)
            .is_none()
    );
    assert!(admin::close(&mut connection, reset).await.unwrap());
    let profile = json!({"version":1,"kind":"scores","starts_at":timestamp-10,"ends_at":timestamp+7200,"account_capacity":2});
    let input = admin::read_profile(profile.to_string().as_bytes()).unwrap();
    let score = admin::start(&mut connection, &input).await.unwrap();
    owned_ids.lock().unwrap().push(score);
    assert_eq!(
        events::scores::active_run(public)
            .await
            .unwrap()
            .unwrap()
            .id,
        score
    );
    assert!(matches!(
        admin::start(&mut connection, &input).await,
        Err(admin::Error::Active)
    ));
    assert!(admin::close(&mut connection, score).await.unwrap());
    assert!(events::scores::active_run(public).await.unwrap().is_none());
}
