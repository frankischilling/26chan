-- Native HTTP-profile threshold observation only. Staff/history authority and
-- exact source-weight proof remain unchanged. No historical evidence is updated.
-- Freeze the predecessor's public catalog contract before altering its checks.
DO $preflight$ BEGIN IF (
WITH owner_role AS (
    SELECT r.oid FROM pg_catalog.pg_roles r
    WHERE r.rolname='board_report_admission_owner'
      AND NOT (r.rolcanlogin OR r.rolsuper OR r.rolcreatedb OR r.rolcreaterole OR r.rolreplication OR r.rolbypassrls)
), relations AS (
    SELECT c.oid,c.relowner,n.nspname,c.relname
    FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace
    WHERE (n.nspname='post_secrets' AND c.relname IN ('report_membership','report_admission_gate','report_catalog_gate','report_catalog_versions','report_catalog_rows','report_group','report_weight_evidence'))
       OR (n.nspname='content' AND c.relname IN ('reports','reports_id_seq','boards','posts','threads','post_media','moderation_audit'))
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
        ('content.set_report_catalog_active(bigint)','void',false,false,true),
        ('content.clear_reporter(text,bigint)','bigint',false,true,false),
        ('content.clear_report_group(text,bigint,bigint)','bigint',false,true,false)
    ) AS required(signature,result_type,public_allowed,staff_allowed,migrator_allowed)
    WHERE NOT EXISTS (
        SELECT 1 FROM pg_catalog.pg_proc p JOIN owner_role r ON r.oid=p.proowner
        WHERE p.oid=to_regprocedure(required.signature)
          AND p.prokind='f' AND p.prosecdef AND p.provolatile='v' AND NOT p.proretset
          AND p.prorettype=to_regtype(required.result_type)
          AND p.pronargdefaults=0 AND p.provariadic=0
          AND cardinality(p.proconfig)=1
          AND replace(p.proconfig[1],' ','')='search_path=pg_catalog,pg_temp'
          AND has_function_privilege('board_public'::name,p.oid,'EXECUTE')
              = (('board_public'::name='board_public' AND required.public_allowed)
                  OR ('board_public'::name='board_staff' AND required.staff_allowed))
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
        ('delete_reports_for_deleted_target','trigger',false,''),
        ('retire_staff_file_report_membership','void',true,'25 20'),
        ('increment_report_group','trigger',false,''),
        ('retire_empty_report_group','trigger',false,''),
        ('retire_archived_report_membership','trigger',false,'')
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
          AND NOT has_function_privilege('board_public'::name,p.oid,'EXECUTE')
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
          AND p.proname='delete_reports_for_deleted_target'
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
    SELECT 1 FROM (VALUES ('report_membership'),('report_admission_gate'),('report_catalog_gate'),('report_catalog_versions'),('report_catalog_rows'),('report_group'),('report_weight_evidence')) AS required(table_name)
    WHERE NOT EXISTS (
        SELECT 1 FROM relations c CROSS JOIN owner_role r
        WHERE c.nspname='post_secrets' AND c.relname=required.table_name AND c.relowner=r.oid
          AND NOT has_table_privilege('board_public'::name,c.oid,'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
          AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_roles runtime
              WHERE runtime.rolname IN ('board_public','board_staff','board_auth')
                AND (has_table_privilege(runtime.oid,c.oid,'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
                    OR EXISTS (SELECT 1 FROM pg_catalog.pg_attribute a
                        WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped
                          AND has_column_privilege(runtime.oid,c.oid,a.attnum,'SELECT,INSERT,UPDATE,REFERENCES'))))
    )
)
-- Whole-deletion cleanup relies on exactly these report-dependent cascades.
-- Inspect catalog OIDs without granting runtimes private-schema name access.
AND NOT EXISTS (
    SELECT 1 FROM (VALUES ('report_membership'),('anonymous_reports'),('report_weight_evidence')) AS required(table_name)
    WHERE NOT EXISTS (
        SELECT 1 FROM pg_catalog.pg_constraint fk
        JOIN pg_catalog.pg_class child ON child.oid=fk.conrelid
        JOIN pg_catalog.pg_namespace n ON n.oid=child.relnamespace
        JOIN relations parent ON parent.oid=fk.confrelid
        WHERE n.nspname='post_secrets' AND child.relname=required.table_name
          AND parent.nspname='content' AND parent.relname='reports'
          AND fk.contype='f' AND fk.confdeltype='c' AND fk.convalidated
          AND NOT fk.condeferrable AND NOT fk.condeferred
          AND fk.conkey=ARRAY[(SELECT a.attnum FROM pg_catalog.pg_attribute a
              WHERE a.attrelid=child.oid AND a.attname='report_id' AND NOT a.attisdropped)]
          AND fk.confkey=ARRAY[(SELECT a.attnum FROM pg_catalog.pg_attribute a
              WHERE a.attrelid=parent.oid AND a.attname='id' AND NOT a.attisdropped)]
          AND fk.confmatchtype='s' AND fk.confupdtype='a'
          AND (SELECT count(*)=4 FROM pg_catalog.pg_trigger t WHERE t.tgconstraint=fk.oid)
          AND NOT EXISTS (
              SELECT 1 FROM (VALUES
                  ('RI_FKey_check_ins',5,true),('RI_FKey_check_upd',17,true),
                  ('RI_FKey_cascade_del',9,false),('RI_FKey_noaction_upd',17,false)
              ) AS required_trigger(function_name,event_type,on_child)
              WHERE NOT EXISTS (
                  SELECT 1 FROM pg_catalog.pg_trigger t
                  JOIN pg_catalog.pg_proc p ON p.oid=t.tgfoid
                  JOIN pg_catalog.pg_namespace pn ON pn.oid=p.pronamespace
                  WHERE t.tgconstraint=fk.oid AND t.tgisinternal AND t.tgenabled='O'
                    AND t.tgtype=required_trigger.event_type
                    AND t.tgrelid=CASE WHEN required_trigger.on_child THEN child.oid ELSE parent.oid END
                    AND t.tgconstrrelid=CASE WHEN required_trigger.on_child THEN parent.oid ELSE child.oid END
                    AND t.tgconstrindid=fk.conindid
                    AND NOT t.tgdeferrable AND NOT t.tginitdeferred
                    AND t.tgnargs=0 AND octet_length(t.tgargs)=0
                    AND t.tgattr::text='' AND t.tgqual IS NULL
                    AND t.tgoldtable IS NULL AND t.tgnewtable IS NULL
                    AND pn.nspname='pg_catalog' AND p.proname=required_trigger.function_name
                    AND p.proargtypes=''::oidvector AND p.prorettype='pg_catalog.trigger'::regtype))
    )
)
-- The group is a private persisted lifetime, never a runtime-visible counter.
AND EXISTS (
    SELECT 1 FROM relations c CROSS JOIN owner_role r
    JOIN pg_catalog.pg_class storage ON storage.oid=c.oid
    WHERE c.nspname='post_secrets' AND c.relname='report_group' AND c.relowner=r.oid
      AND storage.relkind='r' AND storage.relpersistence='p'
      AND NOT storage.relrowsecurity AND NOT storage.relforcerowsecurity
      AND (SELECT count(*)=6 FROM pg_catalog.pg_attribute a
          WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped)
      AND NOT EXISTS (
          SELECT 1 FROM (VALUES ('board','text',true),('post_id','int8',true),
              ('illegal_count','int8',true),('incomplete','bool',true),
              ('cleared_at','timestamptz',false),('cleared_by','int8',false))
              AS required(column_name,type_name,not_null)
          WHERE NOT EXISTS (
              SELECT 1 FROM pg_catalog.pg_attribute a
              WHERE a.attrelid=c.oid AND a.attname=required.column_name
                AND a.attnum>0 AND NOT a.attisdropped
                AND a.atttypid=to_regtype('pg_catalog.'||required.type_name)
                AND a.atttypmod=-1 AND a.attnotnull=required.not_null
                AND NOT a.atthasdef AND NOT a.atthasmissing
                AND a.attgenerated='' AND a.attidentity=''))
      AND EXISTS (
          SELECT 1 FROM pg_catalog.pg_constraint k
          JOIN pg_catalog.pg_index i ON i.indexrelid=k.conindid AND i.indrelid=c.oid
          WHERE k.conrelid=c.oid AND k.conname='report_group_pkey' AND k.contype='p'
            AND k.convalidated AND NOT k.condeferrable AND NOT k.condeferred
            AND k.conkey=ARRAY(SELECT a.attnum
                FROM unnest(ARRAY['board','post_id']) WITH ORDINALITY AS names(name,ordinal)
                JOIN pg_catalog.pg_attribute a ON a.attrelid=c.oid AND a.attname=names.name
                    AND a.attnum>0 AND NOT a.attisdropped ORDER BY names.ordinal)
            AND i.indisprimary AND i.indisunique AND i.indisvalid AND i.indisready
            AND i.indislive AND i.indimmediate AND i.indnkeyatts=2 AND i.indnatts=2
            AND i.indpred IS NULL AND i.indexprs IS NULL)
      AND EXISTS (
          SELECT 1 FROM pg_catalog.pg_constraint k
          JOIN pg_catalog.pg_attribute a ON a.attrelid=c.oid AND a.attname='illegal_count'
              AND a.attnum>0 AND NOT a.attisdropped
          WHERE k.conrelid=c.oid AND k.conname='report_group_illegal_count_check'
            AND k.contype='c' AND k.convalidated AND NOT k.connoinherit
            AND NOT k.condeferrable AND NOT k.condeferred
            AND k.conkey=ARRAY[a.attnum]::smallint[]
            AND lower(translate(pg_catalog.pg_get_expr(k.conbin,k.conrelid),' ()',''))='illegal_count>=0')
      AND EXISTS (
          SELECT 1 FROM pg_catalog.pg_constraint k
          WHERE k.conrelid=c.oid AND k.conname='report_group_clear_complete'
            AND k.contype='c' AND k.convalidated AND NOT k.connoinherit
            AND NOT k.condeferrable AND NOT k.condeferred
            AND k.conkey=ARRAY(SELECT a.attnum
                FROM unnest(ARRAY['cleared_at','cleared_by']) WITH ORDINALITY AS names(name,ordinal)
                JOIN pg_catalog.pg_attribute a ON a.attrelid=c.oid AND a.attname=names.name
                    AND a.attnum>0 AND NOT a.attisdropped ORDER BY names.ordinal)
            AND lower(translate(pg_catalog.pg_get_expr(k.conbin,k.conrelid),' ()',''))
                ='cleared_atisnull=cleared_byisnull')
      AND NOT EXISTS (
          SELECT 1 FROM pg_catalog.aclexplode(coalesce(storage.relacl,
              pg_catalog.acldefault('r',storage.relowner))) a
          WHERE a.grantee<>r.oid OR a.is_grantable)
      AND NOT EXISTS (
          SELECT 1 FROM pg_catalog.pg_attribute column_acl
          CROSS JOIN LATERAL pg_catalog.aclexplode(column_acl.attacl) a
          WHERE column_acl.attrelid=c.oid AND column_acl.attnum>0 AND NOT column_acl.attisdropped
            AND (a.grantee<>r.oid OR a.is_grantable))
)
-- Trigger functions are private PL/pgSQL entry points with no callable arguments.
-- Inspect catalog OIDs rather than resolving names inside post_secrets.
AND NOT EXISTS (
    SELECT 1 FROM (VALUES ('increment_report_group'),('retire_empty_report_group'),
        ('retire_archived_report_membership')) AS required(function_name)
    WHERE NOT EXISTS (
        SELECT 1 FROM pg_catalog.pg_proc p JOIN owner_role r ON r.oid=p.proowner
        JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace
        JOIN pg_catalog.pg_language l ON l.oid=p.prolang
        WHERE n.nspname='post_secrets' AND p.proname=required.function_name
          AND p.proargtypes=''::oidvector AND p.pronargs=0
          AND p.proallargtypes IS NULL AND p.proargmodes IS NULL AND p.proargnames IS NULL
          AND l.lanname='plpgsql' AND NOT p.proisstrict AND NOT p.proleakproof AND p.proparallel='u'
          AND NOT EXISTS (SELECT 1 FROM pg_catalog.aclexplode(coalesce(p.proacl,
              pg_catalog.acldefault('f',p.proowner))) a
              WHERE a.grantee<>r.oid OR a.is_grantable)
    )
)
AND NOT EXISTS (
    SELECT 1 FROM (VALUES
        ('report_membership_group_insert','increment_report_group',4,NULL::text,'report_group_inserted'),
        ('report_membership_group_delete','retire_empty_report_group',8,'report_group_deleted',NULL::text)
    ) AS required(trigger_name,function_name,event_type,old_table,new_table)
    WHERE NOT EXISTS (
        SELECT 1 FROM pg_catalog.pg_trigger t JOIN relations c ON c.oid=t.tgrelid
        JOIN pg_catalog.pg_proc p ON p.oid=t.tgfoid
        JOIN owner_role r ON r.oid=p.proowner
        JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace
        WHERE c.nspname='post_secrets' AND c.relname='report_membership'
          AND t.tgname=required.trigger_name AND t.tgtype=required.event_type
          AND t.tgenabled='O' AND NOT t.tgisinternal
          AND t.tgnargs=0 AND octet_length(t.tgargs)=0 AND t.tgconstraint=0
          AND NOT t.tgdeferrable AND NOT t.tginitdeferred
          AND t.tgattr::text='' AND t.tgqual IS NULL
          AND t.tgoldtable IS NOT DISTINCT FROM required.old_table
          AND t.tgnewtable IS NOT DISTINCT FROM required.new_table
          AND n.nspname='post_secrets' AND p.proname=required.function_name
          AND p.proargtypes=''::oidvector AND p.pronargs=0
    )
)
AND EXISTS (
    SELECT 1 FROM pg_catalog.pg_trigger t JOIN relations c ON c.oid=t.tgrelid
    JOIN pg_catalog.pg_proc p ON p.oid=t.tgfoid
    JOIN owner_role r ON r.oid=p.proowner
    JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace
    WHERE c.nspname='content' AND c.relname='threads'
      AND t.tgname='retire_archived_report_membership' AND t.tgtype=17
      AND t.tgenabled='O' AND NOT t.tgisinternal
      AND t.tgnargs=0 AND octet_length(t.tgargs)=0 AND t.tgconstraint=0
      AND NOT t.tgdeferrable AND NOT t.tginitdeferred
      AND t.tgoldtable IS NULL AND t.tgnewtable IS NULL
      AND t.tgattr::text=(SELECT a.attnum::text FROM pg_catalog.pg_attribute a
          WHERE a.attrelid=c.oid AND a.attname='archived_at' AND a.attnum>0 AND NOT a.attisdropped)
      AND t.tgqual IS NOT NULL
      AND lower(translate(split_part(split_part(pg_catalog.pg_get_triggerdef(t.oid,false),' WHEN ',2),' EXECUTE ',1),' ()',''))
          ='old.archived_atisnullandnew.archived_atisnotnull'
      AND n.nspname='post_secrets' AND p.proname='retire_archived_report_membership'
      AND p.proargtypes=''::oidvector AND p.pronargs=0
)
AND EXISTS (
    SELECT 1 FROM relations c CROSS JOIN owner_role r
    WHERE c.nspname='content' AND c.relname='reports'
      AND has_table_privilege(r.oid,c.oid,'DELETE')
      AND NOT has_table_privilege(r.oid,c.oid,'TRUNCATE,TRIGGER')
      AND NOT has_table_privilege('board_public'::name,c.oid,'DELETE,TRUNCATE,TRIGGER')
      AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_roles runtime
          WHERE runtime.rolname IN ('board_public','board_staff','board_auth')
            AND has_table_privilege(runtime.oid,c.oid,'DELETE,TRUNCATE,TRIGGER'))
      AND NOT has_table_privilege('board_public'::name,c.oid,'INSERT')
      AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_attribute a
          WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped
            AND has_column_privilege('board_public'::name,c.oid,a.attnum,'INSERT'))
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
      AND NOT has_sequence_privilege('board_public'::name,to_regclass('content.reports_id_seq'),'USAGE,SELECT,UPDATE')
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
        ('reports','category_kind','SELECT'),
        ('reports','reporter_cleared_at','SELECT'),('reports','reporter_cleared_at','UPDATE'),
        ('reports','board','SELECT'),('reports','post_id','SELECT'),
        ('reports','group_cleared_at','SELECT'),('reports','group_cleared_at','UPDATE'),
        ('reports','group_cleared_by','SELECT'),('reports','group_cleared_by','UPDATE'),
        ('reports','group_clear_inherited','SELECT'),('reports','group_clear_inherited','UPDATE'),
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
        ('reports_group_clear_complete',ARRAY['group_cleared_at','group_cleared_by','group_clear_inherited'],
         '(num_nonnulls(group_cleared_at,group_cleared_by,group_clear_inherited)=any(array[0,3]))'),
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
-- Evidence is immutable private admission metadata, not runtime-readable state.
AND EXISTS (
    SELECT 1 FROM relations c CROSS JOIN owner_role r
    JOIN pg_catalog.pg_class storage ON storage.oid=c.oid
    WHERE c.nspname='post_secrets' AND c.relname='report_weight_evidence' AND c.relowner=r.oid
      AND storage.relkind='r' AND storage.relpersistence='p'
      AND NOT storage.relrowsecurity AND NOT storage.relforcerowsecurity
      AND (SELECT count(*)=10 FROM pg_catalog.pg_attribute a
          WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped)
      AND NOT EXISTS (
          SELECT 1 FROM (VALUES
              ('report_id','int8',true),('evaluator_version','int2',true),
              ('known_or_verified','bool',false),('authenticated_janitor_or_higher','bool',false),
              ('threat_at_least_point_four','bool',false),('history_filtered','bool',false),
              ('effective_weight','float8',false),('numeric_proof','text',false),
              ('source_reason','text',false),('evaluated_at','timestamptz',true)
          ) AS required(column_name,type_name,not_null)
          WHERE NOT EXISTS (
              SELECT 1 FROM pg_catalog.pg_attribute a
              WHERE a.attrelid=c.oid AND a.attname=required.column_name
                AND a.attnum>0 AND NOT a.attisdropped
                AND a.atttypid=to_regtype('pg_catalog.'||required.type_name)
                AND a.atttypmod=-1 AND a.attnotnull=required.not_null
                AND NOT a.atthasdef AND NOT a.atthasmissing
                AND a.attgenerated='' AND a.attidentity=''))
      AND EXISTS (
          SELECT 1 FROM pg_catalog.pg_constraint k
          JOIN pg_catalog.pg_index i ON i.indexrelid=k.conindid AND i.indrelid=c.oid
          JOIN pg_catalog.pg_attribute a ON a.attrelid=c.oid AND a.attname='report_id'
              AND a.attnum>0 AND NOT a.attisdropped
          WHERE k.conrelid=c.oid AND k.conname='report_weight_evidence_pkey' AND k.contype='p'
            AND k.convalidated AND NOT k.condeferrable AND NOT k.condeferred
            AND k.conkey=ARRAY[a.attnum]::smallint[]
            AND i.indisprimary AND i.indisunique AND i.indisvalid AND i.indisready
            AND i.indislive AND i.indimmediate AND i.indnkeyatts=1 AND i.indnatts=1
            AND i.indkey[0]=a.attnum AND i.indpred IS NULL AND i.indexprs IS NULL)
      AND (SELECT count(*)=7 FROM pg_catalog.pg_constraint k WHERE k.conrelid=c.oid AND k.contype='c')
      AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_constraint k
          WHERE k.conrelid=c.oid AND k.contype IN ('u','x'))
      AND NOT EXISTS (
          SELECT 1 FROM (VALUES
              ('evaluator_version_check',ARRAY['evaluator_version'],'(evaluator_version=1)'),
              ('authenticated_janitor_or_higher_check',ARRAY['authenticated_janitor_or_higher'],'(authenticated_janitor_or_higherISNULL)'),
              ('threat_at_least_point_four_check',ARRAY['threat_at_least_point_four'],'(threat_at_least_point_fourISNULL)'),
              ('history_filtered_check',ARRAY['history_filtered'],'(history_filteredISNULL)'),
              ('effective_weight_check',ARRAY['effective_weight'],
               '(effective_weight<>ALL(ARRAY[''Infinity''::doubleprecision,''-Infinity''::doubleprecision,''NaN''::doubleprecision]))'),
              ('source_reason_check',ARRAY['source_reason'],'(source_reasonISNULL)'),
              ('numeric_pair_check',ARRAY['effective_weight','numeric_proof'],'(((effective_weightISNULL)AND(numeric_proofISNULL))OR((effective_weightISNOTNULL)AND(numeric_proofISNOTNULL)AND(effective_weight=(0.5)::doubleprecision)AND(numeric_proof=''BaseEqualsFallback''::text)))')
          ) AS required(constraint_suffix,columns,expression)
          WHERE NOT EXISTS (
              SELECT 1 FROM pg_catalog.pg_constraint k
              WHERE k.conrelid=c.oid AND k.conname='report_weight_evidence_'||required.constraint_suffix
                AND k.contype='c' AND k.convalidated AND NOT k.connoinherit
                AND NOT k.condeferrable AND NOT k.condeferred
                AND k.conkey=ARRAY(SELECT a.attnum
                    FROM unnest(required.columns) WITH ORDINALITY AS names(name,ordinal)
                    JOIN pg_catalog.pg_attribute a ON a.attrelid=c.oid AND a.attname=names.name
                        AND a.attnum>0 AND NOT a.attisdropped ORDER BY names.ordinal)
                -- Keep text-literal case: BaseEqualsFallback is an exact proof tag.
                AND regexp_replace(pg_catalog.pg_get_expr(k.conbin,k.conrelid),'[[:space:]]','','g')=required.expression))
      AND (SELECT count(*)=1 FROM pg_catalog.pg_constraint k WHERE k.conrelid=c.oid AND k.contype='f')
      AND EXISTS (
          SELECT 1 FROM pg_catalog.pg_constraint k
          JOIN relations referenced ON referenced.oid=k.confrelid
          JOIN pg_catalog.pg_attribute child_key ON child_key.attrelid=c.oid AND child_key.attname='report_id'
              AND child_key.attnum>0 AND NOT child_key.attisdropped
          JOIN pg_catalog.pg_attribute parent_key ON parent_key.attrelid=referenced.oid AND parent_key.attname='id'
              AND parent_key.attnum>0 AND NOT parent_key.attisdropped
          WHERE k.conrelid=c.oid AND k.conname='report_weight_evidence_report_id_fkey'
            AND referenced.nspname='content' AND referenced.relname='reports'
            AND k.contype='f' AND k.convalidated AND NOT k.condeferrable AND NOT k.condeferred
            AND k.confmatchtype='s' AND k.confupdtype='a' AND k.confdeltype='c'
            AND k.conkey=ARRAY[child_key.attnum]::smallint[]
            AND k.confkey=ARRAY[parent_key.attnum]::smallint[]
            AND NOT has_column_privilege(r.oid,referenced.oid,parent_key.attnum,'REFERENCES')
            AND (SELECT count(*)=4 FROM pg_catalog.pg_trigger t WHERE t.tgconstraint=k.oid)
            AND NOT EXISTS (
                SELECT 1 FROM (VALUES
                    ('RI_FKey_check_ins',5,true),('RI_FKey_check_upd',17,true),
                    ('RI_FKey_cascade_del',9,false),('RI_FKey_noaction_upd',17,false)
                ) AS required(function_name,event_type,on_child)
                WHERE NOT EXISTS (
                    SELECT 1 FROM pg_catalog.pg_trigger t
                    JOIN pg_catalog.pg_proc p ON p.oid=t.tgfoid
                    JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace
                    WHERE t.tgconstraint=k.oid AND t.tgisinternal AND t.tgenabled='O'
                      AND t.tgtype=required.event_type
                      AND t.tgrelid=CASE WHEN required.on_child THEN c.oid ELSE referenced.oid END
                      AND t.tgconstrrelid=CASE WHEN required.on_child THEN referenced.oid ELSE c.oid END
                      AND t.tgconstrindid=k.conindid
                      AND NOT t.tgdeferrable AND NOT t.tginitdeferred
                      AND t.tgnargs=0 AND octet_length(t.tgargs)=0
                      AND t.tgattr::text='' AND t.tgqual IS NULL
                      AND t.tgoldtable IS NULL AND t.tgnewtable IS NULL
                      AND n.nspname='pg_catalog' AND p.proname=required.function_name
                      AND p.proargtypes=''::oidvector AND p.prorettype='pg_catalog.trigger'::regtype))
            AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_trigger t
                WHERE t.tgrelid=c.oid AND t.tgconstraint<>k.oid))
      AND NOT EXISTS (
          SELECT 1 FROM pg_catalog.aclexplode(coalesce(storage.relacl,
              pg_catalog.acldefault('r',storage.relowner))) a
          WHERE a.grantee<>r.oid OR a.is_grantable)
      AND NOT EXISTS (
          SELECT 1 FROM pg_catalog.pg_attribute column_acl
          CROSS JOIN LATERAL pg_catalog.aclexplode(column_acl.attacl) a
          WHERE column_acl.attrelid=c.oid AND column_acl.attnum>0 AND NOT column_acl.attisdropped
            AND (a.grantee<>r.oid OR a.is_grantable))
)
-- Reporter clearing exposes a staff-only count, never private ownership data.
AND EXISTS (
    SELECT 1 FROM pg_catalog.pg_proc p JOIN owner_role r ON r.oid=p.proowner
    JOIN pg_catalog.pg_language l ON l.oid=p.prolang
    WHERE p.oid=to_regprocedure('content.clear_reporter(text,bigint)')
      AND p.proargtypes='25 20'::oidvector AND p.pronargs=2
      AND p.proallargtypes IS NULL AND p.proargmodes IS NULL
      AND p.proargnames=ARRAY['p_board','p_report']::text[]
      AND l.lanname='plpgsql' AND NOT p.proisstrict AND NOT p.proleakproof AND p.proparallel='u'
)
-- Ordinary group clearing has its own target and trusted authenticated actor.
-- The function returns only a bounded count; the group itself remains private.
AND EXISTS (
    SELECT 1 FROM pg_catalog.pg_proc p JOIN owner_role r ON r.oid=p.proowner
    JOIN pg_catalog.pg_language l ON l.oid=p.prolang
    WHERE p.oid=to_regprocedure('content.clear_report_group(text,bigint,bigint)')
      AND p.proargtypes='25 20 20'::oidvector AND p.pronargs=3
      AND p.proallargtypes IS NULL AND p.proargmodes IS NULL
      AND p.proargnames=ARRAY['p_board','p_post','p_account']::text[]
      AND l.lanname='plpgsql' AND NOT p.proisstrict AND NOT p.proleakproof AND p.proparallel='u'
)
AND EXISTS (
    SELECT 1 FROM relations c
    JOIN pg_catalog.pg_index i ON i.indrelid=c.oid
    JOIN pg_catalog.pg_class index_relation ON index_relation.oid=i.indexrelid
    JOIN pg_catalog.pg_am method ON method.oid=index_relation.relam
    WHERE c.nspname='content' AND c.relname='reports'
      AND index_relation.relname='reports_group_clear_history'
      AND index_relation.relnamespace=(SELECT oid FROM pg_catalog.pg_namespace WHERE nspname='content')
      AND index_relation.relkind='i' AND index_relation.relpersistence='p'
      AND index_relation.relowner=c.relowner AND method.amname='btree'
      AND NOT i.indisprimary AND NOT i.indisunique AND NOT i.indisexclusion
      AND i.indisvalid AND i.indisready AND i.indislive AND i.indimmediate
      AND i.indnkeyatts=2 AND i.indnatts=2 AND i.indexprs IS NULL
      AND i.indkey[0]=(SELECT a.attnum FROM pg_catalog.pg_attribute a
          WHERE a.attrelid=c.oid AND a.attname='board' AND a.attnum>0 AND NOT a.attisdropped)
      AND i.indkey[1]=(SELECT a.attnum FROM pg_catalog.pg_attribute a
          WHERE a.attrelid=c.oid AND a.attname='id' AND a.attnum>0 AND NOT a.attisdropped)
      AND i.indoption::text='0 3'
      AND lower(translate(pg_catalog.pg_get_expr(i.indpred,i.indrelid),' ()',''))
          ='group_cleared_atisnotnullandreporter_cleared_atisnull'
)
AND NOT EXISTS (
    SELECT 1 FROM (VALUES ('reports','reporter_cleared_at','timestamptz'),
        ('reports','group_cleared_at','timestamptz'),
        ('reports','group_cleared_by','int8'),('reports','group_clear_inherited','bool'),
        ('moderation_audit','reporter_clear_count','int8'),
        ('moderation_audit','group_clear_count','int8')) AS required(table_name,column_name,type_name)
    WHERE NOT EXISTS (
        SELECT 1 FROM relations c JOIN pg_catalog.pg_attribute a ON a.attrelid=c.oid
        WHERE c.nspname='content' AND c.relname=required.table_name
          AND a.attname=required.column_name AND a.attnum>0 AND NOT a.attisdropped
          AND a.atttypid=to_regtype('pg_catalog.'||required.type_name)
          AND a.atttypmod=-1 AND NOT a.attnotnull AND NOT a.atthasdef AND NOT a.atthasmissing
          AND a.attgenerated='' AND a.attidentity=''
          AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_roles runtime
              WHERE runtime.rolname IN ('board_public','board_staff','board_auth')
                AND has_column_privilege(runtime.oid,c.oid,a.attnum,'UPDATE,REFERENCES'))
          AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_roles runtime
              WHERE runtime.rolname IN ('board_public','board_auth')
                AND has_column_privilege(runtime.oid,c.oid,a.attnum,'SELECT,INSERT'))
          AND has_column_privilege('board_staff',c.oid,a.attnum,'SELECT')
          AND has_column_privilege('board_staff',c.oid,a.attnum,'INSERT')
              =(required.table_name='moderation_audit')
    )
)
AND EXISTS (
    SELECT 1 FROM relations c CROSS JOIN owner_role r
    WHERE c.nspname='content' AND c.relname='reports'
      AND has_table_privilege(r.oid,c.oid,'DELETE')
      AND NOT has_table_privilege(r.oid,c.oid,'TRUNCATE,TRIGGER')
      AND NOT has_table_privilege('board_public'::name,c.oid,'DELETE,TRUNCATE,TRIGGER')
      AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_roles runtime
          WHERE runtime.rolname IN ('board_public','board_staff','board_auth')
            AND has_table_privilege(runtime.oid,c.oid,'DELETE,TRUNCATE,TRIGGER'))
      AND NOT has_table_privilege(r.oid,c.oid,'UPDATE,TRUNCATE')
      AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_attribute a
          WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped
            AND a.attname NOT IN ('reporter_cleared_at','group_cleared_at','group_cleared_by','group_clear_inherited')
            AND has_column_privilege(r.oid,c.oid,a.attnum,'UPDATE'))
      AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_attribute column_acl
          CROSS JOIN LATERAL pg_catalog.aclexplode(column_acl.attacl) a
          WHERE column_acl.attrelid=c.oid
            AND column_acl.attname IN ('reporter_cleared_at','group_cleared_at','group_cleared_by','group_clear_inherited')
            AND (a.is_grantable OR a.grantee<>r.oid OR a.privilege_type NOT IN ('SELECT','UPDATE')))
)
AND NOT EXISTS (
    SELECT 1 FROM (VALUES
        ('moderation_audit_reporter_clear_count',ARRAY['action','reporter_clear_count'],
         'action=''reporter-clear''andreporter_clear_countisnotnullandreporter_clear_count>=1andreporter_clear_count<=10000oraction<>''reporter-clear''andreporter_clear_countisnull'),
        ('moderation_audit_group_clear_count',ARRAY['action','group_clear_count'],
         'action=''report-group-clear''andgroup_clear_countisnotnullandgroup_clear_count>=1andgroup_clear_count<=10000oraction<>''report-group-clear''andgroup_clear_countisnull'),
        ('moderation_audit_action_check',ARRAY['action'],
         'action=anyarray[''close'',''reopen'',''sticky'',''unsticky'',''permasage'',''unpermasage'',''permaage'',''unpermaage'',''remove-post'',''remove-file'',''remove-thread'',''resolve'',''dismiss'',''staff-post'',''spoiler'',''unspoiler'',''undead'',''unundead'',''thread-options'',''force-archive'',''reporter-clear'',''report-group-clear'',''text-thread-transfer'',''self-remove-post'',''self-remove-file'',''self-remove-thread'']')
    ) AS required(constraint_name,columns,expression)
    WHERE NOT EXISTS (
        SELECT 1 FROM relations c JOIN pg_catalog.pg_constraint k ON k.conrelid=c.oid
        WHERE c.nspname='content' AND c.relname='moderation_audit' AND k.conname=required.constraint_name
          AND k.contype='c' AND k.convalidated AND NOT k.connoinherit
          AND NOT k.condeferrable AND NOT k.condeferred
          AND k.conkey=ARRAY(SELECT a.attnum FROM unnest(required.columns) WITH ORDINALITY AS names(name,ordinal)
              JOIN pg_catalog.pg_attribute a ON a.attrelid=c.oid AND a.attname=names.name
                  AND a.attnum>0 AND NOT a.attisdropped ORDER BY names.ordinal)
          AND lower(translate(replace(replace(pg_catalog.pg_get_expr(k.conbin,k.conrelid),
              '::text',''),'::bigint',''),' ()',''))=required.expression
    )
)
-- Network helpers remain private to their owner and the report boundary.
-- The restriction seal includes their definitions and ACLs, as well as callers.

AND NOT EXISTS (
    SELECT 1 FROM (VALUES
        ('content','lock_report_network_policy','board_admission_owner',''::oidvector,'void',false,'plpgsql'),
        ('content','report_network_rules','board_admission_owner','25 20 16'::oidvector,'record',true,'sql'),
        ('post_secrets','report_network_activity','board_anonymous_owner','17 17 17 17 16 20'::oidvector,'record',true,'plpgsql'),
        ('post_secrets','assert_report_network','board_report_admission_owner','25 20 17 17 17 17 17 16 20'::oidvector,'void',false,'plpgsql')
    ) AS required(schema_name,function_name,owner_name,input_types,result_type,returns_set,language_name)
    WHERE NOT EXISTS (
        SELECT 1 FROM pg_catalog.pg_proc p
        JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace
        JOIN pg_catalog.pg_roles o ON o.oid=p.proowner
        JOIN pg_catalog.pg_language l ON l.oid=p.prolang
        CROSS JOIN owner_role report_owner
        WHERE n.nspname=required.schema_name AND p.proname=required.function_name
          AND o.rolname=required.owner_name AND p.proargtypes=required.input_types
          AND NOT (o.rolcanlogin OR o.rolsuper OR o.rolcreatedb OR o.rolcreaterole OR o.rolreplication OR o.rolbypassrls)
          AND p.prokind='f' AND p.prosecdef AND p.provolatile='v'
          AND p.proretset=required.returns_set AND p.prorettype=to_regtype(required.result_type)
          AND l.lanname=required.language_name AND p.pronargdefaults=0 AND p.provariadic=0
          AND NOT p.proisstrict AND NOT p.proleakproof AND p.proparallel='u'
          AND cardinality(p.proconfig)=1
          AND replace(p.proconfig[1],' ','')='search_path=pg_catalog,pg_temp'
          AND NOT has_schema_privilege(o.oid,n.oid,'CREATE')
          AND has_function_privilege(report_owner.oid,p.oid,'EXECUTE')
          AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_roles runtime
              WHERE runtime.rolname IN ('board_public','board_staff','board_auth','board_migrator','board_media','board_media_read','board_media_intake')
                AND has_function_privilege(runtime.oid,p.oid,'EXECUTE'))
          AND NOT EXISTS (SELECT 1 FROM pg_catalog.aclexplode(coalesce(p.proacl,pg_catalog.acldefault('f',p.proowner))) a
              WHERE a.is_grantable OR a.grantee NOT IN (o.oid,report_owner.oid))
    )
)
-- Pre-report observation is an anonymous-owner boundary, callable only by the
-- report owner. Read catalog OIDs so staff need no private-schema USAGE grant.
AND EXISTS (
    SELECT 1 FROM pg_catalog.pg_proc p
    JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace
    JOIN pg_catalog.pg_roles anonymous_owner ON anonymous_owner.oid=p.proowner
    JOIN pg_catalog.pg_language l ON l.oid=p.prolang
    CROSS JOIN owner_role report_owner
    WHERE n.nspname='post_secrets' AND p.proname='report_known_or_verified'
      AND anonymous_owner.rolname='board_anonymous_owner'
      AND NOT (anonymous_owner.rolcanlogin OR anonymous_owner.rolsuper
          OR anonymous_owner.rolcreatedb OR anonymous_owner.rolcreaterole
          OR anonymous_owner.rolreplication OR anonymous_owner.rolbypassrls)
      AND p.proargtypes='17 17 17 17 16 20'::oidvector AND p.pronargs=6
      AND p.proallargtypes IS NULL AND p.proargmodes IS NULL
      AND p.prokind='f' AND p.prosecdef AND p.provolatile='v' AND NOT p.proretset
      AND p.prorettype='pg_catalog.bool'::regtype
      AND p.pronargdefaults=0 AND p.provariadic=0
      AND l.lanname='plpgsql' AND NOT p.proisstrict AND NOT p.proleakproof AND p.proparallel='u'
      AND cardinality(p.proconfig)=1
      AND replace(p.proconfig[1],' ','')='search_path=pg_catalog,pg_temp'
      AND has_schema_privilege(anonymous_owner.oid,n.oid,'USAGE')
      AND NOT has_schema_privilege(anonymous_owner.oid,n.oid,'CREATE')
      AND has_function_privilege(anonymous_owner.oid,p.oid,'EXECUTE')
      AND has_function_privilege(report_owner.oid,p.oid,'EXECUTE')
      AND NOT has_function_privilege('board_public'::name,p.oid,'EXECUTE')
      AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_roles runtime
          WHERE runtime.rolname IN ('board_public','board_staff','board_auth','board_migrator')
            AND has_function_privilege(runtime.oid,p.oid,'EXECUTE'))
      AND NOT EXISTS (SELECT 1 FROM pg_catalog.aclexplode(coalesce(p.proacl,
          pg_catalog.acldefault('f',p.proowner))) a
          WHERE a.is_grantable OR a.grantee NOT IN (anonymous_owner.oid,report_owner.oid))
)
AND NOT EXISTS (
    SELECT 1 FROM (VALUES ('content'),('post_secrets')) AS required(schema_name)
    WHERE NOT EXISTS (
        SELECT 1 FROM pg_catalog.pg_namespace n CROSS JOIN owner_role r
        WHERE n.nspname=required.schema_name AND has_schema_privilege(r.oid,n.oid,'USAGE')
          AND NOT has_schema_privilege(r.oid,n.oid,'CREATE')
    )
)) IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Report evidence predecessor authority is unavailable'; END IF; END $preflight$;
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Transfer authority drifted before report profile'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Restriction authority drifted before report profile'; END IF; END $$;
RESET ROLE;
DO $$ BEGIN IF NOT EXISTS(SELECT 1 FROM pg_proc WHERE oid=
 'post_secrets.report_known_or_verified(bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure
 AND sha256(convert_to(prosrc,'UTF8'))=decode('0193eb90a4ab9517ccd154e8c8210b89ede5de1d8eeef82479d9b5ef83d65f53','hex')) THEN
 RAISE EXCEPTION 'Pre-report observation definition drifted'; END IF; END $$;
CREATE TABLE post_secrets.report_profile_catalog (
 singleton boolean PRIMARY KEY DEFAULT true CHECK(singleton),
 definition_hash bytea NOT NULL CHECK(octet_length(definition_hash)=32)
);
REVOKE ALL ON post_secrets.report_profile_catalog FROM PUBLIC;
DO $$ DECLARE r record; BEGIN
 FOR r IN SELECT DISTINCT a.grantee FROM pg_class c
 CROSS JOIN LATERAL aclexplode(coalesce(c.relacl,acldefault('r',c.relowner))) a
 WHERE c.oid='post_secrets.report_profile_catalog'::regclass AND a.grantee NOT IN(0,c.relowner)
 LOOP EXECUTE format('REVOKE ALL ON post_secrets.report_profile_catalog FROM %I',pg_get_userbyid(r.grantee)); END LOOP;
END $$;
GRANT SELECT ON post_secrets.report_profile_catalog TO board_report_admission_owner;
GRANT CREATE ON SCHEMA content,post_secrets TO board_report_admission_owner;
SET LOCAL ROLE board_report_admission_owner;
ALTER TABLE post_secrets.report_weight_evidence DROP CONSTRAINT report_weight_evidence_evaluator_version_check;
ALTER TABLE post_secrets.report_weight_evidence ADD CONSTRAINT report_weight_evidence_evaluator_version_check CHECK(evaluator_version IN(1,2));
ALTER TABLE post_secrets.report_weight_evidence DROP CONSTRAINT report_weight_evidence_threat_at_least_point_four_check;
ALTER TABLE post_secrets.report_weight_evidence ADD CONSTRAINT report_weight_evidence_threat_at_least_point_four_check
 CHECK((evaluator_version=1 AND threat_at_least_point_four IS NULL)
 OR (evaluator_version=2 AND threat_at_least_point_four IS NOT NULL));
CREATE FUNCTION content.admit_report_profiled(
 p_board text,p_post bigint,p_reason text,p_actor bytea,
 p_token bytea,p_network bytea,p_address bytea,p_environment bytea,
 p_minted boolean,p_request_at bigint,p_threshold boolean
) RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_invoker text:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
 v_report bigint;
BEGIN
 IF v_invoker<>'board_public' THEN RAISE EXCEPTION 'Report profile authority is unavailable.' USING ERRCODE='42501'; END IF;
 LOCK TABLE post_secrets.report_weight_evidence IN ROW EXCLUSIVE MODE;
 IF content.report_profile_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Report request profile is unavailable.' USING ERRCODE='P200U'; END IF;
 v_report:=content.admit_report(p_board,p_post,p_reason,p_actor,p_token,p_network,p_address,p_environment,p_minted,p_request_at);
 IF p_threshold IS NOT NULL THEN
  UPDATE post_secrets.report_weight_evidence SET evaluator_version=2,threat_at_least_point_four=p_threshold
   WHERE report_id=v_report AND evaluator_version=1 AND threat_at_least_point_four IS NULL;
  IF NOT FOUND THEN RAISE EXCEPTION 'Fresh report evidence is unavailable.' USING ERRCODE='P200U'; END IF;
 END IF;
 RETURN v_report;
END $$;
CREATE FUNCTION content.admit_categorical_report_profiled(
 p_board text,p_post bigint,p_category bigint,p_expected_revision bigint,p_actor bytea,
 p_token bytea,p_network bytea,p_address bytea,p_environment bytea,
 p_minted boolean,p_request_at bigint,p_threshold boolean
) RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_invoker text:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
 v_report bigint;
BEGIN
 IF v_invoker<>'board_public' THEN RAISE EXCEPTION 'Report profile authority is unavailable.' USING ERRCODE='42501'; END IF;
 LOCK TABLE post_secrets.report_weight_evidence IN ROW EXCLUSIVE MODE;
 IF content.report_profile_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Report request profile is unavailable.' USING ERRCODE='P200U'; END IF;
 v_report:=content.admit_categorical_report(p_board,p_post,p_category,p_expected_revision,p_actor,p_token,p_network,p_address,p_environment,p_minted,p_request_at);
 IF p_threshold IS NOT NULL THEN
  UPDATE post_secrets.report_weight_evidence SET evaluator_version=2,threat_at_least_point_four=p_threshold
   WHERE report_id=v_report AND evaluator_version=1 AND threat_at_least_point_four IS NULL;
  IF NOT FOUND THEN RAISE EXCEPTION 'Fresh report evidence is unavailable.' USING ERRCODE='P200U'; END IF;
 END IF;
 RETURN v_report;
END $$;
CREATE FUNCTION post_secrets.report_profile_catalog_hash() RETURNS bytea LANGUAGE sql STABLE
 SET search_path=pg_catalog,pg_temp AS $$
 WITH relations AS (
  SELECT c.* FROM pg_class c WHERE c.oid IN('post_secrets.report_weight_evidence'::regclass,
   'post_secrets.report_profile_catalog'::regclass,'post_secrets.anonymous_sessions'::regclass)
 ), funcs AS (
  SELECT p.* FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
  WHERE (n.nspname='content' AND p.proname IN('report_profile_ready','admit_report_profiled','admit_categorical_report_profiled'))
   OR (n.nspname='post_secrets' AND p.proname IN('report_profile_catalog_hash','report_known_or_verified'))
   OR p.oid IN('content.admit_report(text,bigint,text,bytea,bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure,
    'content.admit_categorical_report(text,bigint,bigint,bigint,bytea,bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure)
 )
 SELECT sha256(convert_to(jsonb_build_array(
  (SELECT jsonb_agg(jsonb_build_array(c.oid::regclass::text,pg_get_userbyid(c.relowner),c.relkind,
   c.relpersistence,c.relrowsecurity,c.relforcerowsecurity,c.reloptions,coalesce(c.relacl,acldefault('r',c.relowner))::text,
   (SELECT jsonb_agg(jsonb_build_array(a.attname,format_type(a.atttypid,a.atttypmod),a.attnotnull,a.attidentity,
    pg_get_expr(d.adbin,d.adrelid),a.attacl::text) ORDER BY a.attnum)
    FROM pg_attribute a LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
    WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped),
   (SELECT jsonb_agg(jsonb_build_array(k.conname,pg_get_constraintdef(k.oid,true),k.convalidated) ORDER BY k.conname)
    FROM pg_constraint k WHERE k.conrelid=c.oid),
   (SELECT jsonb_agg(jsonb_build_array(pg_get_indexdef(i.indexrelid),i.indisvalid,i.indisready) ORDER BY i.indexrelid::regclass::text)
    FROM pg_index i WHERE i.indrelid=c.oid),
   (SELECT jsonb_agg(jsonb_build_array(t.tgname,t.tgenabled,pg_get_triggerdef(t.oid)) ORDER BY t.tgname)
    FROM pg_trigger t WHERE t.tgrelid=c.oid AND NOT t.tgisinternal)) ORDER BY c.oid::regclass::text) FROM relations c),
  (SELECT jsonb_agg(jsonb_build_array(p.oid::regprocedure::text,pg_get_userbyid(p.proowner),
   coalesce(p.proacl,acldefault('f',p.proowner))::text,pg_get_functiondef(p.oid)) ORDER BY p.oid::regprocedure::text) FROM funcs p),
  (SELECT jsonb_agg(jsonb_build_array(t.tgrelid::regclass::text,t.tgfoid::regprocedure::text,t.tgenabled,
    t.tgtype,t.tgdeferrable,t.tginitdeferred,t.tgconstrrelid::regclass::text)
    ORDER BY t.tgrelid::regclass::text,t.tgfoid::regprocedure::text,t.tgtype)
   FROM pg_trigger t WHERE t.tgisinternal AND t.tgconstraint IN(SELECT oid FROM pg_constraint
    WHERE conrelid='post_secrets.report_weight_evidence'::regclass AND contype='f')),
  (SELECT jsonb_agg(jsonb_build_array(nspname,pg_get_userbyid(nspowner),coalesce(nspacl,acldefault('n',nspowner))::text) ORDER BY nspname)
   FROM pg_namespace WHERE nspname IN('post_secrets','content'))
 )::text,'UTF8'))
$$;
CREATE FUNCTION content.report_profile_ready() RETURNS boolean LANGUAGE sql STABLE
 SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
 SELECT coalesce((SELECT definition_hash=post_secrets.report_profile_catalog_hash()
  FROM post_secrets.report_profile_catalog WHERE singleton),false)
 AND NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname IN('board_report_admission_owner','board_anonymous_owner')
  AND (rolcanlogin OR rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls))
 AND NOT EXISTS(SELECT 1 FROM pg_auth_members WHERE member IN('board_report_admission_owner'::regrole,'board_anonymous_owner'::regrole))
 AND NOT EXISTS(SELECT 1 FROM pg_roles r WHERE r.rolname IN('board_public','board_staff','board_auth','board_verifier',
  'board_media','board_media_read','board_media_intake','board_monitor','board_mailer','board_moderation_retention')
  AND (pg_has_role(r.oid,'board_report_admission_owner'::regrole,'MEMBER') OR pg_has_role(r.oid,'board_anonymous_owner'::regrole,'MEMBER')))
 AND NOT EXISTS(SELECT 1 FROM (VALUES ('board_report_admission_owner'::regrole),('board_anonymous_owner'::regrole)) AS required(owner_id)
  WHERE NOT EXISTS(SELECT 1 FROM pg_auth_members m WHERE m.roleid=required.owner_id
   AND m.member='board_migrator'::regrole AND NOT m.admin_option AND NOT m.inherit_option AND m.set_option))
 AND NOT EXISTS(SELECT 1 FROM pg_auth_members WHERE roleid IN('board_report_admission_owner'::regrole,'board_anonymous_owner'::regrole)
  AND (member<>'board_migrator'::regrole OR admin_option OR inherit_option OR NOT set_option))
$$;
REVOKE ALL ON FUNCTION content.admit_report_profiled(text,bigint,text,bytea,bytea,bytea,bytea,bytea,boolean,bigint,boolean),
 content.admit_categorical_report_profiled(text,bigint,bigint,bigint,bytea,bytea,bytea,bytea,bytea,boolean,bigint,boolean),
 content.report_profile_ready(),post_secrets.report_profile_catalog_hash() FROM PUBLIC;
DO $$ DECLARE r record; BEGIN
 FOR r IN SELECT DISTINCT p.oid::regprocedure AS identity,a.grantee FROM pg_proc p
 CROSS JOIN LATERAL aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a
 WHERE p.oid IN('content.admit_report_profiled(text,bigint,text,bytea,bytea,bytea,bytea,bytea,boolean,bigint,boolean)'::regprocedure,
  'content.admit_categorical_report_profiled(text,bigint,bigint,bigint,bytea,bytea,bytea,bytea,bytea,boolean,bigint,boolean)'::regprocedure,
  'content.report_profile_ready()'::regprocedure,'post_secrets.report_profile_catalog_hash()'::regprocedure)
  AND a.grantee NOT IN(0,p.proowner)
 LOOP EXECUTE format('REVOKE ALL ON FUNCTION %s FROM %I',r.identity,pg_get_userbyid(r.grantee)); END LOOP;
END $$;
GRANT EXECUTE ON FUNCTION content.admit_report_profiled(text,bigint,text,bytea,bytea,bytea,bytea,bytea,boolean,bigint,boolean),
 content.admit_categorical_report_profiled(text,bigint,bigint,bigint,bytea,bytea,bytea,bytea,bytea,boolean,bigint,boolean) TO board_public;
GRANT EXECUTE ON FUNCTION content.report_profile_ready() TO board_public,board_staff,board_migrator;
RESET ROLE;
REVOKE CREATE ON SCHEMA content,post_secrets FROM board_report_admission_owner;
SET LOCAL ROLE board_report_admission_owner;
SELECT set_config('board.migration_report_profile_hash',encode(post_secrets.report_profile_catalog_hash(),'hex'),true);
RESET ROLE;
INSERT INTO post_secrets.report_profile_catalog VALUES(true,decode(current_setting('board.migration_report_profile_hash'),'hex'));
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Report request profile is unavailable'; END IF; END $$;
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Transfer authority drifted around report profile'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Restriction authority drifted around report profile'; END IF; END $$;
RESET ROLE;
