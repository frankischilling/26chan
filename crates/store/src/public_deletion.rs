use crate::{StoreError, anonymous_session};
use board_domain::public_deletion::{Policy, Rejection, Target};
use chrono::{DateTime, Utc};
use sqlx::{PgConnection, PgPool};

/// Resolved by the HTTP server, never deserialized from a public form. A session
/// carries fingerprints of the current peer/environment, not its previous one.
#[derive(Clone, Copy)]
pub struct PublicDeletionContext {
    pub request_start: DateTime<Utc>,
    pub session: Option<anonymous_session::PostingSession>,
}
impl Default for PublicDeletionContext {
    fn default() -> Self {
        Self {
            request_start: Utc::now(),
            session: None,
        }
    }
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
