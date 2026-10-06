use crate::{StoreError, anonymous_session};
use board_domain::{
    poster_id::PublicDeletionRateIdentity,
    public_deletion::{Policy, Rejection, Target},
};
use chrono::{DateTime, Utc};
use sqlx::{PgConnection, PgPool, Postgres, Transaction};

/// Resolved by the HTTP server, never deserialized from a public form. A session
/// carries fingerprints of the current peer/environment, not its previous one.
#[derive(Clone, Copy)]
pub struct PublicDeletionContext {
    pub request_start: DateTime<Utc>,
    pub session: Option<anonymous_session::PostingSession>,
}
/// One server-owned HTTP request's deletion allowance. The immutable binding
/// prevents carrying a successful charge into another board or request context.
/// Deliberately not Clone, Copy, Debug, or serializable.
pub struct PublicDeletionBatch {
    slug: String,
    context: PublicDeletionContext,
    identity: PublicDeletionRateIdentity,
    state: BatchState,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BatchState {
    Fresh,
    Charged,
    Committing,
    Poisoned,
}

impl PublicDeletionBatch {
    pub fn new(
        slug: impl Into<String>,
        context: PublicDeletionContext,
        identity: PublicDeletionRateIdentity,
    ) -> Self {
        Self {
            slug: slug.into(),
            context,
            identity,
            state: BatchState::Fresh,
        }
    }

    pub fn slug(&self) -> &str {
        &self.slug
    }

    pub fn context(&self) -> PublicDeletionContext {
        self.context
    }

    fn needs_reservation(&self) -> Result<bool, StoreError> {
        match self.state {
            BatchState::Fresh => Ok(true),
            BatchState::Charged => Ok(false),
            BatchState::Committing | BatchState::Poisoned => Err(unavailable()),
        }
    }

    pub(crate) async fn begin(
        &self,
        pool: &PgPool,
    ) -> Result<Transaction<'static, Postgres>, StoreError> {
        let reserve = self.needs_reservation()?;
        let mut tx = pool.begin().await?;
        // This must precede every snapshot-taking statement, especially the
        // reservation and board lock, even on pools defaulting to SERIALIZABLE.
        sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
            .execute(&mut *tx)
            .await?;
        if reserve {
            sqlx::query("SELECT content.reserve_public_deletion($1)")
                .bind(self.identity.as_bytes().as_slice())
                .execute(&mut *tx)
                .await
                .map_err(reservation_error)?;
        }
        Ok(tx)
    }

    pub(crate) async fn commit(
        &mut self,
        tx: Transaction<'static, Postgres>,
    ) -> Result<(), StoreError> {
        self.finish_commit(tx.commit()).await
    }

    async fn finish_commit(
        &mut self,
        commit: impl std::future::Future<Output = Result<(), sqlx::Error>>,
    ) -> Result<(), StoreError> {
        self.needs_reservation()?;
        // Cancellation after this assignment leaves Committing, which is
        // unusable: COMMIT may have reached the server without an acknowledgement.
        self.state = BatchState::Committing;
        match commit.await {
            Ok(()) => {
                self.state = BatchState::Charged;
                Ok(())
            }
            Err(_) => {
                self.state = BatchState::Poisoned;
                Err(unavailable())
            }
        }
    }
}

fn unavailable() -> StoreError {
    StoreError::Database(sqlx::Error::Protocol("Public deletion unavailable.".into()))
}

fn reservation_error(error: sqlx::Error) -> StoreError {
    match error
        .as_database_error()
        .and_then(|error| error.code())
        .as_deref()
    {
        Some("P0081" | "P0082") => {
            StoreError::PublicDeletionRejected("Error: You cannot delete posts this often.")
        }
        // Capacity, malformed identity, missing function/permissions, and all
        // other storage failures fail closed without exposing private keys.
        _ => unavailable(),
    }
}

/// Preserve quota error precedence before target lookup without allocating or
/// charging an actor. The mutation transaction still reserves authoritatively.
pub async fn public_deletion_quota_precheck(
    pool: &PgPool,
    identity: &PublicDeletionRateIdentity,
) -> Result<(), StoreError> {
    sqlx::query("SELECT content.check_public_deletion_quota($1)")
        .bind(identity.as_bytes().as_slice())
        .execute(pool)
        .await
        .map_err(reservation_error)?;
    Ok(())
}

#[derive(sqlx::FromRow)]
pub(crate) struct Eligibility {
    deletion_no_op: bool,
    deletion_no_reply: bool,
    deletion_known_min_seconds: i32,
    deletion_unknown_min_seconds: i32,
    deletion_max_seconds: i32,
    created_at: DateTime<Utc>,
    op: bool,
    archived: bool,
    sticky: bool,
    vg: bool,
    staff_reply: bool,
}
fn rejected(error: Rejection) -> StoreError {
    StoreError::PublicDeletionRejected(error.message())
}
impl Eligibility {
    fn policy(&self) -> Result<Policy, StoreError> {
        Policy::new(
            self.deletion_no_op,
            self.deletion_no_reply,
            self.deletion_known_min_seconds,
            self.deletion_unknown_min_seconds,
            self.deletion_max_seconds,
        )
        .ok_or_else(|| {
            StoreError::Database(sqlx::Error::Protocol(
                "Invalid public deletion policy.".into(),
            ))
        })
    }
    pub(crate) fn before_authority(&self, start: DateTime<Utc>) -> Result<(), StoreError> {
        self.policy()?
            .before_authority(
                self.op,
                start
                    .timestamp()
                    .saturating_sub(self.created_at.timestamp())
                    .max(0) as u64,
            )
            .map_err(rejected)
    }
    pub(crate) async fn after_authority(
        &self,
        connection: &mut PgConnection,
        context: PublicDeletionContext,
    ) -> Result<(), StoreError> {
        let network_age = match context.session.filter(|session| !session.minted) {
            Some(session) => {
                // UserPwd captures its own time when resolving the cookie;
                // the upper-age gate separately uses the HTTP request start.
                let now = session.now.timestamp().max(0) as u64;
                anonymous_session::locked_snapshot(connection, &session.fingerprints.token)
                    .await?
                    .map(|snapshot| {
                        snapshot
                            .for_request(now, &session.fingerprints)
                            .network_age(now)
                    })
                    .unwrap_or(0)
            }
            None => 0,
        };
        self.policy()?
            .after_authority(
                Target {
                    op: self.op,
                    archived: self.archived,
                    sticky: self.sticky,
                    vg: self.vg,
                    staff_reply: self.staff_reply,
                },
                Utc::now()
                    .timestamp()
                    .saturating_sub(self.created_at.timestamp())
                    .max(0) as u64,
                network_age,
            )
            .map_err(rejected)
    }
}

pub(crate) async fn eligibility(
    connection: &mut PgConnection,
    slug: &str,
    id: i64,
) -> Result<Eligibility, StoreError> {
    sqlx::query_as("SELECT b.deletion_no_op,b.deletion_no_reply,b.deletion_known_min_seconds,b.deletion_unknown_min_seconds,b.deletion_max_seconds,p.created_at,p.id=p.thread_id AS op,t.archived_at IS NOT NULL AS archived,(p.id=p.thread_id AND t.sticky) AS sticky,b.slug='vg' AS vg,EXISTS(SELECT 1 FROM content.posts child WHERE child.board=p.board AND child.thread_id=p.thread_id AND child.id<>p.thread_id AND NOT child.deleted AND child.capcode IS NOT NULL) AS staff_reply FROM content.posts p JOIN content.visible_threads t ON t.id=p.thread_id AND t.board=p.board JOIN content.boards b ON b.slug=p.board WHERE p.board=$1 AND p.id=$2 AND NOT p.deleted")
        .bind(slug).bind(id).fetch_optional(connection).await?.ok_or(StoreError::NotFound)
}

/// Non-mutating early gate preserves the source's age/board error precedence.
/// Both proof mutation paths repeat it after acquiring the board lock.
pub async fn public_deletion_precheck(
    pool: &PgPool,
    slug: &str,
    id: i64,
    start: DateTime<Utc>,
) -> Result<(), StoreError> {
    eligibility(&mut *pool.acquire().await?, slug, id)
        .await?
        .before_authority(start)
}

/// Classify a legacy missing target without confusing a hidden/missing board
/// with an absent post. Board visibility and target visibility share one SQL
/// snapshot; a missing credential or attachment on a live post is not absence.
pub async fn public_deletion_target_exists(
    pool: &PgPool,
    slug: &str,
    id: i64,
) -> Result<bool, StoreError> {
    board_domain::BoardSlug::parse(slug).map_err(|_| StoreError::NotFound)?;
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM content.posts p JOIN content.visible_threads t ON t.id=p.thread_id AND t.board=p.board WHERE p.board=b.slug AND p.id=$2 AND NOT p.deleted) FROM content.boards b WHERE b.slug=$1")
        .bind(slug).bind(id).fetch_optional(pool).await?.ok_or(StoreError::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use board_domain::poster_id::PosterIdKey;
    use std::{
        future::Future,
        pin::pin,
        task::{Context, Poll, Waker},
    };

    fn batch() -> PublicDeletionBatch {
        let key = PosterIdKey::parse(&"11".repeat(32)).unwrap();
        PublicDeletionBatch::new(
            "test",
            PublicDeletionContext {
                request_start: Utc::now(),
                session: None,
            },
            key.public_deletion_rate_identity("192.0.2.1".parse().unwrap()),
        )
    }

    #[test]
    fn fresh_requires_charge_and_only_success_acknowledges_it() {
        let mut batch = batch();
        assert!(batch.needs_reservation().unwrap());
        {
            let mut commit = pin!(batch.finish_commit(std::future::ready(Ok(()))));
            let mut context = Context::from_waker(Waker::noop());
            assert!(matches!(
                commit.as_mut().poll(&mut context),
                Poll::Ready(Ok(()))
            ));
        }
        assert!(!batch.needs_reservation().unwrap());
    }

    #[test]
    fn failed_commit_poisoned_even_after_a_previous_success() {
        for state in [BatchState::Fresh, BatchState::Charged] {
            let mut batch = batch();
            batch.state = state;
            {
                let mut commit = pin!(batch.finish_commit(std::future::ready(Err(
                    sqlx::Error::Protocol("unknown commit outcome".into()),
                ))));
                let mut context = Context::from_waker(Waker::noop());
                assert!(matches!(
                    commit.as_mut().poll(&mut context),
                    Poll::Ready(Err(_))
                ));
            }
            assert!(batch.state == BatchState::Poisoned);
            assert!(matches!(
                batch.needs_reservation(),
                Err(StoreError::Database(_))
            ));
        }
    }

    #[test]
    fn cancelled_commit_cannot_reuse_an_ambiguous_charge() {
        for state in [BatchState::Fresh, BatchState::Charged] {
            let mut batch = batch();
            batch.state = state;
            {
                let mut commit = pin!(batch.finish_commit(std::future::pending()));
                let mut context = Context::from_waker(Waker::noop());
                assert!(commit.as_mut().poll(&mut context).is_pending());
                // Dropping the future models cancellation during COMMIT.
            }
            assert!(batch.state == BatchState::Committing);
            assert!(matches!(
                batch.needs_reservation(),
                Err(StoreError::Database(_))
            ));
        }
    }

    #[test]
    fn reservation_failure_does_not_echo_private_database_details() {
        let error = reservation_error(sqlx::Error::Protocol("private identity".into()));
        assert_eq!(error.to_string(), "Database unavailable.");
        assert!(!format!("{error:?}").contains("private identity"));
    }
}
