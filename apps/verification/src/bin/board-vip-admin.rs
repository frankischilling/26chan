//! Operator-only VIP provisioning. Read secrets from stdin, never arguments.
use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use board_domain::challenge_secrets::VerificationKey;
use ring::rand::{SecureRandom, SystemRandom};
use serde::Deserialize;
use sqlx::postgres::PgPoolOptions;
use std::io::{Read, Write};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    action: String,
    identifier: String,
    name: Option<String>,
    password: Option<String>,
    active: Option<bool>,
}

fn validate(input: &Input) -> Result<bool, &'static str> {
    let create = match input.action.as_str() {
        "create" => true,
        "update" => false,
        _ => return Err("Use create or update"),
    };
    if create && (input.name.is_none() || input.password.is_none()) {
        return Err("Creating a VIP identity requires a name and password");
    }
    if !create && input.name.is_none() && input.password.is_none() && input.active.is_none() {
        return Err("An update requires a name, password or active state");
    }
    if input.identifier.chars().any(char::is_control)
        || board_domain::posting_options::without_sage(&input.identifier) != input.identifier
    {
        return Err("Identifier cannot contain controls or a source sage sequence");
    }
    if let Some(name) = &input.name
        && (name.is_empty() || name.len() > 100 || name.chars().any(char::is_control))
    {
        return Err("Name must contain 1 to 100 UTF-8 bytes without controls");
    }
    if let Some(password) = &input.password
        && (matches!(password.as_str(), "" | "0")
            || password.chars().any(char::is_control)
            || "capcode_!!".len() + input.identifier.len() + password.len() > 100
            || board_domain::posting_options::without_sage(password) != *password)
    {
        return Err("Password must fit the source option field without controls or sage removal");
    }
    Ok(create)
}

async fn run() -> Result<(), &'static str> {
    if std::env::args_os().len() != 1 {
        return Err("Pass one VIP operation as JSON on standard input");
    }
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(4097)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read operation")?;
    if bytes.len() > 4096 {
        return Err("VIP operation is too large");
    }
    let input: Input = serde_json::from_slice(&bytes).map_err(|_| "Invalid VIP operation")?;
    let create = validate(&input)?;
    let material =
        std::env::var("VERIFICATION_KEY").map_err(|_| "A valid VERIFICATION_KEY is required")?;
    let key = VerificationKey::parse(&material).ok_or("A valid VERIFICATION_KEY is required")?;
    let identifier = key
        .vip_identifier(&input.identifier)
        .ok_or("Invalid VIP identifier")?;
    let hash = if let Some(password) = &input.password {
        let mut salt = [0; 16];
        SystemRandom::new()
            .fill(&mut salt)
            .map_err(|_| "Secure randomness unavailable")?;
        let salt = SaltString::encode_b64(&salt).map_err(|_| "Password encoding failed")?;
        Some(
            Argon2::default()
                .hash_password(password.as_bytes(), &salt)
                .map_err(|_| "Password hashing failed")?
                .to_string(),
        )
    } else {
        None
    };
    let database =
        std::env::var("MIGRATION_DATABASE_URL").map_err(|_| "Operator database URL required")?;
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
        return Err("VIP operations require the migration operator");
    }
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| "Cannot start VIP operation")?;
    sqlx::query(
        "SELECT set_config('lock_timeout','2s',true),set_config('statement_timeout','5s',true)",
    )
    .execute(&mut *tx)
    .await
    .map_err(|_| "Cannot bound VIP operation")?;
    let id: i64 = sqlx::query_scalar("SELECT content.vip_manage($1,$2,$3,$4,$5)")
        .bind(identifier.as_slice())
        .bind(input.name)
        .bind(hash)
        .bind(if create {
            Some(input.active.unwrap_or(true))
        } else {
            input.active
        })
        .bind(create)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| "VIP operation was refused; no changes committed")?;
    tx.commit()
        .await
        .map_err(|_| "VIP commit was not confirmed; inspect its state before retrying")?;
    writeln!(std::io::stdout(), "{}", serde_json::json!({"id":id}))
        .map_err(|_| "VIP committed but its confirmation could not be written")?;
    pool.close().await;
    Ok(())
}

#[tokio::main]
async fn main() {
    if let Err(message) = run().await {
        eprintln!("{message}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provisioning_rejects_unusable_source_credentials_without_printing_values() {
        let mut input = Input {
            action: "create".into(),
            identifier: "owner".into(),
            name: Some("Owned VIP".into()),
            password: Some("owned-secret!".into()),
            active: None,
        };
        assert_eq!(validate(&input), Ok(true));
        for password in ["", "0", "secretSAGEsuffix", "line\nbreak", &"x".repeat(100)] {
            input.password = Some(password.into());
            assert!(validate(&input).is_err());
        }
        input.password = Some("owned-secret!".into());
        input.identifier = "messSAGEidentifier".into();
        assert!(validate(&input).is_err());
        input.identifier = "owner".into();
        input.action = "update".into();
        input.name = None;
        input.password = None;
        assert!(validate(&input).is_err());
        input.active = Some(false);
        assert_eq!(validate(&input), Ok(false));
    }
}
