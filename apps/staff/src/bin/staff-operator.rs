#![forbid(unsafe_code)]
use board_staff::auth;
use sqlx::PgPool;
use std::{
    io::Write,
    path::{Path, PathBuf},
};

#[tokio::main]
async fn main() {
    if run().await.is_err() {
        eprintln!(
            "Staff operator command failed. Check the command, private destination and migration database access."
        );
        std::process::exit(1);
    }
}
fn name_valid(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
fn private_file(path: &Path) -> Result<std::fs::File, Box<dyn std::error::Error>> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or("Explicit new private parent directory required")?;
    // The caller selects a NEW directory. Its permissions are restricted before
    // any invitation bytes are written. Existing paths are never overwritten.
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        std::fs::DirBuilder::new().mode(0o700).create(parent)?;
        return Ok(std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)?);
    }
    #[cfg(windows)]
    {
        std::fs::create_dir(parent)?;
        let output = std::process::Command::new("whoami")
            .arg("/user")
            .arg("/fo")
            .arg("csv")
            .arg("/nh")
            .output()?;
        if !output.status.success() {
            return Err("Cannot resolve operator identity".into());
        }
        let text = String::from_utf8(output.stdout)?;
        let sid = text
            .trim()
            .split(',')
            .next_back()
            .ok_or("Operator SID missing")?
            .trim_matches('"');
        if !sid.starts_with("S-1-")
            || !sid
                .bytes()
                .all(|b| b.is_ascii_digit() || b == b'S' || b == b'-')
        {
            return Err("Invalid SID".into());
        }
        let result = std::process::Command::new("icacls")
            .arg(parent)
            .arg("/inheritance:r")
            .arg("/grant:r")
            .arg(format!("*{sid}:(OI)(CI)F"))
            .output()?;
        if !result.status.success() {
            return Err("Private ACL unavailable".into());
        }
        Ok(std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?)
    }
}
async fn revoke(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM staff_identity.sessions WHERE account_id=$1")
        .bind(id)
        .execute(&mut **tx)
        .await?;
    sqlx::query("DELETE FROM staff_identity.credentials WHERE account_id=$1")
        .bind(id)
        .execute(&mut **tx)
        .await?;
    sqlx::query("DELETE FROM staff_identity.ceremonies WHERE account_id=$1")
        .bind(id)
        .execute(&mut **tx)
        .await?;
    sqlx::query("DELETE FROM staff_identity.invitations WHERE account_id=$1")
        .bind(id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty()
        || !matches!(
            args[0].as_str(),
            "provision" | "recover" | "revoke" | "role"
        )
    {
        eprintln!(
            "Usage: staff-operator provision NAME moderator|admin NEW_PRIVATE_DIR/invitation.txt | recover NAME NEW_PRIVATE_DIR/invitation.txt | revoke NAME | role NAME moderator|admin"
        );
        return Err("Invalid arguments".into());
    }
    let command = &args[0];
    let expected = match command.as_str() {
        "provision" => 4,
        "recover" | "role" => 3,
        _ => 2,
    };
    if args.len() != expected || !name_valid(&args[1]) {
        return Err("Invalid arguments".into());
    }
    if matches!(command.as_str(), "role" | "provision")
        && !matches!(args[2].as_str(), "moderator" | "admin")
    {
        return Err("Invalid role".into());
    }
    let pool = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL")?).await?;
    let user: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&pool)
        .await?;
    if user != "board_migrator" {
        return Err("Operator requires migration authority".into());
    }
    let mut tx = pool.begin().await?;
    let id: i64 = if command == "provision" {
        sqlx::query_scalar("INSERT INTO staff_identity.accounts(role,username,user_handle) VALUES ($1,$2,$3) RETURNING id").bind(&args[2]).bind(&args[1]).bind(uuid::Uuid::new_v4().to_string()).fetch_one(&mut *tx).await?
    } else {
        sqlx::query_scalar("SELECT id FROM staff_identity.accounts WHERE username=$1 FOR UPDATE")
            .bind(&args[1])
            .fetch_optional(&mut *tx)
            .await?
            .ok_or("Account unavailable")?
    };
    if command == "revoke" || command == "recover" {
        revoke(&mut tx, id).await?;
    }
    if command == "revoke" {
        sqlx::query("UPDATE staff_identity.accounts SET revoked_at=clock_timestamp() WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    if command == "role" {
        sqlx::query("UPDATE staff_identity.accounts SET role=$2 WHERE id=$1")
            .bind(id)
            .bind(&args[2])
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM staff_identity.sessions WHERE account_id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    if command == "provision" || command == "recover" {
        let path = PathBuf::from(args.last().ok_or("Private destination required")?);
        let mut file = private_file(&path)?;
        let invitation = auth::token();
        let outcome:Result<(),Box<dyn std::error::Error>>=async {
            sqlx::query("UPDATE staff_identity.accounts SET revoked_at=NULL WHERE id=$1").bind(id).execute(&mut *tx).await?;
            sqlx::query("INSERT INTO staff_identity.invitations(token_hash,account_id,expires_at) VALUES ($1,$2,clock_timestamp()+interval '30 minutes')").bind(auth::hash(&invitation)).bind(id).execute(&mut *tx).await?;
            file.write_all(invitation.as_bytes())?; file.sync_all()?; drop(file);
            tx.commit().await?;
            Ok(())
        }.await;
        if outcome.is_err() {
            let _ = std::fs::remove_file(&path);
        }
        outcome?;
    } else {
        tx.commit().await?;
    }
    println!("Staff operator change committed.");
    Ok(())
}
