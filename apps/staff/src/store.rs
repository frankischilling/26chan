use crate::{AppError, auth::Session};
use sqlx::PgPool;

#[derive(sqlx::FromRow)]
pub struct Report {
    pub id: i64,
    pub board: String,
    pub post_id: i64,
    pub thread_id: i64,
    pub reason: String,
    pub name: String,
    pub subject: String,
    pub comment: String,
    pub state: String,
    pub closed: bool,
    pub sticky: bool,
    pub deleted: bool,
    #[sqlx(skip)]
    pub attachment: Option<Attachment>,
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
pub async fn reports(pool: &PgPool) -> Result<Vec<Report>, AppError> {
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *tx)
        .await?;
    let mut reports: Vec<Report> = sqlx::query_as("SELECT r.id,r.board,r.post_id,p.thread_id,r.reason,p.name,p.subject,p.comment,r.state,(t.closed OR t.archived_at IS NOT NULL) AS closed,t.sticky,(p.deleted OR t.deleted) AS deleted FROM content.reports r JOIN content.posts p ON p.id=r.post_id AND p.board=r.board JOIN content.threads t ON t.id=p.thread_id AND t.board=p.board ORDER BY (r.state='open') DESC,r.id DESC LIMIT 100").fetch_all(&mut *tx).await?;
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
pub async fn moderate(
    pool: &PgPool,
    session: &Session,
    board: &str,
    target: i64,
    action: &str,
) -> Result<(), AppError> {
    if !matches!(session.role.as_str(), "moderator" | "admin") {
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
            | "remove-post"
            | "remove-file"
            | "remove-thread"
            | "resolve"
            | "dismiss"
    ) || target <= 0
    {
        return Err(AppError::Invalid);
    }
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
        .bind(board)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(AppError::NotFound)?;
    if action == "remove-file" {
        sqlx::query("SELECT content.delete_post_attachment($1,$2)")
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
        let archived: bool = sqlx::query_scalar(
            "SELECT archived_at IS NOT NULL FROM content.threads WHERE board=$1 AND id=$2 AND NOT deleted FOR UPDATE",
        )
        .bind(board)
        .bind(thread)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(AppError::NotFound)?;
        if archived && matches!(action, "reopen" | "sticky") {
            return Err(AppError::Invalid);
        }
        match action {
            "close" | "reopen" => {
                sqlx::query("UPDATE content.threads SET closed=$3,modified_at=clock_timestamp() WHERE board=$1 AND id=$2").bind(board).bind(thread).bind(action=="close").execute(&mut *tx).await?;
            }
            "sticky" | "unsticky" => {
                sqlx::query("UPDATE content.threads SET sticky=$3,modified_at=clock_timestamp() WHERE board=$1 AND id=$2").bind(board).bind(thread).bind(action=="sticky").execute(&mut *tx).await?;
            }
            _ => {
                let whole = action == "remove-thread" || target == thread;
                sqlx::query("UPDATE content.posts SET deleted=true WHERE board=$1 AND (($3 AND thread_id=$2) OR (NOT $3 AND id=$4))").bind(board).bind(thread).bind(whole).bind(target).execute(&mut *tx).await?;
                sqlx::query("UPDATE content.threads SET deleted=deleted OR $3,modified_at=clock_timestamp() WHERE board=$1 AND id=$2").bind(board).bind(thread).bind(whole).execute(&mut *tx).await?;
            }
        }
    }
    sqlx::query("INSERT INTO content.moderation_audit(account_id,board,target_id,action) VALUES ($1,$2,$3,$4)").bind(session.account_id).bind(board).bind(target).bind(action).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
