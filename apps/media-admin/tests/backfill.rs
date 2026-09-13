#![cfg(feature = "database-tests")]
use board_media::{
    ApprovedFiles, OUTPUT_DISK_BYTES, ObjectId, PublicationStore, Quarantine, ValidatedOutput,
};
use board_media_admin::backfill::complete_backfill;
use board_store::{legacy_media::LegacyMediaStore, media_assets::MediaReader};
use sqlx::PgPool;
use std::{
    path::Path,
    process::Command,
    sync::{Arc, Mutex},
};

fn frame(red: u8) -> Vec<u8> {
    let mut bytes = b"IBRGBA01".to_vec();
    bytes.extend_from_slice(&500u32.to_be_bytes());
    bytes.extend_from_slice(&300u32.to_be_bytes());
    for _ in 0..500 * 300 {
        bytes.extend_from_slice(&[red, 80, 30, 255]);
    }
    bytes
}
async fn pool(key: &str) -> PgPool {
    PgPool::connect(&std::env::var(key).unwrap()).await.unwrap()
}

#[tokio::test]
async fn legacy_backfill_preserves_originals_fences_output_and_commits_cache_metadata() {
    let owner = pool("MIGRATION_DATABASE_URL").await;
    let board = ObjectId::generate().unwrap().to_string()[..10].to_owned();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES ($1,'Legacy media fixture','Synthetic',2000,100,100,100,10)")
        .bind(&board).execute(&owner).await.unwrap();
    let ids = Arc::new(Mutex::new(Vec::<String>::new()));
    let task_ids = ids.clone();
    let task_owner = owner.clone();
    let task_board = board.clone();
    let result =
        tokio::spawn(async move { exercise(&task_owner, &task_board, &task_ids).await }).await;
    for query in [
        "DELETE FROM content.post_media WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(query)
            .bind(&board)
            .execute(&owner)
            .await
            .unwrap();
    }
    let ids = ids.lock().unwrap().clone();
    sqlx::query("DELETE FROM media.assets WHERE id=ANY($1)")
        .bind(&ids)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM media_intake.handles WHERE job_id=ANY($1)")
        .bind(&ids)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM media.jobs WHERE id=ANY($1)")
        .bind(&ids)
        .execute(&owner)
        .await
        .unwrap();
    result.unwrap();
}

async fn insert_legacy(
    owner: &PgPool,
    board: &str,
    store: &PublicationStore,
    output: &ValidatedOutput,
    ids: &Mutex<Vec<String>>,
) -> (String, i64) {
    let id = ObjectId::generate().unwrap().to_string();
    ids.lock().unwrap().push(id.clone());
    let full = output.encode().unwrap();
    sqlx::query("INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at) VALUES ($1,$1,$1,$2,$3,500,300,'approved',clock_timestamp())")
        .bind(&id).bind(full.sha256()).bind(full.len() as i64).execute(owner).await.unwrap();
    store
        .try_lock()
        .unwrap()
        .install(id.parse().unwrap(), &full)
        .unwrap();
    let post: i64 =
        sqlx::query_scalar("INSERT INTO content.threads(board) VALUES ($1) RETURNING id")
            .bind(board)
            .fetch_one(owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES ($1,$2,$1,'Synthetic','','Keep legacy text')").bind(post).bind(board).execute(owner).await.unwrap();
    sqlx::query("INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler) VALUES ($1,$2,$2,'legacy.png',$3,500,300,false)").bind(post).bind(&id).bind(full.len() as i64).execute(owner).await.unwrap();
    (id, post)
}

async fn exercise(owner: &PgPool, board: &str, ids: &Mutex<Vec<String>>) {
    let admin = LegacyMediaStore::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    for key in [
        "TEST_PUBLIC_DATABASE_URL",
        "STAFF_DATABASE_URL",
        "AUTH_DATABASE_URL",
        "MEDIA_DATABASE_URL",
        "MEDIA_READ_DATABASE_URL",
        "INTAKE_DATABASE_URL",
    ] {
        assert!(matches!(
            LegacyMediaStore::connect(&std::env::var(key).unwrap()).await,
            Err(board_store::StoreError::UnsafeRole)
        ));
    }
    let temp = tempfile::tempdir().unwrap();
    let quarantine = Quarantine::new(temp.path().join("quarantine")).unwrap();
    let root = temp.path().join("objects");
    let store = PublicationStore::new(&root, &quarantine).unwrap();
    let files = ApprovedFiles::open(&root).unwrap();
    let reader = MediaReader::connect(&std::env::var("MEDIA_READ_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let output = ValidatedOutput::read(frame(50).as_slice()).await.unwrap();
    let (id, post) = insert_legacy(owner, board, &store, &output, ids).await;
    let before = admin.get(&id).await.unwrap();
    assert!(before.variants().unwrap().is_none());
    assert!(reader.get_thumbnail(&id).await.is_err());
    let path = root.join(format!("{id}.png"));
    let original = std::fs::read(&path).unwrap();
    let guard = store.try_lock().unwrap();
    let wrong = ValidatedOutput::read(frame(51).as_slice()).await.unwrap();
    assert!(
        complete_backfill(&admin, &guard, &files, &before, &wrong)
            .await
            .is_err()
    );
    assert!(!root.join(format!("{id}.thumb.png")).exists());
    let mut corrupt = original.clone();
    corrupt[0] ^= 1;
    std::fs::write(&path, &corrupt).unwrap();
    assert!(
        complete_backfill(&admin, &guard, &files, &before, &output)
            .await
            .is_err()
    );
    std::fs::write(&path, &original).unwrap();
    let coordinator = pool("MEDIA_DATABASE_URL").await;
    for (connection, sql) in [
        (
            &coordinator,
            "UPDATE media.assets SET md5=repeat('a',32),thumbnail_sha256=repeat('b',64),thumbnail_bytes=100,thumbnail_width=250,thumbnail_height=150 WHERE id=$1",
        ),
        (
            owner,
            "UPDATE media.assets SET sha256=repeat('b',64) WHERE id=$1",
        ),
    ] {
        let error = sqlx::query(sql)
            .bind(&id)
            .execute(connection)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501")
        );
    }
    let conflict = root.join(format!("{id}.thumb.png"));
    std::fs::write(&conflict, b"synthetic conflicting output").unwrap();
    assert!(
        complete_backfill(&admin, &guard, &files, &before, &output)
            .await
            .is_err()
    );
    assert_eq!(
        std::fs::read(&conflict).unwrap(),
        b"synthetic conflicting output"
    );
    assert!(admin.get(&id).await.unwrap().variants().unwrap().is_none());
    std::fs::remove_file(&conflict).unwrap();
    // Simulate death after durable thumbnail install but before SQL commit.
    guard
        .install_thumbnail(id.parse().unwrap(), &output.thumbnail().unwrap())
        .unwrap();
    assert!(reader.get_thumbnail(&id).await.is_err());
    assert!(admin.get(&id).await.unwrap().variants().unwrap().is_none());
    let prior: (i64, String) = sqlx::query_as("SELECT m.tim,t.modified_at::text FROM content.post_media m JOIN content.posts p ON p.id=m.post_id JOIN content.threads t ON t.id=p.thread_id WHERE p.id=$1").bind(post).fetch_one(owner).await.unwrap();
    complete_backfill(&admin, &guard, &files, &before, &output)
        .await
        .unwrap();
    let current = admin.get(&id).await.unwrap();
    assert_eq!(current.asset, before.asset);
    let variants = current.variants().unwrap().unwrap();
    assert_eq!(variants.md5, output.encode().unwrap().md5());
    assert_eq!(
        (variants.thumbnail.width, variants.thumbnail.height),
        (250, 150)
    );
    let thumbnail = reader.get_thumbnail(&id).await.unwrap();
    assert_eq!(thumbnail.sha256, variants.thumbnail.sha256);
    assert_eq!(std::fs::read(&path).unwrap(), original);
    let after: (i64,String,String,bool) = sqlx::query_as("SELECT m.tim,t.modified_at::text,p.comment,m.file_deleted FROM content.post_media m JOIN content.posts p ON p.id=m.post_id JOIN content.threads t ON t.id=p.thread_id WHERE p.id=$1").bind(post).fetch_one(owner).await.unwrap();
    assert_eq!(prior.0, after.0);
    assert_ne!(prior.1, after.1);
    assert_eq!(after.2, "Keep legacy text");
    assert!(!after.3);
    assert!(
        complete_backfill(&admin, &guard, &files, &before, &output)
            .await
            .is_err()
    );
    for query in [
        "UPDATE media.assets SET md5=repeat('c',32) WHERE id=$1",
        "UPDATE media.assets SET md5=NULL,thumbnail_sha256=NULL,thumbnail_bytes=NULL,thumbnail_width=NULL,thumbnail_height=NULL WHERE id=$1",
    ] {
        let error = sqlx::query(query)
            .bind(&id)
            .execute(owner)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501")
        );
    }
    drop(guard);
    let pending = ObjectId::generate().unwrap().to_string();
    ids.lock().unwrap().push(pending.clone());
    sqlx::query("INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height) SELECT $1,$1,$1,sha256,bytes,width,height FROM media.assets WHERE id=$2")
        .bind(&pending).bind(&id).execute(owner).await.unwrap();
    assert!(admin.get(&pending).await.is_err());
    let error = sqlx::query("UPDATE media.assets SET md5=repeat('a',32),thumbnail_sha256=repeat('b',64),thumbnail_bytes=100,thumbnail_width=250,thumbnail_height=150 WHERE id=$1")
        .bind(&pending).execute(owner).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("42501")
    );
    let (removed, removed_post) = insert_legacy(owner, board, &store, &output, ids).await;
    let before = admin.get(&removed).await.unwrap();
    sqlx::query("SELECT content.delete_post_attachment($1,$2)")
        .bind(board)
        .bind(removed_post)
        .execute(&pool("TEST_PUBLIC_DATABASE_URL").await)
        .await
        .unwrap();
    let guard = store.try_lock().unwrap();
    assert!(
        complete_backfill(&admin, &guard, &files, &before, &output)
            .await
            .is_err()
    );
    assert!(admin.get(&removed).await.is_err());
    assert!(reader.get(&removed).await.is_err());
    let unchanged: bool = sqlx::query_scalar("SELECT md5 IS NULL FROM media.assets WHERE id=$1")
        .bind(&removed)
        .fetch_one(owner)
        .await
        .unwrap();
    assert!(
        unchanged,
        "Removal during processing cannot commit or revive a manifest"
    );
    drop(guard);
    let (dispatched, _) = insert_legacy(owner, board, &store, &output, ids).await;
    cli_dispatch(temp.path(), &root, &dispatched, frame(50)).await;
    posting_during_upgrade(owner, board, &store, &files, &output, ids).await;
    assert!(
        admin
            .get(&dispatched)
            .await
            .unwrap()
            .variants()
            .unwrap()
            .is_some()
    );
}

async fn blocked(owner: &PgPool, role: &str) -> bool {
    for _ in 0..100 {
        let blocked: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_stat_activity WHERE usename=$1 AND pid<>pg_backend_pid() AND cardinality(pg_blocking_pids(pid))>0)")
            .bind(role).fetch_one(owner).await.unwrap();
        if blocked {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    false
}

async fn posting_during_upgrade(
    owner: &PgPool,
    board: &str,
    store: &PublicationStore,
    files: &ApprovedFiles,
    output: &ValidatedOutput,
    ids: &Mutex<Vec<String>>,
) {
    use board_store::{
        media::MediaQueue, media_assets::OutputMetadata, media_intake::IntakeStore,
        post_media::NewAttachment,
    };
    sqlx::query("UPDATE content.boards SET image_limit=100 WHERE slug=$1")
        .bind(board)
        .execute(owner)
        .await
        .unwrap();
    let intake = IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let upload = intake.reserve("legacy-race.png").await.unwrap();
    ids.lock().unwrap().push(upload.id.clone());
    intake
        .begin_upload(&upload.id, &upload.capability)
        .await
        .unwrap();
    intake
        .finish_upload(&upload.id, &upload.capability, 100)
        .await
        .unwrap();
    let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let claim = queue.claim().await.unwrap().unwrap();
    assert_eq!(claim.id, upload.id, "Requires an idle disposable queue");
    let token = claim.lease_token.as_ref().unwrap();
    let full = output.encode().unwrap();
    let metadata = OutputMetadata {
        sha256: full.sha256().into(),
        bytes: full.len() as i64,
        width: 500,
        height: 300,
    };
    let guard = store.try_lock().unwrap();
    let asset = queue
        .prepare_output(&claim.id, token, &metadata)
        .await
        .unwrap();
    ids.lock().unwrap().push(asset.id.clone());
    guard.install(asset.id.parse().unwrap(), &full).unwrap();
    queue
        .approve_output(&claim.id, token, &asset.id)
        .await
        .unwrap();
    let admin = LegacyMediaStore::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let before = admin.get(&asset.id).await.unwrap();
    let mut job_lock = owner.begin().await.unwrap();
    sqlx::query("SELECT id FROM media.jobs WHERE id=$1 FOR UPDATE")
        .bind(&claim.id)
        .fetch_one(&mut *job_lock)
        .await
        .unwrap();
    let mut asset_lock = owner.begin().await.unwrap();
    sqlx::query("SELECT id FROM media.assets WHERE id=$1 FOR UPDATE")
        .bind(&asset.id)
        .fetch_one(&mut *asset_lock)
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let posting_board = board.to_string();
    let posting = tokio::spawn(async move {
        board_store::create_post_with_attachment(
            &public,
            &posting_board,
            0,
            &board_store::NewPost {
                name: "Anonymous".into(),
                subject: String::new(),
                comment: "Concurrent legacy attachment".into(),
                deletion_hash: "synthetic-unused".into(),
                sage: false,
            },
            Some(&NewAttachment {
                upload,
                spoiler: false,
            }),
        )
        .await
    });
    let public_waited = blocked(owner, "board_public").await;
    let upgrade = complete_backfill(&admin, &guard, files, &before, output);
    let coordinate = async {
        let upgrade_waited = blocked(owner, "board_migrator").await;
        job_lock.commit().await.unwrap();
        let post = posting.await.unwrap().unwrap();
        let prior: String =
            sqlx::query_scalar("SELECT modified_at::text FROM content.threads WHERE id=$1")
                .bind(post)
                .fetch_one(owner)
                .await
                .unwrap();
        asset_lock.commit().await.unwrap();
        (upgrade_waited, post, prior)
    };
    let (upgraded, (upgrade_waited, post, prior)) = tokio::join!(upgrade, coordinate);
    assert!(
        public_waited && upgrade_waited,
        "Both real transactions must reach their controlled locks"
    );
    upgraded.unwrap();
    let after: (String,bool) = sqlx::query_as("SELECT t.modified_at::text,t.modified_at>=a.updated_at FROM content.threads t JOIN content.posts p ON p.thread_id=t.id JOIN content.post_media m ON m.post_id=p.id JOIN media.assets a ON a.id=m.asset_id WHERE p.id=$1")
        .bind(post).fetch_one(owner).await.unwrap();
    assert_ne!(
        prior, after.0,
        "Newly attached thread must receive the upgrade's cache timestamp"
    );
    assert!(after.1);
    assert!(
        admin
            .get(&asset.id)
            .await
            .unwrap()
            .variants()
            .unwrap()
            .is_some()
    );
}

fn private(path: &Path, bytes: impl AsRef<[u8]>) {
    std::fs::write(path, bytes).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}

async fn cli_dispatch(temp: &Path, root: &Path, id: &str, mut disk: Vec<u8>) {
    use board_media_dispatch::{
        config::GatewaySettings, protocol::read_request, tls::server_config,
    };
    use rcgen::{
        BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    };
    use tokio::{io::AsyncWriteExt, net::TcpListener};
    use tokio_rustls::TlsAcceptor;
    let path = |name: &str| temp.join(name);
    let mut ca = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca_key = KeyPair::generate().unwrap();
    private(&path("ca.pem"), ca.self_signed(&ca_key).unwrap().pem());
    let issuer = Issuer::new(ca, ca_key);
    for (name, usage) in [
        ("server", ExtendedKeyUsagePurpose::ServerAuth),
        ("client", ExtendedKeyUsagePurpose::ClientAuth),
    ] {
        let mut params = CertificateParams::new(vec!["dispatch.test".into()]).unwrap();
        params.extended_key_usages = vec![usage];
        let key = KeyPair::generate().unwrap();
        private(
            &path(&format!("{name}.pem")),
            params.signed_by(&key, &issuer).unwrap().pem(),
        );
        private(&path(&format!("{name}.key")), key.serialize_pem());
    }
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let gateway = GatewaySettings {
        listen: listener.local_addr().unwrap(),
        server_certificate: path("server.pem"),
        server_key: path("server.key"),
        client_ca: path("ca.pem"),
        authorization_file: path("unused"),
        broker_socket: path("unused.sock"),
    };
    private(&path("client.json"), serde_json::to_vec(&serde_json::json!({"endpoint": gateway.listen.to_string(), "server_name":"dispatch.test", "server_ca":path("ca.pem"), "client_certificate":path("client.pem"), "client_key":path("client.key")})).unwrap());
    let expected = std::fs::read(root.join(format!("{id}.png"))).unwrap();
    let acceptor = TlsAcceptor::from(server_config(&gateway).unwrap());
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let mut stream = acceptor.accept(socket).await.unwrap();
        assert_eq!(
            read_request(&mut stream).await.unwrap(),
            expected,
            "Only exact PNG bytes cross dispatch"
        );
        disk.resize(OUTPUT_DISK_BYTES as usize, 0);
        stream.write_all(b"IBOUT001").await.unwrap();
        stream
            .write_all(&OUTPUT_DISK_BYTES.to_be_bytes())
            .await
            .unwrap();
        stream.write_all(&disk).await.unwrap();
        stream.shutdown().await.unwrap();
    });
    let command = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_media-backfill"));
        command.env_clear();
        for name in ["PATH", "SystemRoot", "WINDIR", "TEMP", "TMP"] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        command
            .env("APP_ENV", "development")
            .env(
                "MIGRATION_DATABASE_URL",
                std::env::var("MIGRATION_DATABASE_URL").unwrap(),
            )
            .arg(path("client.json"))
            .arg(root)
            .arg(path("quarantine"))
            .arg(id);
        command
    };
    // These denials use the same live TLS server, real legacy record and
    // writable store as the successful control below, not missing resources.
    for key in [
        "DATABASE_URL",
        "TEST_PUBLIC_DATABASE_URL",
        "AUTH_DATABASE_URL",
        "STAFF_DATABASE_URL",
        "MEDIA_DATABASE_URL",
        "MEDIA_READ_DATABASE_URL",
        "MONITOR_DATABASE_URL",
        "INTAKE_DATABASE_URL",
        "PUBLIC_INTAKE_TOKEN",
        "APP_ENV",
    ] {
        let mut denied = command();
        denied.env(
            key,
            if key == "APP_ENV" {
                "production"
            } else {
                "synthetic-secret"
            },
        );
        let result = tokio::task::spawn_blocking(move || denied.output().unwrap())
            .await
            .unwrap();
        assert!(
            !result.status.success(),
            "{key} must reject before a healthy dispatcher can approve output"
        );
        assert!(result.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&result.stderr).contains("synthetic-secret"));
        assert!(!root.join(format!("{id}.thumb.png")).exists());
    }
    let mut child = command();
    let result = tokio::task::spawn_blocking(move || child.output().unwrap())
        .await
        .unwrap();
    assert!(
        result.status.success(),
        "Offline CLI rejected controlled TLS output"
    );
    server.await.unwrap();
    assert_eq!(
        String::from_utf8(result.stdout).unwrap().trim(),
        "legacy manifest upgraded"
    );
    let mut retry = command();
    let result = tokio::task::spawn_blocking(move || retry.output().unwrap())
        .await
        .unwrap();
    assert!(
        result.status.success(),
        "Retry must verify files without contacting the stopped dispatcher"
    );
    assert_eq!(
        String::from_utf8(result.stdout).unwrap().trim(),
        "manifest already current; files verified"
    );
}
