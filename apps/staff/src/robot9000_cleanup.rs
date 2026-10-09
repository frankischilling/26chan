//! Authenticated, board-scoped two-year Robot9000 text-history cleanup.
use crate::{AppError, AppState, auth};
use askama::Template;
use axum::{
    body::Bytes,
    extract::{RawQuery, State},
    http::HeaderMap,
    response::Html,
};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Template)]
#[template(path = "robot9000_cleanup.html")]
struct Review {
    board: String,
    csrf: String,
    recent: bool,
    result: Option<Outcome>,
}
#[derive(sqlx::FromRow)]
struct Outcome {
    removed: i64,
    has_more: bool,
    cutoff: chrono::DateTime<chrono::Utc>,
}

fn decode(raw: &[u8]) -> Result<String, AppError> {
    let mut bytes = Vec::with_capacity(raw.len());
    let mut pos = 0;
    while pos < raw.len() {
        match raw[pos] {
            b'%' => {
                let pair = raw.get(pos + 1..pos + 3).ok_or(AppError::Invalid)?;
                let high = (pair[0] as char).to_digit(16).ok_or(AppError::Invalid)?;
                let low = (pair[1] as char).to_digit(16).ok_or(AppError::Invalid)?;
                bytes.push((high * 16 + low) as u8);
                pos += 3;
            }
            b'+' => {
                bytes.push(b' ');
                pos += 1;
            }
            byte => {
                bytes.push(byte);
                pos += 1;
            }
        }
    }
    String::from_utf8(bytes).map_err(|_| AppError::Invalid)
}

fn fields(raw: &[u8], allowed: &[&str]) -> Result<BTreeMap<String, String>, AppError> {
    if raw.is_empty() || raw.len() > 4096 {
        return Err(AppError::Invalid);
    }
    let mut fields = BTreeMap::new();
    for pair in raw.split(|byte| *byte == b'&') {
        let separator = pair
            .iter()
            .position(|byte| *byte == b'=')
            .ok_or(AppError::Invalid)?;
        let name = decode(&pair[..separator])?;
        let value = decode(&pair[separator + 1..])?;
        if !allowed.contains(&name.as_str()) || fields.insert(name, value).is_some() {
            return Err(AppError::Invalid);
        }
    }
    Ok(fields)
}

fn board(fields: &BTreeMap<String, String>) -> Result<String, AppError> {
    let board = fields.get("board").ok_or(AppError::Invalid)?;
    board_domain::BoardSlug::parse(board).map_err(|_| AppError::Invalid)?;
    Ok(board.clone())
}
fn authorize(session: &auth::Session, board: &str) -> Result<(), AppError> {
    if !session
        .permissions
        .can_cleanup_robot9000(&session.role, board)
    {
        return Err(AppError::Forbidden);
    }
    Ok(())
}
pub(crate) async fn show(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Result<Html<String>, AppError> {
    let mut authority = auth::guard(&state, &headers).await?;
    let board = board(&fields(
        query.as_deref().unwrap_or("").as_bytes(),
        &["board"],
    )?)?;
    authorize(&authority.session, &board)?;
    let csrf = auth::cookie(&headers, &format!("{}-csrf", state.config.cookie_name()))?;
    auth::csrf(&authority.session, &csrf)?;
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM content.boards WHERE slug=$1)")
            .bind(&board)
            .fetch_one(&state.staff)
            .await?;
    if !exists {
        return Err(AppError::NotFound);
    }
    let html = Review {
        board,
        csrf,
        recent: authority.session.recent,
        result: None,
    }
    .render()
    .map_err(|_| AppError::Internal)?;
    authority.ensure_current(false).await?;
    authority.finish().await?;
    Ok(Html(html))
}
pub(crate) async fn submit(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Html<String>, AppError> {
    auth::origin(&headers, &state.config.origin)?;
    if headers.get_all("sec-fetch-site").iter().count() != 1 {
        return Err(AppError::Forbidden);
    }
    if headers.get_all("content-type").iter().count() != 1
        || headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(';').next())
            .map(str::trim)
            != Some("application/x-www-form-urlencoded")
    {
        return Err(AppError::Invalid);
    }
    let mut authority = auth::guard(&state, &headers).await?;
    let input = fields(&body, &["board", "csrf"])?;
    let board = board(&input)?;
    let csrf = input.get("csrf").ok_or(AppError::Invalid)?.clone();
    auth::csrf(&authority.session, &csrf)?;
    authorize(&authority.session, &board)?;
    if !authority.session.recent {
        return Err(AppError::Recent);
    }
    let mut tx = state.staff.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
        .execute(&mut *tx)
        .await?;
    let result: Outcome =
        sqlx::query_as("SELECT removed,has_more,cutoff FROM content.cleanup_robot9000($1,$2)")
            .bind(&board)
            .bind(authority.session.account_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|error| {
                if error.as_database_error().and_then(|e| e.code()).as_deref() == Some("P0117") {
                    AppError::NotFound
                } else {
                    AppError::Database(error)
                }
            })?;
    if !(0..=1000).contains(&result.removed) {
        return Err(AppError::Internal);
    }
    let html = Review {
        board,
        csrf,
        recent: true,
        result: Some(result),
    }
    .render()
    .map_err(|_| AppError::Internal)?;
    authority.ensure_current(true).await?;
    tx.commit().await?;
    authority.finish().await?;
    Ok(Html(html))
}

// Read catalog metadata only. Authentication credentials need no content access.
pub(crate) const READY_SQL: &str = r#"
SELECT EXISTS(SELECT 1 FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
 JOIN pg_roles owner ON owner.oid=p.proowner
 WHERE n.nspname='content' AND p.proname='cleanup_robot9000'
 AND p.proargtypes='25 20'::oidvector AND p.prorettype='record'::regtype
 AND p.prosecdef AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp']
 AND owner.rolname='board_robot9000_owner'
 AND NOT(owner.rolcanlogin OR owner.rolsuper OR owner.rolcreatedb OR owner.rolcreaterole OR owner.rolreplication OR owner.rolbypassrls)
 AND NOT EXISTS(SELECT 1 FROM pg_auth_members m WHERE m.member=owner.oid)
 AND has_function_privilege('board_staff',p.oid,'EXECUTE')
 AND NOT EXISTS(SELECT 1 FROM aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a
   WHERE a.privilege_type='EXECUTE' AND (a.is_grantable OR a.grantee NOT IN(p.proowner,(SELECT oid FROM pg_roles WHERE rolname='board_staff')))))
 AND EXISTS(SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname='content' AND c.relname='board_cleanup_audit'
 AND c.relowner=(SELECT oid FROM pg_roles WHERE rolname='board_migrator')
 AND NOT EXISTS(SELECT 1 FROM pg_roles r WHERE r.rolname IN
 ('board_public','board_staff','board_auth','board_media','board_media_read','board_media_intake','board_monitor')
 AND (has_any_column_privilege(r.oid,c.oid,'SELECT,INSERT,UPDATE') OR has_table_privilege(r.oid,c.oid,'DELETE,TRUNCATE,TRIGGER'))))
 AND (SELECT count(*)=2 FROM pg_policy p JOIN pg_class t ON t.oid=p.polrelid
 JOIN pg_namespace n ON n.oid=t.relnamespace
 WHERE n.nspname='content' AND t.relname='boards' AND t.relrowsecurity
 AND p.polroles=ARRAY[(SELECT oid FROM pg_roles WHERE rolname='board_robot9000_owner')]
 AND p.polpermissive AND pg_get_expr(p.polqual,p.polrelid)='true'
 AND ((p.polname='robot9000_cleanup_board_read' AND p.polcmd='r' AND p.polwithcheck IS NULL)
 OR (p.polname='robot9000_cleanup_board_lock' AND p.polcmd='w' AND pg_get_expr(p.polwithcheck,p.polrelid)='true')))
 AND EXISTS(SELECT 1 FROM pg_class t JOIN pg_namespace n ON n.oid=t.relnamespace
 WHERE n.nspname='post_secrets' AND t.relname='robot9000_texts'
 AND has_table_privilege('board_robot9000_owner',t.oid,'DELETE')
 AND NOT has_table_privilege('board_robot9000_owner',t.oid,'TRUNCATE,TRIGGER,REFERENCES'))
 AND EXISTS(SELECT 1 FROM pg_class t JOIN pg_namespace n ON n.oid=t.relnamespace
 WHERE n.nspname='post_secrets' AND t.relname='robot9000_mutes'
 AND NOT has_table_privilege('board_robot9000_owner',t.oid,'DELETE,TRUNCATE,TRIGGER,REFERENCES'))
 AND EXISTS(SELECT 1 FROM pg_class t JOIN pg_namespace n ON n.oid=t.relnamespace
 WHERE n.nspname='content' AND t.relname='board_cleanup_audit'
 AND NOT has_any_column_privilege('board_robot9000_owner',t.oid,'SELECT,UPDATE')
 AND NOT has_table_privilege('board_robot9000_owner',t.oid,'DELETE,TRUNCATE,TRIGGER')
 AND (SELECT count(*)=5 AND bool_and(has_column_privilege('board_robot9000_owner',t.oid,a.attnum,'INSERT'))
 FROM pg_attribute a WHERE a.attrelid=t.oid AND a.attname IN('account_id','board','action','cutoff','removed')))
 AND EXISTS(SELECT 1 FROM pg_class s JOIN pg_namespace n ON n.oid=s.relnamespace
 WHERE n.nspname='content' AND s.relname='board_cleanup_audit_id_seq' AND s.relkind='S'
 AND has_sequence_privilege('board_robot9000_owner',s.oid,'USAGE'))
"#;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cleanup_fields_are_bounded_and_unambiguous() {
        for invalid in [
            "",
            "board=r9k&board=g",
            "board=r9k&cutoff=now",
            "board=%xx",
            "board=r9k&csrf=a&csrf=b",
        ] {
            assert!(fields(invalid.as_bytes(), &["board", "csrf"]).is_err());
        }
        assert!(fields(&vec![b'a'; 4097], &["board"]).is_err());
        assert_eq!(
            board(&fields(b"board=r9k", &["board"]).unwrap()).unwrap(),
            "r9k"
        );
        assert!(board(&fields(b"board=../r9k", &["board"]).unwrap()).is_err());
    }
}
