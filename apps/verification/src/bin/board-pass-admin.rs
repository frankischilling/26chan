//! Operator-only membership provisioning. Input is stdin; output never contains
//! identifiers, PINs, credential hashes or deployment keys.
use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use board_domain::challenge_secrets::VerificationKey;
use chrono::{DateTime, Utc};
use ring::rand::{SecureRandom, SystemRandom};
use serde::Deserialize;
use sqlx::postgres::PgPoolOptions;
use std::io::{Read, Write};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    action: String,
    identifier: String,
    pin: Option<String>,
    status: Option<i16>,
    purchased_at: Option<DateTime<Utc>>,
    expires_at: Option<DateTime<Utc>>,
}
fn pin_hash(pin: &str) -> Result<String, &'static str> {
    if pin.is_empty() || pin.len() > 128 {
        return Err("PIN must contain 1 to 128 bytes");
    }
    let mut salt = [0; 16];
    SystemRandom::new()
        .fill(&mut salt)
        .map_err(|_| "Secure randomness unavailable")?;
    let salt = SaltString::encode_b64(&salt).map_err(|_| "PIN encoding failed")?;
    Argon2::default()
        .hash_password(pin.as_bytes(), &salt)
        .map(|value| value.to_string())
        .map_err(|_| "PIN hashing failed")
}
async fn run() -> Result<(), &'static str> {
    if std::env::args_os().len() != 1 {
        return Err("Pass the membership operation as JSON on standard input");
    }
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(8193)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read operation")?;
    if bytes.len() > 8192 {
        return Err("Membership operation is too large");
    }
    let input: Input =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid membership operation")?;
    let key = std::env::var("VERIFICATION_KEY")
        .ok()
        .and_then(|value| VerificationKey::parse(&value))
        .ok_or("A valid VERIFICATION_KEY is required")?;
    let token = key
        .pass_identifier(&input.identifier)
        .ok_or("Token must contain exactly ten ASCII letters or digits")?;
    let encoded = match input.action.as_str() {
        "create" => {
            if !matches!(input.status, Some(0 | 6 | 7))
                || input.purchased_at.is_none()
                || (input.status == Some(7)) != input.expires_at.is_none()
                || input
                    .expires_at
                    .zip(input.purchased_at)
                    .is_some_and(|(expires, purchased)| expires <= purchased)
            {
                return Err(
                    "Create requires active/new/delayed status and valid purchase/expiry dates",
                );
            }
            Some(pin_hash(
                input.pin.as_deref().ok_or("Create requires a PIN")?,
            )?)
        }
        "reset_pin" => {
            if input.status.is_some() || input.purchased_at.is_some() || input.expires_at.is_some()
            {
                return Err("PIN reset cannot change entitlement dates or status");
            }
            Some(pin_hash(
                input.pin.as_deref().ok_or("PIN reset requires a PIN")?,
            )?)
        }
        "status" => {
            if !matches!(input.status, Some(0..=5))
                || input.pin.is_some()
                || input.purchased_at.is_some()
                || input.expires_at.is_some()
            {
                return Err("Status requires a value from zero through five only");
            }
            None
        }
        "renew" => {
            if input.expires_at.is_none_or(|value| value <= Utc::now())
                || input.pin.is_some()
                || input.status.is_some()
                || input.purchased_at.is_some()
            {
                return Err("Renewal requires a future expiry only");
            }
            None
        }
        _ => return Err("Supported operations: create, reset_pin, status, renew"),
    };
    let database = std::env::var("MIGRATION_DATABASE_URL")
        .map_err(|_| "MIGRATION_DATABASE_URL is required")?;
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(std::time::Duration::from_secs(3))
        .connect(&database)
        .await
        .map_err(|_| "Operator database connection failed")?;
    let role: bool = sqlx::query_scalar(
        "SELECT current_user='board_migrator' AND session_user='board_migrator'",
    )
    .fetch_one(&pool)
    .await
    .map_err(|_| "Operator identity check failed")?;
    if !role {
        return Err("Membership operations require the migration operator");
    }
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| "Cannot start membership operation")?;
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *tx)
        .await
        .map_err(|_| "Membership authority is unavailable")?;
    sqlx::query(
        "SELECT set_config('lock_timeout','2s',true),set_config('statement_timeout','5s',true)",
    )
    .execute(&mut *tx)
    .await
    .map_err(|_| "Cannot bound membership operation")?;
    let ready: bool = sqlx::query_scalar("SELECT content.public_pass_ready()")
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| "Membership readiness check failed")?;
    if !ready {
        return Err("Membership authority has drifted");
    }
    sqlx::query(
        "SELECT singleton FROM verification_private.pass_policy WHERE singleton FOR UPDATE",
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| "Membership policy is unavailable")?;
    if input.action == "renew" {
        let current:Option<(Option<DateTime<Utc>>,)> = sqlx::query_as("SELECT expires_at FROM verification_private.pass_entitlements WHERE token_hash=$1 FOR UPDATE")
            .bind(token.as_slice()).fetch_optional(&mut *tx).await.map_err(|_|"Cannot inspect membership expiry")?;
        let current = current
            .ok_or("Membership was not found; no changes committed")?
            .0;
        if current
            .zip(input.expires_at)
            .is_some_and(|(old, new)| new <= old)
        {
            return Err("Renewal must extend the current expiry");
        }
    }
    let row:Option<(i64,i16)>=match input.action.as_str(){
  "create"=>sqlx::query_as("INSERT INTO verification_private.pass_entitlements(token_hash,pin_hash,status,purchased_at,expires_at) VALUES($1,$2,$3,$4,$5) RETURNING id,status")
   .bind(token.as_slice()).bind(encoded).bind(input.status).bind(input.purchased_at).bind(input.expires_at).fetch_optional(&mut *tx).await,
  "reset_pin"=>sqlx::query_as("UPDATE verification_private.pass_entitlements SET pin_hash=$2 WHERE token_hash=$1 RETURNING id,status")
   .bind(token.as_slice()).bind(encoded).fetch_optional(&mut *tx).await,
  "status"=>sqlx::query_as("UPDATE verification_private.pass_entitlements SET status=$2 WHERE token_hash=$1 RETURNING id,status")
   .bind(token.as_slice()).bind(input.status).fetch_optional(&mut *tx).await,
  "renew"=>sqlx::query_as("UPDATE verification_private.pass_entitlements SET status=0,expires_at=$2 WHERE token_hash=$1 RETURNING id,status")
   .bind(token.as_slice()).bind(input.expires_at).fetch_optional(&mut *tx).await,
  _=>unreachable!(),
 }.map_err(|_|"Membership operation was refused; no changes committed")?;
    let (id, status) = row.ok_or("Membership was not found; no changes committed")?;
    tx.commit()
        .await
        .map_err(|_| "Membership commit was not confirmed; inspect its state before retrying")?;
    let result = serde_json::json!({"id":id,"status":status});
    writeln!(std::io::stdout(), "{result}")
        .map_err(|_| "Membership committed but its confirmation could not be written")?;
    pool.close().await;
    Ok(())
}
#[tokio::main]
async fn main() {
    if let Err(message) = run().await {
        eprintln!("{message}");
        std::process::exit(1)
    }
}
