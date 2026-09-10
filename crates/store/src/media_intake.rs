//! Capability-scoped intake. No queue leases, approvals, or base-table access.
use crate::StoreError;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::time::Duration;

#[derive(Clone)]
pub struct IntakeStore {
    pool: PgPool,
}

#[derive(sqlx::FromRow)]
pub struct IntakeReservation {
    pub id: String,
    pub capability: String,
}

#[derive(sqlx::FromRow)]
pub struct IntakeStatus {
    pub id: String,
    pub state: String,
    pub input_bytes: Option<i64>,
    pub output_id: Option<String>,
}

impl IntakeStore {
    pub async fn connect(url: &str) -> Result<Self, StoreError> {
        let pool = PgPoolOptions::new()
            .max_connections(8)
            .acquire_timeout(Duration::from_secs(3))
            .connect(url)
            .await?;
        let store = Self { pool };
        if let Err(error) = store.ready().await {
            store.pool.close().await;
            return Err(error);
        }
        Ok(store)
    }

    pub async fn reserve(&self, filename: &str) -> Result<IntakeReservation, StoreError> {
        sqlx::query_as("SELECT id, capability FROM media_intake.reserve($1)")
            .bind(filename)
            .fetch_one(&self.pool)
            .await
            .map_err(scoped_error)
    }

    pub async fn begin_upload(&self, id: &str, capability: &str) -> Result<(), StoreError> {
        sqlx::query("SELECT media_intake.begin_upload($1, $2)")
            .bind(id)
            .bind(capability)
            .execute(&self.pool)
            .await
            .map_err(scoped_error)?;
        Ok(())
    }

    pub async fn finish_upload(
        &self,
        id: &str,
        capability: &str,
        bytes: u64,
    ) -> Result<(), StoreError> {
        // Values beyond bigint are represented by an invalid sentinel; SQL still
        // authenticates the handle before reporting the size error.
        let bytes = i64::try_from(bytes).unwrap_or(-1);
        sqlx::query("SELECT media_intake.finish_upload($1, $2, $3)")
            .bind(id)
            .bind(capability)
            .bind(bytes)
            .execute(&self.pool)
            .await
            .map_err(scoped_error)?;
        Ok(())
    }

    pub async fn abort_upload(&self, id: &str, capability: &str) -> Result<(), StoreError> {
        sqlx::query("SELECT media_intake.abort_upload($1, $2)")
            .bind(id)
            .bind(capability)
            .execute(&self.pool)
            .await
            .map_err(scoped_error)?;
        Ok(())
    }

    pub async fn status(&self, id: &str, capability: &str) -> Result<IntakeStatus, StoreError> {
        sqlx::query_as("SELECT id, state, input_bytes, output_id FROM media_intake.status($1, $2)")
            .bind(id)
            .bind(capability)
            .fetch_one(&self.pool)
            .await
            .map_err(scoped_error)
    }

    pub async fn ready(&self) -> Result<(), StoreError> {
        let safe: bool = sqlx::query_scalar(SAFE_ROLE).fetch_one(&self.pool).await?;
        if !safe {
            return Err(StoreError::UnsafeRole);
        }
        let ready: bool = sqlx::query_scalar("SELECT media_intake.ready()")
            .fetch_one(&self.pool)
            .await?;
        if !ready {
            return Err(StoreError::UnsafeRole);
        }
        Ok(())
    }

    pub async fn close(&self) -> Result<(), StoreError> {
        self.pool.close().await;
        Ok(())
    }
}

fn scoped_error(error: sqlx::Error) -> StoreError {
    match error.as_database_error().and_then(|e| e.code()).as_deref() {
        Some("P0002") => StoreError::NotFound,
        Some("P0001") => StoreError::Conflict("Media intake state or capacity is unavailable."),
        Some("22023") => StoreError::Invalid("Invalid media intake metadata or size."),
        _ => StoreError::Database(error),
    }
}

// Validate catalogs as the actual login, before invoking any definer function.
// Column grants are checked individually: a harmless-looking table ACL can still
// conceal authority to write a lease or approval through a column grant.
const SAFE_ROLE: &str = r#"
WITH expected_functions(signature, result) AS (VALUES
 ('media_intake.reserve(text)', 'TABLE(id text, capability text)'),
 ('media_intake.begin_upload(text,text)', 'void'),
 ('media_intake.finish_upload(text,text,bigint)', 'void'),
 ('media_intake.abort_upload(text,text)', 'void'),
 ('media_intake.status(text,text)', 'TABLE(id text, state text, input_bytes bigint, output_id text)'),
 ('media_intake.ready()', 'boolean')
), allowed_columns(relation, column_name, privilege) AS (VALUES
 ('media.queue_policy','singleton','SELECT'), ('media.queue_policy','capacity','SELECT'),
 ('media.queue_policy','singleton','UPDATE'),
 ('media.jobs','id','SELECT'), ('media.jobs','state','SELECT'),
 ('media.jobs','input_bytes','SELECT'), ('media.jobs','expires_at','SELECT'),
 ('media.jobs','id','INSERT'), ('media.jobs','filename','INSERT'), ('media.jobs','expires_at','INSERT'),
 ('media.jobs','state','UPDATE'), ('media.jobs','input_bytes','UPDATE'),
 ('media.jobs','expires_at','UPDATE'), ('media.jobs','updated_at','UPDATE'), ('media.jobs','failure','UPDATE'),
 ('media_intake.handles','job_id','SELECT'), ('media_intake.handles','capability_hash','SELECT'),
 ('media_intake.handles','upload_started_at','SELECT'), ('media_intake.handles','job_id','INSERT'),
 ('media_intake.handles','capability_hash','INSERT'), ('media_intake.handles','upload_started_at','UPDATE'),
 ('media.assets','id','SELECT'), ('media.assets','job_id','SELECT'), ('media.assets','state','SELECT')
), app_tables AS (
 SELECT c.oid, c.relkind, n.nspname || '.' || c.relname AS relation
 FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
 WHERE left(n.nspname,3) <> 'pg_' AND n.nspname <> 'information_schema'
)
SELECT current_user = 'board_media_intake' AND session_user = 'board_media_intake'
 AND EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'board_media_intake' AND rolcanlogin)
 AND EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'board_media_intake_owner' AND NOT rolcanlogin)
 AND NOT EXISTS (
   SELECT 1 FROM pg_roles r WHERE r.rolname IN ('board_media_intake','board_media_intake_owner')
   AND (r.rolsuper OR r.rolcreatedb OR r.rolcreaterole OR r.rolreplication OR r.rolbypassrls
     OR EXISTS (SELECT 1 FROM pg_auth_members m WHERE m.member = r.oid)
     OR EXISTS (SELECT 1 FROM pg_database d WHERE d.datdba = r.oid)
     OR has_database_privilege(r.oid, current_database(), 'CREATE,TEMPORARY')
     OR EXISTS (SELECT 1 FROM pg_namespace n WHERE n.nspowner = r.oid)
     OR EXISTS (SELECT 1 FROM pg_class c WHERE c.relowner = r.oid)
     OR EXISTS (SELECT 1 FROM pg_type t WHERE t.typowner = r.oid)
     OR EXISTS (SELECT 1 FROM pg_namespace n WHERE has_schema_privilege(r.oid,n.oid,'CREATE'))
     OR EXISTS (SELECT 1 FROM pg_namespace n
       WHERE left(n.nspname,3) <> 'pg_' AND n.nspname <> 'information_schema'
       AND n.nspname <> 'media_intake' AND NOT (r.rolname = 'board_media_intake_owner' AND n.nspname = 'media')
       AND has_schema_privilege(r.oid,n.oid,'USAGE'))))
 AND NOT EXISTS (SELECT 1 FROM pg_auth_members m JOIN pg_roles r ON r.oid = m.roleid
   JOIN pg_roles member_role ON member_role.oid = m.member
   WHERE r.rolname = 'board_media_intake_owner' AND member_role.rolname <> 'board_migrator')
 AND NOT EXISTS (SELECT 1 FROM pg_proc p WHERE p.proowner = (SELECT oid FROM pg_roles WHERE rolname = 'board_media_intake'))
 AND has_schema_privilege(current_user, 'media_intake', 'USAGE')
 AND has_schema_privilege('board_media_intake_owner', 'media_intake', 'USAGE')
 AND has_schema_privilege('board_media_intake_owner', 'media', 'USAGE')
 AND NOT EXISTS (SELECT 1 FROM app_tables t WHERE t.relkind IN ('r','p','v','m','f')
   AND (has_table_privilege(current_user,t.oid,'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
     OR has_any_column_privilege(current_user,t.oid,'SELECT,INSERT,UPDATE,REFERENCES')
     OR has_table_privilege('board_media_intake_owner',t.oid,'DELETE,TRUNCATE,REFERENCES,TRIGGER')))
 AND NOT EXISTS (SELECT 1 FROM app_tables t CROSS JOIN pg_roles r
   WHERE t.relkind = 'S' AND r.rolname IN ('board_media_intake','board_media_intake_owner')
   AND has_sequence_privilege(r.oid,t.oid,'USAGE,SELECT,UPDATE'))
 AND NOT EXISTS (
   SELECT 1 FROM app_tables t JOIN pg_attribute a ON a.attrelid = t.oid
   CROSS JOIN (VALUES ('SELECT'),('INSERT'),('UPDATE'),('REFERENCES')) AS priv(name)
   WHERE t.relkind IN ('r','p','v','m','f') AND a.attnum > 0 AND NOT a.attisdropped
   AND has_column_privilege('board_media_intake_owner',t.oid,a.attnum,priv.name)
       <> EXISTS (SELECT 1 FROM allowed_columns ac
          WHERE ac.relation = t.relation AND ac.column_name = a.attname AND ac.privilege = priv.name))
 AND NOT EXISTS (SELECT 1 FROM allowed_columns ac WHERE NOT EXISTS (
   SELECT 1 FROM app_tables t JOIN pg_attribute a ON a.attrelid = t.oid
   WHERE t.relation = ac.relation AND a.attname = ac.column_name AND a.attnum > 0 AND NOT a.attisdropped
   AND has_column_privilege('board_media_intake_owner', t.oid, a.attnum, ac.privilege)))
 AND NOT EXISTS (SELECT 1 FROM expected_functions e LEFT JOIN pg_proc p ON p.oid = to_regprocedure(e.signature)
   WHERE p.oid IS NULL OR p.proowner <> (SELECT oid FROM pg_roles WHERE rolname = 'board_media_intake_owner')
   OR NOT p.prosecdef OR p.proconfig IS DISTINCT FROM ARRAY['search_path=pg_catalog, pg_temp']::text[]
   OR pg_get_function_result(p.oid) <> e.result
   OR NOT has_function_privilege(current_user,p.oid,'EXECUTE')
   OR has_function_privilege(current_user,p.oid,'EXECUTE WITH GRANT OPTION')
   OR EXISTS (SELECT 1 FROM aclexplode(COALESCE(p.proacl,acldefault('f',p.proowner))) acl
     WHERE acl.grantee = 0 AND acl.privilege_type = 'EXECUTE'))
 AND NOT EXISTS (
   SELECT 1 FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace
   WHERE left(n.nspname,3) <> 'pg_' AND n.nspname <> 'information_schema'
   AND (has_function_privilege(current_user,p.oid,'EXECUTE')
     OR has_function_privilege('board_media_intake_owner',p.oid,'EXECUTE')
     OR p.proowner = (SELECT oid FROM pg_roles WHERE rolname = 'board_media_intake_owner'))
   AND NOT EXISTS (SELECT 1 FROM expected_functions e WHERE to_regprocedure(e.signature) = p.oid))
"#;
