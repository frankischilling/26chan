//! Age or clean up a single browser-owned text thread without changing board policy.
#![forbid(unsafe_code)]

#[tokio::main]
async fn main() {
    if run().await.is_err() {
        eprintln!("Owned deletion fixture failed");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    run_with(std::env::args().skip(1).collect()).await
}

async fn run_with(args: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    if args.len() != 5
        || !matches!(args[0].as_str(), "age" | "cleanup")
        || !matches!(
            args[1].as_str(),
            "b" | "sjis" | "news" | "qst" | "tg" | "r9k" | "s4s"
        )
        || !matches!(args[3].as_str(), "subject" | "comment")
        || args[4].len() < 44
        || !args[4].starts_with("OwnedBrowser")
        || !args[4].as_bytes()[12..44]
            .iter()
            .copied()
            .all(|b| b.is_ascii_hexdigit())
    {
        return Err("Invalid owned deletion fixture arguments".into());
    }
    let id: i64 = args[2].parse()?;
    if id <= 0 {
        return Err("Positive receipt ID required".into());
    }
    let pool = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL")?).await?;
    let user: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&pool)
        .await?;
    if user != "board_migrator" {
        return Err("Migration identity required".into());
    }
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
        .bind(&args[1])
        .fetch_one(&mut *tx)
        .await?;
    // A random marker supplied when posting, the exact receipt ID, and the board
    // must all match. Never discover ownership by reading arbitrary public text.
    // Filtered comments store generated markup in comment; their saved logical
    // text keeps formatter word breaks out of this exact ownership comparison.
    // Unfiltered/historical comments still require exact original text.
    let thread: i64 = sqlx::query_scalar("SELECT p.thread_id FROM content.posts p JOIN content.posts op ON op.id=p.thread_id AND op.board=p.board WHERE p.board=$1 AND p.id=$2 AND CASE WHEN $3='subject' THEN op.subject ELSE COALESCE(op.wordfilter_search,op.comment) END=$4 AND op.created_at > now()-interval '1 day'")
        .bind(&args[1]).bind(id).bind(&args[3]).bind(&args[4]).fetch_one(&mut *tx).await?;
    if args[0] == "age" {
        let updated = sqlx::query("UPDATE content.posts p SET created_at=now()-make_interval(secs=>b.deletion_unknown_min_seconds+1) FROM content.boards b WHERE p.id=$1 AND p.board=$2 AND b.slug=p.board AND NOT p.deleted AND b.deletion_unknown_min_seconds+1 < b.deletion_max_seconds")
            .bind(id).bind(&args[1]).execute(&mut *tx).await?;
        if updated.rows_affected() != 1 {
            return Err("Exactly one eligible owned post required".into());
        }
    } else {
        if thread != id {
            return Err("Cleanup requires an owned OP receipt".into());
        }
        for statement in [
            "DELETE FROM content.reports WHERE board=$1 AND post_id IN (SELECT id FROM content.posts WHERE board=$1 AND thread_id=$2)",
            "DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1 AND thread_id=$2)",
            "DELETE FROM content.posts WHERE board=$1 AND thread_id=$2",
            "DELETE FROM content.threads WHERE board=$1 AND id=$2",
        ] {
            sqlx::query(statement)
                .bind(&args[1])
                .bind(thread)
                .execute(&mut *tx)
                .await?;
        }
    }
    tx.commit().await?;
    pool.close().await;
    Ok(())
}

#[cfg(all(test, feature = "database-tests"))]
mod tests {
    use super::*;

    // Serial database qualification: fixtures/demo.sql must already be loaded.
    #[tokio::test]
    async fn exact_ownership_gates_age_and_cleanup_without_changing_board_policy() {
        let pool = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let marker = format!(
            "OwnedBrowser{:032x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let mut ids = Vec::new();
        let mut setup = pool.begin().await.unwrap();
        for suffix in ["", " control"] {
            let id: i64 = sqlx::query_scalar(
                "INSERT INTO content.threads(board) VALUES('news') RETURNING id",
            )
            .fetch_one(&mut *setup)
            .await
            .unwrap();
            sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,'news',$1,'Anonymous',$2,'Owned deletion guard fixture')")
                .bind(id).bind(format!("{marker}{suffix}")).execute(&mut *setup).await.unwrap();
            ids.push(id);
        }
        setup.commit().await.unwrap();
        let work_pool = pool.clone();
        let work_ids = ids.clone();
        // Catch assertion failure in the task so the owned fixture is still removed.
        let checked = tokio::spawn(async move {
            let policy: serde_json::Value =
                sqlx::query_scalar("SELECT to_jsonb(b) FROM content.boards b WHERE slug='news'")
                    .fetch_one(&work_pool)
                    .await
                    .unwrap();
            let before: Vec<(i64, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
                "SELECT id,created_at FROM content.posts WHERE id=ANY($1) ORDER BY id",
            )
            .bind(&work_ids)
            .fetch_all(&work_pool)
            .await
            .unwrap();
            for command in ["age", "cleanup"] {
                for (board, id, proof) in [
                    ("news", work_ids[0], format!("{marker} wrong")),
                    ("b", work_ids[0], marker.clone()),
                    ("fixture", work_ids[0], marker.clone()),
                    ("news", work_ids[1], marker.clone()),
                    ("news", 0, marker.clone()),
                    ("news", i64::MAX, marker.clone()),
                ] {
                    assert!(
                        run_with(vec![
                            command.into(),
                            board.into(),
                            id.to_string(),
                            "subject".into(),
                            proof
                        ])
                        .await
                        .is_err()
                    );
                }
            }
            let unchanged: Vec<(i64, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
                "SELECT id,created_at FROM content.posts WHERE id=ANY($1) ORDER BY id",
            )
            .bind(&work_ids)
            .fetch_all(&work_pool)
            .await
            .unwrap();
            assert_eq!(before, unchanged);
            run_with(vec![
                "age".into(),
                "news".into(),
                work_ids[0].to_string(),
                "subject".into(),
                marker.clone(),
            ])
            .await
            .unwrap();
            let after: Vec<(i64, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
                "SELECT id,created_at FROM content.posts WHERE id=ANY($1) ORDER BY id",
            )
            .bind(&work_ids)
            .fetch_all(&work_pool)
            .await
            .unwrap();
            assert!(after[0].1 < before[0].1);
            assert_eq!(after[1], before[1]);
            run_with(vec![
                "cleanup".into(),
                "news".into(),
                work_ids[0].to_string(),
                "subject".into(),
                marker,
            ])
            .await
            .unwrap();
            let remaining: Vec<i64> =
                sqlx::query_scalar("SELECT id FROM content.posts WHERE id=ANY($1)")
                    .bind(&work_ids)
                    .fetch_all(&work_pool)
                    .await
                    .unwrap();
            assert_eq!(remaining, vec![work_ids[1]]);
            let unchanged_policy: serde_json::Value =
                sqlx::query_scalar("SELECT to_jsonb(b) FROM content.boards b WHERE slug='news'")
                    .fetch_one(&work_pool)
                    .await
                    .unwrap();
            assert_eq!(policy, unchanged_policy);
        })
        .await;
        for statement in [
            "DELETE FROM content.posts WHERE board='news' AND id=ANY($1)",
            "DELETE FROM content.threads WHERE board='news' AND id=ANY($1)",
        ] {
            sqlx::query(statement)
                .bind(&ids)
                .execute(&pool)
                .await
                .unwrap();
        }
        pool.close().await;
        checked.unwrap();
    }

    #[tokio::test]
    async fn filtered_comment_ownership_is_exact_and_rejects_visible_or_literal_markup_changes() {
        use rand_core::{OsRng, RngCore};

        let pool = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let mut nonce = [0_u8; 16];
        OsRng.fill_bytes(&mut nonce);
        let marker = format!(
            "OwnedBrowser{}",
            nonce
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        );
        let mut fixture_key = [0_u8; 32];
        OsRng.fill_bytes(&mut fixture_key);
        let fixture_key = board_domain::poster_id::PosterIdKey::parse(
            &fixture_key
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
        )
        .unwrap();
        let mut setup = pool.begin().await.unwrap();
        let policy: serde_json::Value = sqlx::query_scalar(
            "SELECT to_jsonb(b) FROM content.boards b WHERE slug='qst' FOR UPDATE",
        )
        .fetch_one(&mut *setup)
        .await
        .unwrap();
        assert_eq!(policy["word_filter_enabled"], true);
        assert_eq!(policy["word_filter_profile"], 0);
        let markup = board_domain::comment_markup::MarkupPolicy {
            spoilers: policy["comment_spoiler_cleanup"].as_bool().unwrap(),
            code: policy["comment_code_spacing"].as_bool().unwrap(),
            sjis: policy["comment_sjis_spacing"].as_bool().unwrap(),
            op: policy["op_markup"].as_bool().unwrap(),
        };
        let literal = format!("{}<wbr>{}", &marker[..35], &marker[35..]);
        let inputs = [
            marker.clone(),
            format!("{marker} extra visible text"),
            literal.clone(),
            "Unrelated owned regression post".into(),
            marker.clone(),
            "Owned regression reply".into(),
        ];
        let mut ids = Vec::new();
        for (index, input) in inputs.iter().enumerate() {
            let mut prepared = board_domain::wordfiltered_comment::prepare(
                input,
                markup,
                board_domain::wordfilter::Profile::Global,
                None,
            )
            .unwrap();
            prepared.freeze_format("qst");
            let lines = board_domain::filtered_formatting::lines(&prepared, "qst");
            let saved = board_domain::filtered_formatting::source_projection(&lines);
            let logical = board_domain::formatting::plain_text(&lines);
            if index == 0 {
                assert_eq!(saved, literal);
                assert_eq!(logical, marker);
            } else if index == 2 {
                assert!(saved.contains("&lt;wbr&gt;"));
                assert_eq!(logical, literal);
                assert_ne!(logical, marker);
            }
            let encoded = prepared
                .encode()
                .unwrap()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            sqlx::query("SELECT set_config('board.wordfilter_payload',$1,true),set_config('board.wordfilter_search',$2,true)")
                .bind(encoded).bind(logical).execute(&mut *setup).await.unwrap();
            let id: i64 = if index == 5 {
                sqlx::query_scalar("SELECT nextval('content.post_number')")
                    .fetch_one(&mut *setup)
                    .await
                    .unwrap()
            } else {
                sqlx::query_scalar("INSERT INTO content.threads(board) VALUES('qst') RETURNING id")
                    .fetch_one(&mut *setup)
                    .await
                    .unwrap()
            };
            let thread = if index == 5 { ids[0] } else { id };
            // Imported /qst/ requires the real derived poster identity. These
            // documentation-range peers belong only to this disposable fixture.
            let peer = std::net::IpAddr::V4(std::net::Ipv4Addr::new(192, 0, 2, 240 + index as u8));
            let label = fixture_key.label("qst", thread, peer).unwrap();
            let counts = fixture_key.count_context("qst", thread, peer).unwrap();
            sqlx::query("SELECT set_config('board.poster_id',$1,true),set_config('board.post_sage','false',true),set_config('board.poster_fingerprint',$2,true),set_config('board.poster_epoch',$3,true)")
                .bind(label).bind(counts.fingerprint).bind(counts.epoch)
                .execute(&mut *setup).await.unwrap();
            sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at) VALUES($1,'qst',$2,'Anonymous','Owned filtered cleanup',$3,clock_timestamp()-CASE WHEN $4 THEN interval '2 days' ELSE interval '0 days' END)")
                .bind(id).bind(thread).bind(saved).bind(index == 4)
                .execute(&mut *setup).await.unwrap();
            ids.push(id);
        }
        setup.commit().await.unwrap();
        let work_pool = pool.clone();
        let work_ids = ids.clone();
        let checked = tokio::spawn(async move {
            let before: Vec<(i64, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
                "SELECT id,created_at FROM content.posts WHERE id=ANY($1) ORDER BY id",
            )
            .bind(&work_ids)
            .fetch_all(&work_pool)
            .await
            .unwrap();
            for command in ["age", "cleanup"] {
                for (board, id, proof) in [
                    ("qst", work_ids[0], format!("{marker} wrong")),
                    ("b", work_ids[0], marker.clone()),
                    ("fixture", work_ids[0], marker.clone()),
                    ("qst", 0, marker.clone()),
                    ("qst", i64::MAX, marker.clone()),
                    ("qst", work_ids[1], marker.clone()),
                    ("qst", work_ids[2], marker.clone()),
                    ("qst", work_ids[3], marker.clone()),
                    ("qst", work_ids[4], marker.clone()),
                ] {
                    assert!(
                        run_with(vec![
                            command.into(),
                            board.into(),
                            id.to_string(),
                            "comment".into(),
                            proof
                        ])
                        .await
                        .is_err()
                    );
                }
            }
            // A reply can be aged, but cannot authorize whole-thread cleanup.
            assert!(
                run_with(vec![
                    "cleanup".into(),
                    "qst".into(),
                    work_ids[5].to_string(),
                    "comment".into(),
                    marker.clone()
                ])
                .await
                .is_err()
            );
            let unchanged: Vec<(i64, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
                "SELECT id,created_at FROM content.posts WHERE id=ANY($1) ORDER BY id",
            )
            .bind(&work_ids)
            .fetch_all(&work_pool)
            .await
            .unwrap();
            assert_eq!(before, unchanged);
            run_with(vec![
                "age".into(),
                "qst".into(),
                work_ids[0].to_string(),
                "comment".into(),
                marker.clone(),
            ])
            .await
            .unwrap();
            let aged: chrono::DateTime<chrono::Utc> =
                sqlx::query_scalar("SELECT created_at FROM content.posts WHERE id=$1")
                    .bind(work_ids[0])
                    .fetch_one(&work_pool)
                    .await
                    .unwrap();
            assert!(aged < before[0].1);
            run_with(vec![
                "cleanup".into(),
                "qst".into(),
                work_ids[0].to_string(),
                "comment".into(),
                marker,
            ])
            .await
            .unwrap();
            let remaining: Vec<i64> =
                sqlx::query_scalar("SELECT id FROM content.posts WHERE id=ANY($1) ORDER BY id")
                    .bind(&work_ids)
                    .fetch_all(&work_pool)
                    .await
                    .unwrap();
            assert_eq!(remaining, work_ids[1..5]);
            let unchanged_policy: serde_json::Value =
                sqlx::query_scalar("SELECT to_jsonb(b) FROM content.boards b WHERE slug='qst'")
                    .fetch_one(&work_pool)
                    .await
                    .unwrap();
            assert_eq!(policy, unchanged_policy);
        })
        .await;
        for statement in [
            "DELETE FROM content.posts WHERE board='qst' AND id=ANY($1)",
            "DELETE FROM content.threads WHERE board='qst' AND id=ANY($1)",
        ] {
            sqlx::query(statement)
                .bind(&ids)
                .execute(&pool)
                .await
                .unwrap();
        }
        pool.close().await;
        checked.unwrap();
    }
}
