//! One-use attachment authorization. Public credentials cannot approve media.
use crate::{StoreError, media_intake::IntakeReservation};
use sqlx::PgPool;

/// Deliberately has no Debug implementation: the capability is a bearer secret.
pub struct NewAttachment {
    pub upload: IntakeReservation,
    pub spoiler: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, sqlx::FromRow)]
pub struct PostAttachment {
    pub post_id: i64,
    pub asset_id: String,
    pub filename: String,
    pub bytes: i64,
    pub width: i32,
    pub height: i32,
    pub spoiler: bool,
    pub file_deleted: bool,
    pub tim: i64,
    pub md5: Option<String>,
    pub thumbnail_width: Option<i32>,
    pub thumbnail_height: Option<i32>,
    pub output_format: crate::media_assets::MediaFormat,
}

pub async fn attachment(pool: &PgPool, post_id: i64) -> Result<Option<PostAttachment>, StoreError> {
    Ok(
        sqlx::query_as("SELECT * FROM content.visible_post_media WHERE post_id = $1")
            .bind(post_id)
            .fetch_optional(pool)
            .await?,
    )
}

pub(crate) async fn load(
    connection: &mut sqlx::PgConnection,
    posts: &mut [crate::Post],
) -> Result<(), StoreError> {
    let ids: Vec<i64> = posts.iter().map(|p| p.id).collect();
    let rows: Vec<PostAttachment> =
        sqlx::query_as("SELECT * FROM content.visible_post_media WHERE post_id=ANY($1)")
            .bind(&ids)
            .fetch_all(connection)
            .await?;
    let mut rows: std::collections::BTreeMap<_, _> =
        rows.into_iter().map(|a| (a.post_id, a)).collect();
    for post in posts {
        post.attachment = rows.remove(&post.id);
    }
    Ok(())
}

pub async fn check_upload(pool: &PgPool, job_id: &str, capability: &str) -> Result<(), StoreError> {
    sqlx::query("SELECT content.check_attachment_upload($1,$2)")
        .bind(job_id)
        .bind(capability)
        .execute(pool)
        .await
        .map_err(scoped_error)?;
    Ok(())
}

pub async fn cancel_upload(
    pool: &PgPool,
    job_id: &str,
    capability: &str,
) -> Result<(), StoreError> {
    sqlx::query("SELECT content.cancel_attachment_upload($1,$2)")
        .bind(job_id)
        .bind(capability)
        .execute(pool)
        .await
        .map_err(scoped_error)?;
    Ok(())
}

/// Self/public file-only action; a live text-only or already-marked post succeeds.
/// Caller verifies deletion authority first. Public password deletion uses
/// `delete_with_password_proof` so proof and mutation share the board lock.
/// This helper never clears reports. The administrative staff route must use
/// `content.staff_delete_post_attachment` in its audit transaction instead.
pub async fn delete_attachment<'e>(
    executor: impl sqlx::Executor<'e, Database = sqlx::Postgres>,
    board: &str,
    post_id: i64,
) -> Result<(), StoreError> {
    sqlx::query("SELECT content.delete_post_attachment($1, $2)")
        .bind(board)
        .bind(post_id)
        .execute(executor)
        .await
        .map_err(scoped_error)?;
    Ok(())
}

pub(crate) fn scoped_error(error: sqlx::Error) -> StoreError {
    match error.as_database_error().and_then(|e| e.code()).as_deref() {
        Some("P0002") => StoreError::NotFound,
        Some("P0001") => StoreError::Conflict(
            "Attachment is unavailable, already used, or the image limit was reached.",
        ),
        Some("22023") => StoreError::Invalid("Invalid attachment request."),
        Some("23514") => match error.as_database_error().map(|error| error.message()) {
            Some("Error: File too large.") => StoreError::Invalid("Error: File too large."),
            Some("Error: Upload failed.") => StoreError::Invalid("Error: Upload failed."),
            Some("Error: Image resolution is too small.") => {
                StoreError::Invalid("Error: Image resolution is too small.")
            }
            Some("Error: Image resolution is too large.") => {
                StoreError::Invalid("Error: Image resolution is too large.")
            }
            _ => StoreError::Database(error),
        },
        _ => crate::restrictions::insertion_error(error),
    }
}

/// Catalog-only readiness for the post-level file-only action and the narrow
/// administrative report-cleanup bridge. Never reads private report evidence.
pub const FILE_DELETION_READINESS_SQL: &str = r#"
WITH attachment_owner AS (
    SELECT oid FROM pg_catalog.pg_roles WHERE rolname='board_attachment_owner'
      AND NOT (rolcanlogin OR rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls)
), report_owner AS (
    SELECT oid FROM pg_catalog.pg_roles WHERE rolname='board_report_admission_owner'
      AND NOT (rolcanlogin OR rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls)
), functions AS (
    SELECT p.*,n.nspname FROM pg_catalog.pg_proc p
    JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace
    WHERE (n.nspname='content' AND p.proname IN ('delete_post_attachment','staff_delete_post_attachment'))
       OR (n.nspname='post_secrets' AND p.proname='retire_staff_file_report_membership')
)
SELECT EXISTS(SELECT 1 FROM attachment_owner) AND EXISTS(SELECT 1 FROM report_owner)
AND EXISTS(SELECT 1 FROM pg_catalog.pg_attribute a
    JOIN pg_catalog.pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
    WHERE a.attrelid=to_regclass('content.posts') AND a.attname='file_deleted'
      AND a.attnum>0 AND NOT a.attisdropped AND a.attnotnull
      AND a.atttypid='boolean'::regtype AND a.attgenerated='' AND a.attidentity=''
      AND pg_catalog.pg_get_expr(d.adbin,d.adrelid)='false'
      AND has_column_privilege(current_user,a.attrelid,a.attnum,'SELECT')
      AND has_column_privilege('board_attachment_owner',a.attrelid,a.attnum,'SELECT')
      AND has_column_privilege('board_attachment_owner',a.attrelid,a.attnum,'UPDATE'))
AND NOT EXISTS(SELECT 1 FROM pg_catalog.pg_roles r
    WHERE r.rolname IN('board_public','board_staff','board_auth','board_media',
        'board_media_read','board_media_intake','board_monitor') AND (
        has_column_privilege(r.oid,'content.posts','file_deleted','INSERT,UPDATE')
        OR has_table_privilege(r.oid,'content.reports','DELETE,TRUNCATE')
        OR pg_has_role(r.oid,'board_attachment_owner','MEMBER')
        OR pg_has_role(r.oid,'board_report_admission_owner','MEMBER')))
AND NOT has_table_privilege('board_attachment_owner','content.posts','UPDATE,DELETE,TRUNCATE')
AND NOT has_table_privilege('board_attachment_owner','content.reports','DELETE,TRUNCATE')
AND has_table_privilege('board_report_admission_owner','content.reports','DELETE')
AND NOT has_schema_privilege('board_attachment_owner','content','CREATE')
AND NOT has_schema_privilege('board_report_admission_owner','post_secrets','CREATE')
AND (SELECT count(*)=3 FROM functions)
AND NOT EXISTS(SELECT 1 FROM functions p WHERE
    p.prokind<>'f' OR NOT p.prosecdef OR p.provolatile<>'v' OR p.proretset
    OR p.prorettype<>'void'::regtype OR p.proargtypes::text<>'25 20'
    OR p.pronargdefaults<>0 OR p.provariadic<>0
    OR coalesce(cardinality(p.proconfig),0)<>1
    OR coalesce(replace(p.proconfig[1],' ',''),'')<>'search_path=pg_catalog,pg_temp'
    OR p.proowner<>CASE WHEN p.nspname='content' THEN (SELECT oid FROM attachment_owner)
        ELSE (SELECT oid FROM report_owner) END
    OR has_function_privilege('board_public',p.oid,'EXECUTE')<>(p.proname='delete_post_attachment')
    OR has_function_privilege('board_staff',p.oid,'EXECUTE')<>(p.nspname='content')
    OR NOT has_function_privilege('board_attachment_owner',p.oid,'EXECUTE')
    OR has_function_privilege('board_auth',p.oid,'EXECUTE')
    OR EXISTS(SELECT 1 FROM pg_catalog.aclexplode(coalesce(p.proacl,pg_catalog.acldefault('f',p.proowner))) a
        WHERE a.privilege_type='EXECUTE' AND (a.is_grantable OR a.grantee NOT IN(
            p.proowner,(SELECT oid FROM attachment_owner),
            CASE WHEN p.proname='delete_post_attachment' THEN 'board_public'::regrole ELSE p.proowner END,
            CASE WHEN p.nspname='content' THEN 'board_staff'::regrole ELSE p.proowner END))))
AND EXISTS(SELECT 1 FROM functions p WHERE p.proname='delete_post_attachment'
    AND position('UPDATE content.posts SET file_deleted=true' in p.prosrc)>0
    AND position('UPDATE content.post_media SET file_deleted=true WHERE post_id=p_post' in p.prosrc)>0
    AND position('v.archive_expires_at>clock_timestamp()' in p.prosrc)>0
    AND position('FOR UPDATE OF t' in p.prosrc)>0
    AND position('UPDATE content.threads SET modified_at=clock_timestamp()' in p.prosrc)>0)
AND EXISTS(SELECT 1 FROM functions p WHERE p.proname='staff_delete_post_attachment'
    AND position('PERFORM content.delete_post_attachment(p_board,p_post)' in p.prosrc)>0
    AND position('PERFORM post_secrets.retire_staff_file_report_membership(p_board,p_post)' in p.prosrc)>0)
AND EXISTS(SELECT 1 FROM functions p WHERE p.proname='retire_staff_file_report_membership'
    AND position('DELETE FROM content.reports WHERE board=p_board AND post_id=p_post' in p.prosrc)>0
    AND position('FOR UPDATE NOWAIT' in p.prosrc)>0)
AND NOT EXISTS(SELECT 1 FROM (VALUES
    ('content','visible_post_media'),('content','staff_post_media'),('media','approved_assets')
) required(schema_name,view_name)
WHERE NOT EXISTS(SELECT 1 FROM pg_catalog.pg_class c
    JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace
    WHERE n.nspname=required.schema_name AND c.relname=required.view_name AND c.relkind='v'
      AND c.reloptions @> ARRAY['security_barrier=true']::text[]
      AND position('p.file_deleted' in pg_catalog.pg_get_viewdef(c.oid))>0))
"#;
