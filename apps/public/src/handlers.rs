use crate::{AppState, api, catalog, views::*};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use askama::Template;
use axum::{
    Form,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, Uri},
    response::{Html, IntoResponse, Redirect, Response},
};
use board_store::{NewPost, StoreError};
use rand_core::OsRng;
use serde::Deserialize;
use sha2::{Digest, Sha256};

// The stored hash is not allowed to choose an unbounded verification workload.
// Public posting creates exactly this Argon2id profile; other encodings cannot
// authorize deletion or prove OP ownership. Keep this work inside the shared
// hash semaphore.
fn verify_deletion_password(password: &str, encoded: &str) -> bool {
    if encoded.len() > 256 {
        return false;
    }
    PasswordHash::new(encoded).is_ok_and(|hash| {
        hash.algorithm.as_str() == "argon2id"
            && hash.version == Some(19)
            && hash.params.iter().count() == 3
            && hash.params.get_decimal("m") == Some(19_456)
            && hash.params.get_decimal("t") == Some(2)
            && hash.params.get_decimal("p") == Some(1)
            && hash.hash.as_ref().is_some_and(|output| output.len() == 32)
            && Argon2::default()
                .verify_password(password.as_bytes(), &hash)
                .is_ok()
    })
}

#[derive(Debug)]
pub struct AppError(pub StatusCode, pub &'static str);
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        message_response(self.0, self.1)
    }
}
pub(crate) fn message_response(status: StatusCode, message: &str) -> Response {
    let body = Message {
        title: "Request could not be completed",
        message,
    }
    .render()
    .unwrap_or_else(|_| "Request failed.".into());
    (status, Html(body)).into_response()
}
impl From<StoreError> for AppError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::PageNotFound => Self(StatusCode::NOT_FOUND, "Page not found."),
            StoreError::NotFound => {
                Self(StatusCode::NOT_FOUND, "Board, thread, or post not found.")
            }
            StoreError::Invalid(message) => Self(StatusCode::UNPROCESSABLE_ENTITY, message),
            StoreError::Conflict(message) => Self(StatusCode::CONFLICT, message),
            StoreError::PublicDeletionRejected(message) => Self(StatusCode::FORBIDDEN, message),
            StoreError::AuthorizationChanged => {
                Self(StatusCode::FORBIDDEN, "Error: Password incorrect.")
            }
            error => {
                tracing::warn!(
                    event = "database_operation_failed",
                    error_class = storage_error_class(&error),
                    "database operation failed"
                );
                Self(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Storage is unavailable. Try again later.",
                )
            }
        }
    }
}

// Private diagnostics have a closed vocabulary. Never record Display/Debug,
// source errors, identifiers, or the raw SQLSTATE (which is driver-provided).
// Classification does not change public responses or imply that retry is safe.
fn storage_error_class(error: &StoreError) -> &'static str {
    match error {
        StoreError::Database(error) => match error {
            sqlx::Error::Database(error) => database_error_class(error.code().as_deref()),
            sqlx::Error::PoolTimedOut => "pool_timeout",
            sqlx::Error::PoolClosed => "pool_closed",
            sqlx::Error::Io(_) => "io",
            sqlx::Error::Tls(_) => "tls",
            sqlx::Error::Protocol(_) => "protocol",
            sqlx::Error::RowNotFound => "row_not_found",
            sqlx::Error::Configuration(_) | sqlx::Error::ConfigFile(_) => "configuration",
            sqlx::Error::InvalidArgument(_) => "invalid_argument",
            sqlx::Error::TypeNotFound { .. } => "type_not_found",
            sqlx::Error::ColumnIndexOutOfBounds { .. } => "column_index",
            sqlx::Error::ColumnNotFound(_) => "column_not_found",
            sqlx::Error::ColumnDecode { .. } => "column_decode",
            sqlx::Error::Encode(_) => "encode",
            sqlx::Error::Decode(_) => "decode",
            sqlx::Error::AnyDriverError(_) => "driver",
            sqlx::Error::WorkerCrashed => "worker_crashed",
            sqlx::Error::Migrate(_) => "migration",
            sqlx::Error::InvalidSavePointStatement | sqlx::Error::BeginFailed => {
                "transaction_state"
            }
            _ => "sqlx_other",
        },
        StoreError::UnsafeRole => "unsafe_role",
        StoreError::RandomnessUnavailable => "randomness_unavailable",
        StoreError::ReadLimit => "read_limit",
        StoreError::ContentRejected(_) => "content_rejected",
        StoreError::ContentQuiet { .. } => "content_quiet",
        StoreError::Robot9000Rejected(_) => "robot9000_rejected",
        StoreError::PostingCooldownRejected(_) => "posting_cooldown_rejected",
        _ => "store_other",
    }
}

fn database_error_class(code: Option<&str>) -> &'static str {
    match code {
        Some("40001") => "serialization",
        Some("40P01") => "deadlock",
        Some("55P03") => "lock_unavailable",
        Some("57014") => "statement_cancelled",
        Some("25000" | "25001" | "25006" | "25P01" | "25P02") => "transaction_state",
        Some("25P03" | "57P05") => "idle_timeout",
        Some("42501") => "permissions",
        Some("23502") => "not_null_constraint",
        Some("23503") => "foreign_key_constraint",
        Some("23505") => "unique_constraint",
        Some("23514") => "check_constraint",
        Some("23P01") => "exclusion_constraint",
        Some("08000" | "08001" | "08003" | "08004" | "08006" | "08007" | "08P01") => "connection",
        Some("53300") => "connection_limit",
        Some("57P01" | "57P02" | "57P03") => "server_unavailable",
        Some("42P01" | "42703" | "42883") => "schema_contract",
        _ => "database_other",
    }
}
impl From<askama::Error> for AppError {
    fn from(_: askama::Error) -> Self {
        Self(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Page could not be rendered.",
        )
    }
}
pub async fn not_found() -> AppError {
    AppError(StatusCode::NOT_FOUND, "Page not found.")
}
pub async fn css() -> impl IntoResponse {
    (
        [
            ("content-type", "text/css; charset=utf-8"),
            ("cache-control", "public, max-age=0, must-revalidate"),
        ],
        concat!(
            include_str!("../static/board.css"),
            "\n",
            include_str!("../../../assets/comment-markup.css")
        ),
    )
}
// Read only catalog metadata: readiness must never probe private posting evidence.
const OP_BUMP_CONTEXT_READY_SQL: &str = "SELECT EXISTS (
    SELECT 1 FROM pg_catalog.pg_proc p
    JOIN pg_catalog.pg_roles r ON r.oid=p.proowner
    WHERE p.oid=to_regprocedure('content.posting_op_bump_context(bytea,text,bigint)')
      AND has_function_privilege(current_user,p.oid,'EXECUTE')
      AND p.prosecdef AND p.provolatile='s'
      AND r.rolname='board_posting_cooldown_owner'
      AND NOT (r.rolcanlogin OR r.rolsuper OR r.rolcreatedb OR r.rolcreaterole OR r.rolreplication OR r.rolbypassrls)
      AND EXISTS (SELECT 1 FROM unnest(p.proconfig) AS config(value)
          WHERE replace(config.value,' ','')='search_path=pg_catalog,pg_temp')
      AND pg_catalog.pg_get_function_result(p.oid)='TABLE(own_reply boolean, latest_post_id bigint, latest_created_at timestamp with time zone)'
      AND NOT EXISTS (SELECT 1 FROM pg_catalog.aclexplode(coalesce(p.proacl,pg_catalog.acldefault('f',p.proowner))) a
          WHERE a.grantee=0 AND a.privilege_type='EXECUTE')
)";

// Catalog-only quota contract: never invoke the decision API or read actors.
const USER_THREAD_QUOTA_READY_SQL: &str = "SELECT EXISTS (
    SELECT 1 FROM pg_catalog.pg_proc p
    JOIN pg_catalog.pg_roles r ON r.oid=p.proowner
    WHERE p.oid=to_regprocedure('content.check_user_thread_quota(bytea,text,bigint)')
      AND has_function_privilege(current_user,p.oid,'EXECUTE')
      AND has_function_privilege('board_public',p.oid,'EXECUTE')
      AND has_function_privilege('board_staff',p.oid,'EXECUTE')
      AND p.prosecdef AND p.provolatile='v'
      AND r.rolname='board_posting_cooldown_owner'
      AND NOT (r.rolcanlogin OR r.rolsuper OR r.rolcreatedb OR r.rolcreaterole OR r.rolreplication OR r.rolbypassrls)
      AND EXISTS (SELECT 1 FROM unnest(p.proconfig) AS config(value)
          WHERE replace(config.value,' ','')='search_path=pg_catalog,pg_temp')
      AND pg_catalog.pg_get_function_result(p.oid)='TABLE(rejected boolean, user_thread_limit integer, user_thread_period_hours integer)'
      AND NOT EXISTS (SELECT 1 FROM pg_catalog.aclexplode(coalesce(p.proacl,pg_catalog.acldefault('f',p.proowner))) a
          WHERE a.privilege_type='EXECUTE' AND (a.grantee=0
              OR a.grantee NOT IN (p.proowner,
                  (SELECT oid FROM pg_catalog.pg_roles WHERE rolname='board_public'),
                  (SELECT oid FROM pg_catalog.pg_roles WHERE rolname='board_staff'))))
)
AND NOT EXISTS (
    SELECT 1 FROM (VALUES ('user_thread_limit'),('user_thread_period_hours')) AS required(column_name)
    WHERE NOT EXISTS (
        SELECT 1 FROM pg_catalog.pg_attribute a
        WHERE a.attrelid=to_regclass('content.boards') AND a.attname=required.column_name
          AND a.attnum>0 AND NOT a.attisdropped AND a.attnotnull
          AND a.atttypid='integer'::regtype
          AND has_column_privilege(current_user,a.attrelid,a.attnum,'SELECT')
          AND has_column_privilege('board_posting_cooldown_owner',a.attrelid,a.attnum,'SELECT')
    )
)";

// Catalog-only: never read deletion hashes, invoke a trigger, or mutate content.
const ARCHIVE_DELETION_SECRETS_READY_SQL: &str = r#"WITH relations AS (
    SELECT c.oid,n.nspname,c.relname
    FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace
    WHERE (n.nspname='post_secrets' AND c.relname='deletion')
       OR (n.nspname='content' AND c.relname IN ('boards','threads','posts'))
), owner_role AS (
    SELECT r.oid FROM pg_catalog.pg_roles r
    WHERE r.rolname='board_posting_cooldown_owner'
      AND NOT (r.rolcanlogin OR r.rolsuper OR r.rolcreatedb OR r.rolcreaterole OR r.rolreplication OR r.rolbypassrls)
)
SELECT EXISTS (SELECT 1 FROM owner_role)
AND NOT EXISTS (
    SELECT 1 FROM (VALUES
        ('post_secrets','deletion','deletion_archive_guard','guard_archived_deletion_secret',31,false),
        ('content','threads','retire_archived_deletion_secrets','retire_archived_deletion_secrets',17,true)
    ) AS required(schema_name,table_name,trigger_name,function_name,trigger_type,archive_transition)
    WHERE NOT EXISTS (
        SELECT 1 FROM pg_catalog.pg_trigger t
        JOIN relations c ON c.oid=t.tgrelid
        JOIN pg_catalog.pg_proc p ON p.oid=t.tgfoid
        JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace
        JOIN owner_role r ON r.oid=p.proowner
        WHERE c.nspname=required.schema_name AND c.relname=required.table_name
          AND t.tgname=required.trigger_name AND t.tgtype=required.trigger_type
          AND t.tgenabled='O' AND NOT t.tgisinternal
          AND t.tgnargs=0 AND t.tgconstraint=0 AND NOT t.tgdeferrable AND NOT t.tginitdeferred
          AND t.tgoldtable IS NULL AND t.tgnewtable IS NULL
          AND n.nspname='post_secrets' AND p.proname=required.function_name
          AND p.pronargs=0 AND p.prokind='f' AND p.prorettype='pg_catalog.trigger'::regtype
          AND p.prosecdef AND p.provolatile='v'
          AND EXISTS (SELECT 1 FROM unnest(p.proconfig) AS config(value)
              WHERE replace(config.value,' ','')='search_path=pg_catalog,pg_temp')
          AND NOT has_function_privilege(current_user,p.oid,'EXECUTE')
          AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_roles runtime
              WHERE runtime.rolname IN ('board_public','board_staff','board_auth')
                AND has_function_privilege(runtime.oid,p.oid,'EXECUTE'))
          AND NOT EXISTS (SELECT 1 FROM pg_catalog.aclexplode(coalesce(p.proacl,pg_catalog.acldefault('f',p.proowner))) a
              WHERE a.grantee=0 AND a.privilege_type='EXECUTE')
          AND CASE WHEN required.archive_transition THEN
              t.tgqual IS NOT NULL
              AND t.tgattr::text=(SELECT a.attnum::text FROM pg_catalog.pg_attribute a
                  WHERE a.attrelid=c.oid AND a.attname='archived_at' AND NOT a.attisdropped)
              AND translate(split_part(split_part(pg_catalog.pg_get_triggerdef(t.oid,false),' WHEN ',2),' EXECUTE ',1),' ()','')
                  ='old.archived_atISNULLANDnew.archived_atISNOTNULL'
          ELSE t.tgqual IS NULL AND t.tgattr::text='' END
    )
)
AND NOT EXISTS (
    SELECT 1 FROM (VALUES
        ('post_secrets','deletion','post_id','SELECT'),
        ('content','threads','id','UPDATE'),
        ('content','threads','id','SELECT'),
        ('content','threads','board','SELECT'),
        ('content','threads','archived_at','SELECT'),
        ('content','posts','id','SELECT'),
        ('content','posts','board','SELECT'),
        ('content','posts','thread_id','SELECT'),
        ('content','boards','slug','SELECT'),
        ('content','boards','staff_only','SELECT'),
        ('content','boards','slug','UPDATE')
    ) AS required(schema_name,table_name,column_name,privilege_name)
    WHERE NOT EXISTS (
        SELECT 1 FROM relations c CROSS JOIN owner_role r
        JOIN pg_catalog.pg_attribute a ON a.attname=required.column_name AND NOT a.attisdropped
        WHERE c.nspname=required.schema_name AND c.relname=required.table_name AND a.attrelid=c.oid
          AND has_column_privilege(r.oid,c.oid,a.attnum,required.privilege_name)
    )
)
AND EXISTS (
    SELECT 1 FROM relations c CROSS JOIN owner_role r
    WHERE c.nspname='post_secrets' AND c.relname='deletion'
      AND has_table_privilege(r.oid,c.oid,'DELETE')
      AND NOT has_table_privilege(r.oid,c.oid,'SELECT,INSERT,UPDATE,TRUNCATE,REFERENCES,TRIGGER')
      AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_attribute a
          WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped
            AND ((a.attname<>'post_id' AND has_column_privilege(r.oid,c.oid,a.attnum,'SELECT'))
                OR has_column_privilege(r.oid,c.oid,a.attnum,'INSERT,UPDATE,REFERENCES')))
)
AND EXISTS (
    SELECT 1 FROM relations c CROSS JOIN owner_role r
    WHERE c.nspname='content' AND c.relname='threads'
      AND NOT has_table_privilege(r.oid,c.oid,'UPDATE')
      AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_attribute a
          WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped AND a.attname<>'id'
            AND has_column_privilege(r.oid,c.oid,a.attnum,'UPDATE'))
)
AND NOT EXISTS (
    SELECT 1 FROM (VALUES ('content'),('post_secrets')) AS required(schema_name)
    WHERE NOT EXISTS (
        SELECT 1 FROM pg_catalog.pg_namespace n CROSS JOIN owner_role r
        WHERE n.nspname=required.schema_name AND has_schema_privilege(r.oid,n.oid,'USAGE')
          AND NOT has_schema_privilege(r.oid,n.oid,'CREATE')
    )
)"#;

// Catalog-only contract: never invoke admission or inspect private actor rows.
const REPORT_ADMISSION_READY_SQL: &str = board_store::report_admission::READINESS_SQL;

pub async fn ready(State(state): State<AppState>) -> Result<&'static str, AppError> {
    if state.poster_id_key.is_none() {
        return Err(AppError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Public posting, deletion and reporting are unavailable: POSTER_ID_KEY is required.",
        ));
    }
    sqlx::query("SELECT slug,oekaki,oekaki_replays,oekaki_width,oekaki_height,can_report_posts,expire_neglected,meta_board,poster_id_no_heaven,custom_spoiler_count,spoiler_thumbnail_assets,board_flag_type,deletion_no_op,deletion_no_reply,deletion_known_min_seconds,deletion_unknown_min_seconds,deletion_max_seconds,posting_reply_seconds,posting_image_seconds,posting_thread_seconds FROM content.boards LIMIT 1")
        .execute(&state.pool)
        .await
        .map_err(StoreError::from)?;
    sqlx::query("SELECT json_op_poster_id FROM content.posts LIMIT 1")
        .execute(&state.pool)
        .await
        .map_err(StoreError::from)?;
    // Rank-aware readers require the additive ordering projection before ready.
    sqlx::query("SELECT sticky_rank FROM content.visible_threads LIMIT 0")
        .execute(&state.pool)
        .await
        .map_err(StoreError::from)?;
    let attachment_policy: bool = sqlx::query_scalar("SELECT has_column_privilege('board_attachment_owner','content.boards','meta_board','SELECT') AND has_column_privilege('board_attachment_owner','content.boards','poster_id_no_heaven','SELECT')")
        .fetch_one(&state.pool)
        .await
        .map_err(StoreError::from)?;
    if !attachment_policy {
        return Err(AppError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Service unavailable.",
        ));
    }
    let deletion_quota: bool = sqlx::query_scalar(
        "SELECT coalesce(has_function_privilege(current_user, to_regprocedure('content.reserve_public_deletion(bytea)'), 'EXECUTE'), false) AND coalesce(has_function_privilege(current_user, to_regprocedure('content.check_public_deletion_quota(bytea)'), 'EXECUTE'), false)",
    )
    .fetch_one(&state.pool)
    .await
    .map_err(StoreError::from)?;
    if !deletion_quota {
        return Err(AppError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Public deletion is unavailable.",
        ));
    }
    let automatic_admission: bool =
        sqlx::query_scalar(board_store::automatic_admission::READINESS_SQL)
            .fetch_one(&state.pool)
            .await
            .map_err(StoreError::from)?;
    if !automatic_admission {
        return Err(AppError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Public automatic admission is unavailable.",
        ));
    }
    let sticky_retirement: bool = sqlx::query_scalar(board_store::sticky_retention::READINESS_SQL)
        .fetch_one(&state.pool)
        .await
        .map_err(StoreError::from)?;
    if !sticky_retirement {
        return Err(AppError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Public posting is unavailable.",
        ));
    }
    let report_admission: bool = sqlx::query_scalar(REPORT_ADMISSION_READY_SQL)
        .fetch_one(&state.pool)
        .await
        .map_err(StoreError::from)?;
    if !report_admission {
        return Err(AppError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Public reporting is unavailable.",
        ));
    }
    let blotter_projection: bool = sqlx::query_scalar(board_store::blotter::BLOTTER_READINESS_SQL)
        .fetch_one(&state.pool)
        .await
        .map_err(StoreError::from)?;
    if !blotter_projection {
        return Err(AppError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Public blotter is unavailable.",
        ));
    }
    let poll_projection: bool = sqlx::query_scalar(board_store::POLL_READINESS_SQL)
        .fetch_one(&state.pool)
        .await
        .map_err(StoreError::from)?;
    if !poll_projection {
        return Err(AppError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Public polls are unavailable.",
        ));
    }
    let posting_cooldowns: bool = sqlx::query_scalar(
        "SELECT coalesce(has_function_privilege(current_user, to_regprocedure('content.lock_posting_actor(bytea,boolean)'), 'EXECUTE'), false)
         AND coalesce(has_function_privilege(current_user, to_regprocedure('content.check_posting_cooldown(bytea,text,bigint,boolean,bigint)'), 'EXECUTE'), false)
         AND to_regprocedure('content.record_posting_history(bytea,bigint)') IS NULL
         AND EXISTS (
             SELECT 1 FROM pg_catalog.pg_trigger t
             JOIN pg_catalog.pg_proc p ON p.oid=t.tgfoid
             JOIN pg_catalog.pg_roles r ON r.oid=p.proowner
             WHERE t.tgrelid='content.posts'::regclass
               AND t.tgname='record_inserted_posting_history'
               AND t.tgtype=5 AND t.tgenabled='O' AND NOT t.tgisinternal
               AND t.tgqual IS NULL AND t.tgnargs=0
               AND p.oid=to_regprocedure('content.record_inserted_posting_history()')
               AND p.prorettype='trigger'::regtype AND p.prosecdef
               AND r.rolname='board_posting_cooldown_owner'
               AND EXISTS (SELECT 1 FROM unnest(p.proconfig) AS config(value)
                   WHERE replace(config.value,' ','')='search_path=pg_catalog,pg_temp')
               AND NOT has_function_privilege(current_user,p.oid,'EXECUTE')
         )",
    )
    .fetch_one(&state.pool)
    .await
    .map_err(StoreError::from)?;
    if !posting_cooldowns {
        return Err(AppError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Public posting is unavailable.",
        ));
    }
    let user_thread_quota: bool = sqlx::query_scalar(USER_THREAD_QUOTA_READY_SQL)
        .fetch_one(&state.pool)
        .await
        .map_err(StoreError::from)?;
    if !user_thread_quota {
        return Err(AppError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Public posting is unavailable.",
        ));
    }
    let op_bump_context: bool = sqlx::query_scalar(OP_BUMP_CONTEXT_READY_SQL)
        .fetch_one(&state.pool)
        .await
        .map_err(StoreError::from)?;
    if !op_bump_context {
        return Err(AppError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Public posting is unavailable.",
        ));
    }
    let archive_deletion_secrets: bool = sqlx::query_scalar(ARCHIVE_DELETION_SECRETS_READY_SQL)
        .fetch_one(&state.pool)
        .await
        .map_err(StoreError::from)?;
    if !archive_deletion_secrets {
        return Err(AppError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Public posting and deletion are unavailable.",
        ));
    }
    if let Some(media) = &state.media {
        media.ready().await?;
    }
    Ok("ready")
}
pub async fn home(State(state): State<AppState>) -> Result<Response, AppError> {
    crate::output::html(
        &state,
        &Home {
            boards: board_store::boards(&state.pool).await?,
        },
    )
}
pub async fn board_redirect(
    State(state): State<AppState>,
    Path(board): Path<String>,
) -> Result<Redirect, AppError> {
    board_store::board(&state.pool, &board).await?;
    Ok(Redirect::permanent(&format!("/{board}/")))
}
pub async fn board_index(
    State(state): State<AppState>,
    Path(board): Path<String>,
) -> Result<Response, AppError> {
    board_page(&state, &board, 1, false, catalog::Options::default()).await
}
pub async fn page(
    State(state): State<AppState>,
    Path((board, page)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
) -> Result<Response, AppError> {
    match page.as_str() {
        "catalog" => {
            let options = catalog::Options::parse(&uri).ok_or(AppError(
                StatusCode::BAD_REQUEST,
                "Invalid catalog options.",
            ))?;
            board_page(&state, &board, 1, true, options).await
        }
        "threads.json" => api::thread_list(&state, &board, &headers).await,
        "catalog.json" => api::catalog(&state, &board, &headers).await,
        "archive.json" => api::archive(&state, &board, &headers).await,
        "index.rss" => crate::rss::feed(&state, &board, &headers).await,
        "archive" => {
            let board_store::PageSnapshot {
                snapshot,
                navigation_boards,
                ..
            } = board_store::archive_page_snapshot(&state.pool, &board).await?;
            let entries = snapshot
                .entries
                .into_iter()
                .map(|entry| crate::archive::row(entry, &snapshot.board))
                .collect::<Result<Vec<_>, _>>()?;
            crate::output::html(
                &state,
                &ArchivePage {
                    navigation_boards,
                    board: snapshot.board,
                    entries,
                },
            )
        }
        _ => {
            if let Some(index) = page.strip_suffix(".json") {
                let index = index
                    .parse()
                    .map_err(|_| AppError(StatusCode::NOT_FOUND, "Page not found."))?;
                api::index(&state, &board, index, &headers).await
            } else {
                let index = page
                    .parse::<i64>()
                    .map_err(|_| AppError(StatusCode::NOT_FOUND, "Page not found."))?;
                if !(0..1000).contains(&index) {
                    return Err(AppError(StatusCode::NOT_FOUND, "Page not found."));
                }
                board_page(
                    &state,
                    &board,
                    index + 1,
                    false,
                    catalog::Options::default(),
                )
                .await
            }
        }
    }
}
async fn board_page(
    state: &AppState,
    slug: &str,
    page: i64,
    catalog: bool,
    options: catalog::Options,
) -> Result<Response, AppError> {
    let selection = if catalog {
        board_store::BoardSelection::All
    } else {
        board_store::BoardSelection::Page(page)
    };
    let board_store::PageSnapshot {
        mut snapshot,
        navigation_boards,
        blotter,
    } = if catalog {
        board_store::board_page_snapshot(&state.pool, slug, selection, Some(0)).await?
    } else {
        board_store::source_board_page_snapshot(&state.pool, slug, selection).await?
    };
    if catalog && !snapshot.board.catalog_enabled {
        return Err(AppError(StatusCode::NOT_FOUND, "Catalog not found."));
    }
    let catalog_positions: std::collections::BTreeMap<_, _> = if catalog {
        snapshot
            .threads
            .iter()
            .enumerate()
            .map(|(position, preview)| (preview.thread.id, position))
            .collect()
    } else {
        std::collections::BTreeMap::new()
    };
    let hidden = if catalog {
        options.apply(&mut snapshot)
    } else {
        Vec::new()
    };
    let board = snapshot.board;
    let has_next = snapshot.has_next;
    let mut views = Vec::new();
    let mut hidden_views = Vec::new();
    for (preview, visible) in snapshot
        .threads
        .into_iter()
        .map(|preview| (preview, true))
        .chain(hidden.into_iter().map(|preview| (preview, false)))
    {
        let posts = preview.posts;
        if posts.is_empty() {
            continue;
        }
        let total = preview.visible_posts as usize;
        let omitted = total.saturating_sub(posts.len());
        let view = ThreadView {
            catalog_position: catalog_positions.get(&preview.thread.id).copied(),
            catalog_last_reply: preview.catalog_last_reply,
            tail_size: 0,
            latest_reply_id: preview.latest_reply_id,
            thread: preview.thread,
            posts: posts
                .into_iter()
                .map(|post| {
                    if catalog {
                        PostView::new(post)
                    } else {
                        PostView::resolved(post, &snapshot.quote_targets, None)
                    }
                })
                .collect(),
            omitted,
            image_replies: preview.visible_images,
        };
        if visible {
            views.push(view);
        } else {
            hidden_views.push(view);
        }
    }
    let math_tags = board.math_tags && !catalog;
    let drawing = !catalog && board.ordinary_drawing_enabled(state.media.is_some());
    let response = crate::output::html(
        state,
        &BoardPage {
            public_origin: state.origin.clone(),
            spoiler_thumbnail: crate::views::spoilers::choose_thumbnail(&board),
            navigation_boards,
            blotter,
            quote: String::new(),
            catalog_hidden: hidden_views,
            board,
            threads: views,
            page_number: page,
            parent: 0,
            previous: if page > 1 {
                format!("/{slug}/{}", page - 2)
            } else {
                String::new()
            },
            next: if has_next {
                format!("/{slug}/{page}")
            } else {
                String::new()
            },
            catalog,
            catalog_options: options,
            media_origin: state
                .media
                .as_ref()
                .map(|m| m.settings.origin.as_string())
                .unwrap_or_default(),
        },
    )?;
    Ok(crate::security::drawing_page(
        crate::security::math_page(response, math_tags),
        drawing,
    ))
}
pub async fn thread(
    State(state): State<AppState>,
    Path((board, key)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
) -> Result<Response, AppError> {
    if let Some(id) = key.strip_suffix(".json") {
        let tail = id.ends_with("-tail");
        let id = id.strip_suffix("-tail").unwrap_or(id);
        let raw = id;
        let id = id
            .parse::<i64>()
            .map_err(|_| AppError(StatusCode::NOT_FOUND, "Thread not found."))?;
        if tail && (id <= 0 || id.to_string() != raw) {
            return Err(AppError(StatusCode::NOT_FOUND, "Thread not found."));
        }
        return api::thread_selection(&state, &board, id, &headers, tail).await;
    }
    let id = key
        .parse()
        .map_err(|_| AppError(StatusCode::NOT_FOUND, "Thread not found."))?;
    let board_store::PageSnapshot {
        snapshot,
        navigation_boards,
        blotter,
    } = board_store::thread_page_snapshot(&state.pool, &board, id).await?;
    let board_store::ThreadSnapshot {
        board,
        thread,
        posts,
        tail_size,
        images,
        quote_targets,
        ..
    } = snapshot;
    let latest_reply_id = posts
        .iter()
        .filter(|post| post.id != thread.id)
        .map(|post| post.id)
        .max();
    #[derive(Deserialize)]
    struct ReplyQuery {
        quote: Option<String>,
    }
    let query = Query::<ReplyQuery>::try_from_uri(&uri)
        .map_err(|_| AppError(StatusCode::BAD_REQUEST, "Invalid reply target."))?;
    let quote = match query.quote.as_deref() {
        None => String::new(),
        Some(raw) => {
            let no = raw
                .parse::<i64>()
                .ok()
                .filter(|no| *no > 0 && no.to_string() == raw)
                .ok_or(AppError(StatusCode::BAD_REQUEST, "Invalid reply target."))?;
            if !posts.iter().any(|post| post.id == no) {
                return Err(AppError(StatusCode::NOT_FOUND, "Post not found."));
            }
            if thread.closed || thread.archived_at.is_some() {
                return Err(AppError(StatusCode::BAD_REQUEST, "This thread is closed."));
            }
            format!(">>{no}\n")
        }
    };
    let posts = posts
        .into_iter()
        .map(|post| PostView::resolved(post, &quote_targets, Some(thread.id)))
        .collect();
    let math_tags = board.math_tags;
    let drawing = !thread.closed
        && thread.archived_at.is_none()
        && (board.ordinary_drawing_enabled(state.media.is_some())
            || board.drawing_edit_enabled(state.media.is_some()));
    let response = crate::output::html(
        &state,
        &BoardPage {
            public_origin: state.origin.clone(),
            spoiler_thumbnail: crate::views::spoilers::choose_thumbnail(&board),
            navigation_boards,
            blotter,
            quote,
            catalog_hidden: Vec::new(),
            board,
            threads: vec![ThreadView {
                catalog_position: None,
                catalog_last_reply: None,
                tail_size,
                latest_reply_id,
                thread,
                posts,
                omitted: 0,
                image_replies: images as i64,
            }],
            page_number: 1,
            parent: id,
            previous: String::new(),
            next: String::new(),
            catalog: false,
            catalog_options: catalog::Options::default(),
            media_origin: state
                .media
                .as_ref()
                .map(|m| m.settings.origin.as_string())
                .unwrap_or_default(),
        },
    )?;
    Ok(crate::security::drawing_page(
        crate::security::math_page(response, math_tags),
        drawing,
    ))
}
pub async fn quote(
    State(state): State<AppState>,
    Path((board, id)): Path<(String, i64)>,
) -> Result<Redirect, AppError> {
    let post = board_store::find_post(&state.pool, &board, id).await?;
    Ok(Redirect::to(&format!(
        "/{board}/thread/{}#p{id}",
        post.thread_id
    )))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostForm {
    #[serde(default)]
    flag: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    sub: String,
    #[serde(default)]
    com: String,
    #[serde(default, alias = "pwd")]
    password: String,
    #[serde(default)]
    resto: i64,
    #[serde(default)]
    email: String,
    #[serde(default)]
    upload_id: String,
    #[serde(default)]
    upload_capability: String,
    #[serde(default, deserialize_with = "crate::posting_form::spoiler_checkbox")]
    spoiler: bool,
    #[serde(default)]
    awt: Option<u8>,
    #[serde(default)]
    track: Option<u8>,
    #[serde(default, rename = "mode")]
    _mode: Option<crate::posting_form::Mode>,
    // Source form hints carry no server authority or capacity override.
    #[serde(default, rename = "MAX_FILE_SIZE")]
    _max_file_size: String,
    #[serde(default, rename = "hasjs")]
    _hasjs: String,
    #[serde(default, deserialize_with = "crate::posting_form::checkbox")]
    textonly: bool,
    #[serde(default, deserialize_with = "crate::posting_form::drawing_field")]
    oe_time: Option<String>,
    #[serde(default, deserialize_with = "crate::posting_form::drawing_field")]
    oe_src: Option<String>,
}
pub async fn post(
    State(state): State<AppState>,
    Path(board): Path<String>,
    axum::Extension(start): axum::Extension<crate::security::RequestStart>,
    axum::Extension(peer): axum::Extension<crate::security::RequestPeer>,
    headers: HeaderMap,
    form: Result<crate::posting_form::PostingForm, crate::posting_form::Rejection>,
) -> Response {
    let format = crate::posting_response::Format::from_headers(&headers);
    let response = match form {
        Ok(crate::posting_form::PostingForm(form)) => {
            match submit_post(
                state,
                board,
                headers,
                form,
                format,
                board_store::PostingContext {
                    request_start: start.0,
                    peer: peer.0,
                    op_password_proof: None,
                },
            )
            .await
            {
                Ok(response) => response,
                Err(error) => format.error(error),
            }
        }
        Err(error) => format.invalid_form(error),
    };
    format.finish(response)
}

async fn submit_post(
    state: AppState,
    board: String,
    headers: HeaderMap,
    form: PostForm,
    format: crate::posting_response::Format,
    mut context: board_store::PostingContext,
) -> Result<Response, AppError> {
    // Ordinary cooldown identity must come from verified transport on every
    // real posting route, including development and legacy/multipart requests.
    if context.peer.is_none() || state.poster_id_key.is_none() {
        return Err(AppError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Posting identity is unavailable.",
        ));
    }
    if [form.awt, form.track]
        .into_iter()
        .flatten()
        .any(|value| value > 1)
    {
        return Err(AppError(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Invalid posting preference.",
        ));
    }
    let auto_watch = form.awt == Some(1);
    let track = form.track == Some(1);
    let settings = board_store::board(&state.pool, &board).await?;
    let attachment = match (form.upload_id.is_empty(), form.upload_capability.is_empty()) {
        (true, true) => None,
        (false, false) if state.media.is_some() && !form.textonly => {
            Some(board_store::post_media::NewAttachment {
                upload: board_store::media_intake::IntakeReservation {
                    id: form.upload_id,
                    capability: form.upload_capability,
                },
                spoiler: form.spoiler,
            })
        }
        _ => {
            return Err(AppError(
                StatusCode::UNPROCESSABLE_ENTITY,
                "Invalid or unavailable attachment.",
            ));
        }
    };
    settings.check_attachment_allowed(form.resto, attachment.is_some())?;
    board_domain::prepare_post_content_input(
        &form.name,
        &form.sub,
        &form.com,
        settings.max_comment_chars as usize,
        attachment.is_some(),
        settings.comment_spacing(),
        if form.resto == 0 {
            board_domain::PostKind::Thread {
                subject_required: settings.require_subject,
                text_only: settings.text_only,
            }
        } else {
            board_domain::PostKind::Reply
        },
    )
    .map_err(|e| AppError(StatusCode::UNPROCESSABLE_ENTITY, e.0))?;
    let options = board_domain::posting_options::parse(&form.email)
        .map_err(|error| AppError(StatusCode::UNPROCESSABLE_ENTITY, error.0))?;
    if !form.password.is_empty() && !(8..=128).contains(&form.password.len()) {
        return Err(AppError(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Deletion password must contain 8 to 128 bytes.",
        ));
    }
    let session =
        crate::anonymous_session::Session::resolve(&state, &headers, context.peer).await?;
    let password = session.password(&form.password);
    // The operator may enable OP markup while this request waits on the board
    // lock. Capture proof independently of the earlier policy snapshot.
    let op_hash = if form.resto > 0 {
        board_store::op_deletion_hash(&state.pool, &board, form.resto).await?
    } else {
        None
    };
    let permit = state
        .limits
        .hashes
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            AppError(
                StatusCode::SERVICE_UNAVAILABLE,
                "Password processing is busy. Try again.",
            )
        })?;
    let (hash, op_password_proof) = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let proof = op_hash
            .as_deref()
            .filter(|hash| {
                !form.password.is_empty() && verify_deletion_password(&form.password, hash)
            })
            .map(|hash| <[u8; 32]>::from(Sha256::digest(hash.as_bytes())));
        Argon2::default()
            .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
            .map(|hash| (hash.to_string(), proof))
    })
    .await
    .map_err(|_| {
        AppError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Password processing failed.",
        )
    })?
    .map_err(|_| {
        AppError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Password processing failed.",
        )
    })?;
    context.op_password_proof = op_password_proof;
    let post = NewPost {
        name: if options.anonymous {
            String::new()
        } else {
            form.name
        },
        subject: form.sub,
        comment: form.com,
        deletion_hash: hash,
        sage: options.sage,
    };
    let id = board_store::create_post_with_anonymous_session(
        &state.pool,
        &board,
        form.resto,
        &post,
        attachment.as_ref(),
        board_store::AnonymousPostingContext {
            posting: context,
            session: session.posting,
        },
        board_store::PostMetadata {
            spoiler: form.spoiler,
            country_database: state.country_database.as_deref(),
            flag: &form.flag,
            options: &form.email,
            drawing: form
                .oe_time
                .as_deref()
                .map(|time| board_store::DrawingSubmission {
                    time,
                    source_post: form.oe_src.as_deref(),
                }),
            keys: board_store::PostIdentityKeys {
                tripcode: state.tripcode_key.as_deref(),
                poster_id: state.poster_id_key.as_deref(),
            },
        },
    )
    .await;
    let id = match id {
        Ok(id) => id,
        Err(StoreError::PostingCooldownRejected(rejection)) => {
            return Ok(format.rule_error(&rejection.source_message()));
        }
        Err(StoreError::Robot9000Rejected(message) | StoreError::ContentRejected(message)) => {
            return Ok(format.rule_error(&message));
        }
        Err(StoreError::ContentQuiet { post: quiet }) => {
            let thread = if form.resto == 0 { quiet } else { form.resto };
            let location = if options.return_to_board {
                format!("/{board}/")
            } else {
                format!("/{board}/thread/{thread}#p{quiet}")
            };
            let mut response = format.success(form.resto, quiet, &location);
            session.append(response.headers_mut(), state.production);
            crate::post_preferences::append(
                response.headers_mut(),
                &headers,
                if settings.forced_anon {
                    None
                } else {
                    Some(&post.name)
                },
                if options.anonymous { "" } else { &form.email },
                state.production,
            );
            return Ok(response);
        }
        Err(error) => return Err(error.into()),
    };
    let thread = if form.resto == 0 { id } else { form.resto };
    let location = if options.return_to_board {
        format!("/{board}/")
    } else {
        format!("/{board}/thread/{thread}#p{id}")
    };
    let mut response = format.success(form.resto, id, &location);
    session.append(response.headers_mut(), state.production);
    crate::post_preferences::append(
        response.headers_mut(),
        &headers,
        if settings.forced_anon {
            None
        } else {
            Some(&post.name)
        },
        if options.anonymous { "" } else { &form.email },
        state.production,
    );
    crate::post_receipts::Receipt {
        board: &board,
        thread,
        post: id,
        track,
        watch: auto_watch,
        production: state.production,
    }
    .append(response.headers_mut(), &headers);
    Ok(response)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteForm {
    pub(crate) no: i64,
    #[serde(default)]
    pub(crate) password: String,
    #[serde(default)]
    pub(crate) file_only: bool,
}
pub async fn delete(
    State(state): State<AppState>,
    Path(board): Path<String>,
    axum::Extension(peer): axum::Extension<crate::security::RequestPeer>,
    axum::Extension(start): axum::Extension<crate::security::RequestStart>,
    headers: HeaderMap,
    Form(form): Form<DeleteForm>,
) -> Result<Redirect, AppError> {
    let identity = deletion_rate_identity(&state, peer)?;
    board_store::public_deletion_quota_precheck(&state.pool, &identity).await?;
    board_store::public_deletion_precheck(&state.pool, &board, form.no, start.0).await?;
    let session = crate::anonymous_session::Session::resolve(&state, &headers, peer.0).await?;
    let context = board_store::PublicDeletionContext {
        request_start: start.0,
        session: (!session.posting.minted).then_some(session.posting),
    };
    let mut batch = board_store::PublicDeletionBatch::new(board, context, identity);
    delete_authorized(&state, &mut batch, form).await
}

/// Only the current verified transport peer supplies throttle identity. Session
/// cookies and request fields never choose it, including in development.
pub(crate) fn deletion_rate_identity(
    state: &AppState,
    peer: crate::security::RequestPeer,
) -> Result<board_domain::poster_id::PublicDeletionRateIdentity, AppError> {
    let unavailable = || {
        AppError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Public deletion is unavailable.",
        )
    };
    let key = state.poster_id_key.as_deref().ok_or_else(unavailable)?;
    let peer = peer.0.ok_or_else(unavailable)?;
    Ok(key.public_deletion_rate_identity(peer))
}

/// Reports use only the full, canonical trusted transport IP. Anonymous
/// capabilities are activity identities, never passwords or Pass authority.
pub(crate) fn report_rate_identity(
    state: &AppState,
    peer: crate::security::RequestPeer,
) -> Result<board_domain::poster_id::PublicReportRateIdentity, AppError> {
    let unavailable = || {
        AppError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Public reporting is unavailable.",
        )
    };
    let key = state.poster_id_key.as_deref().ok_or_else(unavailable)?;
    let peer = peer.0.ok_or_else(unavailable)?;
    Ok(key.public_report_rate_identity(peer))
}

/// A legacy batch captures the trusted request/session clocks once. Each item
/// still obtains fresh authority and rechecks policy under its mutation lock.
pub(crate) async fn delete_with_context(
    state: &AppState,
    batch: &mut board_store::PublicDeletionBatch,
    form: DeleteForm,
) -> Result<Redirect, AppError> {
    board_store::public_deletion_precheck(
        &state.pool,
        batch.slug(),
        form.no,
        batch.context().request_start,
    )
    .await?;
    delete_authorized(state, batch, form).await
}

async fn delete_authorized(
    state: &AppState,
    batch: &mut board_store::PublicDeletionBatch,
    form: DeleteForm,
) -> Result<Redirect, AppError> {
    let board = batch.slug().to_owned();
    if let Some(session) = batch.context().session {
        let token = session.fingerprints.token;
        if let Some(proof) =
            board_store::anonymous_session::post_proof(&state.pool, &token, &board, form.no).await?
        {
            board_store::delete_with_anonymous_proof_context(
                &state.pool,
                batch,
                form.no,
                token,
                proof,
                form.file_only,
            )
            .await?;
            return Ok(Redirect::to(&format!("/{board}/")));
        }
    }
    if !(8..=128).contains(&form.password.len()) {
        return Err(AppError(
            StatusCode::FORBIDDEN,
            "Error: Password incorrect.",
        ));
    }
    let hash = board_store::deletion_hash(&state.pool, &board, form.no).await?;
    let permit = state
        .limits
        .hashes
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            AppError(
                StatusCode::SERVICE_UNAVAILABLE,
                "Password processing is busy. Try again.",
            )
        })?;
    let proof = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        verify_deletion_password(&form.password, &hash)
            .then(|| <[u8; 32]>::from(Sha256::digest(hash.as_bytes())))
    })
    .await
    .unwrap_or(None);
    let Some(proof) = proof else {
        return Err(AppError(
            StatusCode::FORBIDDEN,
            "Error: Password incorrect.",
        ));
    };
    board_store::delete_with_password_proof_context(
        &state.pool,
        batch,
        form.no,
        proof,
        form.file_only,
    )
    .await?;
    Ok(Redirect::to(&format!("/{board}/")))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportForm {
    #[serde(deserialize_with = "canonical_report_id")]
    no: i64,
    reason: String,
}

fn canonical_report_id<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<i64, D::Error> {
    let value = String::deserialize(deserializer)?;
    crate::legacy_report::positive_id(&value)
        .ok_or_else(|| serde::de::Error::custom("Invalid report target."))
}

pub(crate) enum ReportSubmission {
    FreeText {
        no: i64,
        reason: String,
    },
    Categorical {
        no: i64,
        category_id: i64,
        expected_revision: i64,
    },
}

pub async fn report(
    State(state): State<AppState>,
    Path(board): Path<String>,
    axum::Extension(peer): axum::Extension<crate::security::RequestPeer>,
    headers: HeaderMap,
    form: Result<Form<ReportForm>, axum::extract::rejection::FormRejection>,
) -> Result<Response, AppError> {
    let fail = |no, error| crate::legacy_report::error(&state, &board, no, error);
    if !crate::legacy_report::safe_board(&board) {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "Invalid reporting request.",
        ));
    }
    let Form(form) = match form {
        Ok(form) => form,
        Err(error) => {
            return Ok(fail(
                None,
                AppError(error.status(), "Invalid reporting form."),
            ));
        }
    };
    submit_report(
        &state,
        &board,
        peer,
        &headers,
        ReportSubmission::FreeText {
            no: form.no,
            reason: form.reason,
        },
    )
    .await
}

pub(crate) async fn submit_report(
    state: &AppState,
    board: &str,
    peer: crate::security::RequestPeer,
    headers: &HeaderMap,
    form: ReportSubmission,
) -> Result<Response, AppError> {
    let no = match &form {
        ReportSubmission::FreeText { no, .. } | ReportSubmission::Categorical { no, .. } => *no,
    };
    let fail = |error| crate::legacy_report::error(state, board, Some(no), error);
    if !crate::legacy_report::safe_board(board) || no <= 0 {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "Invalid reporting request.",
        ));
    }
    let identity = match report_rate_identity(state, peer) {
        Ok(identity) => identity,
        Err(error) => return Ok(fail(error)),
    };
    // Target policy precedes cookie resolution; admission rechecks under locks.
    let target = match board_store::report_target(&state.pool, board, no).await {
        Ok(target) => target,
        Err(error) => return Ok(fail(error.into())),
    };
    let fail = |error| crate::legacy_report::target_error(state, &target, error);
    let session = match crate::anonymous_session::Session::resolve(state, headers, peer.0).await {
        Ok(session) => session,
        Err(error) => return Ok(fail(error)),
    };
    // Reserve and render the bounded response before committing. An exhausted
    // output budget must never make a saved report appear to have failed.
    let mut response = crate::legacy_report::success(state, &target)?;
    let result = match form {
        ReportSubmission::FreeText { reason, .. } => {
            board_store::report_with_anonymous_session(
                &state.pool,
                board,
                no,
                &reason,
                &identity,
                session.posting,
            )
            .await
        }
        ReportSubmission::Categorical {
            category_id,
            expected_revision,
            ..
        } => {
            board_store::report_categorical_with_anonymous_session(
                &state.pool,
                board,
                no,
                category_id,
                expected_revision,
                &identity,
                session.posting,
            )
            .await
        }
    };
    if let Err(error) = result {
        drop(response);
        return Ok(fail(error.into()));
    }
    session.append(response.headers_mut(), state.production);
    // Session cookie handling supplies its generic no-store policy; this
    // dedicated report shell also remains explicitly private.
    response.headers_mut().insert(
        "cache-control",
        axum::http::HeaderValue::from_static("private, no-store"),
    );
    Ok(response)
}

#[cfg(test)]
mod readiness_tests {

    #[test]
    fn automatic_admission_readiness_uses_shared_catalog_contract() {
        let sql = board_store::automatic_admission::READINESS_SQL;
        for required in [
            "resolve_automatic_identity",
            "lookup_automatic_identity",
            "17 16 20 16",
            "17 20",
            "17 25 20 17 16 20",
            "TABLE(automatic_identity uuid, source_new boolean)",
            "TABLE(rejected boolean, user_thread_limit integer, user_thread_period_hours integer)",
            "registration_xid",
            "pg_current_xact_id()",
            "i.indisunique",
            "NOT a.attnotnull",
            "p.prosecdef",
            "search_path=pg_catalog,pg_temp",
            "pg_has_role(runtime.oid,owners.oid,'MEMBER')",
            "has_column_privilege",
            "pg_catalog.aclexplode",
        ] {
            assert!(sql.contains(required), "{required}");
        }
        for forbidden in [
            "FROM post_secrets.",
            "JOIN post_secrets.",
            "FROM content.",
            "JOIN content.",
            "to_regprocedure('post_secrets.",
            "INSERT INTO",
            "UPDATE post_secrets.",
            "DELETE FROM",
            "SELECT content.check_user_thread_quota",
            "has_sequence_privilege",
        ] {
            assert!(!sql.contains(forbidden), "{forbidden}");
        }
    }
    use super::{
        ARCHIVE_DELETION_SECRETS_READY_SQL, OP_BUMP_CONTEXT_READY_SQL, REPORT_ADMISSION_READY_SQL,
        USER_THREAD_QUOTA_READY_SQL,
    };

    #[test]
    fn report_admission_readiness_is_catalog_only_and_fail_closed() {
        for contract in [
            "board_report_admission_owner",
            "NOT (r.rolcanlogin OR r.rolsuper OR r.rolcreatedb OR r.rolcreaterole OR r.rolreplication OR r.rolbypassrls)",
            "pg_has_role(runtime.oid,r.oid,'MEMBER')",
            "content.check_report_admission(text,bigint,bytea)",
            "content.admit_report(text,bigint,text,bytea)",
            "content.check_report_admission(text,bigint,bytea,bytea,bigint)",
            "content.admit_report(text,bigint,text,bytea,bytea,bytea,bytea,bytea,boolean,bigint)",
            "has_function_privilege('board_public',p.oid,'EXECUTE')=required.public_allowed",
            "OR (current_user='board_staff' AND required.staff_allowed)",
            "has_function_privilege('board_staff',p.oid,'EXECUTE')=required.staff_allowed",
            "p.pronargdefaults=0 AND p.provariadic=0",
            "cardinality(p.proconfig)=1",
            "p.prosecdef AND p.provolatile='v'",
            "search_path=pg_catalog,pg_temp",
            "has_function_privilege(current_user,p.oid,'EXECUTE')",
            "a.grantee NOT IN",
            "('report_target','bigint',false,'25 20')",
            "('report_target','bigint',false,'25 20 1184')",
            "('check_report_limits','void',false,'25 20 17 1184')",
            "('check_report_limits','void',false,'25 20 17 2950 1184')",
            "('delete_reports_for_deleted_target','trigger',false,'')",
            "('retire_staff_file_report_membership','void',true,'25 20')",
            "NOT has_function_privilege(current_user,p.oid,'EXECUTE')",
            "required.attachment_authority AND NOT a.is_grantable",
            "attachment.rolname='board_attachment_owner' AND attachment.oid=a.grantee",
            "('posts','retire_deleted_post_report_membership')",
            "('threads','retire_deleted_thread_report_membership')",
            "t.tgname=required.trigger_name AND t.tgtype=17",
            "t.tgenabled='O' AND NOT t.tgisinternal",
            "t.tgnargs=0 AND octet_length(t.tgargs)=0 AND t.tgconstraint=0",
            "NOT t.tgdeferrable AND NOT t.tginitdeferred",
            "t.tgoldtable IS NULL AND t.tgnewtable IS NULL",
            "p.proname='delete_reports_for_deleted_target'",
            "p.pronamespace=(SELECT oid FROM pg_catalog.pg_namespace WHERE nspname='post_secrets')",
            "p.pronargs=0 AND p.prokind='f' AND p.prorettype='pg_catalog.trigger'::regtype",
            "t.tgattr::text=(SELECT a.attnum::text",
            "a.attname='deleted' AND NOT a.attisdropped",
            "='notold.deletedandnew.deleted'",
            "('report_membership'),('report_admission_gate')",
            "('report_membership'),('anonymous_reports'),('report_weight_evidence')",
            "fk.contype='f' AND fk.confdeltype='c' AND fk.convalidated",
            "has_table_privilege(r.oid,c.oid,'DELETE')",
            "NOT has_table_privilege(current_user,c.oid,'DELETE,TRUNCATE,TRIGGER')",
            "c.relowner=r.oid",
            "has_table_privilege(runtime.oid,c.oid,'INSERT')",
            "has_column_privilege(runtime.oid,c.oid,a.attnum,'INSERT')",
            "has_sequence_privilege(runtime.oid,to_regclass('content.reports_id_seq'),'USAGE,SELECT,UPDATE')",
            "has_sequence_privilege(r.oid,to_regclass('content.reports_id_seq'),'USAGE')",
            "('reports','created_at','INSERT')",
            "a.attname='membership_limit' AND a.attnum>0 AND NOT a.attisdropped",
            "a.atttypid='pg_catalog.int4'::regtype AND a.attnotnull",
            "pg_catalog.pg_get_expr(d.adbin,d.adrelid)='100000'",
            "k.contype='c' AND k.convalidated",
            "k.conkey=ARRAY[a.attnum]::smallint[]",
            "='membership_limit>=1andmembership_limit<=1000000'",
            "NOT has_schema_privilege(r.oid,n.oid,'CREATE')",
        ] {
            assert!(REPORT_ADMISSION_READY_SQL.contains(contract), "{contract}");
        }
        for forbidden in [
            "FROM content.",
            "FROM post_secrets.",
            "JOIN content.",
            "JOIN post_secrets.",
            "actor_hash",
            "INSERT INTO",
            "DELETE FROM",
            "UPDATE content.",
            "SELECT content.admit_report",
            "SELECT content.check_report_admission",
        ] {
            assert!(
                !REPORT_ADMISSION_READY_SQL.contains(forbidden),
                "{forbidden}"
            );
        }
    }

    #[test]
    fn user_thread_quota_readiness_checks_only_restricted_catalog_contracts() {
        for contract in [
            "to_regprocedure('content.check_user_thread_quota(bytea,text,bigint)')",
            "has_function_privilege(current_user,p.oid,'EXECUTE')",
            "has_function_privilege('board_public',p.oid,'EXECUTE')",
            "has_function_privilege('board_staff',p.oid,'EXECUTE')",
            "p.prosecdef AND p.provolatile='v'",
            "r.rolname='board_posting_cooldown_owner'",
            "NOT (r.rolcanlogin OR r.rolsuper OR r.rolcreatedb OR r.rolcreaterole OR r.rolreplication OR r.rolbypassrls)",
            "search_path=pg_catalog,pg_temp",
            "TABLE(rejected boolean, user_thread_limit integer, user_thread_period_hours integer)",
            "coalesce(p.proacl,pg_catalog.acldefault('f',p.proowner))",
            "a.grantee=0",
            "a.grantee NOT IN (p.proowner,",
            "('user_thread_limit'),('user_thread_period_hours')",
            "a.attrelid=to_regclass('content.boards')",
            "a.attnum>0 AND NOT a.attisdropped AND a.attnotnull",
            "a.atttypid='integer'::regtype",
            "has_column_privilege('board_posting_cooldown_owner',a.attrelid,a.attnum,'SELECT')",
        ] {
            assert!(USER_THREAD_QUOTA_READY_SQL.contains(contract), "{contract}");
        }
        assert!(USER_THREAD_QUOTA_READY_SQL.starts_with("SELECT EXISTS ("));
        for forbidden in [
            "FROM content.",
            "FROM post_secrets.",
            "JOIN content.",
            "JOIN post_secrets.",
            "actor_hash",
            "INSERT INTO",
            "DELETE FROM",
            "UPDATE content.",
            "SELECT content.check_user_thread_quota",
        ] {
            assert!(
                !USER_THREAD_QUOTA_READY_SQL.contains(forbidden),
                "{forbidden}"
            );
        }
    }

    #[test]
    fn archive_secret_readiness_requires_exact_trigger_metadata() {
        for contract in [
            "('post_secrets','deletion','deletion_archive_guard','guard_archived_deletion_secret',31,false)",
            "('content','threads','retire_archived_deletion_secrets','retire_archived_deletion_secrets',17,true)",
            "t.tgenabled='O' AND NOT t.tgisinternal",
            "t.tgnargs=0 AND t.tgconstraint=0 AND NOT t.tgdeferrable AND NOT t.tginitdeferred",
            "t.tgoldtable IS NULL AND t.tgnewtable IS NULL",
            "n.nspname='post_secrets' AND p.proname=required.function_name",
            "p.pronargs=0 AND p.prokind='f' AND p.prorettype='pg_catalog.trigger'::regtype",
            "p.prosecdef AND p.provolatile='v'",
            "r.rolname='board_posting_cooldown_owner'",
            "NOT (r.rolcanlogin OR r.rolsuper OR r.rolcreatedb OR r.rolcreaterole OR r.rolreplication OR r.rolbypassrls)",
            "search_path=pg_catalog,pg_temp",
            "NOT has_function_privilege(current_user,p.oid,'EXECUTE')",
            "runtime.rolname IN ('board_public','board_staff','board_auth')",
            "a.grantee=0 AND a.privilege_type='EXECUTE'",
            "t.tgattr::text=(SELECT a.attnum::text",
            "a.attname='archived_at' AND NOT a.attisdropped",
            "old.archived_atISNULLANDnew.archived_atISNOTNULL",
            "ELSE t.tgqual IS NULL AND t.tgattr::text='' END",
        ] {
            assert!(
                ARCHIVE_DELETION_SECRETS_READY_SQL.contains(contract),
                "{contract}"
            );
        }
    }

    #[test]
    fn archive_secret_readiness_requires_narrow_owner_privileges_without_probes() {
        for contract in [
            "SELECT EXISTS (SELECT 1 FROM owner_role)",
            "WHERE NOT EXISTS (",
            "('post_secrets','deletion','post_id','SELECT')",
            "('content','threads','id','UPDATE')",
            "('content','threads','archived_at','SELECT')",
            "('content','posts','thread_id','SELECT')",
            "('content','boards','staff_only','SELECT')",
            "('content','boards','slug','UPDATE')",
            "has_table_privilege(r.oid,c.oid,'DELETE')",
            "NOT has_table_privilege(r.oid,c.oid,'SELECT,INSERT,UPDATE,TRUNCATE,REFERENCES,TRIGGER')",
            "a.attname<>'post_id' AND has_column_privilege(r.oid,c.oid,a.attnum,'SELECT')",
            "has_column_privilege(r.oid,c.oid,a.attnum,'INSERT,UPDATE,REFERENCES')",
            "NOT has_table_privilege(r.oid,c.oid,'UPDATE')",
            "a.attname<>'id'",
            "has_schema_privilege(r.oid,n.oid,'USAGE')",
            "NOT has_schema_privilege(r.oid,n.oid,'CREATE')",
        ] {
            assert!(
                ARCHIVE_DELETION_SECRETS_READY_SQL.contains(contract),
                "{contract}"
            );
        }
        for forbidden in [
            "FROM content.",
            "FROM post_secrets.",
            "JOIN content.",
            "JOIN post_secrets.",
            "password_hash",
            "guard_archived_deletion_secret()",
            "retire_archived_deletion_secrets()",
            "to_regprocedure(",
            "INSERT INTO",
            "DELETE FROM",
            "UPDATE content.",
            "UPDATE post_secrets.",
        ] {
            assert!(
                !ARCHIVE_DELETION_SECRETS_READY_SQL.contains(forbidden),
                "{forbidden}"
            );
        }
    }

    #[test]
    fn op_bump_readiness_requires_the_exact_restricted_api() {
        for contract in [
            "to_regprocedure('content.posting_op_bump_context(bytea,text,bigint)')",
            "has_function_privilege(current_user,p.oid,'EXECUTE')",
            "p.prosecdef AND p.provolatile='s'",
            "r.rolname='board_posting_cooldown_owner'",
            "NOT (r.rolcanlogin OR r.rolsuper OR r.rolcreatedb OR r.rolcreaterole OR r.rolreplication OR r.rolbypassrls)",
            "search_path=pg_catalog,pg_temp",
            "TABLE(own_reply boolean, latest_post_id bigint, latest_created_at timestamp with time zone)",
            "coalesce(p.proacl,pg_catalog.acldefault('f',p.proowner))",
            "a.grantee=0 AND a.privilege_type='EXECUTE'",
        ] {
            assert!(OP_BUMP_CONTEXT_READY_SQL.contains(contract), "{contract}");
        }
        assert!(OP_BUMP_CONTEXT_READY_SQL.starts_with("SELECT EXISTS ("));
        assert!(!OP_BUMP_CONTEXT_READY_SQL.contains("post_secrets"));
        assert!(!OP_BUMP_CONTEXT_READY_SQL.contains("FROM content."));
        assert!(!OP_BUMP_CONTEXT_READY_SQL.contains("content.staff_op_bump_context"));
    }
}

#[cfg(test)]
mod op_password_tests {
    use super::*;

    #[test]
    fn op_password_checks_the_real_hash_and_rejects_unbounded_profiles() {
        let hash = Argon2::default()
            .hash_password(b"owned-op-password", &SaltString::generate(&mut OsRng))
            .unwrap()
            .to_string();
        assert!(verify_deletion_password("owned-op-password", &hash));
        assert!(!verify_deletion_password("different-password", &hash));
        assert!(!verify_deletion_password(
            "owned-op-password",
            &hash.replace("m=19456", "m=4294967295")
        ));
        assert!(!verify_deletion_password(
            "owned-op-password",
            &hash.replace("t=2", "t=4294967295")
        ));
        assert!(!verify_deletion_password(
            "owned-op-password",
            &hash.replace("argon2id", "argon2i")
        ));
        assert!(!verify_deletion_password("owned-op-password", "missing"));
    }
}

#[cfg(test)]
mod storage_diagnostic_tests {
    use super::*;
    use std::{
        borrow::Cow,
        error::Error,
        fmt,
        io::Write,
        sync::{Arc, Mutex},
    };

    const HOSTILE: &str =
        "secret-password request-id /private/path SELECT private_data\nforged_event=true";

    #[derive(Debug)]
    struct FakeDatabaseError {
        code: Option<&'static str>,
    }

    impl fmt::Display for FakeDatabaseError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(HOSTILE)
        }
    }
    impl Error for FakeDatabaseError {}
    impl sqlx::error::DatabaseError for FakeDatabaseError {
        fn message(&self) -> &str {
            HOSTILE
        }
        fn code(&self) -> Option<Cow<'_, str>> {
            self.code.map(Cow::Borrowed)
        }
        fn as_error(&self) -> &(dyn Error + Send + Sync + 'static) {
            self
        }
        fn as_error_mut(&mut self) -> &mut (dyn Error + Send + Sync + 'static) {
            self
        }
        fn into_error(self: Box<Self>) -> Box<dyn Error + Send + Sync + 'static> {
            self
        }
        fn kind(&self) -> sqlx::error::ErrorKind {
            sqlx::error::ErrorKind::Other
        }
    }

    #[derive(Clone)]
    struct LogWriter(Arc<Mutex<Vec<u8>>>);
    impl Write for LogWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn convert_with_log(error: StoreError) -> (AppError, String) {
        let writer = LogWriter(Arc::new(Mutex::new(Vec::new())));
        let captured = writer.0.clone();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .without_time()
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        let response = tracing::subscriber::with_default(subscriber, || AppError::from(error));
        let log = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
        (response, log)
    }

    #[test]
    fn storage_diagnostic_sqlstates_use_only_exact_allowlisted_classes() {
        for (code, class) in [
            ("40001", "serialization"),
            ("40P01", "deadlock"),
            ("55P03", "lock_unavailable"),
            ("57014", "statement_cancelled"),
            ("25000", "transaction_state"),
            ("25001", "transaction_state"),
            ("25006", "transaction_state"),
            ("25P01", "transaction_state"),
            ("25P02", "transaction_state"),
            ("25P03", "idle_timeout"),
            ("57P05", "idle_timeout"),
            ("42501", "permissions"),
            ("23502", "not_null_constraint"),
            ("23503", "foreign_key_constraint"),
            ("23505", "unique_constraint"),
            ("23514", "check_constraint"),
            ("23P01", "exclusion_constraint"),
            ("08006", "connection"),
            ("53300", "connection_limit"),
            ("57P01", "server_unavailable"),
            ("42P01", "schema_contract"),
            ("42703", "schema_contract"),
            ("42883", "schema_contract"),
            ("99999", "database_other"),
            ("40001-extra", "database_other"),
            ("42501\nforged_event=true", "database_other"),
            (HOSTILE, "database_other"),
        ] {
            let error = StoreError::Database(sqlx::Error::Database(Box::new(FakeDatabaseError {
                code: Some(code),
            })));
            assert_eq!(storage_error_class(&error), class);
        }
        assert_eq!(database_error_class(None), "database_other");
    }

    #[test]
    fn storage_diagnostic_sqlx_categories_discard_payloads() {
        for (error, class) in [
            (sqlx::Error::PoolTimedOut, "pool_timeout"),
            (sqlx::Error::PoolClosed, "pool_closed"),
            (sqlx::Error::Io(std::io::Error::other(HOSTILE)), "io"),
            (sqlx::Error::Protocol(HOSTILE.into()), "protocol"),
            (sqlx::Error::RowNotFound, "row_not_found"),
            (
                sqlx::Error::Decode(Box::new(std::io::Error::other(HOSTILE))),
                "decode",
            ),
            (
                sqlx::Error::ColumnDecode {
                    index: HOSTILE.into(),
                    source: Box::new(std::io::Error::other(HOSTILE)),
                },
                "column_decode",
            ),
            (
                sqlx::Error::ColumnNotFound(HOSTILE.into()),
                "column_not_found",
            ),
            (sqlx::Error::BeginFailed, "transaction_state"),
        ] {
            assert_eq!(storage_error_class(&StoreError::Database(error)), class);
        }
        assert_eq!(
            storage_error_class(&StoreError::ContentRejected(HOSTILE.into())),
            "content_rejected"
        );
        assert_eq!(storage_error_class(&StoreError::ReadLimit), "read_limit");
    }

    #[tokio::test]
    async fn storage_diagnostic_logs_only_static_fields_and_preserves_503_body() {
        for (code, class) in [
            (Some(HOSTILE), "database_other"),
            (Some("40P01"), "deadlock"),
            (None, "database_other"),
        ] {
            let (error, log) = convert_with_log(StoreError::Database(sqlx::Error::Database(
                Box::new(FakeDatabaseError { code }),
            )));
            assert_eq!(error.0, StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(error.1, "Storage is unavailable. Try again later.");
            let event: serde_json::Value = serde_json::from_str(log.trim()).unwrap();
            assert_eq!(
                event["fields"],
                serde_json::json!({
                    "event": "database_operation_failed",
                    "error_class": class,
                    "message": "database operation failed"
                })
            );
            for forbidden in [
                HOSTILE,
                "secret-password",
                "request-id",
                "/private/path",
                "SELECT",
                "forged_event",
                "40P01",
            ] {
                assert!(!log.contains(forbidden));
            }
            let response = error.into_response();
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
            let actual = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let expected = axum::body::to_bytes(
                message_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Storage is unavailable. Try again later.",
                )
                .into_body(),
                usize::MAX,
            )
            .await
            .unwrap();
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn storage_diagnostic_not_found_remains_404_without_storage_warning() {
        for (error, message) in [
            (StoreError::NotFound, "Board, thread, or post not found."),
            (StoreError::PageNotFound, "Page not found."),
        ] {
            let (response, log) = convert_with_log(error);
            assert_eq!(response.0, StatusCode::NOT_FOUND);
            assert_eq!(response.1, message);
            assert!(log.is_empty());
        }
    }
}
