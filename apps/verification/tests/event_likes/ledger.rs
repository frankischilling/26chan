use super::*;
use board_store::verification::ChallengeCheck;

struct Harness<'a> {
    owner: &'a PgPool,
    store: &'a VerificationStore,
    key: &'a VerificationKey,
    context: protocol::ChallengeContext,
    run: i64,
}
impl Harness<'_> {
    async fn proof(&self, target: i64) -> ([u8; 32], [u8; 32], [u8; 32]) {
        let peer = self.key.network_identity(self.context.session.peer);
        sqlx::query("UPDATE verification_private.peers SET refresh_after=clock_timestamp()-interval '1 second' WHERE peer_hash=$1")
            .bind(peer.as_slice()).execute(self.owner).await.unwrap();
        let token = VerificationToken::generate(TokenKind::Challenge).unwrap();
        let hash = self
            .key
            .form_context(
                "like",
                self.context.board.as_deref(),
                target,
                &board_domain::source_events::scores::form_nonce(self.run, &"ab".repeat(32))
                    .unwrap(),
            )
            .unwrap();
        let solution = self.key.challenge_answer(&token, 6).unwrap();
        let answer = self.key.answer_hash(&token, solution.as_str()).unwrap();
        assert!(
            self.store
                .issue_challenge(&NewChallenge {
                    token_hash: &self.key.token_hash(&token),
                    answer_hash: &answer,
                    peer_hash: &peer,
                    session_hash: &self.context.session.fingerprints.token,
                    purpose: ChallengePurpose::Like,
                    board: self.context.board.as_deref(),
                    target,
                    context_hash: &hash,
                    suspicious: false,
                })
                .await
                .unwrap()
                .issued
        );
        let receipt = ReceiptToken::generate().unwrap().storage_hash();
        assert!(
            self.store
                .check_challenge(&ChallengeCheck {
                    token_hash: &self.key.token_hash(&token),
                    answer_hash: Some(&answer),
                    peer_hash: &peer,
                    session_hash: &self.context.session.fingerprints.token,
                    purpose: ChallengePurpose::Like,
                    board: self.context.board.as_deref(),
                    target,
                    context_hash: &hash,
                    receipt_hash: &receipt,
                })
                .await
                .unwrap()
        );
        (receipt, peer, hash)
    }
    async fn like(
        &self,
        public: &PgPool,
        target: i64,
        actor: &[u8; 32],
        blocked: bool,
        proof: &([u8; 32], [u8; 32], [u8; 32]),
    ) -> Result<i32, sqlx::Error> {
        sqlx::query_scalar("SELECT content.event_like($1,$2,$3,$4,$5,$6,$7,$8,$9)")
            .bind(self.run)
            .bind(self.context.board.as_deref())
            .bind(target)
            .bind(actor.as_slice())
            .bind(self.context.session.fingerprints.token.as_slice())
            .bind(proof.0.as_slice())
            .bind(proof.1.as_slice())
            .bind(proof.2.as_slice())
            .bind(if blocked { Some(3221226219_i64) } else { None })
            .fetch_one(public)
            .await
    }
    async fn known(
        &self,
        public: &PgPool,
        target: i64,
        actor: &[u8; 32],
    ) -> Result<i32, sqlx::Error> {
        sqlx::query_scalar("SELECT content.event_like_known($1,$2,$3,$4,$5,NULL)")
            .bind(self.run)
            .bind(self.context.board.as_deref())
            .bind(target)
            .bind(actor.as_slice())
            .bind(self.context.session.fingerprints.token.as_slice())
            .fetch_one(public)
            .await
    }
    async fn counts(&self) -> (i64, i64, i64, i32, i32) {
        sqlx::query_as("SELECT (SELECT count(*) FROM event_private.score_votes WHERE run_id=$1),
            (SELECT coalesce(sum(score),0)::bigint FROM event_private.score_accounts WHERE run_id=$1),
            (SELECT coalesce(sum(likes),0)::bigint FROM event_private.score_posts WHERE run_id=$1),
            account_count,vote_count FROM event_private.runs WHERE id=$1")
            .bind(self.run).fetch_one(self.owner).await.unwrap()
    }
}
fn refused(result: Result<i32, sqlx::Error>, message: &str) {
    let err = result.unwrap_err();
    assert_eq!(err.as_database_error().unwrap().message(), message);
}
pub(super) async fn exercise(
    owner: &PgPool,
    store: &VerificationStore,
    key: &VerificationKey,
    context: protocol::ChallengeContext,
    run: i64,
) {
    let public = PgPoolOptions::new()
        .max_connections(3)
        .connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let h = Harness {
        owner,
        store,
        key,
        context,
        run,
    };
    let board = h.context.board.as_ref().unwrap();
    let range_id:i64=sqlx::query_scalar("INSERT INTO admission.network_rules(range_start,range_end) VALUES(3221226219,3221226219) RETURNING id").fetch_one(owner).await.unwrap();
    let op = h.context.target;
    let mut targets = Vec::new();
    for n in 0..3 {
        let id:i64=sqlx::query_scalar("INSERT INTO content.posts(board,thread_id,name,subject,comment) VALUES($1,$2,'Anonymous','',$3) RETURNING id")
            .bind(board).bind(op).bind(format!("Owned vote target {n}")).fetch_one(owner).await.unwrap();
        sqlx::query("INSERT INTO post_secrets.posting_history(post_id,board,thread_id,actor_hash,request_at) VALUES($1,$2,$3,$4,floor(extract(epoch FROM now()))::bigint)")
            .bind(id).bind(board).bind(op).bind([41u8;32].as_slice()).execute(owner).await.unwrap();
        targets.push(id);
    }
    let actor = [42u8; 32];
    assert!(
        !board_store::events::scores::known_address(
            &public,
            run,
            &actor,
            &h.context.session.fingerprints.token
        )
        .await
        .unwrap()
    );
    refused(
        h.known(&public, targets[0], &actor).await,
        "Like verification is required.",
    );
    let first = h.proof(targets[0]).await;
    refused(
        h.like(&public, targets[0], &[41; 32], false, &first).await,
        "You can't like your own posts.",
    );
    refused(
        h.like(&public, targets[0], &actor, true, &first).await,
        "You have to wait a while before liking this post.",
    );
    assert_eq!(h.counts().await, (0, 0, 0, 0, 0));
    assert_eq!(
        h.like(&public, targets[0], &actor, false, &first)
            .await
            .unwrap(),
        1
    );
    assert_eq!(h.counts().await, (1, 11, 1, 2, 1));
    assert!(
        board_store::events::scores::known_address(
            &public,
            run,
            &actor,
            &h.context.session.fingerprints.token
        )
        .await
        .unwrap()
    );
    refused(
        h.known(&public, targets[1], &actor).await,
        "You have to wait a while before liking this post.",
    );

    assert!(
        h.like(&public, targets[0], &actor, false, &first)
            .await
            .is_err(),
        "receipt replay"
    );
    let duplicate = h.proof(targets[0]).await;
    refused(
        h.like(&public, targets[0], &actor, false, &duplicate).await,
        "You already like this post.",
    );
    let next = h.proof(targets[1]).await;
    refused(
        h.like(&public, targets[1], &actor, false, &next).await,
        "You have to wait a while before liking this post.",
    );
    assert_eq!(h.counts().await, (1, 11, 1, 2, 1));
    sqlx::query("UPDATE event_private.score_accounts SET last_vote_at=floor(extract(epoch FROM clock_timestamp()))::bigint-120 WHERE run_id=$1 AND last_vote_at IS NOT NULL")
        .bind(run).execute(owner).await.unwrap();
    assert_eq!(h.known(&public, targets[1], &actor).await.unwrap(), 1);
    assert_eq!(h.counts().await, (2, 22, 2, 2, 2));
    let mut verified = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *verified)
        .await
        .unwrap();
    let identity:String=sqlx::query_scalar("INSERT INTO verification_private.identities(email_hash) VALUES(sha256(convert_to(gen_random_uuid()::text,'UTF8'))) RETURNING id::text")
        .fetch_one(&mut *verified).await.unwrap();
    sqlx::query("INSERT INTO verification_private.browser_identities(session_hash,identity_id) VALUES($1,$2::text::uuid)")
        .bind(h.context.session.fingerprints.token.as_slice()).bind(&identity).execute(&mut *verified).await.unwrap();
    verified.commit().await.unwrap();
    assert!(
        !board_store::events::scores::known_address(
            &public,
            run,
            &actor,
            &h.context.session.fingerprints.token
        )
        .await
        .unwrap()
    );
    refused(
        h.known(&public, targets[2], &actor).await,
        "Like verification is required.",
    );
    let mut verified = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *verified)
        .await
        .unwrap();
    sqlx::query("DELETE FROM verification_private.browser_identities WHERE session_hash=$1")
        .bind(h.context.session.fingerprints.token.as_slice())
        .execute(&mut *verified)
        .await
        .unwrap();
    sqlx::query("DELETE FROM verification_private.identities WHERE id=$1::text::uuid")
        .bind(identity)
        .execute(&mut *verified)
        .await
        .unwrap();
    verified.commit().await.unwrap();

    let third = h.proof(targets[2]).await;
    sqlx::query("UPDATE event_private.score_accounts SET last_vote_at=0 WHERE run_id=$1")
        .bind(run)
        .execute(owner)
        .await
        .unwrap();
    sqlx::query("UPDATE event_private.runs SET account_capacity=account_count WHERE id=$1")
        .bind(run)
        .execute(owner)
        .await
        .unwrap();
    refused(
        h.like(&public, targets[2], &[43; 32], false, &third).await,
        "Event account capacity reached.",
    );
    assert_eq!(h.counts().await, (2, 22, 2, 2, 2));
    sqlx::query("UPDATE event_private.runs SET account_capacity=10000 WHERE id=$1")
        .bind(run)
        .execute(owner)
        .await
        .unwrap();
    sqlx::query("INSERT INTO event_private.score_posts VALUES($1,$2,1048575)")
        .bind(run)
        .bind(targets[2])
        .execute(owner)
        .await
        .unwrap();
    refused(
        h.like(&public, targets[2], &[43; 32], false, &third).await,
        "You can't like this post anymore.",
    );
    assert_eq!(h.counts().await, (2, 22, 1048577, 2, 2));
    sqlx::query("DELETE FROM event_private.score_posts WHERE run_id=$1 AND post_id=$2")
        .bind(run)
        .bind(targets[2])
        .execute(owner)
        .await
        .unwrap();
    assert_eq!(
        h.like(&public, targets[2], &[43; 32], false, &third)
            .await
            .unwrap(),
        1
    );
    assert_eq!(h.counts().await, (3, 33, 3, 3, 3));
    sqlx::query("UPDATE content.boards SET captcha_enabled=false WHERE slug=$1")
        .bind(board)
        .execute(owner)
        .await
        .unwrap();
    let posting_key = board_domain::poster_id::PosterIdKey::parse(&"45".repeat(32)).unwrap();
    let unscored = board_store::create_post_with_metadata(
        &public,
        board,
        op,
        &board_store::NewPost {
            name: "Anonymous".into(),
            subject: String::new(),
            comment: "Owned default-off post".into(),
            deletion_hash: "owned-test-hash".into(),
            sage: false,
        },
        None,
        board_store::PostingContext {
            request_start: chrono::Utc::now(),
            peer: Some("192.0.2.181".parse().unwrap()),
            op_password_proof: None,
        },
        board_store::PostMetadata {
            event_scores: false,
            keys: board_store::PostIdentityKeys {
                tripcode: None,
                poster_id: Some(&posting_key),
            },
            country_database: None,
            flag: "",
            options: "animal",
            spoiler: false,
            drawing: None,
        },
    )
    .await
    .unwrap();
    assert!(unscored > op);
    assert_eq!(
        sqlx::query_scalar::<_, Option<i16>>("SELECT event_perk FROM content.posts WHERE id=$1")
            .bind(unscored)
            .fetch_one(owner)
            .await
            .unwrap(),
        None
    );
    assert_eq!(h.counts().await, (3, 33, 3, 3, 3));
    let posted = board_store::create_post_with_metadata(
        &public,
        board,
        op,
        &board_store::NewPost {
            name: "Anonymous".into(),
            subject: String::new(),
            comment: "Owned event post award".into(),
            deletion_hash: "owned-test-hash".into(),
            sage: false,
        },
        None,
        board_store::PostingContext {
            request_start: chrono::Utc::now(),
            peer: Some("192.0.2.182".parse().unwrap()),
            op_password_proof: None,
        },
        board_store::PostMetadata {
            event_scores: true,
            keys: board_store::PostIdentityKeys {
                tripcode: None,
                poster_id: Some(&posting_key),
            },
            country_database: None,
            flag: "",
            options: "animal",
            spoiler: false,
            drawing: None,
        },
    )
    .await
    .unwrap();
    sqlx::query("UPDATE content.boards SET captcha_enabled=true WHERE slug=$1")
        .bind(board)
        .execute(owner)
        .await
        .unwrap();
    assert!(posted > op);
    let saved: i16 = sqlx::query_scalar("SELECT event_perk FROM content.posts WHERE id=$1")
        .bind(posted)
        .fetch_one(owner)
        .await
        .unwrap();
    assert_eq!(
        board_domain::source_events::perks::from_stored(saved)
            .unwrap()
            .name(),
        "animal"
    );
    assert_eq!(h.counts().await, (3, 35, 3, 4, 3));
    // The runtime cannot mint scores or inspect private identities directly.
    for query in [
        "SELECT * FROM event_private.score_accounts",
        "UPDATE event_private.runs SET enabled=true",
        "SELECT event_private.reserve_score_actor(1,decode(repeat('11',32),'hex'))",
    ] {
        assert!(sqlx::query(query).execute(&public).await.is_err());
    }
    sqlx::query("DELETE FROM admission.network_rules WHERE id=$1")
        .bind(range_id)
        .execute(owner)
        .await
        .unwrap();
    sqlx::query("UPDATE content.boards SET captcha_enabled=false WHERE slug=$1")
        .bind(board)
        .execute(owner)
        .await
        .unwrap();
    sqlx::query("UPDATE event_private.runs SET ends_at=floor(extract(epoch FROM clock_timestamp()))::bigint WHERE id=$1").bind(run).execute(owner).await.unwrap();
    let closed_post = board_store::create_post_with_metadata(
        &public,
        board,
        op,
        &board_store::NewPost {
            name: "Anonymous".into(),
            subject: String::new(),
            comment: "Owned closed event perk".into(),
            deletion_hash: "owned-test-hash".into(),
            sage: false,
        },
        None,
        board_store::PostingContext {
            request_start: chrono::Utc::now(),
            peer: Some("192.0.2.183".parse().unwrap()),
            op_password_proof: None,
        },
        board_store::PostMetadata {
            event_scores: true,
            keys: board_store::PostIdentityKeys {
                tripcode: None,
                poster_id: Some(&posting_key),
            },
            country_database: None,
            flag: "",
            options: "animal",
            spoiler: false,
            drawing: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, Option<i16>>("SELECT event_perk FROM content.posts WHERE id=$1")
            .bind(closed_post)
            .fetch_one(owner)
            .await
            .unwrap(),
        None
    );
    assert_eq!(h.counts().await, (3, 35, 3, 4, 3));
    sqlx::query("UPDATE event_private.runs SET ends_at=253402300799 WHERE id=$1")
        .bind(run)
        .execute(owner)
        .await
        .unwrap();
    sqlx::query("UPDATE content.boards SET captcha_enabled=true WHERE slug=$1")
        .bind(board)
        .execute(owner)
        .await
        .unwrap();
    public.close().await;
    maintenance_waits_before_taking_the_runtime_counter_gate(owner, run).await;
}

async fn maintenance_waits_before_taking_the_runtime_counter_gate(owner: &PgPool, run: i64) {
    use sqlx::Connection;
    let mut holder = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *holder)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM event_private.runs WHERE id=$1 FOR UPDATE")
        .bind(run)
        .fetch_one(&mut *holder)
        .await
        .unwrap();
    let mut closer = sqlx::PgConnection::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut closer)
        .await
        .unwrap();
    let closing =
        tokio::spawn(async move { board_store::events::admin::close(&mut closer, run).await });
    tokio::time::timeout(std::time::Duration::from_secs(3),async {
        loop {
            let waiting:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_locks WHERE pid=$1 AND relation='event_private.runs'::regclass AND NOT granted)")
                .bind(pid).fetch_one(owner).await.unwrap();
            if waiting {break;}
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.unwrap();
    // A queued operator must not prevent the row-lock owner from completing
    // its counter update, which upgrades its table lock to ROW EXCLUSIVE.
    sqlx::query("UPDATE event_private.runs SET vote_count=vote_count WHERE id=$1")
        .bind(run)
        .execute(&mut *holder)
        .await
        .unwrap();
    holder.commit().await.unwrap();
    assert!(closing.await.unwrap().unwrap());
    assert!(
        !sqlx::query_scalar::<_, bool>("SELECT enabled FROM event_private.runs WHERE id=$1")
            .bind(run)
            .fetch_one(owner)
            .await
            .unwrap()
    );
    // Restore only this owned fixture so subsequent proof tests can continue.
    sqlx::query("UPDATE event_private.runs SET enabled=true WHERE id=$1")
        .bind(run)
        .execute(owner)
        .await
        .unwrap();
}
