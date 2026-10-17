#![cfg(feature = "database-tests")]
mod support;

use board_domain::{capcode::Capcode, poster_id::PosterIdKey};
use board_store::{
    NewPost, PostIdentityKeys, PostMetadata, PostingContext, PostingCooldownReason,
    StaffPostAuthority, StaffPostIdentity, StoreError,
    media::MediaQueue,
    media_assets::OutputMetadata,
    media_intake::IntakeStore,
    post_media::{NewAttachment, attachment, cancel_upload},
};
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use std::{
    net::IpAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

// Real runtime credentials and synthetic, fixture-owned receipts. These tests
// qualify DB/store authority, not HTTP upload handling or decoder isolation.
static TEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone)]
struct Fixture {
    asn_database: Option<Arc<board_domain::content_admission::network::asn::Database>>,
    peer_override: Option<IpAddr>,
    hints: Option<board_domain::content_admission::network::hints::DerivedHints>,
    owner: PgPool,
    staff: PgPool,
    auth: PgPool,
    public: PgPool,
    intake: IntakeStore,
    queue: MediaQueue,
    board: String,
    account: i64,
    session: Vec<u8>,
    csrf: Vec<u8>,
    key: String,
    jobs: Arc<Mutex<Vec<String>>>,
    extra_boards: Arc<Mutex<Vec<String>>>,
}

async fn pool(name: &str) -> PgPool {
    PgPool::connect(&std::env::var(name).expect("explicit owned database URL required"))
        .await
        .unwrap()
}

impl Fixture {
    async fn new() -> Self {
        let owner = pool("MIGRATION_DATABASE_URL").await;
        let seed: String = sqlx::query_scalar("SELECT replace(gen_random_uuid()::text,'-','')")
            .fetch_one(&owner)
            .await
            .unwrap();
        let board = format!("a{}", &seed[..9]);
        sqlx::query("INSERT INTO content.boards(captcha_enabled,slug,title,description,max_comment_chars,max_authorized_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,image_limit,posting_reply_seconds,posting_image_seconds,posting_thread_seconds,user_thread_limit,user_ids,comment_spoiler_cleanup) VALUES(false,$1,'Owned staff media','Synthetic',2000,4000,100,100,100,10,100,0,0,0,100,true,true)")
            .bind(&board).execute(&owner).await.unwrap();
        let account = sqlx::query_scalar("INSERT INTO staff_identity.accounts(role,flags) VALUES('moderator',ARRAY['capcode','capcodename','developer']) RETURNING id")
            .fetch_one(&owner).await.unwrap();
        sqlx::query(
            "INSERT INTO staff_identity.credentials(id,account_id,credential) VALUES($1,$2,'{}')",
        )
        .bind(seed.as_bytes())
        .bind(account)
        .execute(&owner)
        .await
        .unwrap();
        let (session, csrf, key): (Vec<u8>, Vec<u8>, String) = sqlx::query_as("SELECT sha256(convert_to(gen_random_uuid()::text,'UTF8')),sha256(convert_to(gen_random_uuid()::text,'UTF8')),encode(sha256(convert_to(gen_random_uuid()::text,'UTF8')),'hex')")
            .fetch_one(&owner).await.unwrap();
        sqlx::query("INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id) VALUES($1,$2,$3,$4)")
            .bind(&session).bind(&csrf).bind(account).bind(seed.as_bytes()).execute(&owner).await.unwrap();
        let staff = pool("STAFF_DATABASE_URL").await;
        let auth = pool("AUTH_DATABASE_URL").await;
        let public = pool("TEST_PUBLIC_DATABASE_URL").await;
        for (connection, expected) in [
            (&staff, "board_staff"),
            (&auth, "board_auth"),
            (&public, "board_public"),
        ] {
            let actual: String = sqlx::query_scalar("SELECT current_user::text")
                .fetch_one(connection)
                .await
                .unwrap();
            assert_eq!(actual, expected);
        }
        Self {
            owner,
            staff,
            auth,
            public,
            board,
            account,
            session,
            csrf,
            key,
            intake: IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
                .await
                .unwrap(),
            queue: MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
                .await
                .unwrap(),
            jobs: Arc::new(Mutex::new(Vec::new())),
            extra_boards: Arc::new(Mutex::new(Vec::new())),
            asn_database: None,
            peer_override: None,
            hints: None,
        }
    }

    async fn approved(&self, filename: &str, spoiler: bool) -> NewAttachment {
        let upload = self.intake.reserve(filename).await.unwrap();
        self.jobs.lock().unwrap().push(upload.id.clone());
        self.intake
            .begin_upload(&upload.id, &upload.capability)
            .await
            .unwrap();
        self.intake
            .finish_upload(&upload.id, &upload.capability, 100)
            .await
            .unwrap();
        let claim = self
            .queue
            .claim()
            .await
            .unwrap()
            .expect("owned idle media queue");
        assert_eq!(
            claim.id, upload.id,
            "Run serially against the disposable media queue"
        );
        let lease = claim.lease_token.unwrap();
        let output = self
            .queue
            .prepare_output(
                &upload.id,
                &lease,
                &OutputMetadata {
                    sha256: "a".repeat(64),
                    bytes: 123,
                    width: 10,
                    height: 20,
                },
            )
            .await
            .unwrap();
        self.queue
            .approve_output(&upload.id, &lease, &output.id)
            .await
            .unwrap();
        NewAttachment { upload, spoiler }
    }

    async fn role(&self, role: &str) {
        sqlx::query("UPDATE staff_identity.accounts SET role=$2 WHERE id=$1")
            .bind(self.account)
            .bind(role)
            .execute(&self.owner)
            .await
            .unwrap();
    }

    async fn write(
        &self,
        parent: i64,
        a: Option<&NewAttachment>,
        ordinary: bool,
        input: &NewPost,
        ip: u8,
        at: DateTime<Utc>,
    ) -> Result<i64, StoreError> {
        self.write_with_options(parent, a, ordinary, input, ip, at, "")
            .await
    }

    async fn write_public(
        &self,
        parent: i64,
        attachment: Option<&NewAttachment>,
        input: &NewPost,
        ip: u8,
        at: DateTime<Utc>,
    ) -> Result<i64, StoreError> {
        let capability = board_domain::anonymous_session::Capability::generate().unwrap();
        let peer = Some(self.peer_override.unwrap_or_else(|| peer(ip)));
        let key = PosterIdKey::parse(&self.key).unwrap();
        board_store::create_protected_post_with_network(
            &self.public,
            &self.board,
            parent,
            input,
            attachment,
            board_store::NetworkPostingContext {
                hints: self.hints.as_ref(),
                asn_database: self.asn_database.as_deref(),
                posting: board_store::ProtectedPostingContext {
                    challenge: None,
                    anonymous: board_store::AnonymousPostingContext {
                        posting: PostingContext {
                            request_start: at,
                            peer,
                            op_password_proof: None,
                        },
                        session: board_store::anonymous_session::PostingSession {
                            fingerprints: capability.fingerprints(peer, *b"XX"),
                            minted: true,
                            now: at,
                        },
                    },
                },
            },
            PostMetadata {
                drawing: None,
                keys: PostIdentityKeys {
                    tripcode: None,
                    poster_id: Some(&key),
                },
                country_database: None,
                flag: "",
                options: "",
                spoiler: false,
            },
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn write_with_options(
        &self,
        parent: i64,
        a: Option<&NewAttachment>,
        ordinary: bool,
        input: &NewPost,
        ip: u8,
        at: DateTime<Utc>,
        options: &str,
    ) -> Result<i64, StoreError> {
        let (role, badge_name_allowed): (String,bool) =
            sqlx::query_as("SELECT role,role='admin' OR 'capcodename'=ANY(flags) FROM staff_identity.accounts WHERE id=$1")
                .bind(self.account)
                .fetch_one(&self.owner)
                .await
                .unwrap();
        let ticket: Vec<u8> =
            sqlx::query_scalar("SELECT sha256(convert_to(gen_random_uuid()::text,'UTF8'))")
                .fetch_one(&self.owner)
                .await
                .unwrap();
        let ticket: [u8; 32] = ticket.try_into().unwrap();
        let key = PosterIdKey::parse(&self.key).unwrap();
        let keys = PostIdentityKeys {
            tripcode: None,
            poster_id: Some(&key),
        };
        let authority = StaffPostAuthority {
            auth_pool: &self.auth,
            session_hash: &self.session,
            csrf_hash: &self.csrf,
            ticket_hash: &ticket,
            idle_seconds: 900,
            highlight: false,
            authorized_limits: role != "janitor",
            raw_name_nonempty: !input.name.is_empty(),
            identity: Some(StaffPostIdentity {
                capcode: (!ordinary).then_some(Capcode::Moderator),
                name_allowed: ordinary || badge_name_allowed,
                administrator: role == "admin",
                tripcode_key: None,
            }),
        };
        let context = PostingContext {
            request_start: at,
            peer: Some(self.peer_override.unwrap_or_else(|| peer(ip))),
            op_password_proof: None,
        };
        if ordinary {
            board_store::create_ordinary_staff_post_with_network(
                &self.staff,
                &self.board,
                parent,
                board_store::StaffPostContent {
                    post: input,
                    attachment: a,
                },
                board_store::NetworkPostingContext {
                    hints: self.hints.as_ref(),
                    posting: context,
                    asn_database: self.asn_database.as_deref(),
                },
                PostMetadata {
                    drawing: None,
                    keys,
                    country_database: None,
                    flag: "",
                    options,
                    spoiler: false,
                },
                authority,
            )
            .await
        } else {
            board_store::create_staff_post_with_network(
                &self.staff,
                &self.board,
                parent,
                board_store::StaffPostContent {
                    post: input,
                    attachment: a,
                },
                board_store::NetworkPostingContext {
                    hints: self.hints.as_ref(),
                    posting: context,
                    asn_database: self.asn_database.as_deref(),
                },
                keys,
                authority,
            )
            .await
        }
    }

    async fn receipt(&self, a: &NewAttachment) -> bool {
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM media_intake.handles WHERE job_id=$1) AND NOT EXISTS(SELECT 1 FROM content.post_media WHERE job_id=$1)")
            .bind(&a.upload.id).fetch_one(&self.owner).await.unwrap()
    }

    async fn counts(&self) -> (i64, i64, i64, i64) {
        sqlx::query_as("SELECT (SELECT count(*) FROM content.posts WHERE board=$1),(SELECT count(*) FROM content.threads WHERE board=$1),(SELECT count(*) FROM content.post_media WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)),(SELECT count(*) FROM post_secrets.staff_attachment_handoffs WHERE board=$1)")
            .bind(&self.board).fetch_one(&self.owner).await.unwrap()
    }

    async fn cleanup(&self) {
        let mut boards = vec![self.board.clone()];
        boards.extend(self.extra_boards.lock().unwrap().iter().cloned());
        // Remove only this fixture's restrictions before taking content locks.
        let mut tx = self.owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_restriction_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("SELECT singleton FROM restrictions.capacity WHERE singleton FOR UPDATE")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("SELECT content.lock_automatic_range_policy(true)")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("DELETE FROM restrictions.automatic_ranges WHERE board=ANY($1)")
            .bind(&boards)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("UPDATE restrictions.capacity SET used_bytes=used_bytes-coalesce((SELECT sum(charged_bytes) FROM restrictions.bans WHERE board=ANY($1)),0) WHERE singleton")
            .bind(&boards).execute(&mut *tx).await.unwrap();
        sqlx::query("DELETE FROM restrictions.audit WHERE ban_id IN(SELECT id FROM restrictions.bans WHERE board=ANY($1))")
            .bind(&boards).execute(&mut *tx).await.unwrap();
        sqlx::query("DELETE FROM restrictions.bans WHERE board=ANY($1)")
            .bind(&boards)
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let mut tx = support::begin_cleanup(&self.owner, &boards).await;
        for sql in [
            "DELETE FROM content.moderation_audit WHERE board=$1",
            "DELETE FROM post_secrets.staff_attachment_handoffs WHERE board=$1",
            "DELETE FROM post_secrets.staff_post_intents WHERE board=$1",
            "DELETE FROM content.post_media WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
            "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
            "DELETE FROM content.posts WHERE board=$1",
            "DELETE FROM content.threads WHERE board=$1",
            "DELETE FROM post_secrets.posting_thread_actions WHERE board=$1",
            "DELETE FROM admission.network_rules WHERE boards=$1",
            "DELETE FROM admission.rules WHERE board=$1",
            "DELETE FROM admission.logs WHERE board=$1",
            "DELETE FROM admission.hits WHERE board=$1",
            "DELETE FROM admission.bans WHERE board=$1",
            "DELETE FROM content.boards WHERE slug=$1",
        ] {
            for board in &boards {
                sqlx::query(sql)
                    .bind(board)
                    .execute(&mut *tx)
                    .await
                    .unwrap();
            }
        }
        let jobs = self.jobs.lock().unwrap().clone();
        for sql in [
            "DELETE FROM media_intake.handles WHERE job_id=ANY($1)",
            "DELETE FROM media.assets WHERE job_id=ANY($1)",
            "DELETE FROM media.jobs WHERE id=ANY($1)",
        ] {
            sqlx::query(sql)
                .bind(&jobs)
                .execute(&mut *tx)
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
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        tx.commit().await.unwrap();
    }
}

fn peer(n: u8) -> IpAddr {
    IpAddr::from([192, 0, 2, n])
}
fn post() -> NewPost {
    NewPost {
        name: "Owned staff".into(),
        subject: "Owned attachment".into(),
        comment: "Synthetic attachment body".into(),
        deletion_hash: "synthetic-not-a-password".into(),
        sage: false,
    }
}

macro_rules! database_case {
    ($name:ident,$body:ident) => {
        #[tokio::test]
        async fn $name() {
            let _serial = TEST.lock().await;
            let f = Fixture::new().await;
            let owned = f.clone();
            let result = tokio::spawn(async move { $body(&owned).await }).await;
            f.cleanup().await;
            result.unwrap();
        }
    };
}

database_case!(
    source_and_ordinary_receipts_are_atomic_one_use_and_capture_identity,
    successful_modes
);
async fn successful_modes(f: &Fixture) {
    let mut ip = 1;
    for role in ["moderator", "manager", "admin", "janitor"] {
        f.role(role).await;
        for ordinary in [false, true] {
            if role == "janitor" && !ordinary {
                continue;
            }
            for empty in [false, true] {
                let a = f.approved("<owned & file>.png", true).await;
                let mut input = post();
                if empty {
                    input.comment.clear();
                }
                let id = f
                    .write(0, Some(&a), ordinary, &input, ip, Utc::now())
                    .await
                    .unwrap();
                let saved = attachment(&f.public, id).await.unwrap().unwrap();
                assert_eq!(saved.filename, "<owned & file>.png");
                assert!(saved.spoiler);
                assert!(
                    saved.tim > 0,
                    "Committed attachment receives its media number"
                );
                assert!(
                    !f.receipt(&a).await,
                    "A committed association spends its receipt"
                );
                let (capcode,spoiler,comment,fingerprint):(Option<String>,bool,String,bool)=
                    sqlx::query_as("SELECT capcode,image_spoiler,comment,EXISTS(SELECT 1 FROM post_secrets.poster_contexts WHERE post_id=$1) FROM content.posts WHERE id=$1")
                        .bind(id).fetch_one(&f.owner).await.unwrap();
                assert_eq!(capcode, if ordinary { None } else { Some("mod".into()) });
                assert!(spoiler);
                assert_eq!(comment, input.comment);
                assert_eq!(
                    fingerprint, ordinary,
                    "Only ordinary mode retains captured public identity"
                );
                let before = f.counts().await;
                assert!(
                    f.write(0, Some(&a), ordinary, &input, ip + 100, Utc::now())
                        .await
                        .is_err()
                );
                assert_eq!(
                    f.counts().await,
                    before,
                    "Replay cannot leave an orphan post or thread"
                );
                ip += 1;
            }
        }
    }
    f.role("moderator").await;
    // Text wrappers remain valid after the new attachment proof fields exist.
    for ordinary in [false, true] {
        f.write(0, None, ordinary, &post(), ip, Utc::now())
            .await
            .unwrap();
        ip += 1;
    }
    assert_eq!(f.counts().await.3, 0, "No committed handoff state remains");
}

database_case!(
    receipt_rollback_board_policy_and_filename_admission,
    denied_policies
);
async fn denied_policies(f: &Fixture) {
    let op = f
        .write(0, None, true, &post(), 40, Utc::now())
        .await
        .unwrap();
    let mut retry_ip = 60;
    for ordinary in [false, true] {
        for setting in [
            "UPDATE content.boards SET image_limit=0 WHERE slug=$1",
            "UPDATE content.boards SET text_only=true WHERE slug=$1",
        ] {
            let a = f.approved("owned.png", false).await;
            sqlx::query(setting)
                .bind(&f.board)
                .execute(&f.owner)
                .await
                .unwrap();
            let before = f.counts().await;
            assert!(
                f.write(op, Some(&a), ordinary, &post(), 41, Utc::now())
                    .await
                    .is_err()
            );
            assert_eq!(f.counts().await, before);
            assert!(
                f.receipt(&a).await,
                "Policy rejection preserves receipt for a valid retry"
            );
            sqlx::query("UPDATE content.boards SET image_limit=100,text_only=false WHERE slug=$1")
                .bind(&f.board)
                .execute(&f.owner)
                .await
                .unwrap();
            f.write(op, Some(&a), ordinary, &post(), retry_ip, Utc::now())
                .await
                .unwrap();
            retry_ip += 1;
        }
    }
    let a = f.approved("owned.png", false).await;
    sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1")
        .bind(op)
        .execute(&f.owner)
        .await
        .unwrap();
    for ordinary in [false, true] {
        assert!(
            f.write(op, Some(&a), ordinary, &post(), 43, Utc::now())
                .await
                .is_err()
        );
        assert!(f.receipt(&a).await);
    }
    // Source filename admission uses the authenticated approved job filename.
    f.role("janitor").await;
    let abnormal = f.approved("phpé", false).await;
    let result = f
        .write(0, Some(&abnormal), true, &post(), 44, Utc::now())
        .await;
    assert!(
        matches!(result, Err(StoreError::ContentRejected(_))),
        "Abnormal filename must run ordinary janitor admission: {result:?}"
    );
    assert!(f.receipt(&abnormal).await);
}

database_case!(
    janitor_image_timer_and_limit_keep_proof_derived_moderator_exemption,
    image_policy
);
async fn image_policy(f: &Fixture) {
    sqlx::query("UPDATE content.boards SET image_limit=1,posting_reply_seconds=0,posting_image_seconds=21 WHERE slug=$1")
        .bind(&f.board).execute(&f.owner).await.unwrap();
    let at = Utc::now();
    f.role("janitor").await;
    let first = f.approved("first.png", false).await;
    let op = f
        .write(
            0,
            Some(&first),
            true,
            &post(),
            49,
            at - chrono::Duration::seconds(1000),
        )
        .await
        .unwrap();
    f.write(op, None, true, &post(), 50, at).await.unwrap();
    let next = f.approved("next.png", false).await;
    let result = f
        .write(
            op,
            Some(&next),
            true,
            &post(),
            50,
            at + chrono::Duration::seconds(10),
        )
        .await;
    assert!(
        matches!(result,Err(StoreError::PostingCooldownRejected(ref r)) if r.reason==PostingCooldownReason::ImageReply && r.remaining_seconds==1),
        "Janitor image interval is ceil-half: {result:?}"
    );
    assert!(f.receipt(&next).await);
    f.write(
        op,
        Some(&next),
        true,
        &post(),
        50,
        at + chrono::Duration::seconds(11),
    )
    .await
    .unwrap();
    let over = f.approved("over.png", false).await;
    assert!(
        f.write(op, Some(&over), true, &post(), 51, at)
            .await
            .is_err(),
        "Janitor cannot bypass the reply image count"
    );
    assert!(f.receipt(&over).await);
    f.role("moderator").await;
    f.write(op, Some(&over), true, &post(), 51, at)
        .await
        .unwrap();
    let source = f.approved("source.png", false).await;
    f.write(
        op,
        Some(&source),
        false,
        &post(),
        50,
        at + chrono::Duration::seconds(16),
    )
    .await
    .unwrap();
}

struct Proof {
    ticket: Vec<u8>,
    id: i64,
    parent: i64,
    at: DateTime<Utc>,
}
impl Fixture {
    async fn proof(&self, parent: i64, a: Option<&NewAttachment>, existing: Option<i64>) -> Proof {
        self.proof_with_body(parent, a, existing, &post()).await
    }

    async fn proof_with_body(
        &self,
        parent: i64,
        a: Option<&NewAttachment>,
        existing: Option<i64>,
        input: &NewPost,
    ) -> Proof {
        let id = match existing {
            Some(id) => id,
            None => sqlx::query_scalar("SELECT nextval('content.post_number')")
                .fetch_one(&self.owner)
                .await
                .unwrap(),
        };
        let ticket: Vec<u8> =
            sqlx::query_scalar("SELECT sha256(convert_to(gen_random_uuid()::text,'UTF8'))")
                .fetch_one(&self.owner)
                .await
                .unwrap();
        let at = DateTime::from_timestamp(Utc::now().timestamp(), 0).unwrap();
        let sql = if a.is_some() {
            "SELECT staff_identity.issue_source_attachment_post_authority($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23)"
        } else {
            "SELECT staff_identity.issue_source_post_authority($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20)"
        };
        let mut query = sqlx::query(sql)
            .bind(&ticket)
            .bind(&self.session)
            .bind(&self.csrf)
            .bind(900_i32)
            .bind(false)
            .bind(id)
            .bind(&self.board)
            .bind(parent)
            .bind(&input.name)
            .bind(&input.subject)
            .bind(&input.comment)
            .bind(at)
            .bind(true)
            .bind(4000_i32)
            .bind(None::<Vec<u8>>)
            .bind(None::<String>)
            .bind("capcode_mod")
            .bind(None::<String>)
            .bind(true)
            .bind(true);
        if let Some(a) = a {
            query = query
                .bind(&a.upload.id)
                .bind(&a.upload.capability)
                .bind(a.spoiler);
        }
        query.execute(&self.auth).await.unwrap();
        Proof {
            ticket,
            id,
            parent,
            at,
        }
    }

    async fn direct_tx(
        &self,
        p: &Proof,
        a: Option<&NewAttachment>,
    ) -> Transaction<'static, Postgres> {
        let mut tx = self.staff.begin().await.unwrap();
        let key = PosterIdKey::parse(&self.key).unwrap();
        let actor = key.public_posting_rate_identity(peer(200));
        sqlx::query("SELECT content.lock_posting_actor($1,false)")
            .bind(actor.as_bytes().as_slice())
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("SELECT set_config('board.posting_actor',encode($1::bytea,'hex'),true),set_config('board.staff_post_ticket',encode($2::bytea,'hex'),true),set_config('board.staff_raw_name_nonempty','true',true),set_config('board.wordfilter_payload','',true),set_config('board.post_trip','',true),set_config('board.staff_attachment_job',$3,true),set_config('board.staff_attachment_capability',$4,true),set_config('board.staff_attachment_spoiler',$5,true)")
            .bind(actor.as_bytes().as_slice()).bind(&p.ticket).bind(a.map_or("",|a|a.upload.id.as_str()))
            .bind(a.map_or("",|a|a.upload.capability.as_str())).bind(a.map_or("",|a|if a.spoiler {"true"} else {"false"}))
            .execute(&mut *tx).await.unwrap();
        sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
            .bind(&self.board)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("SELECT id FROM content.threads WHERE board=$1 AND id=$2 FOR UPDATE")
            .bind(&self.board)
            .bind(p.parent)
            .execute(&mut *tx)
            .await
            .unwrap();
        tx
    }

    async fn direct_insert(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        p: &Proof,
    ) -> Result<(), sqlx::Error> {
        let input = post();
        sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at) VALUES($1,$2,$3,$4,$5,$6,$7)")
            .bind(p.id).bind(&self.board).bind(p.parent).bind(input.name).bind(input.subject)
            .bind(input.comment).bind(p.at).execute(&mut **tx).await.map(|_|())
    }

    async fn proof_exists(&self, p: &Proof) -> bool {
        sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM post_secrets.staff_post_intents WHERE token_hash=$1)",
        )
        .bind(&p.ticket)
        .fetch_one(&self.owner)
        .await
        .unwrap()
    }
}

fn sqlstate(error: &sqlx::Error, code: &str) {
    assert_eq!(
        error.as_database_error().and_then(|e| e.code()).as_deref(),
        Some(code),
        "{error}"
    );
}

database_case!(
    proof_binds_exact_receipt_and_cannot_forge_handoff_or_attach_existing_post,
    direct_authority
);
async fn direct_authority(f: &Fixture) {
    let op = f
        .write(0, None, true, &post(), 70, Utc::now())
        .await
        .unwrap();
    let a = f.approved("a.png", true).await;
    let b = f.approved("b.png", true).await;
    let proof = f.proof(op, Some(&a), None).await;
    let before = f.counts().await;
    for (job, capability, spoiler) in [
        (&b.upload.id, &a.upload.capability, "true"),
        (&a.upload.id, &b.upload.capability, "true"),
        (&a.upload.id, &a.upload.capability, "false"),
    ] {
        let mut tx = f.direct_tx(&proof, Some(&a)).await;
        sqlx::query("SELECT set_config('board.staff_attachment_job',$1,true),set_config('board.staff_attachment_capability',$2,true),set_config('board.staff_attachment_spoiler',$3,true)")
            .bind(job).bind(capability).bind(spoiler).execute(&mut *tx).await.unwrap();
        sqlstate(
            &f.direct_insert(&mut tx, &proof).await.unwrap_err(),
            "28000",
        );
        tx.rollback().await.unwrap();
        assert!(f.proof_exists(&proof).await);
        assert!(f.receipt(&a).await && f.receipt(&b).await);
        assert_eq!(f.counts().await, before);
    }
    // Correct proof consumption cannot be committed without its matching INSERT.
    let mut tx = f.direct_tx(&proof, Some(&a)).await;
    let input = post();
    sqlx::query("SELECT content.consume_staff_post_authority($1,$2,$3,$4,$5,$6,$7,$8)")
        .bind(&proof.ticket)
        .bind(proof.id)
        .bind(&f.board)
        .bind(op)
        .bind(&input.name)
        .bind(&input.subject)
        .bind(&input.comment)
        .bind(proof.at)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlstate(&tx.commit().await.unwrap_err(), "28000");
    assert!(f.proof_exists(&proof).await);
    assert!(f.receipt(&a).await);
    assert_eq!(f.counts().await, before);
    // An explicit caller rollback restores both the consumed proof and receipt.
    let mut tx = f.direct_tx(&proof, Some(&a)).await;
    f.direct_insert(&mut tx, &proof).await.unwrap();
    tx.rollback().await.unwrap();
    assert!(f.proof_exists(&proof).await);
    assert!(f.receipt(&a).await);
    let mut tx = f.direct_tx(&proof, Some(&a)).await;
    f.direct_insert(&mut tx, &proof).await.unwrap();
    tx.commit().await.unwrap();
    assert!(!f.proof_exists(&proof).await);
    assert!(!f.receipt(&a).await);
    let existing = f.proof(op, Some(&b), Some(proof.id)).await;
    let mut tx = f.direct_tx(&existing, Some(&b)).await;
    sqlstate(
        &f.direct_insert(&mut tx, &existing).await.unwrap_err(),
        "28000",
    );
    tx.rollback().await.unwrap();
    assert!(f.receipt(&b).await);
    // A text-only proof never permits appending an attachment by forged settings.
    let text = f.proof(op, None, None).await;
    let mut tx = f.direct_tx(&text, Some(&b)).await;
    sqlstate(&f.direct_insert(&mut tx, &text).await.unwrap_err(), "28000");
    tx.rollback().await.unwrap();
    let mut tx = f.direct_tx(&text, None).await;
    f.direct_insert(&mut tx, &text).await.unwrap();
    tx.commit().await.unwrap();
    // All runtime roles lack private handoff/table access and owner-only media calls.
    for connection in [&f.staff, &f.public, &f.auth] {
        for sql in [
            "SELECT * FROM post_secrets.staff_attachment_handoffs",
            "INSERT INTO post_secrets.staff_attachment_handoffs SELECT * FROM post_secrets.staff_attachment_handoffs WHERE false",
            "SET ROLE board_staff_post_owner",
            "SET ROLE board_attachment_owner",
        ] {
            let mut control = f.owner.begin().await.unwrap();
            sqlx::query(sql).execute(&mut *control).await.unwrap();
            control.rollback().await.unwrap();
            sqlstate(
                &sqlx::query(sql).execute(connection).await.unwrap_err(),
                "42501",
            );
        }
        let error=sqlx::query("SELECT content.consume_staff_attachment_receipt($1,$2,$3,$4,sha256(convert_to($5,'UTF8')),true,true)")
            .bind(proof.id).bind(&f.board).bind(op).bind(&b.upload.id).bind(&b.upload.capability)
            .execute(connection).await.unwrap_err();
        sqlstate(&error, "42501");
    }
    let forged = f.proof(op, None, None).await;
    let mut tx = f.direct_tx(&forged, None).await;
    sqlx::query("SELECT set_config('board.staff_post_ticket',repeat('0',64),true),set_config('board.staff_authorized_limits','true',true),set_config('board.staff_is_admin','true',true),set_config('board.staff_attachment_job',$1,true),set_config('board.staff_attachment_capability',$2,true),set_config('board.staff_attachment_spoiler','true',true)")
        .bind(&b.upload.id).bind(&b.upload.capability).execute(&mut *tx).await.unwrap();
    sqlstate(
        &f.direct_insert(&mut tx, &forged).await.unwrap_err(),
        "28000",
    );
    tx.rollback().await.unwrap();
    assert!(f.receipt(&b).await);
}

async fn wait_blocked(owner: &PgPool, pid: i32, blocker: i32) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let blocked: bool = sqlx::query_scalar("SELECT $2=ANY(pg_blocking_pids($1))")
                .bind(pid)
                .bind(blocker)
                .fetch_one(owner)
                .await
                .unwrap();
            if blocked {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("The runtime writer must really wait on the fixture's lock");
}

async fn pid(tx: &mut Transaction<'_, Postgres>) -> i32 {
    sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut **tx)
        .await
        .unwrap()
}

impl Fixture {
    async fn snapshot(&self) -> serde_json::Value {
        let jobs = self.jobs.lock().unwrap().clone();
        sqlx::query_scalar("SELECT jsonb_build_object('posts',(SELECT coalesce(jsonb_agg(to_jsonb(p) ORDER BY id),'[]') FROM content.posts p WHERE board=$1),'threads',(SELECT coalesce(jsonb_agg(to_jsonb(t) ORDER BY id),'[]') FROM content.threads t WHERE board=$1),'media',(SELECT coalesce(jsonb_agg(to_jsonb(m) ORDER BY post_id),'[]') FROM content.post_media m WHERE job_id=ANY($2)),'audit',(SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY id),'[]') FROM content.moderation_audit a WHERE board=$1),'history',(SELECT coalesce(jsonb_agg(to_jsonb(h) ORDER BY post_id),'[]') FROM post_secrets.posting_history h WHERE board=$1),'actions',(SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY actor_hash),'[]') FROM post_secrets.posting_thread_actions a WHERE board=$1),'clock',(SELECT last_number FROM content.media_clock))")
            .bind(&self.board).bind(&jobs).fetch_one(&self.owner).await.unwrap()
    }
}

database_case!(
    role_scope_session_and_recent_auth_revocation_survive_real_lock_waits,
    revoked_authority
);
async fn revoked_authority(f: &Fixture) {
    let op = f
        .write(0, None, true, &post(), 80, Utc::now())
        .await
        .unwrap();
    for change in [
        "UPDATE staff_identity.accounts SET role='janitor' WHERE id=$1",
        "UPDATE staff_identity.accounts SET revoked_at=clock_timestamp() WHERE id=$1",
        "UPDATE staff_identity.accounts SET deny_boards=ARRAY[$2] WHERE id=$1",
        "UPDATE staff_identity.sessions SET expires_at=clock_timestamp()-interval '1 second' WHERE account_id=$1",
        "UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp()-interval '11 minutes' WHERE account_id=$1",
    ] {
        let a = f.approved("revocation.png", false).await;
        let proof = f.proof(op, Some(&a), None).await;
        let mut writer = f.direct_tx(&proof, Some(&a)).await;
        let writer_pid = pid(&mut writer).await;
        let mut held = f.owner.begin().await.unwrap();
        let blocker = pid(&mut held).await;
        let mut update = sqlx::query(change).bind(f.account);
        if change.contains("$2") {
            update = update.bind(&f.board);
        }
        update.execute(&mut *held).await.unwrap();
        let before = f.snapshot().await;
        let owned = f.clone();
        let ticket = proof.ticket.clone();
        let task = tokio::spawn(async move {
            let result = owned.direct_insert(&mut writer, &proof).await;
            writer.rollback().await.unwrap();
            result
        });
        wait_blocked(&f.owner, writer_pid, blocker).await;
        held.commit().await.unwrap();
        let error = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        sqlstate(&error, "28000");
        assert_eq!(
            f.snapshot().await,
            before,
            "Revocation cannot leave content, counters, history, audit or media behind"
        );
        assert!(f.receipt(&a).await);
        let retained: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM post_secrets.staff_post_intents WHERE token_hash=$1)",
        )
        .bind(&ticket)
        .fetch_one(&f.owner)
        .await
        .unwrap();
        assert!(retained, "Failed direct consumption rolls proof back");
        sqlx::query("UPDATE staff_identity.accounts SET role='moderator',revoked_at=NULL,deny_boards=ARRAY[]::text[] WHERE id=$1")
            .bind(f.account).execute(&f.owner).await.unwrap();
        sqlx::query("UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp(),expires_at=clock_timestamp()+interval '8 hours',last_activity_at=clock_timestamp() WHERE account_id=$1")
            .bind(f.account).execute(&f.owner).await.unwrap();
    }
}

database_case!(
    job_lock_rechecks_proof_receipt_and_recent_auth_expiry_and_cancellation,
    waited_media
);
async fn waited_media(f: &Fixture) {
    let op = f
        .write(0, None, true, &post(), 90, Utc::now())
        .await
        .unwrap();
    let defaults: (String, String) = sqlx::query_as(
        "SELECT current_setting('lock_timeout'),current_setting('statement_timeout')",
    )
    .fetch_one(&f.staff)
    .await
    .unwrap();
    assert_eq!(
        defaults,
        ("2s".into(), "5s".into()),
        "Production runtime timeout defaults remain unchanged"
    );
    for boundary in ["proof", "receipt", "recent-auth", "session"] {
        let a = f.approved("expiry.png", false).await;
        let proof = f.proof(op, Some(&a), None).await;
        // Set a near deadline, then verify a real lock wait spans that deadline.
        let deadline:DateTime<Utc>=match boundary {
            "proof"=>sqlx::query_scalar("UPDATE post_secrets.staff_post_intents SET expires_at=clock_timestamp()+interval '2 seconds' WHERE token_hash=$1 RETURNING expires_at")
                .bind(&proof.ticket).fetch_one(&f.owner).await.unwrap(),
            "receipt"=>sqlx::query_scalar("UPDATE media.jobs SET created_at=clock_timestamp()-interval '2 hours'+interval '2 seconds' WHERE id=$1 RETURNING created_at+interval '2 hours'")
                .bind(&a.upload.id).fetch_one(&f.owner).await.unwrap(),
            "recent-auth"=>sqlx::query_scalar("UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp()-interval '10 minutes'+interval '2 seconds' WHERE account_id=$1 RETURNING authenticated_at+interval '10 minutes'")
                .bind(f.account).fetch_one(&f.owner).await.unwrap(),
            _=>sqlx::query_scalar("UPDATE staff_identity.sessions SET expires_at=clock_timestamp()+interval '2 seconds' WHERE account_id=$1 RETURNING expires_at")
                .bind(f.account).fetch_one(&f.owner).await.unwrap(),
        };
        let mut writer = f.direct_tx(&proof, Some(&a)).await;
        // This explicit expiry probe must outwait its two-second deadline.
        // Only this transaction gets a longer bounded lock timeout; it is
        // not coverage of the service's ordinary two-second lock timeout.
        sqlx::query("SET LOCAL lock_timeout = '4s'")
            .execute(&mut *writer)
            .await
            .unwrap();
        let writer_pid = pid(&mut writer).await;
        let mut held = f.owner.begin().await.unwrap();
        sqlx::query("SELECT id FROM media.jobs WHERE id=$1 FOR UPDATE")
            .bind(&a.upload.id)
            .execute(&mut *held)
            .await
            .unwrap();
        let blocker = pid(&mut held).await;
        let before = f.snapshot().await;
        let owned = f.clone();
        let ticket = proof.ticket.clone();
        let task = tokio::spawn(async move {
            let result = owned.direct_insert(&mut writer, &proof).await;
            writer.rollback().await.unwrap();
            result
        });
        wait_blocked(&f.owner, writer_pid, blocker).await;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let elapsed: bool = sqlx::query_scalar("SELECT clock_timestamp()>$1")
                    .bind(deadline)
                    .fetch_one(&f.owner)
                    .await
                    .unwrap();
                if elapsed {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert!(
            !task.is_finished(),
            "The expiry check must occur after releasing the job lock"
        );
        held.commit().await.unwrap();
        let error = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        sqlstate(
            &error,
            if boundary == "receipt" {
                "P0002"
            } else {
                "28000"
            },
        );
        assert_eq!(
            f.snapshot().await,
            before,
            "Expired {boundary} rolls back post, audit, media number, history and counters"
        );
        assert!(f.receipt(&a).await);
        let retained: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM post_secrets.staff_post_intents WHERE token_hash=$1)",
        )
        .bind(&ticket)
        .fetch_one(&f.owner)
        .await
        .unwrap();
        assert!(retained);
        sqlx::query("UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp(),expires_at=clock_timestamp()+interval '8 hours',last_activity_at=clock_timestamp() WHERE account_id=$1")
            .bind(f.account).execute(&f.owner).await.unwrap();
    }
    let a = f.approved("cancel.png", false).await;
    let proof = f.proof(op, Some(&a), None).await;
    let mut writer = f.direct_tx(&proof, Some(&a)).await;
    let writer_pid = pid(&mut writer).await;
    let mut held = f.owner.begin().await.unwrap();
    sqlx::query("SELECT id FROM media.jobs WHERE id=$1 FOR UPDATE")
        .bind(&a.upload.id)
        .execute(&mut *held)
        .await
        .unwrap();
    let blocker = pid(&mut held).await;
    // Queue the real public cancellation before the staff's receipt consumption.
    let cancellation = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let cancel_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&cancellation)
        .await
        .unwrap();
    let job = a.upload.id.clone();
    let cap = a.upload.capability.clone();
    let cancel = tokio::spawn(async move { cancel_upload(&cancellation, &job, &cap).await });
    wait_blocked(&f.owner, cancel_pid, blocker).await;
    let owned = f.clone();
    let before = f.snapshot().await;
    let task = tokio::spawn(async move {
        let result = owned.direct_insert(&mut writer, &proof).await;
        writer.rollback().await.unwrap();
        result
    });
    // The queued canceller can own the tuple lock while waiting for the
    // original transaction. The later writer therefore waits on the
    // canceller, not necessarily directly on the original lock holder.
    // Observe both edges before releasing the holder; preserve the public
    // role's normal two-second lock timeout without retries or relaxation.
    wait_blocked(&f.owner, writer_pid, cancel_pid).await;
    wait_blocked(&f.owner, cancel_pid, blocker).await;
    held.commit().await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), cancel)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let error = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    sqlstate(&error, "P0002");
    assert!(
        !f.receipt(&a).await,
        "The runtime cancellation wins and stays revoked"
    );
    assert_eq!(f.snapshot().await, before);
}

impl Fixture {
    async fn ordinary_proof(&self, parent: i64, a: &NewAttachment) -> (Proof, serde_json::Value) {
        let id = sqlx::query_scalar("SELECT nextval('content.post_number')")
            .fetch_one(&self.owner)
            .await
            .unwrap();
        let ticket: Vec<u8> =
            sqlx::query_scalar("SELECT sha256(convert_to(gen_random_uuid()::text,'UTF8'))")
                .fetch_one(&self.owner)
                .await
                .unwrap();
        let at = DateTime::from_timestamp(Utc::now().timestamp(), 0).unwrap();
        let key = PosterIdKey::parse(&self.key).unwrap();
        let context = key.count_context(&self.board, parent, peer(200)).unwrap();
        let bound = serde_json::json!({
            "poster_id":key.label(&self.board,parent,peer(200)).unwrap(),
            "poster_fingerprint":context.fingerprint,"poster_epoch":context.epoch,
            "post_sage":"false","country":"","country_name":"","flag":"",
            "source_op_reply":"false","dice_result":"","fortune_text":"","fortune_color":"",
            "peer":peer(200).to_string(),"deletion_hash":"synthetic-not-a-password","op_password_proof":""
        });
        let input = post();
        sqlx::query("SELECT staff_identity.issue_ordinary_attachment_post_authority($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23)")
            .bind(&ticket).bind(&self.session).bind(&self.csrf).bind(900_i32).bind(id).bind(&self.board)
            .bind(parent).bind(input.name).bind(input.subject).bind(input.comment).bind(at).bind(false)
            .bind(2000_i32).bind(None::<Vec<u8>>).bind(None::<String>).bind("").bind(None::<String>)
            .bind(true).bind(&bound).bind(true).bind(&a.upload.id).bind(&a.upload.capability).bind(a.spoiler)
            .execute(&self.auth).await.unwrap();
        (
            Proof {
                ticket,
                id,
                parent,
                at,
            },
            bound,
        )
    }
}

database_case!(
    valid_janitor_proof_cannot_forge_moderator_image_limit_exemption,
    forged_limit
);
async fn forged_limit(f: &Fixture) {
    sqlx::query("UPDATE content.boards SET image_limit=1,op_markup=false WHERE slug=$1")
        .bind(&f.board)
        .execute(&f.owner)
        .await
        .unwrap();
    let op = f
        .write(0, None, true, &post(), 110, Utc::now())
        .await
        .unwrap();
    let first = f.approved("first.png", false).await;
    f.write(op, Some(&first), true, &post(), 111, Utc::now())
        .await
        .unwrap();
    f.role("janitor").await;
    let next = f.approved("janitor.png", false).await;
    let (proof, bound) = f.ordinary_proof(op, &next).await;
    let mut tx = f.direct_tx(&proof, Some(&next)).await;
    for (field, value) in bound.as_object().unwrap() {
        sqlx::query("SELECT set_config($1,$2,true)")
            .bind(format!("board.{field}"))
            .bind(value.as_str().unwrap())
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    sqlx::query("SELECT set_config('board.staff_post_options','',true),set_config('board.staff_authorized_limits','true',true),set_config('board.staff_is_admin','true',true),set_config('board.staff_ordinary_post','false',true)")
        .execute(&mut *tx).await.unwrap();
    let before = f.snapshot().await;
    sqlstate(
        &f.direct_insert(&mut tx, &proof).await.unwrap_err(),
        "P0001",
    );
    tx.rollback().await.unwrap();
    assert_eq!(f.snapshot().await, before);
    assert!(f.receipt(&next).await);
    assert!(f.proof_exists(&proof).await);
}

database_case!(
    attachment_handoff_cannot_be_spliced_into_separate_text_proof,
    cross_proof_splice
);
async fn cross_proof_splice(f: &Fixture) {
    let op = f
        .write(0, None, true, &post(), 120, Utc::now())
        .await
        .unwrap();
    let attachment = f.approved("proof-a.png", true).await;
    let a = f.proof(op, Some(&attachment), None).await;
    let mut other = post();
    other.name = "Separate staff identity".into();
    other.subject = "Separate authorized text post".into();
    other.comment = "Proof B has no attachment and a different body".into();
    let b = f.proof_with_body(op, None, Some(a.id), &other).await;
    let before = f.snapshot().await;
    let mut tx = f.direct_tx(&a, Some(&attachment)).await;
    let original = post();
    sqlx::query("SELECT content.consume_staff_post_authority($1,$2,$3,$4,$5,$6,$7,$8)")
        .bind(&a.ticket)
        .bind(a.id)
        .bind(&f.board)
        .bind(op)
        .bind(&original.name)
        .bind(&original.subject)
        .bind(&original.comment)
        .bind(a.at)
        .execute(&mut *tx)
        .await
        .unwrap();
    // A has left a private handoff. B is a separately legitimate text proof
    // for the same id; clearing all request media settings must not splice
    // A's receipt and authorization into B's saved content or identity.
    sqlx::query("SELECT set_config('board.staff_post_ticket',encode($1::bytea,'hex'),true),set_config('board.staff_attachment_job','',true),set_config('board.staff_attachment_capability','',true),set_config('board.staff_attachment_spoiler','',true)")
        .bind(&b.ticket).execute(&mut *tx).await.unwrap();
    let error=sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at) VALUES($1,$2,$3,$4,$5,$6,$7)")
        .bind(b.id).bind(&f.board).bind(op).bind(&other.name).bind(&other.subject).bind(&other.comment)
        .bind(b.at).execute(&mut *tx).await.unwrap_err();
    sqlstate(&error, "28000");
    tx.rollback().await.unwrap();
    assert_eq!(
        f.snapshot().await,
        before,
        "Rejected cross-proof handoff must leave no saved effects"
    );
    assert!(f.proof_exists(&a).await && f.proof_exists(&b).await);
    assert!(f.receipt(&attachment).await);
    assert_eq!(f.counts().await.3, 0);
    // The original exact A proof still works after the attack transaction rolls back.
    let mut tx = f.direct_tx(&a, Some(&attachment)).await;
    f.direct_insert(&mut tx, &a).await.unwrap();
    tx.commit().await.unwrap();
    let saved: (String, String) =
        sqlx::query_as("SELECT name,comment FROM content.posts WHERE id=$1")
            .bind(a.id)
            .fetch_one(&f.owner)
            .await
            .unwrap();
    assert_eq!(saved, (original.name, original.comment));
    assert!(!f.receipt(&attachment).await);
    assert!(f.proof_exists(&b).await);
}

impl Fixture {
    async fn additional_board(&self) -> Self {
        let board = format!("b{}", &self.board[1..]);
        sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,max_authorized_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,image_limit,posting_reply_seconds,posting_image_seconds,posting_thread_seconds,user_thread_limit,user_ids,comment_spoiler_cleanup) VALUES($1,'Owned concurrent staff media','Synthetic',2000,4000,100,100,100,10,100,0,0,0,100,true,true)")
            .bind(&board).execute(&self.owner).await.unwrap();
        self.extra_boards.lock().unwrap().push(board.clone());
        let mut other = self.clone();
        other.board = board;
        other
    }
}

database_case!(
    ordinary_and_badged_same_receipt_have_no_job_authority_lock_cycle,
    receipt_authority_order
);
async fn receipt_authority_order(f: &Fixture) {
    let receipt = Arc::new(f.approved("shared-receipt.png", false).await);
    let mut ordinary = f.clone();
    let mut badged = f.additional_board().await;
    // Dedicated real runtime connections make the observed lock graph exact.
    for writer in [&mut ordinary, &mut badged] {
        writer.staff = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&std::env::var("STAFF_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let settings:(String,String,String)=sqlx::query_as("SELECT current_user::text,current_setting('lock_timeout'),current_setting('statement_timeout')")
            .fetch_one(&writer.staff).await.unwrap();
        assert_eq!(settings, ("board_staff".into(), "2s".into(), "5s".into()));
    }
    let ordinary_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&ordinary.staff)
        .await
        .unwrap();
    let badged_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&badged.staff)
        .await
        .unwrap();
    let losing_board = badged.clone();
    let before = losing_board.counts().await;
    let mut held = f.owner.begin().await.unwrap();
    sqlx::query("SELECT id FROM media.jobs WHERE id=$1 FOR UPDATE")
        .bind(&receipt.upload.id)
        .execute(&mut *held)
        .await
        .unwrap();
    let blocker = pid(&mut held).await;
    let a = receipt.clone();
    let ordinary_task = tokio::spawn(async move {
        ordinary
            .write(0, Some(a.as_ref()), true, &post(), 130, Utc::now())
            .await
    });
    // Ordinary filename admission reaches J before its separate auth issuer.
    wait_blocked(&f.owner, ordinary_pid, blocker).await;
    let b = receipt.clone();
    let badged_task = tokio::spawn(async move {
        badged
            .write(0, Some(b.as_ref()), false, &post(), 131, Utc::now())
            .await
    });
    // Same receipt/account/session, different board and posting actor. The
    // badged consumer must queue for J before holding account/session locks.
    // Otherwise ordinary's later auth UPDATE and badged's J wait form a cycle.
    wait_blocked(&f.owner, badged_pid, ordinary_pid).await;
    wait_blocked(&f.owner, ordinary_pid, blocker).await;
    held.commit().await.unwrap();
    let id = tokio::time::timeout(Duration::from_secs(5), ordinary_task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let rejected = tokio::time::timeout(Duration::from_secs(5), badged_task)
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(rejected, Err(StoreError::Conflict(_))),
        "The badged writer must reject the spent receipt, not time out/deadlock: {rejected:?}"
    );
    assert_eq!(
        losing_board.counts().await,
        before,
        "The losing writer rolls back its new thread and post"
    );
    let association: (i64, i64) =
        sqlx::query_as("SELECT count(*),min(post_id) FROM content.post_media WHERE job_id=$1")
            .bind(&receipt.upload.id)
            .fetch_one(&f.owner)
            .await
            .unwrap();
    assert_eq!(association, (1, id));
    assert!(!f.receipt(receipt.as_ref()).await);
    let leftovers:i64=sqlx::query_scalar("SELECT (SELECT count(*) FROM post_secrets.staff_post_intents WHERE account_id=$1)+(SELECT count(*) FROM post_secrets.staff_attachment_handoffs WHERE account_id=$1)")
        .bind(f.account).fetch_one(&f.owner).await.unwrap();
    assert_eq!(
        leftovers, 0,
        "Neither completed store writer leaves authority state behind"
    );
}

database_case!(
    staff_image_only_threads_require_options_and_moderator_authority,
    image_only_threads
);
async fn image_only_threads(f: &Fixture) {
    let mut ip = 1;
    for role in ["moderator", "manager", "admin", "janitor"] {
        f.role(role).await;
        for (ordinary, options) in [(false, ""), (true, ""), (true, "sage"), (true, "nonoko")] {
            if role == "janitor" && !ordinary {
                continue;
            }
            for attached in [false, true] {
                let a = f.approved("owned-image-only.png", false).await;
                let mut input = post();
                input.subject.clear();
                input.comment.clear();
                let before = f.counts().await;
                let result = f
                    .write_with_options(
                        0,
                        attached.then_some(&a),
                        ordinary,
                        &input,
                        ip,
                        Utc::now(),
                        options,
                    )
                    .await;
                let allowed = role != "janitor" && attached && (!ordinary || options == "nonoko");
                if allowed {
                    let id = result.expect("Authorized source Options admit an image-only OP");
                    let (subject, comment, thread): (String, String, i64) = sqlx::query_as(
                        "SELECT subject,comment,thread_id FROM content.posts WHERE id=$1",
                    )
                    .bind(id)
                    .fetch_one(&f.owner)
                    .await
                    .unwrap();
                    assert!(subject.is_empty() && comment.is_empty());
                    assert_eq!(thread, id);
                    assert!(attachment(&f.public, id).await.unwrap().is_some());
                    assert!(!f.receipt(&a).await);
                } else {
                    assert!(matches!(
                        result,
                        Err(StoreError::Invalid(
                            "Error: New threads require a subject or comment."
                        ))
                    ));
                    assert_eq!(
                        f.counts().await,
                        before,
                        "Rejected admission cannot create a post, thread or handoff"
                    );
                    assert!(
                        f.receipt(&a).await,
                        "Rejected admission retains the upload receipt"
                    );
                }
                ip += 1;
            }
        }
    }
    f.role("moderator").await;
    sqlx::query("UPDATE content.boards SET require_subject=true WHERE slug=$1")
        .bind(&f.board)
        .execute(&f.owner)
        .await
        .unwrap();
    let a = f.approved("owned-required-subject.png", false).await;
    let mut input = post();
    input.subject.clear();
    input.comment.clear();
    let before = f.counts().await;
    assert!(matches!(
        f.write(0, Some(&a), false, &input, 60, Utc::now()).await,
        Err(StoreError::Invalid("Error: New threads require a subject."))
    ));
    assert_eq!(f.counts().await, before);
    assert!(f.receipt(&a).await);
}

database_case!(
    fixed_name_trip_admission_runs_for_all_staff_modes,
    staff_name_trip_checks
);
async fn staff_name_trip_checks(f: &Fixture) {
    let mut ip = 70;
    for role in ["moderator", "manager", "admin", "janitor"] {
        f.role(role).await;
        for ordinary in [false, true] {
            if role == "janitor" && !ordinary {
                continue;
            }
            for name in ["moot Ep8pui8Vw2", "\u{10000}\u{309d}", "Owned staff"] {
                let a = f.approved("owned-name-admission.png", false).await;
                let mut input = post();
                input.name = name.into();
                let before = f.counts().await;
                let result = f.write(0, Some(&a), ordinary, &input, ip, Utc::now()).await;
                if name == "\u{10000}\u{309d}" {
                    assert!(
                        matches!(result, Err(StoreError::Database(sqlx::Error::Protocol(_)))),
                        "All staff modes must fail closed on the documented native normalization failure"
                    );
                } else if role == "janitor" && name.starts_with("moot") {
                    assert!(
                        matches!(result,Err(StoreError::ContentRejected(ref message)) if message=="Error: Upload failed.")
                    );
                } else {
                    let id = result
                        .expect("Source moderator exception applies with and without a badge");
                    assert!(attachment(&f.public, id).await.unwrap().is_some());
                    assert!(!f.receipt(&a).await);
                    ip += 1;
                    continue;
                }
                assert_eq!(f.counts().await, before);
                assert!(f.receipt(&a).await);
                ip += 1;
            }
        }
    }
}

database_case!(
    masked_badged_names_do_not_store_the_admission_trip,
    masked_staff_trip
);
async fn masked_staff_trip(f: &Fixture) {
    f.role("moderator").await;
    sqlx::query(
        "UPDATE staff_identity.accounts SET flags=ARRAY['capcode','developer'] WHERE id=$1",
    )
    .bind(f.account)
    .execute(&f.owner)
    .await
    .unwrap();
    let a = f.approved("owned-masked-trip.png", false).await;
    let mut input = post();
    input.name = "Hidden#password".into();
    let id = f
        .write(0, Some(&a), false, &input, 120, Utc::now())
        .await
        .unwrap();
    let (name, trip): (String, Option<String>) =
        sqlx::query_as("SELECT name,trip FROM content.posts WHERE id=$1")
            .bind(id)
            .fetch_one(&f.owner)
            .await
            .unwrap();
    assert_eq!(name, "Anonymous");
    assert!(trip.is_none());
    assert!(!f.receipt(&a).await);
}

database_case!(
    ordinary_staff_writes_require_an_explicit_unbadged_identity,
    missing_ordinary_identity
);
async fn missing_ordinary_identity(f: &Fixture) {
    let input = post();
    let a = f.approved("owned-explicit-identity.png", false).await;
    let before = f.counts().await;
    let key = PosterIdKey::parse(&f.key).unwrap();
    let result = board_store::create_ordinary_staff_post_with_attachment(
        &f.staff,
        &f.board,
        0,
        board_store::StaffPostContent {
            post: &input,
            attachment: Some(&a),
        },
        PostingContext {
            request_start: Utc::now(),
            peer: Some(peer(121)),
            op_password_proof: None,
        },
        PostMetadata {
            keys: PostIdentityKeys {
                tripcode: None,
                poster_id: Some(&key),
            },
            country_database: None,
            flag: "",
            options: "nonoko",
            spoiler: false,
            drawing: None,
        },
        StaffPostAuthority {
            auth_pool: &f.auth,
            session_hash: &f.session,
            csrf_hash: &f.csrf,
            ticket_hash: &[1; 32],
            idle_seconds: 900,
            highlight: false,
            authorized_limits: true,
            raw_name_nonempty: true,
            identity: None,
        },
    )
    .await;
    assert!(matches!(
        result,
        Err(StoreError::Invalid(
            "Ordinary staff posting context is unavailable."
        ))
    ));
    assert_eq!(f.counts().await, before);
    assert!(f.receipt(&a).await);
}

database_case!(
    generated_staff_content_keeps_exact_payload_and_limit_authority,
    generated_staff_content
);
async fn generated_staff_content(f: &Fixture) {
    sqlx::query("UPDATE content.boards SET dice_roll=true,fortune_trip=true WHERE slug=$1")
        .bind(&f.board)
        .execute(&f.owner)
        .await
        .unwrap();
    let mut ip = 1;
    for role in ["moderator", "manager", "admin", "janitor"] {
        f.role(role).await;
        for (enabled, profile) in [(false, 0), (true, 0), (true, 4)] {
            sqlx::query("UPDATE content.boards SET word_filter_enabled=$2,word_filter_profile=$3 WHERE slug=$1")
                .bind(&f.board).bind(enabled).bind(profile as i16).execute(&f.owner).await.unwrap();
            for options in ["dice+1d1", "fortune"] {
                let mut input = post();
                input.comment.clear();
                input.subject.clear();
                let id = f
                    .write_with_options(0, None, true, &input, ip, Utc::now(), options)
                    .await
                    .unwrap();
                ip += 1;
                let saved = board_store::find_post(&f.public, &f.board, id)
                    .await
                    .unwrap();
                assert!(saved.has_generated_comment());
                assert_eq!(saved.staff_authorized_limits, role != "janitor");
                assert!(saved.capcode.is_none());
                let bytes = saved.wordfilter_payload.as_deref().unwrap();
                assert_eq!(
                    &bytes[..4],
                    if role == "janitor" { b"WF03" } else { b"WF04" }
                );
                let prepared =
                    board_domain::wordfiltered_comment::PreparedComment::decode(bytes).unwrap();
                assert!(prepared.has_generated_content());
                assert_eq!(prepared.rolls().is_some(), enabled && profile == 4);
                let lines = saved.formatted_lines();
                assert_eq!(
                    board_domain::filtered_formatting::source_projection(&lines),
                    saved.comment
                );
                assert_eq!(saved.separate_dice_result(), None);
                assert_eq!(saved.separate_fortune_text(), None);
                assert_eq!(saved.separate_fortune_color(), None);
            }
        }
    }
    assert_eq!(f.counts().await.3, 0, "No completed proof remains");
}

database_case!(
    verified_staff_bypasses_network_rules,
    ordinary_staff_network
);
async fn ordinary_staff_network(f: &Fixture) {
    sqlx::query(
        "INSERT INTO admission.network_rules(boards,range_start,range_end) VALUES($1,0,4294967295)",
    )
    .bind(&f.board)
    .execute(&f.owner)
    .await
    .unwrap();
    let reference: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/staff-network-bypass-reference.json")).unwrap();
    let cases = reference["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 16);
    for (index, case) in cases.iter().enumerate() {
        assert_eq!(case["valid"], true);
        assert_eq!(case["range_bypass"], true);
        assert_eq!(case["network_blocked"], false);
        let role = match case["role"].as_str().unwrap() {
            "mod" => "moderator",
            other => other,
        };
        f.role(role).await;
        f.write(0, None, true, &post(), 180 + index as u8, Utc::now())
            .await
            .unwrap();
    }
    f.role("moderator").await;
    let before = f.counts().await;
    assert!(
        matches!(f.write_public(0,None,&post(),209,Utc::now()).await,
        Err(StoreError::ContentRejected(message)) if message.starts_with("Posting from your IP range"))
    );
    assert_eq!(f.counts().await, before);
    for (index, invalid) in [
        "csrf", "session", "role", "scope", "expired", "revoked", "stale",
    ]
    .into_iter()
    .enumerate()
    {
        let mut denied = f.clone();
        match invalid {
            "csrf" => denied.csrf = vec![0; 32],
            "session" => denied.session = vec![0; 32],
            "role" => {
                denied.auth = f.public.clone();
            }
            "scope" => {
                sqlx::query("UPDATE staff_identity.accounts SET deny_boards=ARRAY[$2] WHERE id=$1")
                    .bind(f.account)
                    .bind(&f.board)
                    .execute(&f.owner)
                    .await
                    .unwrap();
            }
            "expired" => {
                sqlx::query("UPDATE staff_identity.sessions SET expires_at=clock_timestamp()-interval '1 second' WHERE token_hash=$1")
                .bind(&f.session).execute(&f.owner).await.unwrap();
            }
            "revoked" => {
                sqlx::query(
                    "UPDATE staff_identity.accounts SET revoked_at=clock_timestamp() WHERE id=$1",
                )
                .bind(f.account)
                .execute(&f.owner)
                .await
                .unwrap();
            }
            "stale" => {
                sqlx::query("UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp()-interval '11 minutes' WHERE token_hash=$1")
                .bind(&f.session).execute(&f.owner).await.unwrap();
            }
            _ => unreachable!(),
        }
        let result = denied
            .write(0, None, true, &post(), 210 + index as u8, Utc::now())
            .await;
        assert!(
            matches!(
                result,
                Err(StoreError::AuthorizationChanged | StoreError::Database(_))
            ),
            "{invalid} must not confer bypass"
        );
        assert_eq!(f.counts().await, before);
        sqlx::query(
            "UPDATE staff_identity.accounts SET revoked_at=NULL,deny_boards='{}' WHERE id=$1",
        )
        .bind(f.account)
        .execute(&f.owner)
        .await
        .unwrap();
        sqlx::query("UPDATE staff_identity.sessions SET expires_at=clock_timestamp()+interval '1 hour',authenticated_at=clock_timestamp() WHERE token_hash=$1")
            .bind(&f.session).execute(&f.owner).await.unwrap();
    }
    // Revocation after the early bypass read must still prevent acceptance at
    // the independent posting-proof boundary after the board lock wait.
    let mut held = f.owner.begin().await.unwrap();
    sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
        .bind(&f.board)
        .execute(&mut *held)
        .await
        .unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *held)
        .await
        .unwrap();
    let pending = f.clone();
    let writer =
        tokio::spawn(async move { pending.write(0, None, true, &post(), 219, Utc::now()).await });
    wait_for_policy_blocker(&f.owner, blocker).await;
    sqlx::query("UPDATE staff_identity.accounts SET revoked_at=clock_timestamp() WHERE id=$1")
        .bind(f.account)
        .execute(&f.owner)
        .await
        .unwrap();
    held.commit().await.unwrap();
    assert!(matches!(
        writer.await.unwrap(),
        Err(StoreError::AuthorizationChanged | StoreError::Database(_))
    ));
    assert_eq!(f.counts().await, before);
    sqlx::query("UPDATE staff_identity.accounts SET revoked_at=NULL WHERE id=$1")
        .bind(f.account)
        .execute(&f.owner)
        .await
        .unwrap();
}

database_case!(
    staff_network_bypass_precedes_hint_providers,
    staff_hint_scope
);
async fn staff_hint_scope(f: &Fixture) {
    use board_domain::content_admission::network::hints::{
        BrowserHeaders, BrowserKey, DerivedHints,
    };
    let key = BrowserKey::parse(&"11".repeat(32)).unwrap();
    let mut hinted = f.clone();
    hinted.hints = Some(DerivedHints::derive(
        Some(&key),
        Some(BrowserHeaders {
            user_agent: Some(b"Firefox/123.4"),
            ..Default::default()
        }),
        None,
    ));
    sqlx::query("INSERT INTO admission.network_rules(boards,range_start,range_end,signatures) VALUES($1,0,4294967295,'030393b9b')")
        .bind(&f.board).execute(&f.owner).await.unwrap();
    let before = f.counts().await;
    let result = hinted.write(0, None, true, &post(), 214, Utc::now()).await;
    result.unwrap();
    assert_eq!(f.counts().await.0, before.0 + 1);
    let mut unknown = hinted.clone();
    unknown.hints = None;
    unknown
        .write(0, None, true, &post(), 217, Utc::now())
        .await
        .unwrap();
    sqlx::query("INSERT INTO admission.network_rules(boards,asn) SELECT $1,15169 FROM generate_series(1,129)")
        .bind(&f.board).execute(&f.owner).await.unwrap();
    unknown
        .write(0, None, true, &post(), 218, Utc::now())
        .await
        .unwrap();
    hinted
        .write(0, None, false, &post(), 215, Utc::now())
        .await
        .unwrap();
    let rule: i64 = sqlx::query_scalar("INSERT INTO admission.network_rules(boards,range_start,range_end,signatures) VALUES('test',0,4294967295,'030393b9b') RETURNING id")
        .fetch_one(&f.owner).await.unwrap();
    hinted.board = "test".into();
    let before = hinted.counts().await;
    assert!(matches!(
        hinted.write_public(0, None, &post(), 242, Utc::now()).await,
        Err(StoreError::NotFound)
    ));
    for (role, developer) in [("janitor", true), ("moderator", false), ("admin", false)] {
        sqlx::query("UPDATE staff_identity.accounts SET role=$2,flags=CASE WHEN $3 THEN ARRAY['developer'] ELSE ARRAY[]::text[] END WHERE id=$1")
            .bind(f.account).bind(role).bind(developer).execute(&f.owner).await.unwrap();
        assert!(matches!(
            hinted.write(0, None, true, &post(), 243, Utc::now()).await,
            Err(StoreError::AuthorizationChanged)
        ));
        assert_eq!(hinted.counts().await, before);
    }
    sqlx::query("UPDATE staff_identity.accounts SET role='moderator',flags=ARRAY['capcode','capcodename','developer'] WHERE id=$1")
        .bind(f.account).execute(&f.owner).await.unwrap();
    let result = hinted.write(0, None, false, &post(), 216, Utc::now()).await;
    let after = hinted.counts().await;
    sqlx::query("DELETE FROM admission.network_rules WHERE id=$1")
        .bind(rule)
        .execute(&f.owner)
        .await
        .unwrap();
    result.unwrap();
    assert_eq!(after.0, before.0 + 1);
}

database_case!(
    accepted_mobile_metadata_is_private_and_independent_of_badge,
    automatic_metadata
);
async fn automatic_metadata(f: &Fixture) {
    use board_domain::content_admission::network::{
        ProviderValue,
        hints::{BrowserHeaders, BrowserKey, DerivedHints},
    };
    let browser_key = BrowserKey::parse(&"11".repeat(32)).unwrap();
    let mut mobile = f.clone();
    mobile.peer_override = Some("192.0.2.17".parse().unwrap());
    mobile.hints = Some(DerivedHints::derive(
        Some(&browser_key),
        Some(BrowserHeaders {
            user_agent: Some(b"Android/1"),
            ..Default::default()
        }),
        None,
    ));
    let ProviderValue::Available(Some(browser)) =
        mobile.hints.as_ref().unwrap().signatures().browser_id
    else {
        panic!("owned browser derivation failed");
    };
    let key = PosterIdKey::parse(&f.key).unwrap();
    let prefix = key
        .automatic_range_prefix(mobile.peer_override.unwrap())
        .unwrap();
    let op = mobile
        .write(0, None, false, &post(), 217, Utc::now())
        .await
        .unwrap();
    mobile.peer_override = Some("192.0.2.18".parse().unwrap());
    for (id, parent) in [
        (op, 0),
        (
            mobile
                .write(op, None, true, &post(), 218, Utc::now())
                .await
                .unwrap(),
            op,
        ),
    ] {
        let saved: (String, Vec<u8>, i64) = sqlx::query_as("SELECT automatic_browser,automatic_prefix,CASE WHEN post_id=thread_id THEN 0 ELSE thread_id END FROM post_secrets.posting_history WHERE post_id=$1")
            .bind(id).fetch_one(&f.owner).await.unwrap();
        assert_eq!(saved.0, browser);
        assert_eq!(saved.1, prefix.as_bytes());
        assert_eq!(saved.2, parent);
    }
    let mut absent = mobile.clone();
    absent.hints = None;
    absent.peer_override = Some("192.0.2.19".parse().unwrap());
    let unknown = absent
        .write(op, None, false, &post(), 219, Utc::now())
        .await
        .unwrap();
    absent.hints = Some(DerivedHints::derive(
        Some(&browser_key),
        Some(BrowserHeaders {
            user_agent: Some(b"Firefox/123.4"),
            ..Default::default()
        }),
        None,
    ));
    absent.peer_override = Some("192.0.2.20".parse().unwrap());
    let desktop = absent
        .write(op, None, false, &post(), 220, Utc::now())
        .await
        .unwrap();
    absent.hints = mobile.hints.clone();
    absent.peer_override = Some("2001:db8::17".parse().unwrap());
    let ipv6 = absent
        .write(op, None, false, &post(), 221, Utc::now())
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM post_secrets.posting_history WHERE post_id=ANY($1) AND automatic_browser IS NULL AND automatic_prefix IS NULL")
        .bind([unknown, desktop, ipv6].as_slice()).fetch_one(&f.owner).await.unwrap();
    assert_eq!(count, 3);
    for runtime in [&f.public, &f.staff] {
        assert!(sqlx::query("SELECT automatic_browser,automatic_prefix FROM post_secrets.posting_history WHERE post_id=$1")
            .bind(op).fetch_all(runtime).await.is_err());
    }
    let visible: serde_json::Value =
        sqlx::query_scalar("SELECT to_jsonb(p) FROM content.posts p WHERE id=$1")
            .bind(op)
            .fetch_one(&f.public)
            .await
            .unwrap();
    assert!(
        !visible
            .as_object()
            .unwrap()
            .contains_key("automatic_browser")
    );
    assert!(
        !visible
            .as_object()
            .unwrap()
            .contains_key("automatic_prefix")
    );
}

database_case!(
    automatic_range_refusal_rolls_back_post_effects_and_records_only_hit,
    automatic_range_refusal
);
async fn automatic_range_refusal(f: &Fixture) {
    use board_domain::content_admission::network::hints::{
        BrowserHeaders, BrowserKey, DerivedHints,
    };
    for pool in [&f.public, &f.staff] {
        assert!(
            sqlx::query_scalar::<_, bool>(board_store::AUTOMATIC_RANGE_READINESS_SQL)
                .fetch_one(pool)
                .await
                .unwrap()
        );
    }
    for mutation in [
        "ALTER FUNCTION content.automatic_range_events(text,bigint,bytea,text) SECURITY INVOKER",
        "REVOKE EXECUTE ON FUNCTION content.automatic_range_events(text,bigint,bytea,text) FROM board_public",
        "GRANT SELECT ON restrictions.automatic_ranges TO board_public",
        "GRANT USAGE ON SEQUENCE restrictions.automatic_ranges_id_seq TO board_staff",
    ] {
        let mut tx = f.owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_restriction_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::raw_sql(mutation).execute(&mut *tx).await.unwrap();
        // Inspect public permissions under the existing owner without granting
        // runtime role membership or committing schema changes.
        let query = board_store::AUTOMATIC_RANGE_READINESS_SQL
            .replace("current_user", "'board_public'::name");
        let ready: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(query))
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert!(!ready, "Automatic readiness accepted an authority mutation");
        tx.rollback().await.unwrap();
    }
    let key = BrowserKey::parse(&"11".repeat(32)).unwrap();
    let mut mobile = f.clone();
    mobile.hints = Some(DerivedHints::derive(
        Some(&key),
        Some(BrowserHeaders {
            user_agent: Some(b"Android/1"),
            ..Default::default()
        }),
        None,
    ));
    let target = mobile
        .write(0, None, false, &post(), 222, Utc::now())
        .await
        .unwrap();
    // A custom global ban is not a source global-rule template. Its existing
    // public issuer must not acquire the automatic producer as a side effect.
    let ban: i64 = sqlx::query_scalar("SELECT content.issue_post_restriction($1,$2,900,$3,$4,true,false,60,'Owned custom ban','Synthetic')")
        .bind(&f.session).bind(&f.csrf).bind(&f.board).bind(target).fetch_one(&f.staff).await.unwrap();
    assert_eq!(automatic_stats(f).await, (0, 0));
    // Owned private fixture exercises the future qualified producer seam.
    // It does not pretend the custom-ban endpoint provides template authority.
    let mut tx = f.owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    for _ in 0..2 {
        sqlx::query("SELECT restrictions.record_automatic_range($1,$2,$3)")
            .bind(&f.board)
            .bind(target)
            .bind(ban)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    tx.commit().await.unwrap();
    assert_eq!(automatic_stats(f).await, (1, 0));
    // Source authentication bypasses the entire network function before its
    // browser/provider reads, even for unbadged staff with unknown hints.
    mobile
        .write(0, None, true, &post(), 240, Utc::now())
        .await
        .unwrap();
    let mut staff_unknown = mobile.clone();
    staff_unknown.hints = None;
    staff_unknown
        .write(0, None, true, &post(), 241, Utc::now())
        .await
        .unwrap();
    assert_eq!(automatic_stats(f).await, (1, 0));
    let rule: i64 = sqlx::query_scalar("INSERT INTO admission.rules(board,pattern,log) VALUES($1,'Synthetic attachment body',true) RETURNING id")
        .bind(&f.board).fetch_one(&f.owner).await.unwrap();
    let attachment = f.approved("automatic.png", false).await;
    let before = f.counts().await;
    let rejected = mobile
        .write_public(0, Some(&attachment), &post(), 223, Utc::now())
        .await;
    assert!(matches!(
        rejected,
        Err(StoreError::AutomaticRangeRejected {
            cookie_refreshed: true
        })
    ));
    assert_eq!(f.counts().await, before);
    assert_eq!(automatic_stats(f).await, (1, 1));
    let effects: i64 = sqlx::query_scalar("SELECT (SELECT count(*) FROM admission.logs WHERE rule_id=$1)+(SELECT count(*) FROM admission.hits WHERE rule_id=$1)")
        .bind(rule).fetch_one(&f.owner).await.unwrap();
    assert_eq!(
        effects, 0,
        "Only the separate automatic hit may survive rejection"
    );
    sqlx::query(
        "UPDATE media.jobs SET created_at=clock_timestamp()-interval '2 hours' WHERE id=$1",
    )
    .bind(&attachment.upload.id)
    .execute(&f.owner)
    .await
    .unwrap();
    assert!(matches!(
        mobile
            .write_public(0, Some(&attachment), &post(), 230, Utc::now())
            .await,
        Err(StoreError::NotFound)
    ));
    assert_eq!(
        automatic_stats(f).await,
        (1, 1),
        "An expired upload cannot add an automatic hit"
    );
    sqlx::query(
        "INSERT INTO admission.network_rules(boards,range_start,range_end) VALUES($1,0,4294967295)",
    )
    .bind(&f.board)
    .execute(&f.owner)
    .await
    .unwrap();
    let rejected = mobile.write_public(0, None, &post(), 224, Utc::now()).await;
    assert!(
        matches!(rejected, Err(StoreError::ContentRejected(message)) if message.contains("permanently blocked"))
    );
    assert_eq!(
        automatic_stats(f).await,
        (1, 1),
        "Configured network rule short-circuits automatic range stage"
    );
    sqlx::query("DELETE FROM admission.network_rules WHERE boards=$1")
        .bind(&f.board)
        .execute(&f.owner)
        .await
        .unwrap();
    // A reply has a positive parent; this event is only for parent zero.
    mobile
        .write_public(target, None, &post(), 225, Utc::now())
        .await
        .unwrap();
    // Badged normal-board posts retain the source filter-scope exemption.
    mobile
        .write(0, None, false, &post(), 226, Utc::now())
        .await
        .unwrap();
    assert_eq!(automatic_stats(f).await, (1, 1));
    board_store::delete_post(&f.staff, &f.board, target)
        .await
        .unwrap();
    let history_gone: bool = sqlx::query_scalar(
        "SELECT NOT EXISTS(SELECT 1 FROM post_secrets.posting_history WHERE post_id=$1)",
    )
    .bind(target)
    .fetch_one(&f.owner)
    .await
    .unwrap();
    assert!(history_gone);
    assert_eq!(
        automatic_stats(f).await,
        (1, 1),
        "Deleting the target must not erase its active event"
    );
    assert!(matches!(
        mobile.write_public(0, None, &post(), 229, Utc::now()).await,
        Err(StoreError::AutomaticRangeRejected {
            cookie_refreshed: true
        })
    ));
    assert_eq!(automatic_stats(f).await, (1, 2));
    let mut unknown = mobile.clone();
    unknown.hints = None;
    assert!(matches!(
        unknown
            .write_public(0, None, &post(), 227, Utc::now())
            .await,
        Err(StoreError::Database(_))
    ));
    assert_eq!(automatic_stats(f).await, (1, 2));
    // Expiry uses the database clock, independently of request/session age.
    let mut tx = f.owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE restrictions.automatic_ranges SET created_at=floor(extract(epoch FROM clock_timestamp()))::bigint-7200 WHERE board=$1")
        .bind(&f.board).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    mobile
        .write_public(0, None, &post(), 228, Utc::now())
        .await
        .unwrap();
    assert_eq!(automatic_stats(f).await, (1, 2));
    sqlx::query("DELETE FROM admission.rules WHERE id=$1")
        .bind(rule)
        .execute(&f.owner)
        .await
        .unwrap();
}

async fn automatic_stats(f: &Fixture) -> (i64, i64) {
    let mut tx = f.owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let stats = sqlx::query_as("SELECT count(*),coalesce(sum(hit_count),0)::bigint FROM restrictions.automatic_ranges WHERE board=$1")
        .bind(&f.board).fetch_one(&mut *tx).await.unwrap();
    tx.rollback().await.unwrap();
    stats
}

database_case!(
    automatic_event_policy_serializes_both_directions,
    automatic_policy_locks
);
async fn automatic_policy_locks(f: &Fixture) {
    use board_domain::content_admission::network::hints::{
        BrowserHeaders, BrowserKey, DerivedHints,
    };
    let key = BrowserKey::parse(&"11".repeat(32)).unwrap();
    let mut mobile = f.clone();
    mobile.hints = Some(DerivedHints::derive(
        Some(&key),
        Some(BrowserHeaders {
            user_agent: Some(b"Android/1"),
            ..Default::default()
        }),
        None,
    ));
    let target = mobile
        .write(0, None, false, &post(), 230, Utc::now())
        .await
        .unwrap();
    let mut held = f.public.begin().await.unwrap();
    sqlx::query("SELECT * FROM content.lock_content_admission($1,'192.0.2.231')")
        .bind(&f.board)
        .fetch_all(&mut *held)
        .await
        .unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *held)
        .await
        .unwrap();
    let owner = f.owner.clone();
    let board = f.board.clone();
    let writer = tokio::spawn(async move {
        let mut tx = owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_restriction_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("SELECT restrictions.record_automatic_range($1,$2,1)")
            .bind(&board)
            .bind(target)
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
    });
    wait_for_policy_blocker(&f.owner, blocker).await;
    assert!(!writer.is_finished());
    held.commit().await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), writer)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(automatic_stats(f).await, (1, 0));
    // Publish an expired event, then hold its reactivation uncommitted. A post
    // waiting on the generation must see the committed reactivation afterward.
    let mut held = f.owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *held)
        .await
        .unwrap();
    sqlx::query("UPDATE restrictions.automatic_ranges SET created_at=1 WHERE board=$1")
        .bind(&f.board)
        .execute(&mut *held)
        .await
        .unwrap();
    held.commit().await.unwrap();
    let mut held = f.owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *held)
        .await
        .unwrap();
    sqlx::query("SELECT content.lock_automatic_range_policy(true)")
        .execute(&mut *held)
        .await
        .unwrap();
    sqlx::query("UPDATE restrictions.automatic_ranges SET created_at=floor(extract(epoch FROM clock_timestamp()))::bigint WHERE board=$1")
        .bind(&f.board).execute(&mut *held).await.unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *held)
        .await
        .unwrap();
    let writer =
        tokio::spawn(async move { mobile.write_public(0, None, &post(), 232, Utc::now()).await });
    wait_for_policy_blocker(&f.owner, blocker).await;
    assert!(!writer.is_finished());
    held.commit().await.unwrap();
    let result = tokio::time::timeout(Duration::from_secs(5), writer)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        result,
        Err(StoreError::AutomaticRangeRejected {
            cookie_refreshed: true
        })
    ));
    assert_eq!(automatic_stats(f).await, (1, 1));
}

async fn wait_for_policy_blocker(owner: &PgPool, blocker: i32) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let blocked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity a WHERE a.datname=current_database() AND $1=ANY(pg_blocking_pids(a.pid)))")
            .bind(blocker).fetch_one(owner).await.unwrap();
        if blocked {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "Expected generation wait was not observed"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

database_case!(
    authenticated_staff_refusal_and_quiet_paths_preserve_uploads,
    terminal_staff_filters
);
async fn terminal_staff_filters(f: &Fixture) {
    let rule: i64 = sqlx::query_scalar(
        "INSERT INTO admission.rules(board,pattern) VALUES($1,'synthetic') RETURNING id",
    )
    .bind(&f.board)
    .fetch_one(&f.owner)
    .await
    .unwrap();
    let mut ip = 210;
    for role in ["janitor", "moderator", "manager", "admin"] {
        f.role(role).await;
        for quiet in [false, true] {
            sqlx::query("UPDATE admission.rules SET quiet=$2 WHERE id=$1")
                .bind(rule)
                .bind(quiet)
                .execute(&f.owner)
                .await
                .unwrap();
            let upload = f.approved("owned.png", false).await;
            let before = f.counts().await;
            let result = f
                .write(0, Some(&upload), true, &post(), ip, Utc::now())
                .await;
            if quiet {
                assert!(matches!(result, Err(StoreError::ContentQuiet { .. })));
            } else {
                assert!(matches!(result, Err(StoreError::ContentRejected(_))));
            }
            assert_eq!(f.counts().await, before);
            assert!(f.receipt(&upload).await);
            ip += 1;
        }
    }
    let hits: i64 = sqlx::query_scalar("SELECT count(*) FROM admission.hits WHERE rule_id=$1")
        .bind(rule)
        .fetch_one(&f.owner)
        .await
        .unwrap();
    assert_eq!(hits, 8);
    let intents: i64 =
        sqlx::query_scalar("SELECT count(*) FROM post_secrets.staff_post_intents WHERE board=$1")
            .bind(&f.board)
            .fetch_one(&f.owner)
            .await
            .unwrap();
    assert_eq!(intents, 0);
}
