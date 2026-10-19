#![cfg(feature = "database-tests")]
use board_domain::anonymous_session::Capability;
use board_store::{
    media::MediaQueue,
    media_assets::{
        InputComponent, InputReceipt, MediaFormat, OutputMetadata, OutputVariants, SourceProfile,
        SourceProvenance, StillOutput,
    },
    media_intake::IntakeStore,
};
use sqlx::PgPool;
use std::sync::{Arc, Mutex};
static TEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
type Outcome = (String, Option<String>, Option<i64>, bool);
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
async fn pool(name: &str) -> PgPool {
    PgPool::connect(&std::env::var(name).unwrap())
        .await
        .unwrap()
}
#[derive(Clone)]
struct Fixture {
    owner: PgPool,
    public: PgPool,
    staff: PgPool,
    board: String,
    job: String,
    capability: String,
    rule: i64,
    account: i64,
    session: [u8; 32],
    csrf: [u8; 32],
    actors: Arc<Mutex<Vec<Vec<u8>>>>,
    tokens: Arc<Mutex<Vec<Vec<u8>>>>,
    exemptions: Arc<Mutex<Vec<String>>>,
    clock: Option<String>,
}
impl Fixture {
    async fn new(board: &str, filename: &str) -> Self {
        let owner = pool("MIGRATION_DATABASE_URL").await;
        let public = pool("TEST_PUBLIC_DATABASE_URL").await;
        let staff = pool("STAFF_DATABASE_URL").await;
        let intake = IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let upload = intake
            .reserve_image(filename, board, 0, false)
            .await
            .unwrap();
        intake
            .begin_upload(&upload.id, &upload.capability)
            .await
            .unwrap();
        intake
            .finish_upload(&upload.id, &upload.capability, 100)
            .await
            .unwrap();
        let job = queue.claim().await.unwrap().unwrap();
        assert_eq!(job.id, upload.id);
        let lease = job.lease_token.as_deref().unwrap();
        let output = StillOutput {
            format: MediaFormat::Png,
            primary: OutputMetadata {
                sha256: "a".repeat(64),
                bytes: 100,
                width: 200,
                height: 100,
            },
            variants: OutputVariants {
                md5: "07".repeat(16),
                thumbnail: OutputMetadata {
                    sha256: "b".repeat(64),
                    bytes: 50,
                    width: 100,
                    height: 50,
                },
            },
            mobile: None,
            source: SourceProvenance {
                input_sha256: "d".repeat(64),
                input_bytes: 100,
                profile: SourceProfile::StillV6,
                retained_bytes: 100,
                md5: [7; 16],
            },
        };
        let asset = queue
            .prepare_still_output(&job.id, lease, &output)
            .await
            .unwrap();
        let receipt = InputReceipt {
            component: InputComponent::Upload,
            sha256: "d".repeat(64),
            bytes: 100,
            md5: [8; 16],
        };
        queue
            .record_input_receipt(&job.id, lease, &asset.id, &receipt)
            .await
            .unwrap();
        queue
            .approve_output_with_input(&job.id, lease, &asset.id, &receipt)
            .await
            .unwrap();
        let rule=sqlx::query_scalar("INSERT INTO admission.file_rules(board,digest,public_reason,description) VALUES($1,decode(repeat('07',16),'hex'),'Owned public reason','Owned complaint') RETURNING id").bind(board).fetch_one(&owner).await.unwrap();
        let account = sqlx::query_scalar(
            "INSERT INTO staff_identity.accounts(role) VALUES('janitor') RETURNING id",
        )
        .fetch_one(&owner)
        .await
        .unwrap();
        let session = Capability::generate().unwrap().storage_hash();
        let csrf = Capability::generate().unwrap().storage_hash();
        sqlx::query(
            "INSERT INTO staff_identity.credentials(id,account_id,credential) VALUES($1,$2,'{}')",
        )
        .bind(session.as_slice())
        .bind(account)
        .execute(&owner)
        .await
        .unwrap();
        sqlx::query("INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id) VALUES($1,$2,$3,$1)").bind(session.as_slice()).bind(csrf.as_slice()).bind(account).execute(&owner).await.unwrap();
        let mut tx = owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_restriction_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        let clock =
            sqlx::query_scalar("SELECT timezone FROM restrictions.file_clock WHERE singleton")
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        tx.rollback().await.unwrap();
        Self {
            owner,
            public,
            staff,
            board: board.into(),
            job: upload.id,
            capability: upload.capability,
            rule,
            account,
            session,
            csrf,
            actors: Arc::new(Mutex::new(Vec::new())),
            tokens: Arc::new(Mutex::new(Vec::new())),
            exemptions: Arc::new(Mutex::new(Vec::new())),
            clock,
        }
    }
    async fn policy(&self, mode: i16, quiet: bool, original: bool) {
        sqlx::query("UPDATE admission.file_rules SET mode=$2,ban_days=CASE WHEN $2=1 THEN 3 ELSE 0 END,quiet=$3,digest=$4 WHERE id=$1").bind(self.rule).bind(mode).bind(quiet).bind(if original{vec![8u8;16]}else{vec![7u8;16]}).execute(&self.owner).await.unwrap();
    }
    async fn clock(&self, zone: Option<&str>) {
        let mut tx = self.owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_restriction_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("UPDATE restrictions.file_clock SET timezone=$1 WHERE singleton")
            .bind(zone)
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }
    async fn finish(
        &self,
        runtime: &PgPool,
        actor: &[u8],
        overrides: serde_json::Value,
    ) -> Result<Outcome, sqlx::Error> {
        self.actors.lock().unwrap().push(actor.to_vec());
        let (revision,digest):(i64,String)=sqlx::query_as("SELECT p.revision,encode(r.digest,'hex') FROM admission.policy p CROSS JOIN admission.file_rules r WHERE p.singleton AND r.id=$1").bind(self.rule).fetch_one(&self.owner).await.unwrap();
        let expected:serde_json::Value=sqlx::query_scalar("SELECT jsonb_build_object('filename',filename,'normalized',normalized_md5,'original',encode(original_md5,'hex'),'eligible',original_eligible) FROM content.attachment_upload_filter_metadata($1,$2,$3,0,false,false)").bind(&self.job).bind(&self.capability).bind(&self.board).fetch_one(&self.public).await.unwrap();
        let mut args = serde_json::json!({"peer":"192.0.2.192","token":null,"board":self.board,"parent":0,"job":self.job,"capability":self.capability,"revision":revision,"rule":self.rule,"digest":digest,"expected":expected,"session":null,"csrf":null,"idle":null,"cookie":null});
        for (k, v) in overrides.as_object().unwrap() {
            args[k] = v.clone();
        }
        sqlx::query_as("SELECT * FROM content.finalize_file_filter($1,$2::inet,decode($3,'hex'),$4,$5,$6,$7,$8,$9,decode($10,'hex'),$11,decode($12,'hex'),decode($13,'hex'),$14,$15)")
   .bind(actor).bind(args["peer"].as_str()).bind(args["token"].as_str()).bind(args["board"].as_str()).bind(args["parent"].as_i64()).bind(args["job"].as_str()).bind(args["capability"].as_str()).bind(args["revision"].as_i64()).bind(args["rule"].as_i64()).bind(args["digest"].as_str()).bind(&args["expected"]).bind(args["session"].as_str()).bind(args["csrf"].as_str()).bind(args["idle"].as_i64().map(|n|n as i32)).bind(if args["cookie"].is_null(){None}else{Some(&args["cookie"])}).fetch_one(runtime).await
    }
    async fn bans(&self, actor: &[u8]) -> i64 {
        let mut tx = self.owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_restriction_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        let count =
            sqlx::query_scalar("SELECT count(*) FROM restrictions.bans WHERE actor_hash=$1")
                .bind(actor)
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        tx.rollback().await.unwrap();
        count
    }
    async fn cleanup(self) {
        let actors = self.actors.lock().unwrap().clone();
        let mut tx = self.owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_restriction_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("SELECT restrictions.gate(true)")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("UPDATE restrictions.capacity SET used_bytes=used_bytes-coalesce((SELECT sum(charged_bytes) FROM restrictions.bans WHERE actor_hash=ANY($1)),0) WHERE singleton").bind(&actors).execute(&mut *tx).await.unwrap();
        sqlx::query("DELETE FROM restrictions.audit WHERE ban_id IN(SELECT id FROM restrictions.bans WHERE actor_hash=ANY($1))").bind(&actors).execute(&mut *tx).await.unwrap();
        sqlx::query("DELETE FROM restrictions.bans WHERE actor_hash=ANY($1)")
            .bind(&actors)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("UPDATE restrictions.file_clock SET timezone=$1 WHERE singleton")
            .bind(self.clock)
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        sqlx::query("DELETE FROM admission.dmca_warnings WHERE actor_hash=ANY($1)")
            .bind(&actors)
            .execute(&self.owner)
            .await
            .unwrap();
        sqlx::query("DELETE FROM admission.file_rules WHERE id=$1")
            .bind(self.rule)
            .execute(&self.owner)
            .await
            .unwrap();
        let tokens = self.tokens.lock().unwrap().clone();
        sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=ANY($1)")
            .bind(&tokens)
            .execute(&self.owner)
            .await
            .unwrap();
        let exemptions = self.exemptions.lock().unwrap().clone();
        for network in exemptions {
            sqlx::query("DELETE FROM admission.automatic_ban_exemptions WHERE network=$1::cidr")
                .bind(network)
                .execute(&self.owner)
                .await
                .unwrap();
        }
        for sql in [
            "DELETE FROM media.assets WHERE job_id=$1",
            "DELETE FROM media_intake.handles WHERE job_id=$1",
            "DELETE FROM media.jobs WHERE id=$1",
        ] {
            sqlx::query(sql)
                .bind(&self.job)
                .execute(&self.owner)
                .await
                .unwrap();
        }
        for sql in [
            "DELETE FROM staff_identity.sessions WHERE account_id=$1",
            "DELETE FROM staff_identity.credentials WHERE account_id=$1",
            "DELETE FROM staff_identity.accounts WHERE id=$1",
        ] {
            sqlx::query(sql)
                .bind(self.account)
                .execute(&self.owner)
                .await
                .unwrap();
        }
        self.staff.close().await;
        self.public.close().await;
        self.owner.close().await;
    }
}
fn code(e: sqlx::Error, expected: &str) {
    assert_eq!(
        e.as_database_error().unwrap().code().as_deref(),
        Some(expected)
    );
}
#[tokio::test]
async fn file_finalizer_checks_upload_rule_staff_and_atomic_ban_warning_effects() {
    let _serial = TEST.lock().await;
    let f = Fixture::new("g", "owned.JPEG").await;
    let owned = f.clone();
    let result = tokio::spawn(async move { core(&owned).await }).await;
    f.cleanup().await;
    result.unwrap();
}
async fn core(f: &Fixture) {
    let actor = Capability::generate().unwrap().storage_hash();
    assert_eq!(
        f.finish(&f.public, &actor, serde_json::json!({}))
            .await
            .unwrap(),
        ("failed".into(), None, None, false)
    );
    assert_eq!(f.bans(&actor).await, 0);
    for change in [
        serde_json::json!({"capability":"invalid"}),
        serde_json::json!({"rule":f.rule+1}),
        serde_json::json!({"digest":"09".repeat(16)}),
        serde_json::json!({"revision":-1}),
        serde_json::json!({"board":"a"}),
    ] {
        assert!(f.finish(&f.public, &actor, change).await.is_err());
        assert_eq!(f.bans(&actor).await, 0);
    }
    f.policy(1, false, false).await;
    f.clock(None).await;
    code(
        f.finish(&f.public, &actor, serde_json::json!({}))
            .await
            .unwrap_err(),
        "55000",
    );
    assert_eq!(f.bans(&actor).await, 0);
    f.clock(Some("UTC")).await;
    assert_eq!(
        f.finish(&f.public, &actor, serde_json::json!({}))
            .await
            .unwrap()
            .0,
        "failed"
    );
    assert_eq!(f.bans(&actor).await, 1);
    f.policy(1, true, true).await;
    let quiet = f
        .finish(&f.public, &actor, serde_json::json!({}))
        .await
        .unwrap();
    assert_eq!(quiet.0, "quiet");
    assert!(quiet.2.is_some());
    assert_eq!(f.bans(&actor).await, 2);
    f.policy(2, true, false).await;
    let dmca = Capability::generate().unwrap().storage_hash();
    assert_eq!(
        f.finish(&f.public, &dmca, serde_json::json!({}))
            .await
            .unwrap()
            .0,
        "dmca"
    );
    assert_eq!(f.bans(&dmca).await, 0);
    assert_eq!(
        f.finish(&f.public, &dmca, serde_json::json!({}))
            .await
            .unwrap()
            .0,
        "banned"
    );
    assert_eq!(f.bans(&dmca).await, 1);
    f.policy(0, false, false).await;
    let staff = serde_json::json!({"session":hex(&f.session),"csrf":hex(&f.csrf),"idle":900});
    assert_eq!(
        f.finish(&f.staff, &actor, staff.clone()).await.unwrap().0,
        "failed"
    );
    let mut bad = staff.clone();
    bad["csrf"] = serde_json::json!("00".repeat(32));
    code(f.finish(&f.staff, &actor, bad).await.unwrap_err(), "28000");
    code(
        f.finish(&f.public, &actor, staff).await.unwrap_err(),
        "28000",
    );
    let mut held = f.owner.begin().await.unwrap();
    sqlx::query("SELECT id FROM staff_identity.accounts WHERE id=$1 FOR UPDATE")
        .bind(f.account)
        .execute(&mut *held)
        .await
        .unwrap();
    code(
        f.finish(
            &f.staff,
            &actor,
            serde_json::json!({"session":hex(&f.session),"csrf":hex(&f.csrf),"idle":900}),
        )
        .await
        .unwrap_err(),
        "28000",
    );
    held.rollback().await.unwrap();
    let used: i64 = sqlx::query_scalar("SELECT count(*) FROM content.post_media WHERE job_id=$1")
        .bind(&f.job)
        .fetch_one(&f.owner)
        .await
        .unwrap();
    assert_eq!(used, 0);
}

#[tokio::test]
async fn file_quiet_targets_match_source_across_unrelated_threads() {
    let _serial = TEST.lock().await;
    let owner = pool("MIGRATION_DATABASE_URL").await;
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("UPDATE content.boards SET word_filter_enabled=false WHERE slug='g'")
        .execute(&mut *tx)
        .await
        .unwrap();
    let ids: Vec<i64> =
        sqlx::query_scalar("SELECT nextval('content.post_number') FROM generate_series(1,4)")
            .fetch_all(&mut *tx)
            .await
            .unwrap();
    for (op, reply) in [(ids[0], ids[1]), (ids[2], ids[3])] {
        sqlx::query("INSERT INTO content.threads(id,board) VALUES($1,'g')")
            .bind(op)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,'g',$1,'Owned','Owned quiet target','Synthetic'),($2,'g',$1,'Owned','','Synthetic reply')").bind(op).bind(reply).execute(&mut *tx).await.unwrap();
    }
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    for (parent, expected) in [(0, ids[2]), (ids[0], ids[3] + 1)] {
        let actual: i64 = sqlx::query_scalar("SELECT restrictions.file_quiet_post('g',$1)")
            .bind(parent)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert_eq!(actual, expected);
    }
    tx.rollback().await.unwrap();
    owner.close().await;
}

#[tokio::test]
async fn file_diagnostics_exemptions_and_cookie_refresh_stay_scoped() {
    let _serial = TEST.lock().await;
    let f = Fixture::new("test", "a<&\"'.JPEG").await;
    let owned = f.clone();
    let result = tokio::spawn(async move { responses(&owned).await }).await;
    f.cleanup().await;
    result.unwrap();
}
async fn responses(f: &Fixture) {
    let actor = Capability::generate().unwrap().storage_hash();
    f.policy(0, true, false).await;
    let diagnostic = f
        .finish(&f.public, &actor, serde_json::json!({}))
        .await
        .unwrap();
    assert_eq!(
        diagnostic,
        (
            "diagnostic".into(),
            Some(format!(
                "Blacklisted md5 - {} - Filename: a<&\"'.png",
                "07".repeat(16)
            )),
            None,
            false
        )
    );
    f.policy(1, false, false).await;
    f.clock(Some("UTC")).await;
    sqlx::query("UPDATE admission.file_rules SET public_reason='' WHERE id=$1")
        .bind(f.rule)
        .execute(&f.owner)
        .await
        .unwrap();
    assert_eq!(
        f.finish(&f.public, &actor, serde_json::json!({}))
            .await
            .unwrap()
            .0,
        "failed"
    );
    let mut tx = f.owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let reason:(String,String)=sqlx::query_as("SELECT public_reason,private_reason FROM restrictions.bans WHERE actor_hash=$1 ORDER BY id DESC LIMIT 1").bind(actor.as_slice()).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(
        reason,
        (
            "Posting a blocked file.".into(),
            format!(
                "Blacklisted md5 - {} - Filename: a&lt;&amp;&quot;&#039;.png",
                "07".repeat(16)
            )
        )
    );
    tx.rollback().await.unwrap();
    let network = "198.51.100.192/32";
    sqlx::query("INSERT INTO admission.automatic_ban_exemptions(network) VALUES($1::cidr)")
        .bind(network)
        .execute(&f.owner)
        .await
        .unwrap();
    f.exemptions.lock().unwrap().push(network.into());
    let exempt = Capability::generate().unwrap().storage_hash();
    f.clock(None).await;
    let peer = serde_json::json!({"peer":"198.51.100.192"});
    assert_eq!(
        f.finish(&f.public, &exempt, peer.clone()).await.unwrap().0,
        "failed"
    );
    assert_eq!(f.bans(&exempt).await, 0);
    f.policy(2, true, false).await;
    assert_eq!(
        f.finish(&f.public, &exempt, peer.clone()).await.unwrap().0,
        "dmca"
    );
    assert_eq!(
        f.finish(&f.public, &exempt, peer).await.unwrap().0,
        "banned"
    );
    assert_eq!(f.bans(&exempt).await, 0);
    f.policy(0, false, false).await;
    let token = Capability::generate().unwrap().storage_hash();
    f.tokens.lock().unwrap().push(token.to_vec());
    let now: i64 =
        sqlx::query_scalar("SELECT floor(extract(epoch FROM clock_timestamp()))::bigint")
            .fetch_one(&f.owner)
            .await
            .unwrap();
    let mut cookie = serde_json::json!({"token":hex(&token),"cookie":{"network":"01".repeat(32),"address":"02".repeat(32),"environment":"03".repeat(32),"minted":true,"now":now}});
    assert!(f.finish(&f.public, &actor, cookie.clone()).await.unwrap().3);
    let before: serde_json::Value = sqlx::query_scalar(
        "SELECT to_jsonb(s) FROM post_secrets.anonymous_sessions s WHERE token_hash=$1",
    )
    .bind(token.as_slice())
    .fetch_one(&f.owner)
    .await
    .unwrap();
    for key in ["posts", "images", "threads", "reports"] {
        assert_eq!(before[key], 0);
    }
    cookie["cookie"]["minted"] = serde_json::json!(false);
    assert!(f.finish(&f.public, &actor, cookie.clone()).await.unwrap().3);
    cookie["cookie"]["network"] = serde_json::json!("00");
    assert!(!f.finish(&f.public, &actor, cookie).await.unwrap().3);
    let after: serde_json::Value = sqlx::query_scalar(
        "SELECT to_jsonb(s) FROM post_secrets.anonymous_sessions s WHERE token_hash=$1",
    )
    .bind(token.as_slice())
    .fetch_one(&f.owner)
    .await
    .unwrap();
    assert_eq!(before, after);
    let auth = pool("AUTH_DATABASE_URL").await;
    code(
        f.finish(&auth, &actor, serde_json::json!({}))
            .await
            .unwrap_err(),
        "42501",
    );
    auth.close().await;
    for runtime in [&f.public, &f.staff] {
        code(
            sqlx::query("SELECT * FROM restrictions.file_clock")
                .execute(runtime)
                .await
                .unwrap_err(),
            "42501",
        );
        code(
            sqlx::query("SELECT * FROM content.file_effect_upload_metadata($1,$2,$3,0,false)")
                .bind(&f.job)
                .bind(&f.capability)
                .bind(&f.board)
                .execute(runtime)
                .await
                .unwrap_err(),
            "42501",
        );
    }
}

#[tokio::test]
async fn expired_upload_after_cookie_wait_rolls_back_the_repeat_ban() {
    let _serial = TEST.lock().await;
    let f = Fixture::new("g", "owned-expiry.png").await;
    let owned = f.clone();
    let result = tokio::spawn(async move {
        let actor=Capability::generate().unwrap().storage_hash();
        let token=Capability::generate().unwrap().storage_hash();
        owned.tokens.lock().unwrap().push(token.to_vec());
        let now:i64=sqlx::query_scalar("SELECT floor(extract(epoch FROM clock_timestamp()))::bigint").fetch_one(&owned.owner).await.unwrap();
        let mut cookie=serde_json::json!({"token":hex(&token),"cookie":{"network":"01".repeat(32),"address":"02".repeat(32),"environment":"03".repeat(32),"minted":true,"now":now}});
        assert!(owned.finish(&owned.public,&actor,cookie.clone()).await.unwrap().3);
        cookie["cookie"]["minted"]=serde_json::json!(false);
        owned.policy(2,false,false).await;
        owned.clock(Some("UTC")).await;
        assert_eq!(owned.finish(&owned.public,&actor,serde_json::json!({})).await.unwrap().0,"dmca");
        let before:serde_json::Value=sqlx::query_scalar("SELECT to_jsonb(w) FROM admission.dmca_warnings w WHERE actor_hash=$1").bind(actor.as_slice()).fetch_one(&owned.owner).await.unwrap();
        let runtime = sqlx::postgres::PgPoolOptions::new().max_connections(1)
            .connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap()).await.unwrap();
        // Only this disposable test session waits beyond the normal role
        // limits. Production roles/functions keep their original timeouts.
        sqlx::raw_sql("SET lock_timeout='20s'; SET statement_timeout='30s'")
            .execute(&runtime).await.unwrap();
        let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&runtime).await.unwrap();
        let mut blocker = owned.owner.begin().await.unwrap();
        sqlx::query("SELECT token_hash FROM post_secrets.anonymous_sessions WHERE token_hash=$1 FOR UPDATE")
            .bind(token.as_slice()).execute(&mut *blocker).await.unwrap();
        let blocker_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *blocker).await.unwrap();
        sqlx::query("UPDATE media.jobs SET created_at=clock_timestamp()-interval '2 hours'+interval '10 seconds' WHERE id=$1")
            .bind(&owned.job).execute(&owned.owner).await.unwrap();
        let runner = owned.clone();
        let task = tokio::spawn(async move {
            // This regression delay would fail the former one-second observer.
            // The original hosted failure had no deliberate delay.
            tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
            let result = runner.finish(&runtime, &actor, cookie).await;
            runtime.close().await;
            result
        });
        // Prove this exact row owner blocks the finalizer after its initial
        // upload check, then observe PostgreSQL's actual expiry boundary.
        // Retain errors until the blocker is released and the task is drained:
        // an assertion must never detach a writer into the next fixture.
        let observed = tokio::time::timeout(std::time::Duration::from_secs(15), async {
            loop {
                let expired_while_blocked: bool = sqlx::query_scalar(
                    "SELECT $2=ANY(pg_blocking_pids($1)) AND EXISTS(SELECT 1 FROM pg_locks WHERE pid=$1 AND NOT granted AND locktype='transactionid') AND clock_timestamp()>=(SELECT created_at+interval '2 hours' FROM media.jobs WHERE id=$3)")
                    .bind(pid).bind(blocker_pid).bind(&owned.job)
                    .fetch_one(&owned.owner).await?;
                if expired_while_blocked { return Ok::<_, sqlx::Error>(true); }
                if task.is_finished() { return Ok(false); }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        }).await;
        let released = blocker.rollback().await;
        let completed = task.await;
        released.unwrap();
        let observed = observed.expect("expiry observer exceeded its bounded wait").unwrap();
        assert!(observed, "finalizer finished before the observed cookie wait and upload expiry");
        code(completed.unwrap().unwrap_err(), "P0002");
        assert_eq!(owned.bans(&actor).await,0);
        let after:serde_json::Value=sqlx::query_scalar("SELECT to_jsonb(w) FROM admission.dmca_warnings w WHERE actor_hash=$1").bind(actor.as_slice()).fetch_one(&owned.owner).await.unwrap();
        assert_eq!(before,after);
        let consumed:i64=sqlx::query_scalar("SELECT count(*) FROM content.post_media WHERE job_id=$1").bind(&owned.job).fetch_one(&owned.owner).await.unwrap();
        assert_eq!(consumed,0);
    }).await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn refused_pass_upload_retains_membership_in_automatic_ban() {
    use board_domain::challenge_secrets::{PassSessionToken, ReceiptToken, VerificationKey};
    use board_store::{
        anonymous_session::PostingSession,
        verification::{VerificationStore, pass::WriteRequest},
        *,
    };
    let _serial = TEST.lock().await;
    let f = Fixture::new("g", "owned-pass.png").await;
    f.policy(1, false, false).await;
    f.clock(Some("UTC")).await;
    let material = PassSessionToken::generate().unwrap();
    let key = VerificationKey::parse(&material.encode()[3..]).unwrap();
    let poster_key = board_domain::poster_id::PosterIdKey::parse(&material.encode()[3..]).unwrap();
    let peer = "192.0.2.198".parse().unwrap();
    let actor = poster_key.public_posting_rate_identity(peer);
    f.actors.lock().unwrap().push(actor.as_bytes().to_vec());
    let fingerprints = Capability::generate()
        .unwrap()
        .fingerprints(Some(peer), *b"US");
    f.tokens.lock().unwrap().push(fingerprints.token.to_vec());
    let verifier =
        VerificationStore::connect(&std::env::var("TEST_VERIFICATION_DATABASE_URL").unwrap())
            .await
            .unwrap();
    verifier.ensure_session(&fingerprints, true).await.unwrap();
    let mut setup = f.owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *setup)
        .await
        .unwrap();
    let account:i64=sqlx::query_scalar("INSERT INTO verification_private.pass_entitlements(token_hash,pin_hash,status,purchased_at,expires_at) VALUES($1,repeat('owned-placeholder',3),0,'2020-01-01',clock_timestamp()+interval '1 year') RETURNING id").bind(material.storage_hash().as_slice()).fetch_one(&mut *setup).await.unwrap();
    sqlx::query("INSERT INTO verification_private.pass_sessions VALUES($1,$2,1,clock_timestamp(),clock_timestamp()+interval '1 day')").bind(material.storage_hash().as_slice()).bind(account).execute(&mut *setup).await.unwrap();
    setup.commit().await.unwrap();
    let owned = f.clone();
    let result=tokio::spawn(async move {
        let receipt=ReceiptToken::generate().unwrap();
        let context=key.form_context("thread",Some(&owned.board),0,&"cd".repeat(32)).unwrap();
        let networks=key.pass_networks(peer).unwrap();
        assert_eq!(verifier.pass_write_receipt(&WriteRequest{pass_session:&material.storage_hash(),receipt:&receipt.storage_hash(),networks:&networks,anonymous:&fingerprints.token,context:&context,purpose:"thread",board:&owned.board,target:0}).await.unwrap().status,"ready");
        let image=post_media::NewAttachment{upload:media_intake::IntakeReservation{id:owned.job.clone(),capability:owned.capability.clone()},spoiler:false};
        let now=chrono::Utc::now();
        let error=create_post_with_challenge(&owned.public,&owned.board,0,&NewPost{name:"Anonymous".into(),subject:"Owned Pass file ban".into(),comment:"Owned membership refusal qualification".into(),deletion_hash:"owned".into(),sage:false},Some(&image),ProtectedPostingContext{request_facts:None,anonymous:AnonymousPostingContext{posting:PostingContext{request_start:now,peer:Some(peer),op_password_proof:None},session:PostingSession{fingerprints,minted:false,now}},challenge:WriteChallengeProof::from_issuer(&receipt.encode(),networks.peer,context)},PostMetadata{keys:PostIdentityKeys{tripcode:None,poster_id:Some(&poster_key)},country_database:None,flag:"",options:"",spoiler:false,drawing:None}).await.unwrap_err();
        assert!(matches!(error,StoreError::FileRejected(_)),"{error:?}");
        let mut private=owned.owner.begin().await.unwrap();sqlx::query("SET LOCAL ROLE board_restriction_owner").execute(&mut *private).await.unwrap();
        let identity:Option<Vec<u8>>=sqlx::query_scalar("SELECT pass_identity_hash FROM restrictions.bans WHERE actor_hash=$1").bind(actor.as_bytes().as_slice()).fetch_one(&mut *private).await.unwrap();assert_eq!(identity,Some(material.storage_hash().to_vec()));
        let decision:String=sqlx::query_scalar("SELECT disposition FROM restrictions.posting_decision_qualified(sha256('changed-pass-file-peer'),NULL,$1,$2,$3)").bind(&owned.board).bind(now.timestamp()).bind(material.storage_hash().as_slice()).fetch_one(&mut *private).await.unwrap();assert_eq!(decision,"banned");private.rollback().await.unwrap();
    }).await;
    let mut cleanup = f.owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *cleanup)
        .await
        .unwrap();
    sqlx::query("DELETE FROM verification_private.pass_entitlements WHERE id=$1")
        .bind(account)
        .execute(&mut *cleanup)
        .await
        .unwrap();
    cleanup.commit().await.unwrap();
    f.cleanup().await;
    result.unwrap();
}
