use crate::{AppError, auth::Session};
use sqlx::PgPool;

#[derive(sqlx::FromRow)]
pub struct Report {
    pub id: i64,
    pub board: String,
    pub post_id: i64,
    pub thread_id: i64,
    pub reason: String,
    pub category_id: Option<i64>,
    pub category_kind: Option<i16>,
    pub name: String,
    pub trip: Option<String>,
    pub poster_id: Option<String>,
    pub capcode: Option<String>,
    pub country: Option<String>,
    pub country_name: Option<String>,
    pub board_flag: Option<String>,
    pub board_flag_type: String,
    pub flag_name: Option<String>,
    pub subject: String,
    pub comment: String,
    pub comment_format: i16,
    pub staff_authorized_limits: bool,
    pub wordfilter_payload: Option<Vec<u8>>,
    pub state: String,
    pub closed: bool,
    pub sticky: bool,
    pub permasage: bool,
    pub permaage: bool,
    pub undead: bool,
    pub archived: bool,
    pub deleted: bool,
    pub spoilers_enabled: bool,
    pub image_spoiler: bool,
    #[sqlx(skip)]
    pub attachment: Option<Attachment>,
}
impl Report {
    pub fn category_kind_label(&self) -> &'static str {
        match self.category_kind {
            Some(1) => "rule",
            Some(2) => "illegal",
            _ => "unknown",
        }
    }
}
#[derive(Clone, sqlx::FromRow)]
pub struct Attachment {
    pub post_id: i64,
    pub filename: String,
    pub bytes: i64,
    pub width: i32,
    pub height: i32,
    pub spoiler: bool,
    pub tim: i64,
    pub thumbnail_width: Option<i32>,
    pub thumbnail_height: Option<i32>,
    pub available: bool,
}
pub async fn reports(pool: &PgPool, session: &Session) -> Result<Vec<Report>, AppError> {
    if !session.at_least(crate::access::Level::Janitor) {
        return Err(AppError::Forbidden);
    }
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *tx)
        .await?;
    let mut reports: Vec<Report> = sqlx::query_as("SELECT r.id,r.board,r.post_id,p.thread_id,r.reason,r.category_id,r.category_kind,p.name,p.trip,p.poster_id,p.capcode,p.country,p.country_name,p.board_flag,p.board_flag_type,p.flag_name,p.subject,p.comment,p.comment_format,p.staff_authorized_limits,p.wordfilter_payload,r.state,(t.closed OR t.archived_at IS NOT NULL) AS closed,t.sticky,t.permasage,t.permaage,t.undead,(t.archived_at IS NOT NULL) AS archived,(p.deleted OR t.deleted) AS deleted,b.comment_spoiler_cleanup AS spoilers_enabled,p.image_spoiler FROM content.reports r JOIN content.posts p ON p.id=r.post_id AND p.board=r.board JOIN content.threads t ON t.id=p.thread_id AND t.board=p.board JOIN content.boards b ON b.slug=p.board WHERE ('all'=ANY($1) OR r.board=ANY($1)) AND NOT r.board=ANY($2) ORDER BY (r.state='open') DESC,r.id DESC LIMIT 100")
        .bind(&session.permissions.allow_boards).bind(&session.permissions.deny_boards)
        .fetch_all(&mut *tx).await?;
    let ids: Vec<i64> = reports.iter().map(|r| r.post_id).collect();
    let attachments: Vec<Attachment> =
        sqlx::query_as("SELECT * FROM content.staff_post_media WHERE post_id=ANY($1)")
            .bind(ids)
            .fetch_all(&mut *tx)
            .await?;
    for report in &mut reports {
        report.attachment = attachments
            .iter()
            .find(|a| a.post_id == report.post_id)
            .cloned();
    }
    tx.commit().await?;
    Ok(reports)
}
pub(crate) async fn prepare_moderation(
    pool: &PgPool,
    session: &Session,
    board: &str,
    target: i64,
    action: &str,
) -> Result<sqlx::Transaction<'static, sqlx::Postgres>, AppError> {
    if !session.at_least(crate::access::Level::Janitor) {
        return Err(AppError::Forbidden);
    }
    if !session.recent {
        return Err(AppError::Recent);
    }
    if !matches!(
        action,
        "close"
            | "reopen"
            | "sticky"
            | "unsticky"
            | "permasage"
            | "unpermasage"
            | "permaage"
            | "unpermaage"
            | "undead"
            | "unundead"
            | "remove-post"
            | "remove-file"
            | "spoiler"
            | "unspoiler"
            | "remove-thread"
            | "resolve"
            | "dismiss"
    ) || target <= 0
    {
        return Err(AppError::Invalid);
    }
    if !session
        .permissions
        .action_allowed(&session.role, board, action)
    {
        return Err(AppError::Forbidden);
    }
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
        .execute(&mut *tx)
        .await?;
    let mut audit_changed = true;
    sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
        .bind(board)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(AppError::NotFound)?;
    if matches!(action, "spoiler" | "unspoiler") {
        let changed: bool = sqlx::query_scalar("SELECT content.set_post_image_spoiler($1,$2,$3)")
            .bind(board)
            .bind(target)
            .bind(action == "spoiler")
            .fetch_one(&mut *tx)
            .await
            .map_err(
                |error| match error.as_database_error().and_then(|e| e.code()).as_deref() {
                    Some("P0002") => AppError::NotFound,
                    Some("22023") => AppError::Invalid,
                    _ => AppError::Database(error),
                },
            )?;
        if !changed {
            return Ok(tx);
        }
    } else if action == "remove-file" {
        sqlx::query("SELECT content.staff_delete_post_attachment($1,$2)")
            .bind(board)
            .bind(target)
            .execute(&mut *tx)
            .await
            .map_err(|error| {
                if error.as_database_error().and_then(|e| e.code()).as_deref() == Some("P0002") {
                    AppError::NotFound
                } else {
                    AppError::Database(error)
                }
            })?;
    } else if matches!(action, "resolve" | "dismiss") {
        let thread: i64 = sqlx::query_scalar("SELECT p.thread_id FROM content.reports r JOIN content.posts p ON p.id=r.post_id AND p.board=r.board WHERE r.board=$1 AND r.id=$2 FOR UPDATE OF r").bind(board).bind(target).fetch_optional(&mut *tx).await?.ok_or(AppError::NotFound)?;
        sqlx::query("UPDATE content.reports SET state=$3 WHERE board=$1 AND id=$2")
            .bind(board)
            .bind(target)
            .bind(if action == "resolve" {
                "resolved"
            } else {
                "dismissed"
            })
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "UPDATE content.threads SET modified_at=clock_timestamp() WHERE board=$1 AND id=$2",
        )
        .bind(board)
        .bind(thread)
        .execute(&mut *tx)
        .await?;
    } else {
        let thread: i64 = if action == "remove-post" {
            sqlx::query_scalar("SELECT thread_id FROM content.posts WHERE board=$1 AND id=$2 AND NOT deleted FOR UPDATE").bind(board).bind(target).fetch_optional(&mut *tx).await?.ok_or(AppError::NotFound)?
        } else {
            target
        };
        let (archived, closed, sticky, permasage, permaage, undead):
            (bool, bool, bool, bool, bool, bool) = sqlx::query_as(
            "SELECT archived_at IS NOT NULL,closed,sticky,permasage,permaage,undead FROM content.threads WHERE board=$1 AND id=$2 AND NOT deleted FOR UPDATE",
        )
        .bind(board)
        .bind(thread)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(AppError::NotFound)?;
        if archived
            && matches!(
                action,
                "close"
                    | "reopen"
                    | "sticky"
                    | "unsticky"
                    | "permasage"
                    | "unpermasage"
                    | "permaage"
                    | "unpermaage"
                    | "undead"
                    | "unundead"
            )
        {
            return Err(AppError::Invalid);
        }
        match action {
            "close" | "reopen" => {
                audit_changed = closed != (action == "close");
                sqlx::query("UPDATE content.threads SET closed=$3,modified_at=clock_timestamp() WHERE board=$1 AND id=$2").bind(board).bind(thread).bind(action=="close").execute(&mut *tx).await?;
            }
            "sticky" | "unsticky" => {
                let requested_sticky = action == "sticky";
                audit_changed = sticky != requested_sticky;
                // Source thread options refresh root only on actual unsticky.
                // Use mutation time after the locks, not transaction start.
                sqlx::query("UPDATE content.threads SET sticky=$3,bumped_at=CASE WHEN $4 THEN clock_timestamp() ELSE bumped_at END,modified_at=clock_timestamp() WHERE board=$1 AND id=$2")
                    .bind(board)
                    .bind(thread)
                    .bind(requested_sticky)
                    .bind(sticky && !requested_sticky)
                    .execute(&mut *tx)
                    .await?;
            }
            "permasage" | "unpermasage" => {
                audit_changed = permasage != (action == "permasage");
                sqlx::query("UPDATE content.threads SET permasage=$3,modified_at=clock_timestamp() WHERE board=$1 AND id=$2").bind(board).bind(thread).bind(action=="permasage").execute(&mut *tx).await?;
            }
            "permaage" | "unpermaage" => {
                audit_changed = permaage != (action == "permaage");
                sqlx::query("UPDATE content.threads SET permaage=$3,modified_at=clock_timestamp() WHERE board=$1 AND id=$2").bind(board).bind(thread).bind(action=="permaage").execute(&mut *tx).await?;
            }
            "undead" | "unundead" => {
                audit_changed = undead != (action == "undead");
                sqlx::query("UPDATE content.threads SET undead=$3,modified_at=clock_timestamp() WHERE board=$1 AND id=$2").bind(board).bind(thread).bind(action=="undead").execute(&mut *tx).await?;
            }
            "remove-post" | "remove-thread" => {
                let whole = action == "remove-thread" || target == thread;
                sqlx::query("UPDATE content.posts SET deleted=true WHERE board=$1 AND (($3 AND thread_id=$2) OR (NOT $3 AND id=$4))").bind(board).bind(thread).bind(whole).bind(target).execute(&mut *tx).await?;
                sqlx::query("UPDATE content.threads SET deleted=deleted OR $3,modified_at=clock_timestamp() WHERE board=$1 AND id=$2").bind(board).bind(thread).bind(whole).execute(&mut *tx).await?;
            }
            _ => return Err(AppError::Invalid),
        }
    }
    if audit_changed {
        sqlx::query("INSERT INTO content.moderation_audit(account_id,board,target_id,action) VALUES ($1,$2,$3,$4)").bind(session.account_id).bind(board).bind(target).bind(action).execute(&mut *tx).await?;
    }
    Ok(tx)
}

#[cfg(feature = "database-tests")]
pub async fn moderate(
    pool: &PgPool,
    session: &Session,
    board: &str,
    target: i64,
    action: &str,
) -> Result<(), AppError> {
    prepare_moderation(pool, session, board, target, action)
        .await?
        .commit()
        .await?;
    Ok(())
}
