//! Exact-file candidates carry a private receipt into a fresh effect transaction.
use super::attachment::Preflight;
use crate::{StaffPostAuthority, StoreError, anonymous_session::PostingSession};
use board_domain::{
    content_admission::{
        files::{Digest, Mode, Policy, Request, Response, Rule},
        network::ProviderValue,
    },
    poster_id::PublicPostingRateIdentity,
};
use sqlx::{PgConnection, PgPool};
use std::{fmt, net::IpAddr};

/// Public response facts only. No capability, policy reason or private identity.
#[derive(Debug)]
pub struct FileRefusal {
    response: Response,
    post: Option<i64>,
    diagnostic: Option<String>,
    cookie_refreshed: bool,
}
impl FileRefusal {
    pub fn response(&self) -> Response {
        self.response
    }
    pub fn quiet_post(&self) -> Option<i64> {
        self.post
    }
    pub fn cookie_refreshed(&self) -> bool {
        self.cookie_refreshed
    }
    pub fn message(&self) -> &str {
        match self.response {
            Response::FailedUpload => "Error: Upload failed.",
            Response::PrivateDiagnostic => self
                .diagnostic
                .as_deref()
                .unwrap_or("Error: Upload failed."),
            Response::Quiet => "Post accepted.",
            Response::DmcaWarning => {
                "Error: This image cannot be uploaded because it is the subject of a copyright infringement claim. Please refrain from posting it."
            }
            Response::BanRedirect => crate::restrictions::BANNED,
        }
    }
}
impl fmt::Display for FileRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}
fn unavailable() -> StoreError {
    StoreError::Database(sqlx::Error::Protocol(
        "File admission is unavailable.".into(),
    ))
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn digest(value: &str) -> Result<Digest, StoreError> {
    if value.len() != 32 {
        return Err(unavailable());
    }
    let mut result = [0; 16];
    for (output, pair) in result.iter_mut().zip(value.as_bytes().chunks_exact(2)) {
        *output = u8::from_str_radix(std::str::from_utf8(pair).map_err(|_| unavailable())?, 16)
            .map_err(|_| unavailable())?;
    }
    Ok(result)
}
#[derive(sqlx::FromRow)]
struct Row {
    id: i64,
    board: String,
    digest: Vec<u8>,
    mode: i16,
    ban_days: i32,
    quiet: bool,
}

// Deliberately not Debug/Serialize: this holds a one-use upload capability.
pub(crate) struct Candidate {
    board: String,
    parent: i64,
    job: String,
    capability: String,
    revision: i64,
    rule: i64,
    digest: Digest,
    expected: serde_json::Value,
}
pub(crate) async fn candidate(
    connection: &mut PgConnection,
    preflight: &Preflight<'_>,
    worksafe: bool,
    revision: i64,
) -> Result<Option<Candidate>, StoreError> {
    let context = preflight.file_context();
    // The selected staff badge is trusted server state. Accepted posts still
    // pass the later issuer's live badge and permission revalidation.
    if context.badged {
        return Ok(None);
    }
    let rows: Vec<Row> = sqlx::query_as("SELECT * FROM content.file_admission_rules($1)")
        .bind(context.board)
        .fetch_all(connection)
        .await?;
    let rules = rows
        .into_iter()
        .map(|row| {
            Ok(Rule {
                id: row.id,
                scope: row.board,
                digest: row.digest.try_into().map_err(|_| unavailable())?,
                active: true,
                quiet: row.quiet,
                mode: match row.mode {
                    0 => Mode::Reject,
                    1 => Mode::Ban { days: row.ban_days },
                    2 => Mode::Dmca,
                    _ => return Err(unavailable()),
                },
            })
        })
        .collect::<Result<Vec<_>, StoreError>>()?;
    let normalized = match context.normalized {
        Some(value) => ProviderValue::Available(Some(digest(value)?)),
        None => ProviderValue::Unavailable,
    };
    let original = match context.original {
        Some(value) => ProviderValue::Available(Some(value.try_into().map_err(|_| unavailable())?)),
        None if context.eligible == Some(false) => ProviderValue::Available(None),
        None => ProviderValue::Unavailable,
    };
    let found = Policy::compile(rules)
        .map_err(|_| unavailable())?
        .match_rule(Request {
            board: context.board,
            worksafe,
            test_board: context.board == "test",
            badged: false,
            normalized,
            original,
            original_eligible: context
                .eligible
                .map_or(ProviderValue::Unavailable, ProviderValue::Available),
            prior_dmca: ProviderValue::Unavailable,
        })
        .map_err(|_| unavailable())?;
    Ok(found.map(|found|Candidate{
        board:context.board.into(),parent:context.parent,job:context.job.into(),capability:context.capability.into(),revision,
        rule:found.rule,digest:found.digest,
        expected:serde_json::json!({"filename":context.filename,"normalized":context.normalized,"original":context.original.map(hex),"eligible":context.eligible}),
    }))
}
impl Candidate {
    /// Both identity inputs come from the writer's same verified transport peer.
    /// Never retry this operation after an uncertain database commit.
    pub(crate) async fn record(
        &self,
        pool: &PgPool,
        actor: &PublicPostingRateIdentity,
        peer: IpAddr,
        anonymous: Option<PostingSession>,
        staff: Option<&StaffPostAuthority<'_>>,
        challenge: Option<&crate::WriteChallengeProof>,
    ) -> Result<FileRefusal, StoreError> {
        if staff.is_some() && anonymous.is_some()
            || staff
                .and_then(|s| s.identity)
                .is_some_and(|i| i.capcode.is_some())
        {
            return Err(StoreError::AuthorizationChanged);
        }
        let cookie=anonymous.map(|session|serde_json::json!({
            "network":hex(&session.fingerprints.network),"address":hex(&session.fingerprints.address),
            "environment":hex(&session.fingerprints.environment),"minted":session.minted,"now":session.now.timestamp(),
        }));
        let mut tx = pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
            .execute(&mut *tx)
            .await?;
        crate::write_challenge::install(&mut tx, challenge, anonymous.as_ref()).await?;
        let (kind,diagnostic,post,cookie_refreshed):(String,Option<String>,Option<i64>,bool)=sqlx::query_as(
            "SELECT * FROM content.finalize_file_filter($1,$2::inet,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)"
        ).bind(actor.as_bytes().as_slice()).bind(peer.to_canonical().to_string())
         .bind(anonymous.map(|session|session.fingerprints.token.to_vec())).bind(&self.board).bind(self.parent)
         .bind(&self.job).bind(&self.capability).bind(self.revision).bind(self.rule).bind(self.digest.as_slice()).bind(&self.expected)
         .bind(staff.map(|s|s.session_hash)).bind(staff.map(|s|s.csrf_hash)).bind(staff.map(|s|s.idle_seconds)).bind(cookie)
         .fetch_one(&mut *tx).await.map_err(|error| {
            if error.as_database_error().and_then(|e|e.code()).as_deref()==Some("28000") {StoreError::AuthorizationChanged} else {StoreError::Database(error)}
         })?;
        let response = match kind.as_str() {
            "failed" => Response::FailedUpload,
            "diagnostic" => Response::PrivateDiagnostic,
            "quiet" => Response::Quiet,
            "dmca" => Response::DmcaWarning,
            "banned" => Response::BanRedirect,
            _ => return Err(unavailable()),
        };
        if (response == Response::Quiet) != post.is_some()
            || post.is_some_and(|p| p < 0)
            || (response == Response::PrivateDiagnostic) != diagnostic.is_some()
            || diagnostic
                .as_ref()
                .is_some_and(|d| self.board != "test" || d.is_empty() || d.len() > 2048)
            || staff.is_some() && cookie_refreshed
        {
            return Err(unavailable());
        }
        tx.commit().await?;
        Ok(FileRefusal {
            response,
            post,
            diagnostic,
            cookie_refreshed,
        })
    }
}
