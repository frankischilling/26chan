use crate::{
    AppError, AppState,
    access::{Level, Permissions},
};
use axum::http::HeaderMap;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{RngCore, rngs::OsRng};
use sha2::{Digest, Sha256};
use sqlx::PgPool;

// Inspect attachment authority through catalogs; never read a proof or consume one.
pub(crate) const STAFF_ATTACHMENT_READY_SQL: &str = r#"WITH function_specs(signature, owner_name, result_type, definer, callers) AS (VALUES
    ('staff_identity.issue_source_attachment_post_authority(bytea,bytea,bytea,integer,boolean,bigint,text,bigint,text,text,text,timestamptz,boolean,integer,bytea,text,text,text,boolean,boolean,text,text,boolean)', 'board_staff_post_owner', 'boolean', true, ARRAY['board_auth']::text[]),
    ('staff_identity.issue_ordinary_attachment_post_authority(bytea,bytea,bytea,integer,bigint,text,bigint,text,text,text,timestamptz,boolean,integer,bytea,text,text,text,boolean,jsonb,boolean,text,text,boolean)', 'board_staff_post_owner', 'boolean', true, ARRAY['board_auth']::text[]),
    ('staff_identity.bind_post_attachment_context(bytea,text,text,boolean)', 'board_staff_post_owner', 'void', false, ARRAY[]::text[]),
    ('content.consume_staff_post_authority(bytea,bigint,text,bigint,text,text,text,timestamptz)', 'board_staff_post_owner', 'text', true, ARRAY['board_staff']::text[]),
    ('content.consume_staff_post_authority_without_attachment(bytea,bigint,text,bigint,text,text,text,timestamptz)', 'board_staff_post_owner', 'text', true, ARRAY[]::text[]),
    ('content.lock_staff_attachment_receipt(text,bigint,text,bytea)', 'board_attachment_owner', 'void', true, ARRAY['board_staff_post_owner']::text[]),
    ('content.consume_staff_attachment_receipt(bigint,text,bigint,text,bytea,boolean,boolean)', 'board_attachment_owner', 'void', true, ARRAY['board_staff_post_owner']::text[]),
    ('content.attach_staff_post_receipt()', 'board_staff_post_owner', 'trigger', true, ARRAY[]::text[]),
    ('content.reject_orphan_staff_attachment()', 'board_staff_post_owner', 'trigger', true, ARRAY[]::text[]),
    ('content.attachment_upload_filename(text,text)', 'board_attachment_owner', 'text', true, ARRAY['board_public','board_staff']::text[]),
    ('content.check_attachment_upload(text,text)', 'board_attachment_owner', 'void', true, ARRAY['board_public','board_staff']::text[]),
    ('content.cancel_attachment_upload(text,text)', 'board_attachment_owner', 'void', true, ARRAY['board_public','board_staff']::text[])
), required_functions AS (
    SELECT spec.*,p.oid FROM function_specs spec
    LEFT JOIN (pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace)
      ON n.nspname=split_part(spec.signature,'.',1)
      AND p.proname=split_part(split_part(spec.signature,'.',2),'(',1)
      AND p.proargtypes::text=coalesce((SELECT string_agg(to_regtype(arg.name)::oid::text,' ' ORDER BY arg.ordinal)
          FROM unnest(string_to_array(substring(spec.signature FROM '\((.*)\)'),',')) WITH ORDINALITY arg(name,ordinal)), '')
), private_tables AS (
    SELECT c.oid,c.relowner,c.relacl,n.nspname,c.relname FROM pg_catalog.pg_class c
    JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace
    WHERE n.nspname='post_secrets' AND c.relname IN ('staff_post_intents','staff_attachment_handoffs') AND c.relkind='r'
), restricted_roles AS (
    SELECT oid,rolname FROM pg_catalog.pg_roles WHERE rolname IN
        ('board_public','board_staff','board_auth','board_media','board_media_read','board_media_intake',
         'board_monitor','board_attachment_owner','board_media_intake_owner','board_media_retention_owner')
)
SELECT NOT EXISTS (
    SELECT 1 FROM required_functions required WHERE NOT EXISTS (
        SELECT 1 FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_roles r ON r.oid=p.proowner
        WHERE p.oid=required.oid AND r.rolname=required.owner_name
          AND p.prosecdef=required.definer AND p.prorettype=to_regtype(required.result_type)
          AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp']
          AND NOT (r.rolcanlogin OR r.rolsuper OR r.rolcreatedb OR r.rolcreaterole OR r.rolreplication OR r.rolbypassrls)
          AND NOT EXISTS(SELECT 1 FROM pg_catalog.pg_auth_members m WHERE m.member=r.oid)
          AND NOT EXISTS(SELECT 1 FROM unnest(required.callers) caller(name)
              WHERE NOT coalesce(has_function_privilege(caller.name,p.oid,'EXECUTE'),false))
          AND NOT EXISTS(SELECT 1 FROM pg_catalog.aclexplode(coalesce(p.proacl,pg_catalog.acldefault('f',p.proowner))) a
              WHERE a.privilege_type='EXECUTE' AND a.grantee<>p.proowner
                AND (a.is_grantable OR NOT EXISTS(SELECT 1 FROM pg_catalog.pg_roles allowed WHERE allowed.oid=a.grantee AND allowed.rolname=ANY(required.callers))))
    )
)
AND (SELECT count(*)=2 AND bool_and(relowner=(SELECT oid FROM pg_catalog.pg_roles WHERE rolname='board_migrator')) FROM private_tables)
AND NOT EXISTS (
    SELECT 1 FROM private_tables t CROSS JOIN restricted_roles r
    WHERE has_any_column_privilege(r.oid,t.oid,'SELECT,INSERT,UPDATE,REFERENCES')
       OR has_table_privilege(r.oid,t.oid,'DELETE,TRUNCATE,TRIGGER')
)
AND EXISTS (
    SELECT 1 FROM private_tables t WHERE t.relname='staff_attachment_handoffs'
      AND has_table_privilege('board_staff_post_owner',t.oid,'SELECT')
      AND has_table_privilege('board_staff_post_owner',t.oid,'INSERT')
      AND has_table_privilege('board_staff_post_owner',t.oid,'DELETE')
      AND NOT has_any_column_privilege('board_staff_post_owner',t.oid,'UPDATE,REFERENCES')
      AND NOT has_table_privilege('board_staff_post_owner',t.oid,'TRUNCATE,TRIGGER')
      AND NOT EXISTS(SELECT 1 FROM pg_catalog.aclexplode(coalesce(t.relacl,pg_catalog.acldefault('r',t.relowner))) a
          WHERE a.grantee<>t.relowner AND NOT (a.grantee=(SELECT oid FROM pg_catalog.pg_roles WHERE rolname='board_staff_post_owner')
              AND a.privilege_type IN ('SELECT','INSERT','DELETE') AND NOT a.is_grantable))
)
AND NOT EXISTS (
    SELECT 1 FROM (VALUES ('post_id','bigint'),('board','text'),('thread_id','bigint'),('job_id','text'),
        ('capability_hash','bytea'),('spoiler','boolean'),('authorized_limits','boolean'),('account_id','bigint'),
        ('session_hash','bytea'),('idle_seconds','integer'),('expires_at','timestamptz'),('transaction_id','bigint')) required(column_name,type_name)
    WHERE NOT EXISTS(SELECT 1 FROM pg_catalog.pg_attribute a JOIN private_tables t ON t.oid=a.attrelid
        WHERE t.relname='staff_attachment_handoffs' AND a.attname=required.column_name AND a.attnum>0
          AND NOT a.attisdropped AND a.attnotnull AND a.atttypid=to_regtype(required.type_name))
)
AND (SELECT count(*)=12 FROM pg_catalog.pg_attribute a JOIN private_tables t ON t.oid=a.attrelid
    WHERE t.relname='staff_attachment_handoffs' AND a.attnum>0 AND NOT a.attisdropped)
AND EXISTS (
    SELECT 1 FROM pg_catalog.pg_constraint k JOIN private_tables t ON t.oid=k.conrelid
    JOIN pg_catalog.pg_attribute a ON a.attrelid=t.oid AND a.attname='post_id'
    WHERE t.relname='staff_attachment_handoffs' AND k.contype='p' AND k.conkey=ARRAY[a.attnum]
)
AND NOT EXISTS (
    SELECT 1 FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace
    CROSS JOIN restricted_roles r WHERE n.nspname='staff_identity' AND c.relkind IN ('r','v','m','p')
      AND r.rolname<>'board_auth'
      AND (has_any_column_privilege(r.oid,c.oid,'SELECT,INSERT,UPDATE,REFERENCES')
        OR has_table_privilege(r.oid,c.oid,'DELETE,TRUNCATE,TRIGGER'))
)
AND NOT EXISTS (
    SELECT 1 FROM (VALUES ('attachment_job','text'),('attachment_capability_hash','bytea'),('attachment_spoiler','boolean')) required(column_name,type_name)
    WHERE NOT EXISTS(SELECT 1 FROM pg_catalog.pg_attribute a JOIN private_tables t ON t.oid=a.attrelid
        WHERE t.relname='staff_post_intents' AND a.attname=required.column_name AND a.attnum>0 AND NOT a.attisdropped
          AND NOT a.attnotnull AND a.atttypid=to_regtype(required.type_name)
          AND has_column_privilege('board_staff_post_owner',t.oid,a.attnum,'SELECT')
          AND has_column_privilege('board_staff_post_owner',t.oid,a.attnum,'UPDATE'))
)
AND NOT EXISTS (
    SELECT 1 FROM (VALUES
        ('content.posts','attach_staff_post_receipt','content.attach_staff_post_receipt()',false),
        ('post_secrets.staff_attachment_handoffs','reject_orphan_staff_attachment','content.reject_orphan_staff_attachment()',true)
    ) required(relation_name,trigger_name,function_name,deferred)
    WHERE NOT EXISTS(SELECT 1 FROM pg_catalog.pg_trigger t
        JOIN pg_catalog.pg_class c ON c.oid=t.tgrelid
        JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace
        WHERE n.nspname=split_part(required.relation_name,'.',1) AND c.relname=split_part(required.relation_name,'.',2)
          AND t.tgname=required.trigger_name
          AND t.tgfoid=(SELECT f.oid FROM required_functions f WHERE f.signature=required.function_name)
          AND t.tgtype=5 AND t.tgenabled='O'
          AND NOT t.tgisinternal AND t.tgqual IS NULL AND t.tgnargs=0
          AND t.tgdeferrable=required.deferred AND t.tginitdeferred=required.deferred
          AND (t.tgconstraint<>0)=required.deferred)
)
AND NOT has_schema_privilege('board_staff_post_owner','media','USAGE')
AND NOT has_schema_privilege('board_staff_post_owner','media_intake','USAGE')
AND NOT has_any_column_privilege('board_staff_post_owner',(SELECT c.oid FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='content' AND c.relname='posts'),'INSERT,UPDATE')
AND NOT has_table_privilege('board_staff_post_owner',(SELECT c.oid FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='content' AND c.relname='posts'),'DELETE')
AND NOT EXISTS (
    SELECT 1 FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace
    WHERE n.nspname IN ('media','media_intake') AND c.relkind IN ('r','v','m','p')
      AND (has_any_column_privilege('board_staff_post_owner',c.oid,'SELECT,INSERT,UPDATE,REFERENCES')
        OR has_table_privilege('board_staff_post_owner',c.oid,'DELETE,TRUNCATE,TRIGGER'))
)
AND NOT EXISTS (
    SELECT 1 FROM pg_catalog.pg_namespace n CROSS JOIN restricted_roles r
    WHERE n.nspname='staff_identity' AND r.rolname IN ('board_attachment_owner','board_media_intake_owner','board_media_retention_owner')
      AND has_schema_privilege(r.oid,n.oid,'USAGE')
)
AND NOT EXISTS (
    SELECT 1 FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace
    CROSS JOIN restricted_roles r
    WHERE r.rolname IN ('board_public','board_staff','board_auth')
      AND (n.nspname IN ('media','media_intake') OR (n.nspname='content' AND c.relname='post_media'))
      AND c.relkind IN ('r','v','m','p')
      AND (has_any_column_privilege(r.oid,c.oid,'SELECT,INSERT,UPDATE,REFERENCES')
        OR has_table_privilege(r.oid,c.oid,'DELETE,TRUNCATE,TRIGGER'))
)
AND NOT EXISTS (
    SELECT 1 FROM pg_catalog.pg_namespace n CROSS JOIN pg_catalog.pg_roles r
    WHERE n.nspname IN ('content','staff_identity','post_secrets','media','media_intake')
      AND r.rolname IN ('board_staff_post_owner','board_attachment_owner') AND has_schema_privilege(r.oid,n.oid,'CREATE')
)"#;

pub fn token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}
pub fn hash(value: &str) -> Vec<u8> {
    Sha256::digest(value.as_bytes()).to_vec()
}
pub fn cookie(headers: &HeaderMap, name: &str) -> Result<String, AppError> {
    let mut values = headers
        .get_all("cookie")
        .iter()
        .filter_map(|h| h.to_str().ok())
        .flat_map(|s| s.split(';'))
        .filter_map(|s| s.trim().split_once('='))
        .filter(|(k, _)| *k == name)
        .map(|(_, v)| v);
    let value = values.next().ok_or(AppError::Unauthorized)?;
    if values.next().is_some()
        || value.len() != 43
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(AppError::Unauthorized);
    }
    Ok(value.to_owned())
}
pub fn origin(headers: &HeaderMap, expected: &str) -> Result<(), AppError> {
    if headers.get_all("origin").iter().count() != 1
        || headers.get("origin").and_then(|v| v.to_str().ok()) != Some(expected)
        || headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()) != Some("same-origin")
    {
        return Err(AppError::Forbidden);
    }
    Ok(())
}
#[derive(sqlx::FromRow)]
pub struct Session {
    pub account_id: i64,
    pub role: String,
    pub csrf_hash: Vec<u8>,
    pub recent: bool,
    #[sqlx(flatten)]
    pub permissions: Permissions,
}

impl Session {
    pub fn at_least(&self, level: Level) -> bool {
        Level::parse(&self.role).is_some_and(|current| current >= level)
    }
}

pub struct Authority {
    pub session: Session,
    transaction: sqlx::Transaction<'static, sqlx::Postgres>,
    token_hash: Vec<u8>,
    idle_seconds: i32,
}

impl Authority {
    pub(crate) fn connection(&mut self) -> &mut sqlx::PgConnection {
        &mut self.transaction
    }

    /// Account and session locks stay held while the content transaction runs.
    /// Recheck the clock after content locks, before making its changes durable.
    pub async fn ensure_current(&mut self, require_recent: bool) -> Result<(), AppError> {
        let status: Option<(bool,bool)> = sqlx::query_as(
            "SELECT s.expires_at>clock_timestamp() AND s.last_activity_at>clock_timestamp()-make_interval(secs=>$2) \
             AND a.revoked_at IS NULL AND a.role IN ('janitor','moderator','manager','admin'), \
             s.authenticated_at>clock_timestamp()-interval '10 minutes' \
             FROM staff_identity.sessions s JOIN staff_identity.accounts a ON a.id=s.account_id \
             WHERE s.token_hash=$1",
        ).bind(&self.token_hash).bind(self.idle_seconds).fetch_optional(&mut *self.transaction).await?;
        let (live, recent) = status.ok_or(AppError::Unauthorized)?;
        if !live {
            return Err(AppError::Unauthorized);
        }
        if require_recent && !recent {
            return Err(AppError::Recent);
        }
        Ok(())
    }

    pub async fn finish(self) -> Result<Session, AppError> {
        self.transaction.commit().await?;
        Ok(self.session)
    }
}

pub async fn guard(state: &AppState, headers: &HeaderMap) -> Result<Authority, AppError> {
    let value = cookie(headers, state.config.cookie_name())?;
    let token_hash = hash(&value);
    let idle_seconds = state.config.idle_timeout.as_secs() as i32;
    let mut transaction = state.auth.begin().await?;
    let session: Session = sqlx::query_as("SELECT * FROM staff_identity.lock_session($1,$2)")
        .bind(&token_hash)
        .bind(idle_seconds)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(AppError::Unauthorized)?;
    Ok(Authority {
        session,
        transaction,
        token_hash,
        idle_seconds,
    })
}

pub async fn session(state: &AppState, headers: &HeaderMap) -> Result<Session, AppError> {
    guard(state, headers).await?.finish().await
}
pub fn csrf(session: &Session, value: &str) -> Result<(), AppError> {
    if value.len() != 43 || hash(value) != session.csrf_hash {
        return Err(AppError::Forbidden);
    }
    Ok(())
}
pub async fn check_identity(pool: &PgPool, expected: &str) -> Result<(), AppError> {
    let (name, privileged): (String,bool) = sqlx::query_as("SELECT current_user::text,rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls FROM pg_roles WHERE rolname=current_user").fetch_one(pool).await?;
    if name != expected || privileged {
        return Err(AppError::Forbidden);
    }
    let authority: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_auth_members WHERE member=(SELECT oid FROM pg_roles WHERE rolname=current_user)) OR EXISTS (SELECT 1 FROM pg_database WHERE datname=current_database() AND datdba=(SELECT oid FROM pg_roles WHERE rolname=current_user)) OR EXISTS (SELECT 1 FROM pg_namespace WHERE nspowner=(SELECT oid FROM pg_roles WHERE rolname=current_user)) OR has_schema_privilege(current_user,'deployment','USAGE') OR has_schema_privilege(current_user,'post_secrets','USAGE')").fetch_one(pool).await?;
    if authority {
        return Err(AppError::Forbidden);
    }
    let cross: bool = sqlx::query_scalar("SELECT CASE WHEN current_user='board_auth' THEN has_schema_privilege(current_user,'content','USAGE') ELSE has_schema_privilege(current_user,'staff_identity','USAGE') END").fetch_one(pool).await?;
    if cross {
        return Err(AppError::Forbidden);
    }
    let attachments: bool = sqlx::query_scalar(STAFF_ATTACHMENT_READY_SQL)
        .fetch_one(pool)
        .await?;
    if !attachments {
        return Err(AppError::Forbidden);
    }
    let posting: bool = match expected {
        "board_auth" => {
            sqlx::query("SELECT public_capcode,allow_boards,deny_boards,flags FROM staff_identity.accounts LIMIT 0")
                .execute(pool)
                .await?;
            let source:bool=sqlx::query_scalar("SELECT bool_and(coalesce(has_function_privilege(current_user,to_regprocedure(signature),'EXECUTE'),false) AND coalesce((SELECT prorettype='boolean'::regtype FROM pg_proc WHERE oid=to_regprocedure(signature)),false)) FROM (VALUES
                ('staff_identity.issue_source_post_authority(bytea,bytea,bytea,integer,boolean,bigint,text,bigint,text,text,text,timestamptz,boolean,integer,bytea,text,text,text,boolean,boolean)'),
                ('staff_identity.issue_limited_post_authority(bytea,bytea,bytea,integer,boolean,bigint,text,bigint,text,text,text,timestamptz,boolean,integer,bytea,text,boolean)'),
                ('staff_identity.issue_ordinary_post_authority(bytea,bytea,bytea,integer,bigint,text,bigint,text,text,text,timestamptz,boolean,integer,bytea,text,text,text,boolean,jsonb,boolean)')) AS required(signature)")
                .fetch_one(pool).await?;
            if !source {
                return Err(AppError::Forbidden);
            }
            sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname='staff_identity' AND ((p.proname IN ('issue_post_authority','issue_wordfiltered_post_authority')) OR (p.proname='issue_limited_post_authority' AND p.pronargs=16) OR (p.proname IN ('issue_source_post_authority','issue_ordinary_post_authority') AND p.pronargs=19)) AND has_function_privilege(current_user,p.oid,'EXECUTE')) AND coalesce(has_function_privilege(current_user,to_regprocedure('staff_identity.lock_session(bytea,integer)'),'EXECUTE'),false) AND NOT has_column_privilege(current_user,'staff_identity.accounts','allow_boards','UPDATE') AND NOT has_column_privilege(current_user,'staff_identity.accounts','deny_boards','UPDATE') AND NOT has_column_privilege(current_user,'staff_identity.accounts','flags','UPDATE') AND NOT EXISTS(SELECT 1 FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname='content' AND p.proname='consume_staff_post_authority' AND has_function_privilege(current_user,p.oid,'EXECUTE'))").fetch_one(pool).await?
        }
        "board_staff" => {
            sqlx::query("SELECT capcode FROM content.posts LIMIT 0")
                .execute(pool)
                .await?;
            sqlx::query("SELECT id FROM content.visible_threads LIMIT 0")
                .execute(pool)
                .await?;
            let source:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname='staff_identity' AND p.proname='issue_source_post_authority' AND p.pronargs=20 AND p.prorettype='boolean'::regtype) AND NOT EXISTS(SELECT 1 FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname='staff_identity' AND p.proname IN ('issue_post_authority','issue_limited_post_authority','issue_source_post_authority','issue_wordfiltered_post_authority','issue_ordinary_post_authority','issue_source_attachment_post_authority','issue_ordinary_attachment_post_authority') AND has_function_privilege(current_user,p.oid,'EXECUTE'))")
                .fetch_one(pool).await?;
            if !source {
                return Err(AppError::Forbidden);
            }
            sqlx::query_scalar("SELECT has_function_privilege(current_user,'content.consume_staff_post_authority(bytea,bigint,text,bigint,text,text,text,timestamptz)','EXECUTE') AND NOT EXISTS(SELECT 1 FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname='staff_identity' AND p.proname='issue_post_authority' AND has_function_privilege(current_user,p.oid,'EXECUTE'))").fetch_one(pool).await?
        }
        _ => false,
    };
    if !posting {
        return Err(AppError::Forbidden);
    }
    Ok(())
}
