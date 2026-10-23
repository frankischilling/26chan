use super::*;
use board_domain::site_submissions::{
    ContactEmail, applications as domain,
    ownership::{Capability as Ownership, Kind},
};
use board_store::site_submissions::{applications, feedback::Proof};
struct Fixture<'a> {
    owner: &'a PgPool,
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
                purpose: self.purpose().0,
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
                self.purpose().1,
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
    fn purpose(&self) -> (ChallengePurpose, &'static str) {
        match self.context.purpose {
            protocol::Purpose::SiteApplication => {
                (ChallengePurpose::SiteApplication, "site_application")
            }
            protocol::Purpose::SiteApplicationAuth => (
                ChallengePurpose::SiteApplicationAuth,
                "site_application_auth",
            ),
            protocol::Purpose::SiteApplicationEdit => (
                ChallengePurpose::SiteApplicationEdit,
                "site_application_edit",
            ),
            _ => panic!("not an application fixture"),
        }
    }
    fn proof<'a>(
        &'a self,
        receipt: &'a [u8; 32],
        peer: &'a [u8; 32],
        hash: &'a [u8; 32],
    ) -> Proof<'a> {
        Proof {
            session: &self.context.session.fingerprints.token,
            receipt,
            peer,
            context: hash,
        }
    }
}
fn fields(profile: &domain::Profile, email: &str) -> domain::Submission {
    domain::Submission::prepare(
        profile,
        domain::Input {
            firstname: "Owned <name>",
            handle: "<script>literal</script>",
            email,
            age: 25,
            timezone: -12,
            hours: 24,
            times: "16:00-18:00",
            board1: "g",
            board2: "",
            answers: ["First\nline", "Two", "Three", "Four"],
        },
    )
    .unwrap()
}
pub(super) async fn exercise(
    owner: &PgPool,
    public: &PgPool,
    app: &Router,
    store: &VerificationStore,
    key: &VerificationKey,
    original: &protocol::ChallengeContext,
) {
    let campaign = original.target;
    let mut create = original.clone();
    create.purpose = protocol::Purpose::SiteApplication;
    let f = Fixture {
        owner,
        app,
        store,
        key,
        context: &create,
    };
    let profile = board_store::site_submissions::application_profile(public)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(profile.id, campaign);
    let cap = Ownership::generate(Kind::Application).unwrap();
    let unknown = Ownership::generate(Kind::Application).unwrap();
    assert!(
        applications::target(public, &unknown)
            .await
            .unwrap()
            .is_none()
    );
    let input = fields(&profile.form, "Owned+application@example.org");
    let peer = key.network_identity(create.session.peer);
    let hash = f.hash();
    let receipt = f.receipt().await;
    let proof = f.proof(&receipt, &peer, &hash);
    // The database independently rejects ambiguous JSON even from a direct runtime call.
    let valid = serde_json::json!({"version":1,"firstname":"Owned","handle":"Owned","email":"owned@example.org","age":25,"timezone":0,"hours":1,"times":"Owned","board1":"g","board2":"","answers":["a","b","c","d"]});
    for (field, value) in [
        ("version", serde_json::Value::Null),
        ("age", serde_json::json!("25")),
        ("timezone", serde_json::json!(14)),
        ("answers", serde_json::json!(["a", "b", "c", null])),
        ("board1", serde_json::json!("missing")),
        ("firstname", serde_json::json!("")),
    ] {
        let mut bad = valid.clone();
        bad[field] = value;
        let refused = sqlx::query("SELECT submission_api.submit_application($1,$2,$3,$4,$5,$6,$7)")
            .bind(campaign)
            .bind(cap.storage_hash().as_slice())
            .bind(bad)
            .bind(proof.session.as_slice())
            .bind(proof.receipt.as_slice())
            .bind(proof.peer.as_slice())
            .bind(proof.context.as_slice())
            .execute(public)
            .await
            .unwrap_err();
        assert_eq!(
            refused
                .as_database_error()
                .and_then(|e| e.code())
                .as_deref(),
            Some("P225J")
        );
        assert!(f.retained(&receipt).await);
    }
    let id = applications::submit(public, campaign, &cap, &input, &proof)
        .await
        .unwrap();
    assert!(!f.retained(&receipt).await);
    assert!(
        applications::submit(public, campaign, &cap, &input, &proof)
            .await
            .is_err()
    );
    let target = applications::target(public, &cap).await.unwrap().unwrap();
    assert_eq!((target.id, target.campaign), (id, campaign));
    let review: serde_json::Value =
        sqlx::query_scalar("SELECT to_jsonb(a) FROM submission_private.applications a WHERE id=$1")
            .bind(id)
            .fetch_one(owner)
            .await
            .unwrap();
    for forbidden in [
        "firstname",
        "handle",
        "email",
        "ownership_hash",
        "actor_hash",
    ] {
        assert!(review.get(forbidden).is_none());
    }
    for query in [
        "SELECT * FROM submission_private.applications",
        "SELECT * FROM submission_private.application_identity",
    ] {
        assert!(sqlx::query(query).execute(public).await.is_err());
    }
    for query in [
        "SET LOCAL ROLE board_submission_owner",
        "SET LOCAL ROLE board_submission_review_owner",
    ] {
        let mut tx = owner.begin().await.unwrap();
        sqlx::query(query).execute(&mut *tx).await.unwrap();
        assert!(
            sqlx::query("SELECT email FROM submission_private.application_identity")
                .execute(&mut *tx)
                .await
                .is_err()
        );
        tx.rollback().await.unwrap();
    }
    // Capacity blocks new applications, but never ownership authentication/editing.
    sqlx::query(
        "UPDATE submission_private.application_campaigns SET capacity=received WHERE id=$1",
    )
    .bind(campaign)
    .execute(owner)
    .await
    .unwrap();
    let mut auth = create.clone();
    auth.purpose = protocol::Purpose::SiteApplicationAuth;
    auth.target = id;
    let a = Fixture {
        owner,
        app,
        store,
        key,
        context: &auth,
    };
    let ah = a.hash();
    let wrong = ContactEmail::parse("wrong@example.org").unwrap();
    let email = ContactEmail::parse("OWNED+APPLICATION@EXAMPLE.ORG").unwrap();
    let receipt = a.receipt().await;
    assert!(
        applications::authenticate(
            public,
            &cap,
            &wrong,
            &profile,
            &a.proof(&receipt, &peer, &ah)
        )
        .await
        .unwrap()
        .is_none()
    );
    assert!(
        !a.retained(&receipt).await,
        "wrong email must consume the proof"
    );
    assert!(
        applications::authenticate(
            public,
            &cap,
            &email,
            &profile,
            &a.proof(&receipt, &peer, &ah)
        )
        .await
        .is_err()
    );
    let receipt = a.receipt().await;
    let editable = applications::authenticate(
        public,
        &cap,
        &email,
        &profile,
        &a.proof(&receipt, &peer, &ah),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(editable.revision, 1);
    assert_eq!(editable.target.id, id);
    assert_eq!(editable.fields.identity.firstname, "Owned <name>");
    assert_eq!(editable.fields.review.answers[0], "First\nline");
    let mut edit = auth.clone();
    edit.purpose = protocol::Purpose::SiteApplicationEdit;
    let e = Fixture {
        owner,
        app,
        store,
        key,
        context: &edit,
    };
    let eh = e.hash();
    let changed = fields(&profile.form, "changed@example.org");
    // An authentication proof cannot authorize an edit and is not burned by the mismatch.
    let receipt = a.receipt().await;
    assert!(
        applications::edit(
            public,
            &cap,
            &email,
            1,
            &changed,
            &a.proof(&receipt, &peer, &ah)
        )
        .await
        .is_err()
    );
    assert!(a.retained(&receipt).await);
    assert!(
        applications::authenticate(
            public,
            &cap,
            &email,
            &profile,
            &a.proof(&receipt, &peer, &ah)
        )
        .await
        .unwrap()
        .is_some()
    );
    let receipt = e.receipt().await;
    assert_eq!(
        applications::edit(
            public,
            &cap,
            &email,
            1,
            &changed,
            &e.proof(&receipt, &peer, &eh)
        )
        .await
        .unwrap(),
        Some(2)
    );
    assert!(!e.retained(&receipt).await);
    let changed_email = ContactEmail::parse("changed@example.org").unwrap();
    let receipt = e.receipt().await;
    assert_eq!(
        applications::edit(
            public,
            &cap,
            &changed_email,
            1,
            &input,
            &e.proof(&receipt, &peer, &eh)
        )
        .await
        .unwrap(),
        None
    );
    assert!(
        !e.retained(&receipt).await,
        "stale revision consumes its proof"
    );
    let receipt = a.receipt().await;
    assert!(
        applications::authenticate(
            public,
            &cap,
            &email,
            &profile,
            &a.proof(&receipt, &peer, &ah)
        )
        .await
        .unwrap()
        .is_none()
    );
    let receipt = a.receipt().await;
    let edited = applications::authenticate(
        public,
        &cap,
        &changed_email,
        &profile,
        &a.proof(&receipt, &peer, &ah),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(edited.revision, 2);
    assert_eq!(
        edited.fields.identity.email.recipient(),
        "changed@example.org"
    );
    // Accepted/signed applications cannot be changed through their public edit capability.
    for state in [2_i16, 4] {
        sqlx::query("UPDATE submission_private.applications SET state=$2 WHERE id=$1")
            .bind(id)
            .bind(state)
            .execute(owner)
            .await
            .unwrap();
        let receipt = e.receipt().await;
        assert_eq!(
            applications::edit(
                public,
                &cap,
                &changed_email,
                2,
                &input,
                &e.proof(&receipt, &peer, &eh)
            )
            .await
            .unwrap(),
            None
        );
        assert!(!e.retained(&receipt).await);
    }
    sqlx::query("UPDATE submission_private.applications SET state=0 WHERE id=$1")
        .bind(id)
        .execute(owner)
        .await
        .unwrap();
    // Two independent proofs racing the same revision yield exactly one update.
    let first = e.receipt().await;
    let second = e.receipt().await;
    let p1 = e.proof(&first, &peer, &eh);
    let p2 = e.proof(&second, &peer, &eh);
    let (left, right) = tokio::join!(
        applications::edit(public, &cap, &changed_email, 2, &changed, &p1),
        applications::edit(public, &cap, &changed_email, 2, &changed, &p2)
    );
    let results = [left.unwrap(), right.unwrap()];
    assert_eq!(results.iter().filter(|r| **r == Some(3)).count(), 1);
    assert_eq!(results.iter().filter(|r| r.is_none()).count(), 1);
    assert!(!e.retained(&first).await);
    assert!(!e.retained(&second).await);
    // Recheck the campaign deadline after the verifier's policy-row lock wait.
    let receipt = e.receipt().await;
    sqlx::query("UPDATE submission_private.application_campaigns SET ends_at=floor(extract(epoch FROM clock_timestamp()))::bigint+2 WHERE id=$1").bind(campaign).execute(owner).await.unwrap();
    let mut blocker = owner.begin().await.unwrap();
    sqlx::query("SELECT singleton FROM verification_private.policy WHERE singleton FOR UPDATE")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let proof = e.proof(&receipt, &peer, &eh);
    // Only this owned connection waits past the two-second test boundary.
    // Production role limits are unchanged.
    let waiting = PgPoolOptions::new()
        .max_connections(1)
        .after_connect(|connection, _| {
            Box::pin(async move {
                sqlx::query("SET lock_timeout='4s'")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let (result, ()) = tokio::join!(
        applications::edit(&waiting, &cap, &changed_email, 3, &input, &proof),
        async {
            tokio::time::sleep(std::time::Duration::from_millis(2200)).await;
            blocker.rollback().await.unwrap();
        }
    );
    waiting.close().await;
    assert!(matches!(
        result,
        Err(board_store::StoreError::Invalid("Applications are closed."))
    ));
    assert!(e.retained(&receipt).await);
    let revision: i32 =
        sqlx::query_scalar("SELECT revision FROM submission_private.applications WHERE id=$1")
            .bind(id)
            .fetch_one(owner)
            .await
            .unwrap();
    assert_eq!(revision, 3);
    sqlx::query(
        "UPDATE submission_private.application_campaigns SET ends_at=253402300799 WHERE id=$1",
    )
    .bind(campaign)
    .execute(owner)
    .await
    .unwrap();
    let receipt = e.receipt().await;
    sqlx::query("UPDATE submission_private.application_campaigns SET enabled=false WHERE id=$1")
        .bind(campaign)
        .execute(owner)
        .await
        .unwrap();
    assert!(applications::target(public, &cap).await.unwrap().is_none());
    assert_eq!(
        applications::edit(
            public,
            &cap,
            &changed_email,
            3,
            &input,
            &e.proof(&receipt, &peer, &eh)
        )
        .await
        .unwrap(),
        None
    );
    assert!(e.retained(&receipt).await);
    sqlx::query(
        "UPDATE submission_private.application_campaigns SET enabled=true,capacity=100 WHERE id=$1",
    )
    .bind(campaign)
    .execute(owner)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE submission_private.application_campaigns SET cooldown_seconds=0 WHERE id=$1",
    )
    .bind(campaign)
    .execute(owner)
    .await
    .unwrap();
    let first = f.receipt().await;
    let second = f.receipt().await;
    let cap1 = Ownership::generate(Kind::Application).unwrap();
    let cap2 = Ownership::generate(Kind::Application).unwrap();
    let p1 = f.proof(&first, &peer, &hash);
    let p2 = f.proof(&second, &peer, &hash);
    // A unique-email failure rolls back the answer row and proof consumption.
    let duplicate = applications::submit(public, campaign, &cap1, &changed, &p1).await;
    assert!(matches!(
        duplicate,
        Err(board_store::StoreError::Invalid(
            "This application could not be saved."
        ))
    ));
    assert!(f.retained(&first).await);
    assert!(applications::target(public, &cap1).await.unwrap().is_none());
    let received: i32 = sqlx::query_scalar(
        "SELECT received FROM submission_private.application_campaigns WHERE id=$1",
    )
    .bind(campaign)
    .fetch_one(owner)
    .await
    .unwrap();
    assert_eq!(received, 1);
    sqlx::query(
        "UPDATE submission_private.application_campaigns SET capacity=received+1 WHERE id=$1",
    )
    .bind(campaign)
    .execute(owner)
    .await
    .unwrap();
    let one = fields(&profile.form, "one@example.org");
    let two = fields(&profile.form, "two@example.org");
    let (left, right) = tokio::join!(
        applications::submit(public, campaign, &cap1, &one, &p1),
        applications::submit(public, campaign, &cap2, &two, &p2)
    );
    assert_ne!(left.is_ok(), right.is_ok());
    assert_ne!(f.retained(&first).await, f.retained(&second).await);
    let received: i32 = sqlx::query_scalar(
        "SELECT received FROM submission_private.application_campaigns WHERE id=$1",
    )
    .bind(campaign)
    .fetch_one(owner)
    .await
    .unwrap();
    assert_eq!(received, 2);
}
