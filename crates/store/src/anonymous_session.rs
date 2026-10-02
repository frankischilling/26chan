//! Private anonymous state. None of these types are public response models.
use crate::StoreError;
use board_domain::anonymous_session::{Changes, Fingerprints, State};
use chrono::{DateTime, Utc};
use sqlx::{PgConnection, PgPool, Postgres, Transaction};

/// Built from a server-resolved capability, never deserialized from a request.
#[derive(Clone, Copy)]
pub struct PostingSession {
    pub fingerprints: Fingerprints,
    pub minted: bool,
    pub now: DateTime<Utc>,
}

pub struct Snapshot {
    state: State,
    network: [u8; 32],
    address: [u8; 32],
    environment: [u8; 32],
}

impl Snapshot {
    pub fn for_request(&self, now: u64, fingerprints: &Fingerprints) -> State {
        let mut state = self.state;
        state.resume(
            now,
            Changes {
                network: self.network != fingerprints.network,
                address: self.address != fingerprints.address,
                environment: self.environment != fingerprints.environment,
            },
        );
        state
    }
}

#[derive(sqlx::FromRow)]
struct Row {
    created_at: i64,
    network_at: i64,
    address_at: i64,
    environment_at: i64,
    activity_at: i64,
    action_at: i64,
    verified_level: i16,
    posts: i16,
    images: i16,
    threads: i16,
    reports: i16,
    pending: i16,
    change_score: i16,
    network_hash: Vec<u8>,
    address_hash: Vec<u8>,
    environment_hash: Vec<u8>,
}

fn invalid() -> StoreError {
    StoreError::Database(sqlx::Error::Protocol(
        "Invalid anonymous activity result.".into(),
    ))
}

impl TryFrom<Row> for Snapshot {
    type Error = StoreError;

    fn try_from(row: Row) -> Result<Self, Self::Error> {
        if row.pending > 15
            || row.change_score > 32
            || [
                row.created_at,
                row.network_at,
                row.address_at,
                row.environment_at,
            ]
            .iter()
            .any(|time| *time <= 0)
        {
            return Err(invalid());
        }
        Ok(Self {
            state: State {
                created_at: row.created_at.try_into().map_err(|_| invalid())?,
                network_at: row.network_at.try_into().map_err(|_| invalid())?,
                address_at: row.address_at.try_into().map_err(|_| invalid())?,
                environment_at: row.environment_at.try_into().map_err(|_| invalid())?,
                activity_at: row.activity_at.try_into().map_err(|_| invalid())?,
                action_at: row.action_at.try_into().map_err(|_| invalid())?,
                verified_level: row.verified_level.try_into().map_err(|_| invalid())?,
                posts: row.posts.try_into().map_err(|_| invalid())?,
                images: row.images.try_into().map_err(|_| invalid())?,
                threads: row.threads.try_into().map_err(|_| invalid())?,
                reports: row.reports.try_into().map_err(|_| invalid())?,
                pending: row.pending.try_into().map_err(|_| invalid())?,
                change_score: row.change_score.try_into().map_err(|_| invalid())?,
            },
            network: row.network_hash.try_into().map_err(|_| invalid())?,
            address: row.address_hash.try_into().map_err(|_| invalid())?,
            environment: row.environment_hash.try_into().map_err(|_| invalid())?,
        })
    }
}

pub async fn snapshot(pool: &PgPool, token: &[u8; 32]) -> Result<Option<Snapshot>, StoreError> {
    let row: Option<Row> = sqlx::query_as("SELECT * FROM content.anonymous_session($1)")
        .bind(token.as_slice())
        .fetch_optional(pool)
        .await?;
    row.map(Snapshot::try_from).transpose()
}

pub(crate) async fn locked_snapshot(
    connection: &mut PgConnection,
    token: &[u8; 32],
) -> Result<Option<Snapshot>, StoreError> {
    let row: Option<Row> = sqlx::query_as("SELECT * FROM content.lock_anonymous_session($1)")
        .bind(token.as_slice())
        .fetch_optional(connection)
        .await?;
    row.map(Snapshot::try_from).transpose()
}

fn proof(value: Option<Vec<u8>>) -> Result<Option<[u8; 32]>, StoreError> {
    value
        .map(|value| value.try_into().map_err(|_| invalid()))
        .transpose()
}

pub async fn post_proof(
    pool: &PgPool,
    token: &[u8; 32],
    board: &str,
    post: i64,
) -> Result<Option<[u8; 32]>, StoreError> {
    let value = sqlx::query_scalar("SELECT content.anonymous_post_proof($1,$2,$3)")
        .bind(token.as_slice())
        .bind(board)
        .bind(post)
        .fetch_one(pool)
        .await?;
    proof(value)
}

pub(crate) async fn locked_post_proof(
    connection: &mut PgConnection,
    token: &[u8; 32],
    board: &str,
    post: i64,
) -> Result<Option<[u8; 32]>, StoreError> {
    let value = sqlx::query_scalar("SELECT content.lock_anonymous_post_proof($1,$2,$3)")
        .bind(token.as_slice())
        .bind(board)
        .bind(post)
        .fetch_one(connection)
        .await?;
    proof(value)
}

fn mutation_error(error: sqlx::Error) -> StoreError {
    if error
        .as_database_error()
        .and_then(|error| error.code())
        .is_some_and(|code| code == "28000")
    {
        StoreError::AuthorizationChanged
    } else {
        error.into()
    }
}

pub(crate) async fn record_post(
    tx: &mut Transaction<'_, Postgres>,
    session: PostingSession,
    board: &str,
    post: i64,
) -> Result<(), StoreError> {
    let fingerprints = session.fingerprints;
    sqlx::query("SELECT content.register_anonymous_post($1,$2,$3,$4,$5,$6,$7,$8)")
        .bind(fingerprints.token.as_slice())
        .bind(fingerprints.network.as_slice())
        .bind(fingerprints.address.as_slice())
        .bind(fingerprints.environment.as_slice())
        .bind(session.minted)
        .bind(board)
        .bind(post)
        .bind(session.now.timestamp())
        .execute(&mut **tx)
        .await
        .map_err(mutation_error)?;
    Ok(())
}

pub(crate) async fn record_report(
    tx: &mut Transaction<'_, Postgres>,
    session: PostingSession,
    board: &str,
    report: i64,
) -> Result<(), StoreError> {
    let fingerprints = session.fingerprints;
    sqlx::query("SELECT content.register_anonymous_report($1,$2,$3,$4,$5,$6,$7,$8)")
        .bind(fingerprints.token.as_slice())
        .bind(fingerprints.network.as_slice())
        .bind(fingerprints.address.as_slice())
        .bind(fingerprints.environment.as_slice())
        .bind(session.minted)
        .bind(board)
        .bind(report)
        .bind(session.now.timestamp())
        .execute(&mut **tx)
        .await
        .map_err(mutation_error)?;
    Ok(())
}
