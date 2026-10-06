//! Private, report-bound IP admission. GET is advisory; POST inserts report and
//! membership atomically under the database admission gate, then registers any
//! anonymous-session activity in that same transaction.
use crate::StoreError;
use board_domain::poster_id::PublicReportRateIdentity;
use sqlx::{PgConnection, PgPool};

/// Does not reserve capacity, mutate anonymous activity, or acquire row locks.
pub async fn check(
    pool: &PgPool,
    slug: &str,
    id: i64,
    identity: &PublicReportRateIdentity,
) -> Result<(), StoreError> {
    sqlx::query("SELECT content.check_report_admission($1,$2,$3)")
        .bind(slug)
        .bind(id)
        .bind(identity.as_bytes().as_slice())
        .execute(pool)
        .await
        .map_err(admission_error)?;
    Ok(())
}

/// Caller starts READ COMMITTED before its first query. The SQL function
/// reenters the board lock, takes the global gate, and captures database time
/// after contention. Keep the transaction open for anonymous activity and
/// commit so failure there also rolls back both report and membership.
pub(crate) async fn admit_on(
    connection: &mut PgConnection,
    slug: &str,
    id: i64,
    reason: &str,
    identity: &PublicReportRateIdentity,
) -> Result<i64, StoreError> {
    sqlx::query_scalar("SELECT content.admit_report($1,$2,$3,$4)")
        .bind(slug)
        .bind(id)
        .bind(reason)
        .bind(identity.as_bytes().as_slice())
        .fetch_one(connection)
        .await
        .map_err(admission_error)
}

fn admission_error(error: sqlx::Error) -> StoreError {
    if let Some(database) = error.as_database_error() {
        match database.code().as_deref() {
            Some("P0002") => return StoreError::NotFound,
            Some("P0094") => {
                return StoreError::Database(sqlx::Error::Protocol(
                    "Report admission capacity is unavailable.".into(),
                ));
            }
            Some("P0001") => {
                let message = match database.message() {
                    "You cannot report posts on this board." => {
                        "You cannot report posts on this board."
                    }
                    "Error: You cannot report a sticky." => "Error: You cannot report a sticky.",
                    "Error: You cannot report this post." => "Error: You cannot report this post.",
                    "You have already reported this post." => {
                        "You have already reported this post."
                    }
                    "You have to wait a while before reporting another post." => {
                        "You have to wait a while before reporting another post."
                    }
                    _ => return StoreError::Database(error),
                };
                return StoreError::Invalid(message);
            }
            Some("22023")
                if database.message() == "Report reason must contain 1 to 1000 bytes." =>
            {
                return StoreError::Invalid("Report reason must contain 1 to 1000 bytes.");
            }
            _ => {}
        }
    }
    StoreError::Database(error)
}

/// Catalog-only readiness contract shared by public and staff services.
pub const READINESS_SQL: &str = r#"WITH owner_role AS (
    SELECT r.oid FROM pg_catalog.pg_roles r
    WHERE r.rolname='board_report_admission_owner'
      AND NOT (r.rolcanlogin OR r.rolsuper OR r.rolcreatedb OR r.rolcreaterole OR r.rolreplication OR r.rolbypassrls)
), relations AS (
    SELECT c.oid,c.relowner,n.nspname,c.relname
    FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace
    WHERE (n.nspname='post_secrets' AND c.relname IN ('report_membership','report_admission_gate'))
       OR (n.nspname='content' AND c.relname IN ('reports','reports_id_seq','boards','posts','threads'))
)
SELECT EXISTS (SELECT 1 FROM owner_role)
AND NOT EXISTS (
    SELECT 1 FROM pg_catalog.pg_roles runtime CROSS JOIN owner_role r
    WHERE runtime.rolname IN ('board_public','board_staff','board_auth')
      AND pg_has_role(runtime.oid,r.oid,'MEMBER')
)
AND NOT EXISTS (
    SELECT 1 FROM (VALUES
        ('content.check_report_admission(text,bigint,bytea)','void'),
        ('content.admit_report(text,bigint,text,bytea)','bigint')
    ) AS required(signature,result_type)
    WHERE NOT EXISTS (
        SELECT 1 FROM pg_catalog.pg_proc p JOIN owner_role r ON r.oid=p.proowner
        WHERE p.oid=to_regprocedure(required.signature)
          AND p.prokind='f' AND p.prosecdef AND p.provolatile='v'
          AND p.prorettype=to_regtype(required.result_type)
          AND EXISTS (SELECT 1 FROM unnest(p.proconfig) AS config(value)
              WHERE replace(config.value,' ','')='search_path=pg_catalog,pg_temp')
          AND has_function_privilege(current_user,p.oid,'EXECUTE')
          AND has_function_privilege('board_public',p.oid,'EXECUTE')
          AND has_function_privilege('board_staff',p.oid,'EXECUTE')
          AND NOT has_function_privilege('board_auth',p.oid,'EXECUTE')
          AND NOT EXISTS (SELECT 1 FROM pg_catalog.aclexplode(coalesce(p.proacl,pg_catalog.acldefault('f',p.proowner))) a
              WHERE a.privilege_type='EXECUTE' AND a.grantee NOT IN
                  (p.proowner,(SELECT oid FROM pg_catalog.pg_roles WHERE rolname='board_public'),
                   (SELECT oid FROM pg_catalog.pg_roles WHERE rolname='board_staff')))
    )
)
AND NOT EXISTS (
    SELECT 1 FROM (VALUES
        ('report_target','bigint',false,'25 20'),
        ('check_report_limits','void',false,'25 20 17 1184'),
        ('retire_deleted_report_membership','trigger',false,''),
        ('retire_staff_file_report_membership','void',true,'25 20')
    ) AS required(function_name,result_type,attachment_authority,argument_types)
    -- Built-in argument type OIDs avoid resolving names in a private schema.
    -- Staff readiness must not require a new post_secrets USAGE grant.
    WHERE NOT EXISTS (
        SELECT 1 FROM pg_catalog.pg_proc p JOIN owner_role r ON r.oid=p.proowner
        JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace
        WHERE n.nspname='post_secrets' AND p.proname=required.function_name
          AND p.proargtypes=required.argument_types::oidvector
          AND p.prokind='f' AND p.prosecdef AND p.provolatile='v'
          AND p.prorettype=to_regtype(required.result_type)
          AND EXISTS (SELECT 1 FROM unnest(p.proconfig) AS config(value)
              WHERE replace(config.value,' ','')='search_path=pg_catalog,pg_temp')
          AND has_function_privilege(r.oid,p.oid,'EXECUTE')
          AND NOT has_function_privilege(current_user,p.oid,'EXECUTE')
          AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_roles runtime
              WHERE runtime.rolname IN ('board_public','board_staff','board_auth')
                AND has_function_privilege(runtime.oid,p.oid,'EXECUTE'))
          AND (NOT required.attachment_authority OR EXISTS (
              SELECT 1 FROM pg_catalog.pg_roles attachment
              WHERE attachment.rolname='board_attachment_owner'
                AND has_function_privilege(attachment.oid,p.oid,'EXECUTE')))
          AND NOT EXISTS (SELECT 1 FROM pg_catalog.aclexplode(coalesce(p.proacl,pg_catalog.acldefault('f',p.proowner))) a
              WHERE a.privilege_type='EXECUTE' AND a.grantee<>p.proowner
                AND NOT (required.attachment_authority AND NOT a.is_grantable AND EXISTS (
                    SELECT 1 FROM pg_catalog.pg_roles attachment
                    WHERE attachment.rolname='board_attachment_owner' AND attachment.oid=a.grantee)))
    )
)
AND NOT EXISTS (
    SELECT 1 FROM (VALUES
        ('posts','retire_deleted_post_report_membership'),
        ('threads','retire_deleted_thread_report_membership')
    ) AS required(table_name,trigger_name)
    WHERE NOT EXISTS (
        SELECT 1 FROM pg_catalog.pg_trigger t
        JOIN relations c ON c.oid=t.tgrelid
        JOIN pg_catalog.pg_proc p ON p.oid=t.tgfoid
        JOIN owner_role r ON r.oid=p.proowner
        WHERE c.nspname='content' AND c.relname=required.table_name
          AND t.tgname=required.trigger_name AND t.tgtype=17
          AND t.tgenabled='O' AND NOT t.tgisinternal
          AND t.tgnargs=0 AND octet_length(t.tgargs)=0 AND t.tgconstraint=0
          AND NOT t.tgdeferrable AND NOT t.tginitdeferred
          AND t.tgoldtable IS NULL AND t.tgnewtable IS NULL
          AND p.proname='retire_deleted_report_membership'
          AND p.pronamespace=(SELECT oid FROM pg_catalog.pg_namespace WHERE nspname='post_secrets')
          AND p.pronargs=0 AND p.prokind='f' AND p.prorettype='pg_catalog.trigger'::regtype
          AND p.prosecdef AND p.provolatile='v'
          AND EXISTS (SELECT 1 FROM unnest(p.proconfig) AS config(value)
              WHERE replace(config.value,' ','')='search_path=pg_catalog,pg_temp')
          AND t.tgqual IS NOT NULL
          AND t.tgattr::text=(SELECT a.attnum::text FROM pg_catalog.pg_attribute a
              WHERE a.attrelid=c.oid AND a.attname='deleted' AND NOT a.attisdropped)
          AND lower(translate(split_part(split_part(pg_catalog.pg_get_triggerdef(t.oid,false),' WHEN ',2),' EXECUTE ',1),' ()',''))
              ='notold.deletedandnew.deleted'
    )
)
AND NOT EXISTS (
    SELECT 1 FROM (VALUES ('report_membership'),('report_admission_gate')) AS required(table_name)
    WHERE NOT EXISTS (
        SELECT 1 FROM relations c CROSS JOIN owner_role r
        WHERE c.nspname='post_secrets' AND c.relname=required.table_name AND c.relowner=r.oid
          AND NOT has_table_privilege(current_user,c.oid,'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
          AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_roles runtime
              WHERE runtime.rolname IN ('board_public','board_staff','board_auth')
                AND (has_table_privilege(runtime.oid,c.oid,'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
                    OR EXISTS (SELECT 1 FROM pg_catalog.pg_attribute a
                        WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped
                          AND has_column_privilege(runtime.oid,c.oid,a.attnum,'SELECT,INSERT,UPDATE,REFERENCES'))))
    )
)
AND EXISTS (
    SELECT 1 FROM relations c CROSS JOIN owner_role r
    WHERE c.nspname='content' AND c.relname='reports'
      AND NOT has_table_privilege('board_public',c.oid,'INSERT')
      AND NOT has_table_privilege(current_user,c.oid,'INSERT')
      AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_attribute a
          WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped
            AND (has_column_privilege('board_public',c.oid,a.attnum,'INSERT')
                OR has_column_privilege(current_user,c.oid,a.attnum,'INSERT')))
)
AND EXISTS (
    SELECT 1 FROM relations c CROSS JOIN owner_role r
    WHERE c.nspname='content' AND c.relname='reports_id_seq'
      AND has_sequence_privilege(r.oid,to_regclass('content.reports_id_seq'),'USAGE')
      AND NOT has_sequence_privilege('board_public',to_regclass('content.reports_id_seq'),'USAGE,SELECT,UPDATE')
      AND NOT has_sequence_privilege(current_user,to_regclass('content.reports_id_seq'),'USAGE,SELECT,UPDATE')
)
AND NOT EXISTS (
    SELECT 1 FROM (VALUES
        ('boards','slug','SELECT'),('boards','slug','UPDATE'),('boards','staff_only','SELECT'),
        ('boards','can_report_posts','SELECT'),('boards','archive_retention_seconds','SELECT'),
        ('posts','id','SELECT'),('posts','board','SELECT'),('posts','thread_id','SELECT'),
        ('posts','deleted','SELECT'),('posts','capcode','SELECT'),
        ('threads','id','SELECT'),('threads','board','SELECT'),('threads','deleted','SELECT'),
        ('threads','sticky','SELECT'),('threads','archived_at','SELECT'),('threads','archive_expires_at','SELECT'),
        ('reports','id','SELECT'),('reports','board','INSERT'),('reports','post_id','INSERT'),
        ('reports','reason','INSERT'),('reports','created_at','INSERT')
    ) AS required(table_name,column_name,privilege_name)
    WHERE NOT EXISTS (
        SELECT 1 FROM relations c CROSS JOIN owner_role r
        JOIN pg_catalog.pg_attribute a ON a.attname=required.column_name AND NOT a.attisdropped
        WHERE c.nspname='content' AND c.relname=required.table_name AND a.attrelid=c.oid
          AND has_column_privilege(r.oid,c.oid,a.attnum,required.privilege_name)
    )
)
AND EXISTS (
    SELECT 1 FROM relations c
    JOIN pg_catalog.pg_attribute a ON a.attrelid=c.oid
    JOIN pg_catalog.pg_attrdef d ON d.adrelid=c.oid AND d.adnum=a.attnum
    WHERE c.nspname='post_secrets' AND c.relname='report_admission_gate'
      AND a.attname='membership_limit' AND a.attnum>0 AND NOT a.attisdropped
      AND a.atttypid='pg_catalog.int4'::regtype AND a.attnotnull
      AND pg_catalog.pg_get_expr(d.adbin,d.adrelid)='100000'
      AND EXISTS (SELECT 1 FROM pg_catalog.pg_constraint k
          WHERE k.conrelid=c.oid AND k.contype='c' AND k.convalidated
            AND k.conkey=ARRAY[a.attnum]::smallint[]
            AND lower(translate(pg_catalog.pg_get_expr(k.conbin,k.conrelid),' ()',''))
                ='membership_limit>=1andmembership_limit<=1000000')
)
AND NOT EXISTS (
    SELECT 1 FROM (VALUES ('content'),('post_secrets')) AS required(schema_name)
    WHERE NOT EXISTS (
        SELECT 1 FROM pg_catalog.pg_namespace n CROSS JOIN owner_role r
        WHERE n.nspname=required.schema_name AND has_schema_privilege(r.oid,n.oid,'USAGE')
          AND NOT has_schema_privilege(r.oid,n.oid,'CREATE')
    )
)"#;
