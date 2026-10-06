//! Private, report-bound IP and anonymous-session admission. GET is advisory;
//! POST atomically inserts the report, membership, and anonymous activity.
use crate::{StoreError, anonymous_session::PostingSession};
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

/// Read-only admission context from an already verified capability. An absent
/// token checks only the transport identity and never creates session state.
pub async fn check_with_session(
    pool: &PgPool,
    slug: &str,
    id: i64,
    identity: &PublicReportRateIdentity,
    token: Option<&[u8; 32]>,
    request_at: i64,
) -> Result<(), StoreError> {
    sqlx::query("SELECT content.check_report_admission($1,$2,$3,$4,$5)")
        .bind(slug)
        .bind(id)
        .bind(identity.as_bytes().as_slice())
        .bind(token.map(|token| token.as_slice()))
        .bind(request_at)
        .execute(pool)
        .await
        .map_err(admission_error)?;
    Ok(())
}

/// Caller starts READ COMMITTED before its first query. The SQL function
/// reenters the board lock, takes the global gate, and captures database time
/// after contention. This legacy IP-only path is executable only by staff.
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

/// Session resolution, quotas, report insertion, and activity registration all
/// run inside the single SQL admission boundary, including for new sessions.
pub(crate) async fn admit_with_session_on(
    connection: &mut PgConnection,
    slug: &str,
    id: i64,
    reason: &str,
    identity: &PublicReportRateIdentity,
    session: PostingSession,
) -> Result<i64, StoreError> {
    let fingerprints = session.fingerprints;
    sqlx::query_scalar("SELECT content.admit_report($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
        .bind(slug)
        .bind(id)
        .bind(reason)
        .bind(identity.as_bytes().as_slice())
        .bind(fingerprints.token.as_slice())
        .bind(fingerprints.network.as_slice())
        .bind(fingerprints.address.as_slice())
        .bind(fingerprints.environment.as_slice())
        .bind(session.minted)
        .bind(session.now.timestamp())
        .fetch_one(connection)
        .await
        .map_err(admission_error)
}

/// Category resolution and captured metadata are owned by the SQL boundary.
/// The caller supplies only an ID and the advisory form's expected revision.
pub(crate) async fn admit_categorical_with_session_on(
    connection: &mut PgConnection,
    slug: &str,
    id: i64,
    category_id: i64,
    expected_revision: i64,
    identity: &PublicReportRateIdentity,
    session: PostingSession,
) -> Result<i64, StoreError> {
    let fingerprints = session.fingerprints;
    sqlx::query_scalar(
        "SELECT content.admit_categorical_report($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
    )
    .bind(slug)
    .bind(id)
    .bind(category_id)
    .bind(expected_revision)
    .bind(identity.as_bytes().as_slice())
    .bind(fingerprints.token.as_slice())
    .bind(fingerprints.network.as_slice())
    .bind(fingerprints.address.as_slice())
    .bind(fingerprints.environment.as_slice())
    .bind(session.minted)
    .bind(session.now.timestamp())
    .fetch_one(connection)
    .await
    .map_err(admission_error)
}

pub(crate) fn admission_error(error: sqlx::Error) -> StoreError {
    if let Some(database) = error.as_database_error() {
        match database.code().as_deref() {
            Some("P0002") => return StoreError::NotFound,
            Some("28000") => return StoreError::AuthorizationChanged,
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
                    "Free-text reporting is not active." => "Free-text reporting is not active.",
                    "Categorical reporting is not active." => {
                        "Categorical reporting is not active."
                    }
                    "Report categories changed. Please reload the report form." => {
                        "Report categories changed. Please reload the report form."
                    }
                    "Invalid category selected." => "Invalid category selected.",
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
    WHERE (n.nspname='post_secrets' AND c.relname IN ('report_membership','report_admission_gate','report_catalog_gate','report_catalog_versions','report_catalog_rows'))
       OR (n.nspname='content' AND c.relname IN ('reports','reports_id_seq','boards','posts','threads','post_media'))
)
SELECT EXISTS (SELECT 1 FROM owner_role)
AND NOT EXISTS (
    SELECT 1 FROM pg_catalog.pg_roles runtime CROSS JOIN owner_role r
    WHERE runtime.rolname IN ('board_public','board_staff','board_auth')
      AND pg_has_role(runtime.oid,r.oid,'MEMBER')
)
AND NOT EXISTS (
    SELECT 1 FROM (VALUES
        ('content.check_report_admission(text,bigint,bytea)','void',true,true,false),
        ('content.check_report_admission(text,bigint,bytea,bytea,bigint)','void',true,true,false),
        ('content.admit_report(text,bigint,text,bytea)','bigint',false,true,false),
        ('content.admit_report(text,bigint,text,bytea,bytea,bytea,bytea,bytea,boolean,bigint)','bigint',true,false,false),
        ('content.report_category_form(text,bigint)','jsonb',true,true,true),
        ('content.admit_categorical_report(text,bigint,bigint,bigint,bytea,bytea,bytea,bytea,bytea,boolean,bigint)','bigint',true,false,true),
        ('content.set_report_catalog_active(bigint)','void',false,false,true)
    ) AS required(signature,result_type,public_allowed,staff_allowed,migrator_allowed)
    WHERE NOT EXISTS (
        SELECT 1 FROM pg_catalog.pg_proc p JOIN owner_role r ON r.oid=p.proowner
        WHERE p.oid=to_regprocedure(required.signature)
          AND p.prokind='f' AND p.prosecdef AND p.provolatile='v' AND NOT p.proretset
          AND p.prorettype=to_regtype(required.result_type)
          AND p.pronargdefaults=0 AND p.provariadic=0
          AND cardinality(p.proconfig)=1
          AND replace(p.proconfig[1],' ','')='search_path=pg_catalog,pg_temp'
          AND has_function_privilege(current_user,p.oid,'EXECUTE')
              = ((current_user='board_public' AND required.public_allowed)
                  OR (current_user='board_staff' AND required.staff_allowed))
          AND has_function_privilege('board_public',p.oid,'EXECUTE')=required.public_allowed
          AND has_function_privilege('board_staff',p.oid,'EXECUTE')=required.staff_allowed
          AND NOT has_function_privilege('board_auth',p.oid,'EXECUTE')
          AND has_function_privilege('board_migrator',p.oid,'EXECUTE')=required.migrator_allowed
          AND NOT EXISTS (SELECT 1 FROM pg_catalog.aclexplode(coalesce(p.proacl,pg_catalog.acldefault('f',p.proowner))) a
              WHERE a.privilege_type='EXECUTE' AND (a.is_grantable OR a.grantee NOT IN
                  (p.proowner,CASE WHEN required.public_allowed THEN (SELECT oid FROM pg_catalog.pg_roles WHERE rolname='board_public') ELSE p.proowner END,
                   CASE WHEN required.staff_allowed THEN (SELECT oid FROM pg_catalog.pg_roles WHERE rolname='board_staff') ELSE p.proowner END,
                   CASE WHEN required.migrator_allowed THEN (SELECT oid FROM pg_catalog.pg_roles WHERE rolname='board_migrator') ELSE p.proowner END)))
    )
)
AND NOT EXISTS (
    SELECT 1 FROM (VALUES
        ('report_target','bigint',false,'25 20'),
        ('report_target','bigint',false,'25 20 1184'),
        ('check_report_limits','void',false,'25 20 17 1184'),
        ('check_report_limits','void',false,'25 20 17 2950 1184'),
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
          AND p.prokind='f' AND p.prosecdef AND p.provolatile='v' AND NOT p.proretset
          AND p.prorettype=to_regtype(required.result_type)
          AND p.pronargdefaults=0 AND p.provariadic=0
          AND cardinality(p.proconfig)=1
          AND replace(p.proconfig[1],' ','')='search_path=pg_catalog,pg_temp'
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
    SELECT 1 FROM (VALUES ('report_membership'),('report_admission_gate'),('report_catalog_gate'),('report_catalog_versions'),('report_catalog_rows')) AS required(table_name)
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
      AND NOT has_table_privilege(current_user,c.oid,'INSERT')
      AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_attribute a
          WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped
            AND has_column_privilege(current_user,c.oid,a.attnum,'INSERT'))
      AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_roles runtime
          WHERE runtime.rolname IN ('board_public','board_staff','board_auth')
            AND (has_table_privilege(runtime.oid,c.oid,'INSERT')
                OR EXISTS (SELECT 1 FROM pg_catalog.pg_attribute a
                    WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped
                      AND has_column_privilege(runtime.oid,c.oid,a.attnum,'INSERT'))))
)
AND EXISTS (
    SELECT 1 FROM relations c CROSS JOIN owner_role r
    WHERE c.nspname='content' AND c.relname='reports_id_seq'
      AND has_sequence_privilege(r.oid,to_regclass('content.reports_id_seq'),'USAGE')
      AND NOT has_sequence_privilege(current_user,to_regclass('content.reports_id_seq'),'USAGE,SELECT,UPDATE')
      AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_roles runtime
          WHERE runtime.rolname IN ('board_public','board_staff','board_auth')
            AND has_sequence_privilege(runtime.oid,to_regclass('content.reports_id_seq'),'USAGE,SELECT,UPDATE'))
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
        ('reports','reason','INSERT'),('reports','created_at','INSERT'),
        ('reports','category_revision','INSERT'),('reports','category_id','INSERT'),
        ('reports','category_kind','INSERT'),('reports','category_base_weight','INSERT'),
        ('boards','worksafe','SELECT'),('post_media','post_id','SELECT'),
        ('post_media','bytes','SELECT'),('post_media','file_deleted','SELECT')
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
    SELECT 1 FROM (VALUES
        ('content','reports','category_revision','int8'),
        ('content','reports','category_id','int8'),
        ('content','reports','category_kind','int2'),
        ('content','reports','category_base_weight','float8'),
        ('post_secrets','report_admission_gate','active_catalog_revision','int8')
    ) AS required(schema_name,table_name,column_name,type_name)
    WHERE NOT EXISTS (
        SELECT 1 FROM relations c JOIN pg_catalog.pg_attribute a ON a.attrelid=c.oid
        WHERE c.nspname=required.schema_name AND c.relname=required.table_name
          AND a.attname=required.column_name AND a.attnum>0 AND NOT a.attisdropped
          AND a.atttypid=to_regtype('pg_catalog.'||required.type_name)
          AND a.atttypmod=-1 AND NOT a.attnotnull AND NOT a.atthasdef
          AND a.attgenerated='' AND a.attidentity=''
          AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_roles runtime
              WHERE runtime.rolname IN ('board_public','board_staff','board_auth')
                AND has_column_privilege(runtime.oid,c.oid,a.attnum,'UPDATE,REFERENCES'))
          AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_roles runtime
              WHERE runtime.rolname IN ('board_public','board_auth')
                AND has_column_privilege(runtime.oid,c.oid,a.attnum,'SELECT'))
    )
)
AND NOT EXISTS (
    SELECT 1 FROM (VALUES
        ('reports_category_complete',ARRAY['category_revision','category_id','category_kind','category_base_weight'],
         '(num_nonnulls(category_revision,category_id,category_kind,category_base_weight)=any(array[0,4]))'),
        ('reports_category_kind_check',ARRAY['category_kind','category_id'],
         '(category_kind=casewhen(category_id=31)then2else1end)'),
        ('reports_category_weight_check',ARRAY['category_base_weight'],
         '(category_base_weight<>all(array[''infinity''::doubleprecision,''-infinity''::doubleprecision,''nan''::doubleprecision]))'),
        ('reports_reason_check',ARRAY['category_revision','reason'],
         '(((category_revisionisnull)and((octet_length(reason)>=1)and(octet_length(reason)<=1000)))or((category_revisionisnotnull)and((octet_length(reason)>=0)and(octet_length(reason)<=4096))))')
    ) AS required(constraint_name,columns,expression)
    WHERE NOT EXISTS (
        SELECT 1 FROM relations c JOIN pg_catalog.pg_constraint k ON k.conrelid=c.oid
        WHERE c.nspname='content' AND c.relname='reports' AND k.conname=required.constraint_name
          AND k.contype='c' AND k.convalidated AND NOT k.connoinherit
          AND NOT k.condeferrable AND NOT k.condeferred
          AND k.conkey=ARRAY(SELECT a.attnum FROM unnest(required.columns) WITH ORDINALITY AS names(name,ordinal)
              JOIN pg_catalog.pg_attribute a ON a.attrelid=c.oid AND a.attname=names.name
                  AND a.attnum>0 AND NOT a.attisdropped ORDER BY names.ordinal)
          AND lower(regexp_replace(pg_catalog.pg_get_expr(k.conbin,k.conrelid),'[[:space:]]','','g'))
              =required.expression
    )
)
AND NOT EXISTS (
    SELECT 1 FROM (VALUES
        ('content','reports',ARRAY['category_revision','category_id'],
         'report_catalog_rows',ARRAY['revision','id'])
    ) AS required(schema_name,table_name,columns,referenced_table,referenced_columns)
    WHERE NOT EXISTS (
        SELECT 1 FROM relations c JOIN pg_catalog.pg_constraint k ON k.conrelid=c.oid
        JOIN relations referenced ON referenced.oid=k.confrelid
        WHERE c.nspname=required.schema_name AND c.relname=required.table_name
          AND referenced.nspname='post_secrets' AND referenced.relname=required.referenced_table
          AND k.contype='f' AND k.convalidated AND NOT k.condeferrable AND NOT k.condeferred
          AND k.confmatchtype='s' AND k.confupdtype='a' AND k.confdeltype='a'
          AND (SELECT count(*)=4 AND bool_and(t.tgisinternal AND t.tgenabled='O')
              FROM pg_catalog.pg_trigger t WHERE t.tgconstraint=k.oid)
          AND k.conkey=ARRAY(SELECT a.attnum FROM unnest(required.columns) WITH ORDINALITY AS names(name,ordinal)
              JOIN pg_catalog.pg_attribute a ON a.attrelid=c.oid AND a.attname=names.name
                  AND a.attnum>0 AND NOT a.attisdropped ORDER BY names.ordinal)
          AND k.confkey=ARRAY(SELECT a.attnum FROM unnest(required.referenced_columns) WITH ORDINALITY AS names(name,ordinal)
              JOIN pg_catalog.pg_attribute a ON a.attrelid=referenced.oid AND a.attname=names.name
                  AND a.attnum>0 AND NOT a.attisdropped ORDER BY names.ordinal)
    )
)
AND EXISTS (
    SELECT 1 FROM pg_catalog.pg_proc p JOIN owner_role r ON r.oid=p.proowner
    JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace
    WHERE n.nspname='post_secrets' AND p.proname='eligible_report_categories'
      AND p.proargtypes='25 20 20'::oidvector
      AND p.proallargtypes=ARRAY[25,20,20,20,25,21,701,23,23]::oid[]
      AND p.proargmodes=ARRAY['i','i','i','t','t','t','t','t','t']::"char"[]
      AND p.prokind='f' AND NOT p.prosecdef AND p.provolatile='s'
      AND p.prorettype='pg_catalog.record'::regtype AND p.proretset
      AND p.pronargdefaults=0 AND p.provariadic=0
      AND cardinality(p.proconfig)=1
      AND replace(p.proconfig[1],' ','')='search_path=pg_catalog,pg_temp'
      AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_roles runtime
          WHERE runtime.rolname IN ('board_public','board_staff','board_auth','board_migrator')
            AND has_function_privilege(runtime.oid,p.oid,'EXECUTE'))
      AND NOT EXISTS (SELECT 1 FROM pg_catalog.aclexplode(coalesce(p.proacl,pg_catalog.acldefault('f',p.proowner))) a
          WHERE a.privilege_type='EXECUTE' AND (a.is_grantable OR a.grantee<>p.proowner))
)
AND NOT EXISTS (
    SELECT 1 FROM (VALUES ('content'),('post_secrets')) AS required(schema_name)
    WHERE NOT EXISTS (
        SELECT 1 FROM pg_catalog.pg_namespace n CROSS JOIN owner_role r
        WHERE n.nspname=required.schema_name AND has_schema_privilege(r.oid,n.oid,'USAGE')
          AND NOT has_schema_privilege(r.oid,n.oid,'CREATE')
    )
)"#;
