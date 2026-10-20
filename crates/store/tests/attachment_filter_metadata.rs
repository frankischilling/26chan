#![cfg(feature = "database-tests")]
use board_store::{
    media::MediaQueue,
    media_assets::{
        InputComponent, InputReceipt, MediaFormat, OutputMetadata, OutputVariants, SourceProfile,
        SourceProvenance, StillOutput,
    },
    media_intake::IntakeStore,
};
use sqlx::PgPool;

#[tokio::test]
async fn preflight_binds_capability_context_and_original_digest_eligibility() {
    let admin = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = PgPool::connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let staff = PgPool::connect(&std::env::var("STAFF_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let intake = IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let vip = VipFixture::new(&admin).await;
    // Metadata fixtures exercise the queue and policy, not image parsing.
    for board in ["p", "g"] {
        for badged in [false, true] {
            for format in [MediaFormat::Jpeg, MediaFormat::Png] {
                let upload = intake
                    .reserve_image("synthetic.image", board, 0, badged)
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
                assert_eq!(job.id, upload.id, "Use an idle disposable queue");
                let token = job.lease_token.as_deref().unwrap();
                let output = StillOutput {
                    format,
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
                    .prepare_still_output(&job.id, token, &output)
                    .await
                    .unwrap();
                let input = InputReceipt {
                    component: InputComponent::Upload,
                    sha256: "d".repeat(64),
                    bytes: 100,
                    md5: [8; 16],
                };
                queue
                    .record_input_receipt(&job.id, token, &asset.id, &input)
                    .await
                    .unwrap();
                queue
                    .approve_output_with_input(&job.id, token, &asset.id, &input)
                    .await
                    .unwrap();
                let pool = if badged { &staff } else { &public };
                let metadata: (String, Option<String>, Option<Vec<u8>>, Option<bool>) = sqlx::query_as(
                    "SELECT * FROM content.attachment_upload_filter_metadata($1,$2,$3,0,false,$4)"
                ).bind(&job.id).bind(&upload.capability).bind(board).bind(badged).fetch_one(pool).await.unwrap();
                if badged {
                    let mut tx = public.begin().await.unwrap();
                    vip.install(&mut tx, board).await;
                    let verified: (String, Option<String>, Option<Vec<u8>>, Option<bool>) = sqlx::query_as(
                        "SELECT * FROM content.attachment_upload_filter_metadata($1,$2,$3,0,false,true)"
                    ).bind(&job.id).bind(&upload.capability).bind(board).fetch_one(&mut *tx).await.unwrap();
                    assert_eq!(
                        verified, metadata,
                        "A checked VIP uses the same image metadata recipe"
                    );
                    assert!(sqlx::query("SELECT * FROM content.attachment_upload_filter_metadata($1,$2,$3,0,true,true)")
                        .bind(&job.id).bind(&upload.capability).bind(board).execute(&mut *tx).await.is_err(),
                        "A VIP badge cannot grant staff attachment bounds");
                    tx.rollback().await.unwrap();
                }
                let eligible = format != MediaFormat::Jpeg || board != "p";
                assert_eq!(
                    metadata,
                    (
                        "synthetic.image".into(),
                        Some("07".repeat(16)),
                        eligible.then(|| vec![8; 16]),
                        Some(eligible)
                    )
                );
                for capability in ["0".repeat(64), "invalid".into()] {
                    assert!(sqlx::query("SELECT * FROM content.attachment_upload_filter_metadata($1,$2,$3,0,false,$4)")
                        .bind(&job.id).bind(capability).bind(board).bind(badged).execute(pool).await.is_err());
                }
                if board == "p" && format == MediaFormat::Jpeg {
                    assert!(sqlx::query("SELECT * FROM content.attachment_upload_filter_metadata($1,$2,$3,0,false,$4)")
                        .bind(&job.id).bind(&upload.capability).bind(board).bind(!badged).execute(&staff).await.is_err());
                }
                assert!(sqlx::query("SELECT * FROM content.attachment_upload_filter_metadata($1,$2,$3,0,false,true)")
                    .bind(&job.id).bind(&upload.capability).bind(board).execute(&public).await.is_err());
                assert!(sqlx::query("SELECT * FROM content.attachment_upload_filter_metadata($1,$2,$3,0,false,$4)")
                    .bind(&job.id).bind(&upload.capability).bind(if board=="p" {"g"} else {"p"}).bind(badged).execute(pool).await.is_err());
                let attached: i64 =
                    sqlx::query_scalar("SELECT count(*) FROM content.post_media WHERE job_id=$1")
                        .bind(&job.id)
                        .fetch_one(&admin)
                        .await
                        .unwrap();
                assert_eq!(attached, 0, "Preflight must not consume the attachment");
                sqlx::query("UPDATE media.jobs SET created_at=clock_timestamp()-interval '2 hours' WHERE id=$1")
                    .bind(&job.id).execute(&admin).await.unwrap();
                assert!(sqlx::query("SELECT * FROM content.attachment_upload_filter_metadata($1,$2,$3,0,false,$4)")
                    .bind(&job.id).bind(&upload.capability).bind(board).bind(badged).execute(pool).await.is_err());
                for statement in [
                    "DELETE FROM media.assets WHERE job_id=$1",
                    "DELETE FROM media_intake.handles WHERE job_id=$1",
                    "DELETE FROM media.jobs WHERE id=$1",
                ] {
                    sqlx::query(statement)
                        .bind(&job.id)
                        .execute(&admin)
                        .await
                        .unwrap();
                }
            }
        }
    }
    for pool in [&public, &staff] {
        assert!(
            sqlx::query("SELECT md5 FROM media.input_receipts")
                .execute(pool)
                .await
                .is_err()
        );
        assert!(
            sqlx::query("SELECT md5 FROM media.assets")
                .execute(pool)
                .await
                .is_err()
        );
    }
    vip.cleanup(&admin).await;
}

struct VipFixture {
    verifier: board_store::verification::VerificationStore,
    key: board_domain::challenge_secrets::VerificationKey,
    session: board_domain::anonymous_session::Fingerprints,
    pass_session: [u8; 32],
    identifier: [u8; 32],
    pass_account: i64,
    vip_account: i64,
}
impl VipFixture {
    async fn new(owner: &PgPool) -> Self {
        use board_domain::{
            anonymous_session::Capability,
            challenge_secrets::{PassSessionToken, VerificationKey},
        };
        let token = PassSessionToken::generate().unwrap();
        let key = VerificationKey::parse(&token.encode()[3..]).unwrap();
        let peer = "192.0.2.149".parse().unwrap();
        let session = Capability::generate()
            .unwrap()
            .fingerprints(Some(peer), *b"US");
        let verifier = board_store::verification::VerificationStore::connect(
            &std::env::var("TEST_VERIFICATION_DATABASE_URL").unwrap(),
        )
        .await
        .unwrap();
        verifier.ensure_session(&session, true).await.unwrap();
        let identifier = key.vip_identifier("owned-image-metadata").unwrap();
        let vip_account=sqlx::query_scalar("SELECT content.vip_manage($1,'Owned media VIP','$argon2id$v=19$m=19456,t=2,p=1$'||repeat('a',22)||'$'||repeat('b',43),true,true)")
            .bind(identifier.as_slice()).fetch_one(owner).await.unwrap();
        let mut tx = owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_anonymous_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        let pass_account=sqlx::query_scalar("INSERT INTO verification_private.pass_entitlements(token_hash,pin_hash,status,purchased_at,expires_at) VALUES($1,repeat('owned-placeholder',3),0,'2020-01-02',clock_timestamp()+interval '1 year') RETURNING id")
            .bind(token.storage_hash().as_slice()).fetch_one(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO verification_private.pass_sessions VALUES($1,$2,1,clock_timestamp(),clock_timestamp()+interval '1 day')")
            .bind(token.storage_hash().as_slice()).bind(pass_account).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        Self {
            verifier,
            key,
            session,
            pass_session: token.storage_hash(),
            identifier,
            pass_account,
            vip_account,
        }
    }
    async fn install(&self, tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, board: &str) {
        use board_domain::challenge_secrets::ReceiptToken;
        let peer = "192.0.2.149".parse().unwrap();
        let networks = self.key.pass_networks(peer).unwrap();
        let pass = ReceiptToken::generate().unwrap();
        let context = self
            .key
            .form_context("thread", Some(board), 0, &"ab".repeat(32))
            .unwrap();
        assert_eq!(
            self.verifier
                .pass_write_receipt(&board_store::verification::pass::WriteRequest {
                    pass_session: &self.pass_session,
                    receipt: &pass.storage_hash(),
                    networks: &networks,
                    anonymous: &self.session.token,
                    context: &context,
                    purpose: "thread",
                    board,
                    target: 0,
                })
                .await
                .unwrap()
                .status,
            "ready"
        );
        let ticket = ReceiptToken::generate().unwrap();
        let vip = ReceiptToken::generate().unwrap();
        let vip_context = ReceiptToken::generate().unwrap().storage_hash();
        let lookup = self
            .verifier
            .begin_vip_login(&board_store::verification::vip::LoginRequest {
                identifier: &self.identifier,
                peer: &networks.peer,
                ticket: &ticket.storage_hash(),
                anonymous: &self.session.token,
                context: &vip_context,
                board,
                target: 0,
                pass_proof: Some((&pass.storage_hash(), &context)),
            })
            .await
            .unwrap();
        assert_eq!(lookup.status, "verify");
        assert_eq!(
            self.verifier
                .finish_vip_login(
                    &ticket.storage_hash(),
                    &networks.peer,
                    true,
                    &vip.storage_hash()
                )
                .await
                .unwrap(),
            "ready"
        );
        let hex = |v: &[u8; 32]| v.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let carrier = |receipt, context| {
            serde_json::json!({"receipt":hex(&receipt),"peer":hex(&networks.peer),"session":hex(&self.session.token),"context":hex(&context)}).to_string()
        };
        sqlx::query("SELECT set_config('board.vip_context',$1,true),set_config('board.challenge_context',$2,true)")
            .bind(carrier(vip.storage_hash(),vip_context)).bind(carrier(pass.storage_hash(),context)).execute(&mut **tx).await.unwrap();
    }
    async fn cleanup(self, owner: &PgPool) {
        let mut tx = owner.begin().await.unwrap();
        sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
            .bind(self.session.token.as_slice())
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("SET LOCAL ROLE board_anonymous_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("DELETE FROM verification_private.pass_entitlements WHERE id=$1")
            .bind(self.pass_account)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("DELETE FROM verification_private.vip_accounts WHERE id=$1")
            .bind(self.vip_account)
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        self.verifier.close().await;
    }
}
