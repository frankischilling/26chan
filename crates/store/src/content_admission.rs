use crate::{StoreError, anonymous_session};
use board_domain::content_admission::{Actor, Decision, Policy, Post, Rule};
use sqlx::PgConnection;
use std::net::IpAddr;
use std::sync::{Arc, LazyLock};

static WORK: LazyLock<Arc<tokio::sync::Semaphore>> =
    LazyLock::new(|| Arc::new(tokio::sync::Semaphore::new(2)));

pub(crate) struct Snapshot {
    revision: i64,
    peer: Option<String>,
    actor: Actor,
    rules: Vec<Rule>,
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
    let actor = if let Some(session) = session {
        match anonymous_session::locked_snapshot(&mut *connection, &session.fingerprints.token)
            .await?
        {
            Some(snapshot) if !session.minted => {
                let now = session
                    .now
                    .timestamp()
                    .try_into()
                    .map_err(|_| StoreError::Invalid("Invalid anonymous activity context."))?;
                Actor::from_state(&snapshot.for_request(now, &session.fingerprints), now)
            }
            None if session.minted => Actor {
                session_present: true,
                ..Actor::default()
            },
            _ => return Err(StoreError::AuthorizationChanged),
        }
    } else {
        Actor::default()
    };
    let rows: Vec<RuleRow> = sqlx::query_as("SELECT * FROM content.content_admission_rules($1)")
        .bind(board)
        .fetch_all(connection)
        .await?;
    Ok(Snapshot {
        revision,
        peer,
        actor,
        rules: rows.into_iter().map(Into::into).collect(),
    })
}

pub(crate) struct Input {
    pub board: String,
    pub parent: i64,
    pub name: String,
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
            let decision = Policy::compile(Vec::new())
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
                    self.actor,
                )
                .map_err(unavailable)?;
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
            let policy = Policy::compile(rules).map_err(unavailable)?;
            let decision = policy
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

fn unavailable(_: impl Sized) -> StoreError {
    // Native failures and patterns can contain operator rule text. Never echo
    // them through the store error or ordinary application logs.
    StoreError::Database(sqlx::Error::Protocol(
        "Content admission is unavailable.".into(),
    ))
}

impl Evaluation {
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
