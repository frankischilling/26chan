#![cfg(feature = "database-tests")]
use board_domain::content_admission::{AdmissionError, files};
use sqlx::PgPool;

type Row = (i64, String, Vec<u8>, i16, i32, bool);

async fn read(pool: &PgPool, board: &str) -> Vec<Row> {
    sqlx::query_as("SELECT * FROM content.file_admission_rules($1)")
        .bind(board)
        .fetch_all(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn private_file_rules_keep_scopes_bounds_and_shared_generation() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = PgPool::connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let staff = PgPool::connect(&std::env::var("STAFF_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let token: String =
        sqlx::query_scalar("SELECT substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    let boards = [
        format!("fa{token}"),
        format!("fb{token}"),
        format!("fc{token}"),
    ];
    for (index, board) in boards.iter().enumerate() {
        sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,worksafe,staff_only) VALUES($1,'File policy fixture','Synthetic',2000,100,100,100,10,$2,$3)")
            .bind(board).bind(index==0).bind(index==2).execute(&owner).await.unwrap();
    }
    assert!(read(&public, &boards[0]).await.is_empty());
    let before: i64 = sqlx::query_scalar("SELECT revision FROM admission.policy")
        .fetch_one(&owner)
        .await
        .unwrap();
    let mut ids = Vec::new();
    for (scope, active, mode, days) in [
        ("", true, 0_i16, 0),
        ("_ws_", true, 1, -1),
        (boards[0].as_str(), true, 2, 0),
        (boards[1].as_str(), true, 1, 3650),
        (boards[0].as_str(), false, 0, 0),
    ] {
        let id: i64 = sqlx::query_scalar("INSERT INTO admission.file_rules(board,digest,active,mode,ban_days,public_reason,description) VALUES($1,$2,$3,$4,$5,'Private fixture reason','Private fixture description') RETURNING id")
            .bind(scope).bind(vec![ids.len() as u8;16]).bind(active).bind(mode).bind(days)
            .fetch_one(&owner).await.unwrap();
        ids.push(id);
    }
    let after: i64 = sqlx::query_scalar("SELECT revision FROM admission.policy")
        .fetch_one(&owner)
        .await
        .unwrap();
    assert_eq!(after, before + 5);
    for pool in [&public, &staff] {
        let ws = read(pool, &boards[0]).await;
        assert_eq!(
            ws.iter()
                .filter(|r| ids.contains(&r.0))
                .map(|r| r.0)
                .collect::<Vec<_>>(),
            ids[..3]
        );
        let other = read(pool, &boards[1]).await;
        assert_eq!(
            other
                .iter()
                .filter(|r| ids.contains(&r.0))
                .map(|r| r.0)
                .collect::<Vec<_>>(),
            vec![ids[0], ids[3]]
        );
        for board in [&boards[2], "missing-board", ""] {
            let error = sqlx::query("SELECT * FROM content.file_admission_rules($1)")
                .bind(board)
                .execute(pool)
                .await
                .unwrap_err();
            assert_eq!(
                error.as_database_error().unwrap().code().as_deref(),
                Some("P0002")
            );
        }
        for statement in [
            "SELECT * FROM admission.file_rules",
            "DELETE FROM admission.file_rules WHERE false",
            "SELECT last_value FROM admission.file_rules_id_seq",
        ] {
            assert!(sqlx::query(statement).execute(pool).await.is_err());
        }
        let mut tx = pool.begin().await.unwrap();
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .execute(&mut *tx)
            .await
            .unwrap();
        let error = sqlx::query("SELECT * FROM content.file_admission_rules($1)")
            .bind(&boards[0])
            .execute(&mut *tx)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("55000")
        );
        tx.rollback().await.unwrap();
    }
    // The reader keeps the same policy generation until its transaction ends.
    let mut reader = public.begin().await.unwrap();
    sqlx::query("SELECT * FROM content.file_admission_rules($1)")
        .bind(&boards[0])
        .execute(&mut *reader)
        .await
        .unwrap();
    for statement in [
        "UPDATE admission.file_rules SET active=active WHERE false",
        "UPDATE admission.rules SET active=active WHERE false",
        "UPDATE admission.network_rules SET active=active WHERE false",
    ] {
        let mut writer = owner.begin().await.unwrap();
        sqlx::query("SET LOCAL lock_timeout='100ms'")
            .execute(&mut *writer)
            .await
            .unwrap();
        let error = sqlx::query(statement)
            .execute(&mut *writer)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("55P03")
        );
        writer.rollback().await.unwrap();
    }
    reader.commit().await.unwrap();
    sqlx::query("UPDATE admission.file_rules SET quiet=true WHERE id=$1")
        .bind(ids[0])
        .execute(&owner)
        .await
        .unwrap();
    assert!(
        read(&public, &boards[0])
            .await
            .iter()
            .find(|r| r.0 == ids[0])
            .unwrap()
            .5
    );
    for statement in [
        "INSERT INTO admission.file_rules(digest) VALUES(decode('01','hex'))",
        "INSERT INTO admission.file_rules(digest,board) VALUES(decode(repeat('01',16),'hex'),'INVALID')",
        "INSERT INTO admission.file_rules(digest,mode) VALUES(decode(repeat('01',16),'hex'),3)",
        "INSERT INTO admission.file_rules(digest,mode,ban_days) VALUES(decode(repeat('01',16),'hex'),1,-2)",
        "INSERT INTO admission.file_rules(digest,mode,ban_days) VALUES(decode(repeat('01',16),'hex'),1,3651)",
        "INSERT INTO admission.file_rules(digest,mode,ban_days) VALUES(decode(repeat('01',16),'hex'),2,3)",
        "INSERT INTO admission.file_rules(digest,description) VALUES(decode(repeat('01',16),'hex'),repeat('x',4097))",
    ] {
        let error = sqlx::query(statement).execute(&owner).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
    }
    sqlx::query("INSERT INTO admission.file_rules(board,digest) SELECT $1,decode(lpad(to_hex(n),32,'0'),'hex') FROM generate_series(1,130) n")
        .bind(&boards[1]).execute(&owner).await.unwrap();
    let overflow = read(&public, &boards[1]).await;
    assert_eq!(overflow.len(), 129);
    let compiled = files::Policy::compile(
        overflow
            .into_iter()
            .map(|(id, scope, digest, mode, days, quiet)| files::Rule {
                id,
                scope,
                digest: digest.try_into().unwrap(),
                active: true,
                quiet,
                mode: match mode {
                    0 => files::Mode::Reject,
                    1 => files::Mode::Ban { days },
                    2 => files::Mode::Dmca,
                    _ => panic!("Invalid stored mode"),
                },
            })
            .collect(),
    );
    assert!(matches!(compiled, Err(AdmissionError::InvalidPolicy)));
    sqlx::query("DELETE FROM admission.file_rules WHERE id=ANY($1) OR board=ANY($2)")
        .bind(&ids)
        .bind(&boards[..])
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.boards WHERE slug=ANY($1)")
        .bind(&boards[..])
        .execute(&owner)
        .await
        .unwrap();
}
