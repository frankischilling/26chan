use super::*;
use board_domain::site_submissions::feedback::Submission;
use board_store::{
    StoreError,
    site_submissions::feedback::{self, Proof, Request as Write},
};

struct Fixture<'a> {
    owner: &'a PgPool,
    public: &'a PgPool,
    app: &'a Router,
    store: &'a VerificationStore,
    key: &'a VerificationKey,
    context: &'a protocol::ChallengeContext,
}
impl Fixture<'_> {
    async fn receipt(&self) -> [u8; 32] {
        let peer = self.key.network_identity(self.context.session.peer);
        sqlx::query("UPDATE verification_private.peers SET refresh_after=clock_timestamp()-interval '1 second' WHERE peer_hash=$1")
            .bind(peer.as_slice()).execute(self.owner).await.unwrap();
        let token = VerificationToken::generate(TokenKind::Challenge).unwrap();
        let context = self.hash();
        let issued = self
            .store
            .issue_challenge(&NewChallenge {
                token_hash: &self.key.token_hash(&token),
                answer_hash: &self.key.answer_hash(&token, "ADGJ").unwrap(),
                peer_hash: &peer,
                session_hash: &self.context.session.fingerprints.token,
                purpose: ChallengePurpose::SiteFeedback,
                board: None,
                target: self.context.target,
                suspicious: false,
                context_hash: &context,
            })
            .await
            .unwrap();
        assert!(issued.issued);
        let (status, result) = call(
            self.app,
            "/v1/check",
            &protocol::Answer {
                context: self.context.clone(),
                challenge: token.encode(),
                answer: "ADGJ".into(),
            },
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(result["state"], "verified");
        ReceiptToken::parse(result["receipt"].as_str().unwrap())
            .unwrap()
            .storage_hash()
    }
    fn hash(&self) -> [u8; 32] {
        self.key
            .form_context(
                "site_feedback",
                None,
                self.context.target,
                &self.context.form_nonce,
            )
            .unwrap()
    }
    async fn retained(&self, receipt: &[u8; 32]) -> bool {
        sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM verification_private.receipts WHERE token_hash=$1)",
        )
        .bind(receipt.as_slice())
        .fetch_one(self.owner)
        .await
        .unwrap()
    }
    async fn submit(&self, receipt: &[u8; 32], category: u16) -> Result<i64, StoreError> {
        let fields = Submission {
            category,
            subject: "Owned feedback".into(),
            message: "<script>literal text</script>\nOwned line".into(),
        };
        feedback::submit(
            self.public,
            &Write {
                profile: self.context.target,
                fields: &fields,
                address: self.context.session.peer,
                user_agent: "Owned test agent",
                language: "en-US",
                proof: Proof {
                    session: &self.context.session.fingerprints.token,
                    receipt,
                    peer: &self.key.network_identity(self.context.session.peer),
                    context: &self.hash(),
                },
            },
        )
        .await
    }
    async fn configure(&self, query: &'static str) {
        sqlx::query(query)
            .bind(self.context.target)
            .execute(self.owner)
            .await
            .unwrap();
    }
}
fn refused(result: Result<i64, StoreError>, expected: &'static str) {
    assert!(matches!(result, Err(StoreError::Invalid(message)) if message == expected));
}
pub(super) async fn exercise(
    owner: &PgPool,
    public: &PgPool,
    app: &Router,
    store: &VerificationStore,
    key: &VerificationKey,
    context: &protocol::ChallengeContext,
) {
    let f = Fixture {
        owner,
        public,
        app,
        store,
        key,
        context,
    };
    let first = f.receipt().await;
    refused(f.submit(&first, 999).await, "Invalid feedback submission.");
    assert!(f.retained(&first).await);
    let id = f.submit(&first, 1).await.unwrap();
    assert!(!f.retained(&first).await);
    let row: (String, String, String, String, i64) = sqlx::query_as("SELECT subject,message,host(peer),user_agent,profile_id FROM submission_private.feedback WHERE id=$1")
        .bind(id).fetch_one(owner).await.unwrap();
    assert_eq!(
        row,
        (
            "Owned feedback".into(),
            "<script>literal text</script>\nOwned line".into(),
            context.session.peer.to_string(),
            "Owned test agent".into(),
            context.target
        )
    );
    let private = sqlx::query("SELECT message FROM submission_private.feedback WHERE id=$1")
        .bind(id)
        .execute(public)
        .await
        .unwrap_err();
    assert_eq!(
        private
            .as_database_error()
            .and_then(|e| e.code())
            .as_deref(),
        Some("42501")
    );
    let second = f.receipt().await;
    refused(
        f.submit(&second, 1).await,
        "Please wait before submitting more feedback.",
    );
    assert!(f.retained(&second).await);
    f.configure("UPDATE submission_private.feedback_profiles SET cooldown_seconds=0 WHERE id=$1")
        .await;
    f.submit(&second, 1).await.unwrap();
    refused(
        f.submit(&second, 1).await,
        "Submission verification is required.",
    );
    let closed = f.receipt().await;
    f.configure("UPDATE submission_private.feedback_profiles SET enabled=false WHERE id=$1")
        .await;
    refused(f.submit(&closed, 1).await, "Feedback is closed.");
    assert!(f.retained(&closed).await);
    f.configure("UPDATE submission_private.feedback_profiles SET enabled=true WHERE id=$1")
        .await;
    f.submit(&closed, 1).await.unwrap();

    // Removing an unused category while the writer waits forces a foreign-key
    // failure after receipt consumption. Both the proof and counter must roll back.
    sqlx::query(
        "INSERT INTO submission_private.feedback_categories VALUES($1,2,1,'Second','Second')",
    )
    .bind(context.target)
    .execute(owner)
    .await
    .unwrap();
    let rollback = f.receipt().await;
    let mut gate = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *gate)
        .await
        .unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *gate)
        .await
        .unwrap();
    sqlx::query("SELECT singleton FROM verification_private.policy WHERE singleton FOR UPDATE")
        .execute(&mut *gate)
        .await
        .unwrap();
    let release = async {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE usename='board_public' AND $1=ANY(pg_blocking_pids(pid)))")
                    .bind(blocker).fetch_one(owner).await.unwrap();
                if waiting { break; }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }).await.expect("Feedback writer must wait on the owned policy lock");
        sqlx::query(
            "DELETE FROM submission_private.feedback_categories WHERE profile_id=$1 AND id=2",
        )
        .bind(context.target)
        .execute(owner)
        .await
        .unwrap();
        gate.rollback().await.unwrap();
    };
    let (failed, ()) = tokio::join!(f.submit(&rollback, 2), release);
    assert!(matches!(failed, Err(StoreError::Database(ref error))
        if error.as_database_error().and_then(|e| e.code()).as_deref() == Some("23503")));
    assert!(f.retained(&rollback).await);
    let received: i32 =
        sqlx::query_scalar("SELECT received FROM submission_private.feedback_profiles WHERE id=$1")
            .bind(context.target)
            .fetch_one(owner)
            .await
            .unwrap();
    assert_eq!(received, 3);
    sqlx::query(
        "INSERT INTO submission_private.feedback_categories VALUES($1,2,1,'Second','Second')",
    )
    .bind(context.target)
    .execute(owner)
    .await
    .unwrap();
    f.submit(&rollback, 2).await.unwrap();

    // Both valid receipts race for the final slot under the profile lock.
    let left = f.receipt().await;
    let right = f.receipt().await;
    f.configure("UPDATE submission_private.feedback_profiles SET capacity=received+1 WHERE id=$1")
        .await;
    let (a, b) = tokio::join!(f.submit(&left, 1), f.submit(&right, 1));
    assert_ne!(a.is_ok(), b.is_ok());
    let loser = if a.is_ok() { b } else { a };
    refused(loser, "Feedback capacity has been reached.");
    assert_ne!(f.retained(&left).await, f.retained(&right).await);
    let counts: (i64, i64) = sqlx::query_as("SELECT received::bigint,(SELECT count(*) FROM submission_private.feedback WHERE profile_id=$1) FROM submission_private.feedback_profiles WHERE id=$1")
        .bind(context.target).fetch_one(owner).await.unwrap();
    assert_eq!(counts, (5, 5));
    f.configure("UPDATE submission_private.feedback_profiles SET capacity=100 WHERE id=$1")
        .await;
}
