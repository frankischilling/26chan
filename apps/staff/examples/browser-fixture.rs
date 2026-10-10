//! Synthetic browser data only; this executable requires migration authority.
#![forbid(unsafe_code)]
use board_media::{ObjectId, PublicationStore, Quarantine, ValidatedOutput};
use serde_json::json;
#[tokio::main]
async fn main() {
    if run().await.is_err() {
        eprintln!("Synthetic staff fixture failed");
        std::process::exit(1);
    }
}
async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 || args[1].len() != 10 || !args[1].bytes().all(|b| b.is_ascii_alphanumeric())
    {
        return Err("Invalid fixture arguments".into());
    }
    let pool = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL")?).await?;
    let user: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&pool)
        .await?;
    if user != "board_migrator" {
        return Err("Migration identity required".into());
    }
    let board = &args[1];
    match args[0].as_str() {
        "setup" => {
            let mut tx = pool.begin().await?;
            sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES ($1,'Synthetic staff test','Harmless fixtures',16000,100,100,100,10)").bind(board).execute(&mut *tx).await?;
            let thread: i64 =
                sqlx::query_scalar("INSERT INTO content.threads(board) VALUES ($1) RETURNING id")
                    .bind(board)
                    .fetch_one(&mut *tx)
                    .await?;
            sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES ($1,$2,$1,'<b>Synthetic</b>','Harmless <em>subject</em>','<i>Preview text</i>')").bind(thread).bind(board).execute(&mut *tx).await?;
            let post:i64=sqlx::query_scalar("INSERT INTO content.posts(board,thread_id,name,subject,comment) VALUES ($1,$2,'Synthetic reply','','Harmless reply') RETURNING id").bind(board).bind(thread).fetch_one(&mut *tx).await?;
            sqlx::query("UPDATE content.threads SET reply_count=1 WHERE id=$1")
                .bind(thread)
                .execute(&mut *tx)
                .await?;
            let report:i64=sqlx::query_scalar("INSERT INTO content.reports(board,post_id,reason) VALUES ($1,$2,'Review <b>text</b>') RETURNING id").bind(board).bind(post).fetch_one(&mut *tx).await?;
            // Trusted synthetic pixels only. This fixture bypasses intake and
            // guest execution; browser assertions concern staff and reader HTTP.
            let media_root = std::env::current_dir()?
                .join(".local")
                .join(format!("staff-media-{board}"));
            std::fs::create_dir(&media_root)?;
            let quarantine = Quarantine::new(media_root.join("quarantine"))?;
            let store = PublicationStore::new(media_root.join("objects"), &quarantine)?;
            let mut pixels = b"IBRGBA01".to_vec();
            pixels.extend_from_slice(&500u32.to_be_bytes());
            pixels.extend_from_slice(&300u32.to_be_bytes());
            for _ in 0..500 * 300 {
                pixels.extend_from_slice(&[50, 120, 180, 255]);
            }
            let output = ValidatedOutput::read(pixels.as_slice()).await?;
            let full = output.encode()?;
            let thumb = output.thumbnail()?;
            let asset = uuid::Uuid::new_v4().simple().to_string();
            let id: ObjectId = asset.parse()?;
            let guard = store.try_lock()?;
            guard.install(id, &full)?;
            guard.install_thumbnail(id, &thumb)?;
            sqlx::query("INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at,md5,thumbnail_sha256,thumbnail_bytes,thumbnail_width,thumbnail_height) VALUES ($1,$1,$1,$2,$3,500,300,'approved',clock_timestamp(),$4,$5,$6,250,150)")
                .bind(&asset).bind(full.sha256()).bind(full.len() as i64).bind(full.md5()).bind(thumb.sha256()).bind(thumb.len() as i64).execute(&mut *tx).await?;
            let tim: i64 = sqlx::query_scalar("INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler) VALUES ($1,$2,$2,$3,$4,500,300,false) RETURNING tim")
                .bind(post).bind(&asset).bind("<img src=x onerror=alert(1)> & \"fixture\".png").bind(full.len() as i64).fetch_one(&mut *tx).await?;
            tx.commit().await?;
            println!(
                "{}",
                json!({"thread":thread,"post":post,"report":report,"tim":tim,"mediaRoot":media_root})
            );
        }
        "robot9000-seed" => {
            let mut tx = pool.begin().await?;
            let owned: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM content.boards WHERE slug=$1 AND title='Synthetic staff test' AND description='Harmless fixtures')")
                .bind(board).fetch_one(&mut *tx).await?;
            if !owned {
                return Err("Owned Robot9000 fixture board is missing".into());
            }
            // Leave Robot9000 disabled: cleanup must also work for dormant boards.
            sqlx::query("INSERT INTO post_secrets.robot9000_texts(board,digest,seen_at) SELECT $1,decode(lpad(to_hex(n),64,'0'),'hex'),clock_timestamp()-interval '3 years' FROM generate_series(1,1007) n")
                .bind(board).execute(&mut *tx).await?;
            sqlx::query("INSERT INTO post_secrets.robot9000_texts(board,digest,seen_at) VALUES ($1,decode(repeat('ff',32),'hex'),clock_timestamp())")
                .bind(board).execute(&mut *tx).await?;
            sqlx::query("INSERT INTO post_secrets.robot9000_mutes(board,actor,timeout_power,mute_until,next_expire) VALUES ($1,decode(repeat('aa',32),'hex'),4,clock_timestamp()+interval '1 hour',clock_timestamp()+interval '2 hours')")
                .bind(board).execute(&mut *tx).await?;
            tx.commit().await?;
        }
        "robot9000-inspect" => {
            let summary: serde_json::Value = sqlx::query_scalar("SELECT jsonb_build_object('texts',(SELECT count(*) FROM post_secrets.robot9000_texts WHERE board=$1),'old',(SELECT count(*) FROM post_secrets.robot9000_texts WHERE board=$1 AND seen_at<clock_timestamp()-interval '2 years'),'recent',(SELECT count(*) FROM post_secrets.robot9000_texts WHERE board=$1 AND digest=decode(repeat('ff',32),'hex')),'mutes',(SELECT coalesce(jsonb_agg(jsonb_build_array(timeout_power,mute_until::text,next_expire::text) ORDER BY actor),'[]'::jsonb) FROM post_secrets.robot9000_mutes WHERE board=$1),'audit',(SELECT coalesce(jsonb_agg(removed ORDER BY id),'[]'::jsonb) FROM content.board_cleanup_audit WHERE board=$1))")
                .bind(board).fetch_one(&pool).await?;
            println!("{summary}");
        }
        "ordinary-policy" => {
            let changed=sqlx::query("UPDATE content.boards SET text_only=true,user_ids=true,country_flags=true,board_flags=ARRAY['AC'],op_markup=true,dice_roll=true,fortune_trip=true,deletion_known_min_seconds=0,deletion_unknown_min_seconds=0 WHERE slug=$1 AND title='Synthetic staff test' AND description='Harmless fixtures'")
                .bind(board).execute(&pool).await?.rows_affected();
            if changed != 1 {
                return Err("Owned ordinary fixture board is missing".into());
            }
        }
        "ordinary-inspect" => {
            let summary:serde_json::Value=sqlx::query_scalar("SELECT jsonb_build_object('posts',(SELECT count(*) FROM content.posts WHERE board=$1),'deleted',(SELECT count(*) FROM content.posts WHERE board=$1 AND deleted),'deletion',(SELECT count(*) FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)),'contexts',(SELECT count(*) FROM post_secrets.poster_contexts WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)),'op_peers',(SELECT count(*) FROM post_secrets.op_peers WHERE thread_id IN(SELECT id FROM content.threads WHERE board=$1)),'op_replies',(SELECT count(*) FROM post_secrets.op_replies WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)),'proofs',(SELECT count(*) FROM post_secrets.staff_post_intents WHERE board=$1),'audit',(SELECT count(*) FROM content.moderation_audit WHERE board=$1 AND action='staff-post'))")
                .bind(board).fetch_one(&pool).await?;
            println!("{summary}");
        }
        "force-anon" => {
            let changed = sqlx::query("UPDATE content.boards SET forced_anon=true WHERE slug=$1 AND title='Synthetic staff test' AND description='Harmless fixtures'")
                .bind(board).execute(&pool).await?.rows_affected();
            if changed != 1 {
                return Err("Owned synthetic board is missing".into());
            }
        }
        "bump-limit" => {
            sqlx::query("UPDATE content.boards SET bump_limit=1 WHERE slug=$1")
                .bind(board)
                .execute(&pool)
                .await?;
        }
        "image-limit" => {
            let changed=sqlx::query("UPDATE content.boards SET image_limit=1 WHERE slug=$1 AND title='Synthetic staff test' AND description='Harmless fixtures'")
                .bind(board).execute(&pool).await?.rows_affected();
            if changed != 1 {
                return Err("Owned image-limit fixture board is missing".into());
            }
        }
        "authorized-limit" => {
            use std::io::Read;
            let mut input = String::new();
            std::io::stdin().take(6).read_to_string(&mut input)?;
            if input.is_empty()
                || input.len() > 5
                || !input.bytes().all(|byte| byte.is_ascii_digit())
            {
                return Err("Invalid synthetic staff limit".into());
            }
            let maximum: i32 = input.parse()?;
            if !(1..=50000).contains(&maximum) {
                return Err("Invalid synthetic staff limit".into());
            }
            let changed = sqlx::query(
                "UPDATE content.boards SET max_authorized_comment_chars=$2 WHERE slug=$1",
            )
            .bind(board)
            .bind(maximum)
            .execute(&pool)
            .await?
            .rows_affected();
            if changed != 1 {
                return Err("Owned fixture board is missing".into());
            }
        }
        "inspect" => {
            let reports: i64 =
                sqlx::query_scalar("SELECT count(*) FROM content.reports WHERE board=$1")
                    .bind(board)
                    .fetch_one(&pool)
                    .await?;
            let states: Vec<(bool, bool, bool)> = sqlx::query_as(
                "SELECT closed,sticky,deleted FROM content.threads WHERE board=$1 ORDER BY id",
            )
            .bind(board)
            .fetch_all(&pool)
            .await?;
            let audit: Vec<String> = sqlx::query_scalar(
                "SELECT action FROM content.moderation_audit WHERE board=$1 ORDER BY id",
            )
            .bind(board)
            .fetch_all(&pool)
            .await?;
            let credentials:i64=sqlx::query_scalar("SELECT count(*) FROM staff_identity.credentials c JOIN staff_identity.accounts a ON a.id=c.account_id WHERE a.username=$1").bind(board).fetch_one(&pool).await?;
            let bump_flags: Vec<(bool, bool)> = sqlx::query_as(
                "SELECT permasage,permaage FROM content.threads WHERE board=$1 ORDER BY id",
            )
            .bind(board)
            .fetch_all(&pool)
            .await?;
            let sessions:i64=sqlx::query_scalar("SELECT count(*) FROM staff_identity.sessions s JOIN staff_identity.accounts a ON a.id=s.account_id WHERE a.username=$1").bind(board).fetch_one(&pool).await?;
            let thread_options:Vec<(bool,bool,String,String)>=sqlx::query_as("SELECT permaage,undead,bumped_at::text,modified_at::text FROM content.threads WHERE board=$1 ORDER BY id")
                .bind(board).fetch_all(&pool).await?;
            println!(
                "{}",
                json!({"states":states,"bumpFlags":bump_flags,"threadOptions":thread_options,"audit":audit,"credentials":credentials,"sessions":sessions,"reports":reports})
            );
        }
        "spoiler-policy" => {
            let changed=sqlx::query("UPDATE content.boards SET comment_spoiler_cleanup=true WHERE slug=$1 AND title='Synthetic staff test' AND description='Harmless fixtures'")
                .bind(board).execute(&pool).await?.rows_affected();
            if changed != 1 {
                return Err("Owned spoiler fixture board is missing".into());
            }
        }
        "public-spoiler-policy" => {
            use std::io::Read;
            let mut input = String::new();
            std::io::stdin().take(4).read_to_string(&mut input)?;
            let enabled = match input.as_str() {
                "on" => true,
                "off" => false,
                _ => return Err("Invalid owned spoiler policy".into()),
            };
            let changed=sqlx::query("UPDATE content.boards SET image_limit=100,comment_spoiler_cleanup=$2 WHERE slug=$1 AND title='Synthetic staff test' AND description='Harmless fixtures'")
                .bind(board).bind(enabled).execute(&pool).await?.rows_affected();
            if changed != 1 {
                return Err("Owned public spoiler fixture board is missing".into());
            }
        }
        "stale" => {
            sqlx::query("UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp()-interval '11 minutes' WHERE account_id=(SELECT id FROM staff_identity.accounts WHERE username=$1)").bind(board).execute(&pool).await?;
        }
        "age-activity" | "idle" => {
            // Only the migration-authority fixture can move persisted time.
            // The browser server uses the explicit 900-second production policy.
            let seconds = if args[0] == "idle" { 960 } else { 120 };
            let changed = sqlx::query("UPDATE staff_identity.sessions SET last_activity_at=clock_timestamp()-make_interval(secs => $2) WHERE account_id=(SELECT id FROM staff_identity.accounts WHERE username=$1)")
                .bind(board).bind(f64::from(seconds)).execute(&pool).await?;
            println!("{}", json!({"changed":changed.rows_affected()}));
        }
        "session-times" => {
            let times: Vec<(String, String, String)> = sqlx::query_as("SELECT authenticated_at::text,expires_at::text,last_activity_at::text FROM staff_identity.sessions WHERE account_id=(SELECT id FROM staff_identity.accounts WHERE username=$1) ORDER BY authenticated_at")
                .bind(board).fetch_all(&pool).await?;
            // No token, CSRF secret, credential or key leaves the fixture.
            println!("{}", json!(times));
        }
        "expire-ceremony" => {
            use std::io::Read;
            let mut hash = String::new();
            std::io::stdin().take(65).read_to_string(&mut hash)?;
            if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err("Invalid synthetic ceremony hash".into());
            }
            let changed=sqlx::query("UPDATE staff_identity.ceremonies SET expires_at=clock_timestamp()-interval '1 second' WHERE token_hash=decode($2,'hex') AND account_id=(SELECT id FROM staff_identity.accounts WHERE username=$1)").bind(board).bind(hash).execute(&pool).await?;
            println!("{}", json!({"expired":changed.rows_affected()}));
        }
        "expire" => {
            sqlx::query("UPDATE staff_identity.sessions SET expires_at=clock_timestamp()-interval '1 second' WHERE account_id=(SELECT id FROM staff_identity.accounts WHERE username=$1)").bind(board).execute(&pool).await?;
        }
        "cleanup" => {
            let mut tx = pool.begin().await?;
            sqlx::query("DELETE FROM staff_identity.sessions WHERE account_id=(SELECT id FROM staff_identity.accounts WHERE username=$1)").bind(board).execute(&mut *tx).await?;
            sqlx::query("DELETE FROM staff_identity.credentials WHERE account_id=(SELECT id FROM staff_identity.accounts WHERE username=$1)").bind(board).execute(&mut *tx).await?;
            sqlx::query("DELETE FROM staff_identity.invitations WHERE account_id=(SELECT id FROM staff_identity.accounts WHERE username=$1)").bind(board).execute(&mut *tx).await?;
            sqlx::query("DELETE FROM staff_identity.ceremonies WHERE account_id=(SELECT id FROM staff_identity.accounts WHERE username=$1)").bind(board).execute(&mut *tx).await?;
            sqlx::query("DELETE FROM staff_identity.accounts WHERE username=$1")
                .bind(board)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM content.board_cleanup_audit WHERE board=$1")
                .bind(board)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM post_secrets.robot9000_texts WHERE board=$1")
                .bind(board)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM post_secrets.robot9000_mutes WHERE board=$1")
                .bind(board)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM content.moderation_audit WHERE board=$1")
                .bind(board)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM content.reports WHERE board=$1")
                .bind(board)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM media.assets WHERE id IN (SELECT m.asset_id FROM content.post_media m JOIN content.posts p ON p.id=m.post_id WHERE p.board=$1)")
                .bind(board).execute(&mut *tx).await?;
            sqlx::query("DELETE FROM content.post_media WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)")
                .bind(board).execute(&mut *tx).await?;
            sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)")
                .bind(board).execute(&mut *tx).await?;
            sqlx::query("DELETE FROM content.posts WHERE board=$1")
                .bind(board)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM content.threads WHERE board=$1")
                .bind(board)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM content.boards WHERE slug=$1")
                .bind(board)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
        }
        _ => return Err("Unknown fixture command".into()),
    }
    Ok(())
}
