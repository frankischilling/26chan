use crate::{StoreError, anonymous_session};
use board_domain::content_admission::{Actor, Decision, Policy, Post, Rule};
use board_domain::name_trip_admission::{self, Rejection as TripRejection};
use sqlx::PgConnection;
use std::net::IpAddr;
use std::sync::{Arc, LazyLock};

pub(crate) mod attachment;
pub(crate) mod automatic;
mod network;
pub(crate) mod staff;

pub(crate) struct NetworkContext {
    pub asn: board_domain::content_admission::network::ProviderValue<Option<u32>>,
    pub hints: board_domain::content_admission::network::hints::DerivedHints,
    pub staff_bypass: bool,
}

static WORK: LazyLock<Arc<tokio::sync::Semaphore>> =
    LazyLock::new(|| Arc::new(tokio::sync::Semaphore::new(2)));

pub(crate) struct Snapshot {
    hints: board_domain::content_admission::network::hints::DerivedHints,
    revision: i64,
    peer: Option<String>,
    actor: Actor,
    rules: Vec<Rule>,
    activity: Option<board_domain::anonymous_session::State>,
    network: network::Snapshot,
    now: u64,
    activity_now: u64,
    asn: board_domain::content_admission::network::ProviderValue<Option<u32>>,
}

#[derive(sqlx::FromRow)]
struct RuleRow {
    id: i64,
    board: String,
    pattern: String,
    regex: bool,
    autosage: bool,
    log: bool,
    quiet: bool,
    lenient: bool,
    ops_only: bool,
    min_count: i32,
    ban_days: i32,
}

impl From<RuleRow> for Rule {
    fn from(row: RuleRow) -> Self {
        Self {
            id: row.id,
            board: row.board,
            pattern: row.pattern,
            regex: row.regex,
            autosage: row.autosage,
            log: row.log,
            quiet: row.quiet,
            lenient: row.lenient,
            ops_only: row.ops_only,
            min_count: row.min_count,
            ban_days: row.ban_days,
        }
    }
}

pub(crate) async fn begin(
    connection: &mut PgConnection,
    board: &str,
    peer: Option<IpAddr>,
    session: Option<anonymous_session::PostingSession>,
    request_at: i64,
    network_context: NetworkContext,
) -> Result<Snapshot, StoreError> {
    let peer = peer.map(|peer| peer.to_canonical().to_string());
    let (revision, banned): (i64, bool) =
        sqlx::query_as("SELECT * FROM content.lock_content_admission($1,$2)")
            .bind(board)
            .bind(&peer)
            .fetch_one(&mut *connection)
            .await?;
    if banned {
        return Err(StoreError::ContentRejected(
            "You are banned from posting.".into(),
        ));
    }
    let now = u64::try_from(request_at).map_err(unavailable)?;
    let activity_now = session
        .map(|session| session.now.timestamp())
        .unwrap_or(request_at)
        .try_into()
        .map_err(unavailable)?;
    let activity = if let Some(session) = session {
        let session_now = session.now.timestamp().try_into().map_err(unavailable)?;
        match anonymous_session::locked_snapshot(&mut *connection, &session.fingerprints.token)
            .await?
        {
            Some(snapshot) if !session.minted => {
                Some(snapshot.for_request(session_now, &session.fingerprints))
            }
            None if session.minted => {
                Some(board_domain::anonymous_session::State::new(session_now))
            }
            _ => return Err(StoreError::AuthorizationChanged),
        }
    } else {
        None
    };
    let actor = match (activity.as_ref(), session) {
        (Some(_), Some(session)) if session.minted => Actor {
            session_present: true,
            ..Actor::default()
        },
        (Some(state), Some(session)) => Actor::from_state(
            state,
            session.now.timestamp().try_into().map_err(unavailable)?,
        ),
        _ => Actor::default(),
    };
    let rows: Vec<RuleRow> = sqlx::query_as("SELECT * FROM content.content_admission_rules($1)")
        .bind(board)
        .fetch_all(&mut *connection)
        .await?;
    let network =
        network::snapshot(connection, board, request_at, network_context.staff_bypass).await?;
    Ok(Snapshot {
        hints: network_context.hints,
        revision,
        peer,
        actor,
        rules: rows.into_iter().map(Into::into).collect(),
        activity,
        network,
        now,
        activity_now,
        asn: network_context.asn,
    })
}

pub(crate) struct Input {
    pub board: String,
    pub parent: i64,
    pub name: String,
    pub legacy_trip: String,
    pub moderator: bool,
    pub subject: String,
    pub comment: String,
    pub filename: String,
}

pub(crate) struct Evaluation {
    snapshot: Snapshot,
    input: Input,
    pub decision: Decision,
}

impl Snapshot {
    pub(crate) async fn evaluate(mut self, input: Input) -> Result<Evaluation, StoreError> {
        if self.rules.is_empty() {
            let decision = evaluate(Vec::new(), self.actor, &input)?;
            return Ok(Evaluation {
                snapshot: self,
                input,
                decision,
            });
        }
        let permit = WORK
            .clone()
            .try_acquire_owned()
            .map_err(|_| unavailable(()))?;
        let rules = std::mem::take(&mut self.rules);
        let actor = self.actor;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let decision = evaluate(rules, actor, &input)?;
            Ok(Evaluation {
                snapshot: self,
                input,
                decision,
            })
        })
        .await
        .map_err(|_| unavailable(()))?
    }
}

/// Capcoded posts bypass configured public filters, but not the fixed source
/// name/trip hook. Use the same bounded native-work pool for that final check.
pub(crate) async fn evaluate_staff_identity(
    name: String,
    legacy_trip: String,
    moderator: bool,
) -> Result<Option<&'static str>, StoreError> {
    if name.is_ascii() {
        return name_trip_admission::evaluate(&name, &legacy_trip, moderator)
            .map(|rejection| rejection.map(TripRejection::message))
            .map_err(unavailable);
    }
    let permit = WORK
        .clone()
        .try_acquire_owned()
        .map_err(|_| unavailable(()))?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        name_trip_admission::evaluate(&name, &legacy_trip, moderator)
            .map(|rejection| rejection.map(TripRejection::message))
            .map_err(unavailable)
    })
    .await
    .map_err(|_| unavailable(()))?
}

fn evaluate(rules: Vec<Rule>, actor: Actor, input: &Input) -> Result<Decision, StoreError> {
    let decision = Policy::compile(rules)
        .map_err(unavailable)?
        .evaluate(
            Post {
                board: &input.board,
                reply: input.parent > 0,
                name: &input.name,
                subject: &input.subject,
                comment: &input.comment,
                filename: &input.filename,
            },
            actor,
        )
        .map_err(unavailable)?;
    Ok(decision)
}

fn unavailable(_: impl Sized) -> StoreError {
    // Native failures and patterns can contain operator rule text. Never echo
    // them through the store error or ordinary application logs.
    StoreError::Database(sqlx::Error::Protocol(
        "Content admission is unavailable.".into(),
    ))
}

impl Evaluation {
    /// This stage uses the already-locked policy and activity state. Optional
    /// provider-backed rules fail closed until their trusted data is supplied.
    pub(crate) fn network_rejection(
        &self,
        work_safe: bool,
        has_image: bool,
    ) -> Result<Option<String>, StoreError> {
        use board_domain::content_admission::network::{Leniency, Request};
        let Some(policy) = &self.snapshot.network.policy else {
            return Ok(None);
        };
        let peer = self
            .snapshot
            .peer
            .as_deref()
            .map(str::parse::<IpAddr>)
            .transpose()
            .map_err(unavailable)?;
        let address = match peer {
            Some(IpAddr::V4(address)) => Some(address),
            _ => None,
        };
        let matched = policy
            .evaluate(Request {
                now: self.snapshot.now,
                activity_now: Some(self.snapshot.activity_now),
                address,
                asn: self.snapshot.asn,
                board: Some(&self.input.board),
                work_safe,
                op: self.input.parent == 0,
                image: has_image,
                report: false,
                signatures: self.snapshot.hints.signatures(),
                actor: self.snapshot.activity.as_ref(),
            })
            .map_err(unavailable)?;
        Ok(matched.map(|rule| {
            let subject = if rule.images_only {
                "Uploading files"
            } else if rule.ops_only {
                "Posting threads"
            } else {
                "Posting"
            };
            let duration = if rule.expires_at != 0 || rule.leniency != Leniency::None {
                "temporarily"
            } else {
                "permanently"
            };
            let followup = match rule.leniency {
                Leniency::None => "",
                Leniency::KnownOrVerified => {
                    " Please verify your e-mail address or try again later."
                }
                Leniency::Known => " Please try again in a while.",
                Leniency::Verified => " Please verify your e-mail address to continue.",
            };
            format!(
                "{subject} from your IP range has been {duration} blocked due to abuse.{followup}"
            )
        }))
    }

    /// Fixed identity admission runs after the preceding configured stages.
    /// Keep its native normalization bounded independently of content matching.
    pub(crate) async fn identity_rejection(&self) -> Result<Option<&'static str>, StoreError> {
        evaluate_staff_identity(
            self.input.name.clone(),
            self.input.legacy_trip.clone(),
            self.input.moderator,
        )
        .await
    }

    pub(crate) fn autosage_proof(&self) -> Option<(i64, i64)> {
        match self.decision {
            Decision::Autosage { rule } => Some((rule, self.snapshot.revision)),
            _ => None,
        }
    }
    pub(crate) async fn record(&self, connection: &mut PgConnection) -> Result<(), StoreError> {
        let (rule, kind, comment) = match &self.decision {
            Decision::Allow
            | Decision::InvalidSubject {
                logged_rule: None, ..
            } => return Ok(()),
            Decision::Autosage { rule } => (Some(*rule), "autosage", self.input.comment.as_str()),
            Decision::Log { rule, comment } => (Some(*rule), "log", comment.as_str()),
            Decision::Reject { rule, quiet, .. } => (
                Some(*rule),
                if *quiet { "quiet" } else { "reject" },
                self.input.comment.as_str(),
            ),
            Decision::FilenameProxy => (None, "filename", self.input.comment.as_str()),
            Decision::InvalidSubject {
                logged_rule: Some(rule),
                comment,
            } => (Some(*rule), "log", comment.as_str()),
        };
        sqlx::query("SELECT content.record_content_admission($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
            .bind(&self.input.board)
            .bind(&self.snapshot.peer)
            .bind(self.snapshot.revision)
            .bind(rule)
            .bind(kind)
            .bind(self.input.parent)
            .bind(&self.input.name)
            .bind(&self.input.subject)
            .bind(comment)
            .bind(&self.input.filename)
            .execute(connection)
            .await?;
        Ok(())
    }

    pub(crate) fn rejection(&self) -> Option<String> {
        match &self.decision {
            Decision::Reject {
                rule,
                ban_days,
                quiet: false,
            } => {
                let message = if *ban_days != 0 {
                    "Error: Your post contained banned text."
                } else {
                    "Error: Our system thinks your post is spam. Please reformat and try again."
                };
                Some(if self.input.board == "test" {
                    format!("{message} (filter ID: {rule})")
                } else {
                    message.into()
                })
            }
            Decision::InvalidSubject { .. } => Some("You can't post with that subject.".into()),
            Decision::FilenameProxy => Some("Error: Abnormal reply.".into()),
            _ => None,
        }
    }
}

pub(crate) async fn quiet_post(
    connection: &mut PgConnection,
    board: &str,
    parent: i64,
) -> Result<i64, StoreError> {
    let value: Option<i64> = if parent == 0 {
        sqlx::query_scalar("SELECT max(thread_id) FROM content.posts WHERE board=$1 AND id<>thread_id AND NOT deleted")
            .bind(board).fetch_one(connection).await?
    } else {
        sqlx::query_scalar("SELECT max(id) FROM content.posts WHERE board=$1 AND NOT deleted")
            .bind(board)
            .fetch_one(connection)
            .await?
    };
    Ok(if parent == 0 {
        value.unwrap_or(0)
    } else {
        value.and_then(|id| id.checked_add(1)).unwrap_or(0)
    })
}
