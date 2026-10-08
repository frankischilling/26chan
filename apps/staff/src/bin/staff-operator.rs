#![forbid(unsafe_code)]
use board_staff::{access::Level, auth};
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

fn permission_list(value: &str, flags: bool) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    if value == "-" {
        return Ok(Vec::new());
    }
    let mut values = value.split(',').map(str::to_owned).collect::<Vec<_>>();
    let limit = if flags { 32 } else { 10 };
    if values.len() > 128
        || values.iter().any(|value| {
            value.is_empty()
                || value.len() > limit
                || !value.bytes().all(|byte| {
                    byte.is_ascii_lowercase() || byte.is_ascii_digit() || (flags && byte == b'_')
                })
        })
    {
        return Err("Invalid permission list".into());
    }
    values.sort_unstable();
    values.dedup();
    Ok(values)
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
        Ok(std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)?)
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
            "provision" | "recover" | "revoke" | "role" | "capcode" | "scope" | "flags"
        )
    {
        eprintln!(
            "Usage: staff-operator provision NAME janitor|moderator|manager|admin NEW_PRIVATE_DIR/invitation.txt | recover NAME NEW_PRIVATE_DIR/invitation.txt | revoke NAME | role NAME janitor|moderator|manager|admin | capcode NAME default|mod|admin|manager|developer|founder | scope NAME ALLOW_CSV DENY_CSV | flags NAME FLAG_CSV. Use - for an empty list."
        );
        return Err("Invalid arguments".into());
    }
    let command = &args[0];
    let expected = match command.as_str() {
        "provision" | "scope" => 4,
        "recover" | "role" | "capcode" | "flags" => 3,
        _ => 2,
    };
    if args.len() != expected || !name_valid(&args[1]) {
        return Err("Invalid arguments".into());
    }
    let role = if matches!(command.as_str(), "role" | "provision") {
        Some(Level::parse(&args[2]).ok_or("Invalid role")?)
    } else {
        None
    };
    let scope = if command == "scope" {
        Some((
            permission_list(&args[2], false)?,
            permission_list(&args[3], false)?,
        ))
    } else {
        None
    };
    let flags = if command == "flags" {
        Some(permission_list(&args[2], true)?)
    } else {
        None
    };
    if command == "capcode"
        && !matches!(
            args[2].as_str(),
            "default" | "mod" | "admin" | "manager" | "developer" | "founder"
        )
    {
        return Err("Invalid public badge".into());
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
        let role = role.ok_or("Role required")?;
        let allow: Vec<&str> = if role == Level::Janitor {
            vec!["janitor"]
        } else {
            vec!["all"]
        };
        sqlx::query_scalar("INSERT INTO staff_identity.accounts(role,username,user_handle,allow_boards) VALUES ($1,$2,$3,$4) RETURNING id").bind(role.role()).bind(&args[1]).bind(uuid::Uuid::new_v4().to_string()).bind(allow).fetch_one(&mut *tx).await?
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
        sqlx::query("UPDATE staff_identity.accounts SET role=$2,public_capcode=NULL WHERE id=$1")
            .bind(id)
            .bind(role.ok_or("Role required")?.role())
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM staff_identity.sessions WHERE account_id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    if command == "capcode" {
        sqlx::query(
            "UPDATE staff_identity.accounts SET public_capcode=nullif($2,'default') WHERE id=$1",
        )
        .bind(id)
        .bind(&args[2])
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM staff_identity.sessions WHERE account_id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some((allow, deny)) = scope {
        sqlx::query(
            "UPDATE staff_identity.accounts SET allow_boards=$2,deny_boards=$3 WHERE id=$1",
        )
        .bind(id)
        .bind(allow)
        .bind(deny)
        .execute(&mut *tx)
        .await?;
    }
    if let Some(flags) = flags {
        sqlx::query("UPDATE staff_identity.accounts SET flags=$2 WHERE id=$1")
            .bind(id)
            .bind(flags)
            .execute(&mut *tx)
            .await?;
    }
    if matches!(command.as_str(), "scope" | "flags") {
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

#[cfg(test)]
mod tests {
    use super::permission_list;

    #[test]
    fn operator_scope_lists_are_explicit_bounded_and_canonical() {
        assert_eq!(permission_list("g,a,g", false).unwrap(), ["a", "g"]);
        assert!(permission_list("-", false).unwrap().is_empty());
        assert_eq!(
            permission_list("all,noboard", false).unwrap(),
            ["all", "noboard"]
        );
        for value in [
            "",
            "g,",
            ",g",
            "g,,a",
            " g",
            "G",
            "../j",
            "toolongboard",
            "g\nall",
            "g;all",
        ] {
            assert!(permission_list(value, false).is_err(), "{value:?}");
        }
        assert!(permission_list(&["g"; 129].join(","), false).is_err());
    }

    #[test]
    fn flag_names_do_not_expand_board_names_or_accept_arbitrary_syntax() {
        assert_eq!(
            permission_list("developer,show_tool", true).unwrap(),
            ["developer", "show_tool"]
        );
        assert!(permission_list("show_tool", false).is_err());
        assert!(permission_list(&"x".repeat(33), true).is_err());
        assert!(permission_list("developer<script>", true).is_err());
        assert!(permission_list("-", true).unwrap().is_empty());
    }
}
