//! Request mutations revalidate target, board scope and real session authority
//! inside the atomic database writer. Form values never prove an unlock.
use crate::{AppError, AppState, access::Level, auth};
use askama::Template;
use axum::{
    Form,
    extract::{Query, State},
    http::HeaderMap,
    response::Html,
};
use serde::Deserialize;
use std::sync::Arc;

pub(crate) const READY_SQL: &str = r#"
SELECT NOT EXISTS (
 SELECT 1 FROM (VALUES
  ('staff_identity','janitor_request_form','bytea, bytea, integer, text, bigint','jsonb','board_auth'),
  ('content','create_janitor_request','bytea, bytea, integer, text, bigint, bigint, boolean, boolean','bigint','board_staff'),
  ('content','create_janitor_request','bytea, bytea, integer, text, bigint, bigint, boolean, boolean, bytea, bytea','bigint','board_staff'),
  ('staff_identity','janitor_request_media_plan','bytea, bytea, integer, text, bigint, bigint','jsonb','board_auth'),
  ('staff_identity','janitor_request_media','bytea, bytea, integer, bigint, boolean','record','board_auth'),
  ('content','review_janitor_request','bytea, bytea, integer, bigint, boolean, bigint, integer, boolean, text, text','jsonb','board_staff'),
  ('staff_identity','janitor_request_queue','bytea, bytea, integer, text, integer','jsonb','board_auth'),
  ('staff_identity','janitor_request_statistics','bytea, bytea, integer, integer','jsonb','board_auth')
 ) required(namespace,name,arguments,result_type,reader)
 WHERE NOT EXISTS(SELECT 1 FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
  WHERE n.nspname=required.namespace AND p.proname=required.name
   AND oidvectortypes(p.proargtypes)=required.arguments AND p.prosecdef
   AND p.proowner='board_restriction_owner'::regrole AND p.prorettype=to_regtype(required.result_type)
   AND has_function_privilege(current_user,p.oid,'EXECUTE')=(current_user=required.reader))
) AND NOT EXISTS (
 SELECT 1 FROM (VALUES ('restrictions','requests'),('restrictions','request_file_rules'),
  ('admission','file_rules'),('restrictions','request_media'),('restrictions','request_ban_thumbnails'),
  ('restrictions','request_media_capacity')) required(namespace,name)
 WHERE NOT EXISTS(SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
  WHERE n.nspname=required.namespace AND c.relname=required.name AND c.relkind='r'
   AND NOT has_table_privilege(current_user,c.oid,'SELECT,INSERT,UPDATE,DELETE')
   AND NOT has_any_column_privilege(current_user,c.oid,'SELECT,INSERT,UPDATE'))
) AND EXISTS(SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname='media' AND c.relname='request_post_assets' AND c.relkind='v'
 AND NOT has_table_privilege(current_user,c.oid,'SELECT,INSERT,UPDATE,DELETE'))
AND NOT EXISTS (
 SELECT 1 FROM (VALUES ('request_file_rule_decision'),('issue_request_file_rule')) required(name)
 WHERE NOT EXISTS(SELECT 1 FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
  WHERE n.nspname='restrictions' AND p.proname=required.name AND NOT p.prosecdef
   AND p.proowner='board_restriction_owner'::regrole
   AND NOT has_function_privilege(current_user,p.oid,'EXECUTE'))
) AND EXISTS (
 SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname='admission' AND c.relname='file_rules_id_seq' AND c.relkind='S'
  AND NOT has_sequence_privilege(current_user,c.oid,'USAGE,UPDATE')
)
"#;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Context {
    board: String,
    target: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Submission {
    board: String,
    target: i64,
    template: Option<i64>,
    csrf: String,
    warn_req: Option<String>,
    by_tpl: Option<i64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Choice {
    id: i64,
    name: String,
    warning: bool,
    can_warn: bool,
    requires_file: bool,
    available: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedForm {
    subject: String,
    templates: Vec<Choice>,
    related_count: u32,
}
#[derive(Template)]
#[template(path = "janitor_request.html")]
struct Page {
    board: String,
    target: i64,
    csrf: String,
    recent: bool,
    saved: SavedForm,
}
#[derive(Template)]
#[template(path = "janitor_request_submitted.html")]
struct Submitted {
    id: i64,
}

fn validate(board: &str, target: i64) -> Result<(), AppError> {
    if board.is_empty()
        || board.len() > 10
        || board == "j"
        || target <= 0
        || !board
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    {
        return Err(AppError::Invalid);
    }
    Ok(())
}
pub(crate) fn database(error: sqlx::Error) -> AppError {
    match error
        .as_database_error()
        .and_then(|error| error.code())
        .as_deref()
    {
        Some("P134A") => AppError::Forbidden,
        Some("P134Q") => AppError::Recent,
        Some("P134N") => AppError::NotFound,
        Some("23514" | "P0207") => AppError::Invalid,
        Some("P134C" | "54000" | "55P03") => AppError::Capacity,
        _ => AppError::Database(error),
    }
}
pub(crate) async fn form(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(input): Query<Context>,
) -> Result<Html<String>, AppError> {
    validate(&input.board, input.target)?;
    let mut authority = auth::guard(&state, &headers).await?;
    let csrf = auth::cookie(&headers, &crate::handlers::csrf_cookie(&state))?;
    auth::csrf(&authority.session, &csrf)?;
    let token = auth::hash(&auth::cookie(&headers, state.config.cookie_name())?);
    let sqlx::types::Json(saved): sqlx::types::Json<SavedForm> =
        sqlx::query_scalar("SELECT staff_identity.janitor_request_form($1,$2,$3,$4,$5)")
            .bind(token)
            .bind(auth::hash(&csrf))
            .bind(state.config.idle_timeout.as_secs() as i32)
            .bind(&input.board)
            .bind(input.target)
            .fetch_one(authority.connection())
            .await
            .map_err(database)?;
    let recent = authority.session.recent;
    authority.ensure_current(false).await?;
    authority.finish().await?;
    Ok(Html(
        Page {
            board: input.board,
            target: input.target,
            csrf,
            recent,
            saved,
        }
        .render()
        .map_err(|_| AppError::Internal)?,
    ))
}
pub(crate) async fn submit(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Form(input): Form<Submission>,
) -> Result<Html<String>, AppError> {
    auth::origin(&headers, &state.config.origin)?;
    validate(&input.board, input.target)?;
    let session = auth::session(&state, &headers).await?;
    if !session.at_least(Level::Janitor) {
        return Err(AppError::Forbidden);
    }
    auth::csrf(&session, &input.csrf)?;
    if !session.recent {
        return Err(AppError::Recent);
    }
    let asynchronous = input.by_tpl.is_some_and(|id| id != 0);
    let template = input
        .by_tpl
        .filter(|id| *id != 0)
        .or(input.template)
        .ok_or(AppError::Invalid)?;
    let warning = if asynchronous {
        false
    } else {
        match input.warn_req.as_deref() {
            None | Some("" | "0") => false,
            Some("1") => true,
            _ => return Err(AppError::Invalid),
        }
    };
    if !(1..=i64::from(i32::MAX)).contains(&template) {
        return Err(AppError::Invalid);
    }
    let token = auth::hash(&auth::cookie(&headers, state.config.cookie_name())?);
    let capture = crate::request_media::capture(
        &state,
        &headers,
        &input.board,
        input.target,
        template,
        &input.csrf,
    )
    .await?;
    let id: i64 =
        sqlx::query_scalar("SELECT content.create_janitor_request($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
            .bind(token)
            .bind(auth::hash(&input.csrf))
            .bind(state.config.idle_timeout.as_secs() as i32)
            .bind(&input.board)
            .bind(input.target)
            .bind(template)
            .bind(warning)
            .bind(asynchronous)
            .bind(capture.body.as_deref())
            .bind(capture.thumbnail.as_deref())
            .fetch_one(&state.staff)
            .await
            .map_err(database)?;
    Ok(Html(
        Submitted { id }.render().map_err(|_| AppError::Internal)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn request_targets_are_canonical_and_never_staff_discussion() {
        for board in ["", "j", "G", "../g", "a&b", "abcdefghijk"] {
            assert!(validate(board, 1).is_err());
        }
        assert!(validate("g", 0).is_err());
        assert!(validate("g", -1).is_err());
        assert!(validate("r9k", 1).is_ok());
    }
}
