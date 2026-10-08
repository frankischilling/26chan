#!/usr/bin/env bash
# Owned synthetic clusters only. Bound qualification to 0104 -> 0105.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-force-archive.XXXXXXXX)
exec 3>&1
started=0
cleanup() {
    status=$?
    trap - EXIT
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null || status=1
    fi
    [[ $cluster =~ ^/tmp/board-force-archive\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
    [[ $(readlink -f "$cluster") = "$cluster" ]] || exit 1
    rm -rf -- "$cluster"
    exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
chown postgres:postgres "$cluster"
runuser -u postgres -- "$pg_bin/initdb" -D "$cluster/data" --auth=trust --encoding=UTF8 --no-locale > /dev/null
runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" \
    -o "-c listen_addresses='' -c unix_socket_directories='$cluster' -c statement_timeout=30000 -c lock_timeout=5000" -w start > /dev/null
started=1
psql=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h "$cluster")
exec > "$cluster/qualification.log" 2>&1
trap 'printf "Force-archive qualification failed at line %s (private diagnostics removed during cleanup).\n" "$LINENO" >&3' ERR
# The shell opens private root-owned fixture files before dropping privileges.
runuser -u postgres -- "${psql[@]}" -d postgres -f - < deploy/roles.sql
runuser -u postgres -- "${psql[@]}" -d postgres -c 'ALTER ROLE board_staff LOGIN; ALTER ROLE board_auth LOGIN'
create_database() {
    runuser -u postgres -- "${psql[@]}" -d postgres -v database="$1" <<'SQL'
CREATE DATABASE :"database" OWNER board_migrator;
REVOKE ALL ON DATABASE :"database" FROM PUBLIC;
GRANT CONNECT ON DATABASE :"database" TO board_migrator,board_public,board_staff,board_auth;
SQL
}
cat > "$cluster/fixture.sql" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,staff_only,archive_retention_seconds)
VALUES ('groupopt','Owned grouped fixture','Synthetic',1000,100,100,100,10,false,2592000),
       ('grouppriv','Owned private fixture','Synthetic',1000,100,100,100,10,true,2592000);
INSERT INTO content.threads(id,board,created_at,bumped_at,modified_at,sticky,closed,permasage,permaage,undead)
SELECT 10300000+i,CASE WHEN i=8 THEN 'grouppriv' ELSE 'groupopt' END,
       '2020-01-01 UTC'::timestamptz,'2020-01-01 UTC'::timestamptz+i*interval '1 minute',
       '2020-02-01 UTC'::timestamptz,i IN(1,2,3),i=2,i=1,i=3,i=2
FROM generate_series(1,8) i;
BEGIN;
SELECT set_config('board.posting_actor',repeat('11',32),true);
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
SELECT id,board,id,'Synthetic','','Retained body','2020-01-01 UTC'::timestamptz FROM content.threads WHERE id BETWEEN 10300001 AND 10300008;
COMMIT;
-- Register posts while the threads are live, then establish historical states
-- through the normal owner update path. Production history-removal triggers
-- must run; never disable them or manufacture identity for archived content.
UPDATE content.threads SET archived_at='2020-03-01 UTC'::timestamptz,
 archive_expires_at=CASE id WHEN 10300005 THEN '2099-01-01 UTC'::timestamptz ELSE '2020-04-01 UTC'::timestamptz END
WHERE id IN(10300005,10300006);
UPDATE content.threads SET deleted=true WHERE id=10300007;
INSERT INTO content.moderation_audit(account_id,board,target_id,action,created_at)
SELECT 1,'groupopt',10300001,action,'2020-02-02 UTC'::timestamptz
FROM unnest(ARRAY['close','reopen','sticky','unsticky','permasage','unpermasage','permaage','unpermaage',
 'remove-post','remove-file','remove-thread','resolve','dismiss','staff-post','spoiler','unspoiler','undead','unundead']) action;
INSERT INTO content.moderation_audit(account_id,board,target_id,action,before_mask,after_mask,created_at)
VALUES (1,'groupopt',10300001,'thread-options',0,31,'2020-02-03 UTC');
INSERT INTO content.moderation_audit(account_id,board,target_id,action,before_mask,after_mask,
 snapshot_version,snapshot_name,snapshot_subject,snapshot_comment,snapshot_comment_format,
 snapshot_staff_authorized_limits,snapshot_wordfiltered,snapshot_image_spoiler,snapshot_filename)
VALUES(999999,'groupopt',10300004,'thread-options',0,31,1,'Retained name','Retained subject','Retained body',127,true,true,true,'removed.png');
INSERT INTO content.moderation_audit(account_id,board,target_id,action,
 snapshot_version,snapshot_name,snapshot_subject,snapshot_comment,snapshot_comment_format,
 snapshot_staff_authorized_limits,snapshot_wordfiltered,snapshot_image_spoiler)
VALUES(999999,'groupopt',10300004,'spoiler',1,'Spoiler name','','',0,false,false,false),
      (999999,'groupopt',10300004,'unspoiler',1,'Unspoiler name','','',0,false,false,true);
SQL
cat > "$cluster/staff-checks.sql" <<'SQL'
BEGIN;
DO $$ DECLARE
 base jsonb := jsonb_build_object('id',10400990,'account_id',1,'board','groupopt','target_id',10300001,
  'action','thread-options','created_at','2020-04-01 UTC','before_mask',0,'after_mask',31,
  'snapshot_version',1,'snapshot_name','','snapshot_subject','','snapshot_comment','',
  'snapshot_comment_format',0,'snapshot_staff_authorized_limits',false,
  'snapshot_wordfiltered',false,'snapshot_image_spoiler',false);
 patched jsonb; key text; item record; n integer;
BEGIN
 -- Record-shaped INSERT retains explicit identity so rollback tests do not
 -- advance sequences before the administrator restore fingerprint.
 INSERT INTO content.moderation_audit OVERRIDING SYSTEM VALUE
 SELECT * FROM jsonb_populate_record(NULL::content.moderation_audit,base);
 IF NOT EXISTS(SELECT 1 FROM content.moderation_audit WHERE id=10400990 AND snapshot_version=1)
 THEN RAISE EXCEPTION 'Staff snapshot read failed'; END IF;
 -- The grouped mask contract remains strict and exclusive to thread-options.
 FOR patched IN SELECT value FROM jsonb_array_elements(jsonb_build_array(
  jsonb_build_object('before_mask',NULL),jsonb_build_object('after_mask',NULL),
  jsonb_build_object('before_mask',-1),jsonb_build_object('after_mask',32),
  jsonb_build_object('before_mask',31,'after_mask',31)
 )) LOOP
  BEGIN
   INSERT INTO content.moderation_audit OVERRIDING SYSTEM VALUE
   SELECT * FROM jsonb_populate_record(NULL::content.moderation_audit,base || patched || '{"id":10500992}'::jsonb);
   RAISE EXCEPTION 'Invalid grouped mask accepted' USING ERRCODE='ZX001';
  EXCEPTION WHEN check_violation THEN NULL; END;
 END LOOP;
 -- Force archive supports a complete snapshot or the legacy all-NULL shape.
 INSERT INTO content.moderation_audit OVERRIDING SYSTEM VALUE
 SELECT * FROM jsonb_populate_record(NULL::content.moderation_audit,
  base || jsonb_build_object('id',10500990,'action','force-archive','before_mask',NULL,'after_mask',NULL));
 INSERT INTO content.moderation_audit(id,account_id,board,target_id,action)
 OVERRIDING SYSTEM VALUE VALUES(10500991,999999,'groupopt',10300004,'force-archive');
 IF NOT EXISTS(SELECT 1 FROM content.moderation_audit WHERE id=10500990 AND action='force-archive' AND snapshot_version=1
  AND before_mask IS NULL AND after_mask IS NULL)
 OR NOT EXISTS(SELECT 1 FROM content.moderation_audit WHERE id=10500991 AND snapshot_version IS NULL)
 THEN RAISE EXCEPTION 'Force archive append/read failed'; END IF;
 FOR patched IN SELECT value FROM jsonb_array_elements(jsonb_build_array(
  jsonb_build_object('before_mask',0),jsonb_build_object('after_mask',31),
  jsonb_build_object('before_mask',0,'after_mask',31),
  jsonb_build_object('action','unknown-action'),jsonb_build_object('snapshot_version',NULL),
  jsonb_build_object('snapshot_name',NULL),jsonb_build_object('snapshot_subject',NULL),
  jsonb_build_object('snapshot_comment',NULL),jsonb_build_object('snapshot_comment_format',NULL),
  jsonb_build_object('snapshot_staff_authorized_limits',NULL),jsonb_build_object('snapshot_wordfiltered',NULL),
  jsonb_build_object('snapshot_image_spoiler',NULL)
 )) LOOP
  BEGIN
   INSERT INTO content.moderation_audit OVERRIDING SYSTEM VALUE
   SELECT * FROM jsonb_populate_record(NULL::content.moderation_audit,
    base || jsonb_build_object('id',10500992,'action','force-archive','before_mask',NULL,'after_mask',NULL) || patched);
   RAISE EXCEPTION 'Invalid force archive snapshot or mask accepted' USING ERRCODE='ZX001';
  EXCEPTION WHEN check_violation THEN NULL; END;
 END LOOP;
 base := base || jsonb_build_object('action','force-archive','before_mask',NULL,'after_mask',NULL);
 -- Every required member is individually NULL-rejected, including version.
 FOREACH key IN ARRAY ARRAY['snapshot_version','snapshot_name','snapshot_subject','snapshot_comment',
  'snapshot_comment_format','snapshot_staff_authorized_limits','snapshot_wordfiltered','snapshot_image_spoiler'] LOOP
  BEGIN
   INSERT INTO content.moderation_audit OVERRIDING SYSTEM VALUE
   SELECT * FROM jsonb_populate_record(NULL::content.moderation_audit,
    base || jsonb_build_object('id',10400991,key,NULL));
   RAISE EXCEPTION 'Required NULL accepted: %',key USING ERRCODE='ZX001';
  EXCEPTION WHEN check_violation THEN NULL; END;
 END LOOP;
 -- Any member on an otherwise legacy all-NULL snapshot must fail. This catches
 -- SQL CHECK's UNKNOWN loophole even on individually optional members.
 FOR item IN SELECT j.key,j.value FROM jsonb_each(base) j WHERE j.key LIKE 'snapshot_%'
 UNION ALL SELECT * FROM (VALUES ('snapshot_trip','"!0123456789"'::jsonb),
 ('snapshot_capcode','"mod"'::jsonb),('snapshot_filename','"x.png"'::jsonb),
 ('snapshot_dice_result','"1d6 = 6"'::jsonb),('snapshot_fortune_text','"Good"'::jsonb),
 ('snapshot_fortune_color','"#abcdef"'::jsonb)) optional(key,value) LOOP
  BEGIN
   INSERT INTO content.moderation_audit OVERRIDING SYSTEM VALUE
   SELECT * FROM jsonb_populate_record(NULL::content.moderation_audit,
    jsonb_build_object('id',10400991,'account_id',1,'board','groupopt','target_id',10300001,
     'action','spoiler','created_at','2020-04-01 UTC') || jsonb_build_object(item.key,item.value));
   RAISE EXCEPTION 'Partial snapshot accepted: %',item.key USING ERRCODE='ZX001';
  EXCEPTION WHEN check_violation THEN NULL; END;
 END LOOP;
 FOR patched IN SELECT value FROM jsonb_array_elements(jsonb_build_array(
  jsonb_build_object('snapshot_version',0),jsonb_build_object('snapshot_version',2),
  jsonb_build_object('action','close','before_mask',NULL,'after_mask',NULL),
  jsonb_build_object('snapshot_name',repeat('x',256)),jsonb_build_object('snapshot_subject',repeat('x',1021)),
  jsonb_build_object('snapshot_comment',repeat('x',2097153)),jsonb_build_object('snapshot_filename',repeat('x',256)),
  jsonb_build_object('snapshot_trip','invalid'),jsonb_build_object('snapshot_capcode','moderator'),
  jsonb_build_object('snapshot_dice_result',''),jsonb_build_object('snapshot_dice_result',repeat('x',1025)),
  jsonb_build_object('snapshot_dice_result',E'line\nbreak'),
  jsonb_build_object('snapshot_fortune_text','Good'),jsonb_build_object('snapshot_fortune_color','#abcdef'),
  jsonb_build_object('snapshot_fortune_text','','snapshot_fortune_color','#abcdef'),
  jsonb_build_object('snapshot_fortune_text',repeat('x',257),'snapshot_fortune_color','#abcdef'),
  jsonb_build_object('snapshot_fortune_text',E'line\nbreak','snapshot_fortune_color','#abcdef'),
  jsonb_build_object('snapshot_fortune_text','Good','snapshot_fortune_color','#ABCDEF'),
  jsonb_build_object('snapshot_fortune_text','Good','snapshot_fortune_color','#abcdef','snapshot_dice_result','6')
 )) LOOP
  BEGIN
   INSERT INTO content.moderation_audit OVERRIDING SYSTEM VALUE
   SELECT * FROM jsonb_populate_record(NULL::content.moderation_audit,base || patched || '{"id":10400991}'::jsonb);
   RAISE EXCEPTION 'Invalid snapshot accepted' USING ERRCODE='ZX001';
  EXCEPTION WHEN check_violation THEN NULL; END;
 END LOOP;
 -- Enumerate the complete smallint formatter domain around supported values.
 FOR n IN -1..128 LOOP
  BEGIN
   INSERT INTO content.moderation_audit OVERRIDING SYSTEM VALUE
   SELECT * FROM jsonb_populate_record(NULL::content.moderation_audit,
    base || jsonb_build_object('id',10401000+n,'snapshot_comment_format',n));
   IF NOT (n=0 OR n BETWEEN 8 AND 15 OR n BETWEEN 24 AND 31 OR n BETWEEN 40 AND 47
     OR n BETWEEN 56 AND 63 OR n BETWEEN 104 AND 111 OR n BETWEEN 120 AND 127)
   THEN RAISE EXCEPTION 'Invalid formatter accepted: %',n USING ERRCODE='ZX001'; END IF;
  EXCEPTION WHEN check_violation THEN
   IF n=0 OR n BETWEEN 8 AND 15 OR n BETWEEN 24 AND 31 OR n BETWEEN 40 AND 47
     OR n BETWEEN 56 AND 63 OR n BETWEEN 104 AND 111 OR n BETWEEN 120 AND 127
   THEN RAISE EXCEPTION 'Valid formatter rejected: %',n; END IF;
  END;
 END LOOP;
 -- Large saved fields remain valid independently of current admission policy.
 INSERT INTO content.moderation_audit OVERRIDING SYSTEM VALUE
 SELECT * FROM jsonb_populate_record(NULL::content.moderation_audit,base || jsonb_build_object(
  'id',10400992,'snapshot_name',repeat('x',255),'snapshot_subject',repeat('x',1020),
  'snapshot_comment',repeat('x',2097152),'snapshot_trip','!!01234567890','snapshot_capcode','admin_highlight',
  'snapshot_filename',repeat('x',255),'snapshot_dice_result',repeat('x',1024),
  'snapshot_staff_authorized_limits',true,'snapshot_wordfiltered',true));
 INSERT INTO content.moderation_audit OVERRIDING SYSTEM VALUE
 SELECT * FROM jsonb_populate_record(NULL::content.moderation_audit,base || jsonb_build_object(
  'id',10400993,'action','spoiler','before_mask',NULL,'after_mask',NULL,'snapshot_trip','!0123456789',
  'snapshot_fortune_text',repeat('x',256),'snapshot_fortune_color','#abcdef'));
 INSERT INTO content.moderation_audit OVERRIDING SYSTEM VALUE
 SELECT * FROM jsonb_populate_record(NULL::content.moderation_audit,base || jsonb_build_object(
  'id',10400994,'action','unspoiler','before_mask',NULL,'after_mask',NULL));
 -- Legacy explicit-column writers are still admitted for every supported action.
 INSERT INTO content.moderation_audit(id,account_id,board,target_id,action)
 OVERRIDING SYSTEM VALUE VALUES(10400995,1,'groupopt',10300001,'spoiler');
 INSERT INTO content.moderation_audit(id,account_id,board,target_id,action,before_mask,after_mask)
 OVERRIDING SYSTEM VALUE VALUES(10400996,1,'groupopt',10300001,'thread-options',31,0);
 IF EXISTS(SELECT 1 FROM content.moderation_audit WHERE id IN(10400995,10400996) AND snapshot_version IS NOT NULL)
 THEN RAISE EXCEPTION 'Legacy insertion acquired snapshot'; END IF;
 BEGIN
  UPDATE content.moderation_audit SET snapshot_name='rewritten' WHERE id=10400990;
  RAISE EXCEPTION 'Runtime staff updated immutable audit' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN
  DELETE FROM content.moderation_audit WHERE id=10400990;
  RAISE EXCEPTION 'Runtime staff deleted audit' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
ROLLBACK;
SQL
for mode in upgrade fresh; do
 database="force_archive_$mode"
 create_database "$database"
 migrator=("${psql[@]}" -U board_migrator -d "$database")
 admin=(runuser -u postgres -- "${psql[@]}" -d "$database")
 for migration in migrations/*.sql; do
  [[ $migration < migrations/0105_force_archive.sql ]] || break
  "${migrator[@]}" --single-transaction -f - < "$migration"
 done
 # Historical readiness uses catalog introspection, never current Rust queries.
 "${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM pg_attribute WHERE attrelid='content.threads'::regclass AND attname='sticky_rank' AND NOT attisdropped)
 OR NOT EXISTS(SELECT 1 FROM pg_attribute WHERE attrelid='content.moderation_audit'::regclass AND attname='before_mask' AND NOT attisdropped)
 OR NOT EXISTS(SELECT 1 FROM pg_attribute WHERE attrelid='content.moderation_audit'::regclass AND attname='snapshot_version' AND NOT attisdropped)
 OR position('force-archive' IN pg_get_constraintdef((SELECT oid FROM pg_constraint WHERE conrelid='content.moderation_audit'::regclass AND conname='moderation_audit_action_check')))>0
 THEN RAISE EXCEPTION 'Expected historical 0104 boundary'; END IF;
END $$;
CREATE FUNCTION public.capture_rows() RETURNS TABLE(relation text,value jsonb)
LANGUAGE plpgsql AS $$ DECLARE r record; BEGIN
 FOR r IN SELECT n.nspname,c.relname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission') AND c.relkind='r' LOOP
  RETURN QUERY EXECUTE format('SELECT %L,to_jsonb(r) FROM %I.%I r',r.nspname||'.'||r.relname,r.nspname,r.relname);
 END LOOP;
END $$;
REVOKE ALL ON FUNCTION public.capture_rows() FROM PUBLIC;
CREATE VIEW public.old_shape AS SELECT * FROM public.capture_rows();
-- Compare existing relation owners and normalized ACL semantics, not raw ACL
-- array ordering (pg_dump may reorder entries without changing privileges).
-- PostgreSQL's pretty deparser removes redundant same-operator grouping that
-- pg_dump/reparse can flatten, while retaining every constraint expression.
CREATE VIEW public.unchanged_metadata AS
SELECT 'relation'::text kind,jsonb_build_array(n.nspname,c.relname,pg_get_userbyid(c.relowner),
 (SELECT jsonb_agg(jsonb_build_array(pg_get_userbyid(a.grantor),
  CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
  ORDER BY pg_get_userbyid(a.grantor),CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
  FROM aclexplode(coalesce(c.relacl,acldefault(CASE WHEN c.relkind='S' THEN 's'::"char" ELSE 'r'::"char" END,c.relowner))) a),
 c.reloptions,c.relrowsecurity,c.relforcerowsecurity) value
FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
WHERE n.nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission')
UNION ALL
SELECT 'column-grant',to_jsonb(g)-'table_catalog' FROM information_schema.column_privileges g
WHERE table_schema IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission')
UNION ALL
SELECT 'foreign-key',jsonb_build_array(c.conrelid::regclass::text,c.conname,pg_get_constraintdef(c.oid,true))
FROM pg_constraint c JOIN pg_namespace n ON n.oid=c.connamespace
WHERE c.contype='f' AND n.nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission')
UNION ALL
SELECT 'old-constraint',jsonb_build_array(c.conrelid::regclass::text,c.conname,pg_get_constraintdef(c.oid,true))
FROM pg_constraint c JOIN pg_namespace n ON n.oid=c.connamespace
WHERE n.nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission')
AND NOT (c.conrelid='content.moderation_audit'::regclass AND c.conname IN ('moderation_audit_action_check','moderation_audit_snapshot_shape'))
UNION ALL
SELECT 'old-column',jsonb_build_array(n.nspname,c.relname,a.attname,format_type(a.atttypid,a.atttypmod),
 a.attnotnull,a.attidentity,a.attgenerated,pg_get_expr(d.adbin,d.adrelid))
FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace
LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
WHERE a.attnum>0 AND NOT a.attisdropped
AND n.nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission')
UNION ALL
SELECT 'trigger',jsonb_build_array(t.tgrelid::regclass::text,t.tgname,t.tgenabled,pg_get_triggerdef(t.oid))
FROM pg_trigger t JOIN pg_class c ON c.oid=t.tgrelid JOIN pg_namespace n ON n.oid=c.relnamespace
WHERE NOT t.tgisinternal AND n.nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission')
UNION ALL
SELECT 'policy',to_jsonb(p) FROM pg_policies p
WHERE schemaname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission')
UNION ALL
SELECT 'schema',jsonb_build_array(n.nspname,pg_get_userbyid(n.nspowner),
 (SELECT jsonb_agg(jsonb_build_array(pg_get_userbyid(a.grantor),
  CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
  ORDER BY pg_get_userbyid(a.grantor),CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
  FROM aclexplode(coalesce(n.nspacl,acldefault('n',n.nspowner))) a))
FROM pg_namespace n WHERE n.nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission');
SQL
 "${admin[@]}" <<'SQL'
CREATE VIEW public.audit_dependencies AS
-- TOAST relation names embed physical OIDs and legitimately change on restore.
-- Normalize only an actual TOAST relation through its owning table, retaining
-- its SQL owner, referenced object/column and exact dependency type. All other
-- objects retain their full catalog descriptions (including dependent views).
SELECT CASE WHEN toast_parent.oid IS NOT NULL
 THEN format('toast table for %s owned by %I',toast_parent.oid::regclass,pg_get_userbyid(dependent_relation.relowner))
 ELSE pg_describe_object(d.classid,d.objid,d.objsubid) END object,
 pg_describe_object(d.refclassid,d.refobjid,d.refobjsubid) referenced,d.deptype
FROM pg_depend d
LEFT JOIN pg_class dependent_relation ON d.classid='pg_class'::regclass
 AND dependent_relation.oid=d.objid AND d.objsubid=0 AND dependent_relation.relkind='t'
LEFT JOIN pg_class toast_parent ON toast_parent.reltoastrelid=dependent_relation.oid
WHERE d.refclassid='pg_class'::regclass AND d.refobjid='content.moderation_audit'::regclass
AND NOT (d.classid='pg_constraint'::regclass AND d.objid IN
 (SELECT oid FROM pg_constraint WHERE conrelid='content.moderation_audit'::regclass
  AND conname IN('moderation_audit_action_check','moderation_audit_snapshot_shape')));
CREATE VIEW public.audit_snapshot_dependency AS
 SELECT id,action,before_mask,after_mask,snapshot_version,snapshot_comment FROM content.moderation_audit;
REVOKE ALL ON public.audit_snapshot_dependency FROM PUBLIC;
CREATE VIEW public.functions AS
SELECT n.nspname,p.oid::regprocedure::text signature,pg_get_userbyid(p.proowner) owner,
 (SELECT jsonb_agg(jsonb_build_array(pg_get_userbyid(a.grantor),
   CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
   ORDER BY pg_get_userbyid(a.grantor),CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
  FROM aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a) acl,
 p.proconfig::text config,md5(pg_get_functiondef(p.oid)) definition
FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
WHERE n.nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission');
SQL
 if [[ $mode = upgrade ]]; then "${migrator[@]}" -f - < "$cluster/fixture.sql"; fi
 "${admin[@]}" -c 'CREATE TABLE public.before_rows AS TABLE public.old_shape; CREATE TABLE public.before_metadata AS TABLE public.unchanged_metadata; CREATE TABLE public.before_functions AS TABLE public.functions; CREATE TABLE public.before_dependencies AS TABLE public.audit_dependencies'
 "${migrator[@]}" --single-transaction -f - < migrations/0105_force_archive.sql
 "${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(TABLE public.before_rows EXCEPT ALL TABLE public.old_shape)
 OR EXISTS(TABLE public.old_shape EXCEPT ALL TABLE public.before_rows)
  THEN RAISE EXCEPTION 'Migration changed retained rows, masks or historical snapshots'; END IF;
 IF EXISTS(TABLE public.before_metadata EXCEPT ALL TABLE public.unchanged_metadata)
 OR EXISTS(TABLE public.unchanged_metadata EXCEPT ALL TABLE public.before_metadata)
 OR EXISTS(TABLE public.before_functions EXCEPT ALL TABLE public.functions)
 OR EXISTS(TABLE public.functions EXCEPT ALL TABLE public.before_functions)
 OR EXISTS(TABLE public.before_dependencies EXCEPT ALL TABLE public.audit_dependencies)
 OR EXISTS(TABLE public.audit_dependencies EXCEPT ALL TABLE public.before_dependencies)
 THEN RAISE EXCEPTION 'Migration changed owners, grants, foreign keys or functions'; END IF;
END $$;
DROP TABLE public.before_rows,public.before_metadata,public.before_functions,public.before_dependencies;
SQL
 if [[ $mode = fresh ]]; then "${migrator[@]}" -f - < "$cluster/fixture.sql"; fi
 # Persist one new action so administrator restore also proves retention of
 # force-archive evidence, independently of the deleted target below.
 "${psql[@]}" -U board_staff -d "$database" <<'SQL'
INSERT INTO content.moderation_audit(account_id,board,target_id,action,
 snapshot_version,snapshot_name,snapshot_subject,snapshot_comment,snapshot_comment_format,
 snapshot_staff_authorized_limits,snapshot_wordfiltered,snapshot_image_spoiler)
VALUES(999999,'groupopt',10300004,'force-archive',1,'Archived target','','Saved before archive',0,false,false,false);
SQL
 # Saved evidence must outlive its owned target and does not gain an account,
 # post or media FK. No real account or media object exists for these values.
 "${migrator[@]}" <<'SQL'
UPDATE content.posts SET name='Changed after action',comment='Changed body' WHERE id=10300004;
DELETE FROM content.posts WHERE id=10300004;
DELETE FROM content.threads WHERE id=10300004;
SQL
 for phase in live restored; do
  if [[ $phase = restored ]]; then
   runuser -u postgres -- "$pg_bin/pg_dump" -h "$cluster" -U postgres -d "$database" --format=custom > "$cluster/current.dump"
   create_database "${database}_restore"
   runuser -u postgres -- "$pg_bin/pg_restore" -h "$cluster" --dbname="${database}_restore" --single-transaction --exit-on-error < "$cluster/current.dump"
   database="${database}_restore"
   admin=(runuser -u postgres -- "${psql[@]}" -d "$database")
  fi
  "${psql[@]}" -U board_staff -d "$database" -f - < "$cluster/staff-checks.sql"
  "${admin[@]}" <<'SQL'
DO $$ DECLARE r record; col text; BEGIN
 IF (SELECT array_agg(attname::text ORDER BY attname) FROM pg_attribute
  WHERE attrelid='content.moderation_audit'::regclass AND attname LIKE 'snapshot_%' AND NOT attisdropped)
 IS DISTINCT FROM (SELECT array_agg(x ORDER BY x) FROM unnest(ARRAY[
  'snapshot_version','snapshot_name','snapshot_trip','snapshot_capcode','snapshot_subject','snapshot_comment',
  'snapshot_comment_format','snapshot_staff_authorized_limits','snapshot_wordfiltered','snapshot_image_spoiler',
  'snapshot_filename','snapshot_dice_result','snapshot_fortune_text','snapshot_fortune_color']) x)
 OR EXISTS(SELECT 1 FROM pg_attribute WHERE attrelid='content.moderation_audit'::regclass AND attname LIKE 'snapshot_%'
  AND NOT attisdropped AND (attnotnull OR atthasdef))
 OR (SELECT atttypid FROM pg_attribute WHERE attrelid='content.moderation_audit'::regclass AND attname='snapshot_wordfiltered')<>'boolean'::regtype
 THEN RAISE EXCEPTION 'Unexpected snapshot columns, defaults or nullable shape'; END IF;
 FOR col IN SELECT attname FROM pg_attribute WHERE attrelid='content.moderation_audit'::regclass
  AND attname LIKE 'snapshot_%' AND NOT attisdropped LOOP
  IF NOT has_column_privilege('board_staff','content.moderation_audit',col,'INSERT')
  OR NOT has_column_privilege('board_staff','content.moderation_audit',col,'SELECT')
  OR has_column_privilege('board_staff','content.moderation_audit',col,'UPDATE')
  THEN RAISE EXCEPTION 'Staff append/read contract changed: %',col; END IF;
  FOR r IN SELECT rolname FROM pg_roles WHERE rolname LIKE 'board_%' AND rolname NOT IN('board_staff','board_migrator') LOOP
   IF has_column_privilege(r.rolname,'content.moderation_audit',col,'SELECT')
   OR has_column_privilege(r.rolname,'content.moderation_audit',col,'INSERT')
   OR has_column_privilege(r.rolname,'content.moderation_audit',col,'UPDATE')
   OR has_table_privilege(r.rolname,'content.moderation_audit','DELETE')
   THEN RAISE EXCEPTION 'Other role gained snapshot access: %, %',r.rolname,col; END IF;
  END LOOP;
 END LOOP;
 IF has_table_privilege('board_staff','content.moderation_audit','DELETE')
 OR EXISTS(SELECT 1 FROM content.posts WHERE id=10300004)
 OR (SELECT count(*) FROM content.moderation_audit WHERE snapshot_version IS NULL)<>19
 OR (SELECT count(*) FROM content.moderation_audit WHERE snapshot_version=1 AND target_id=10300004)<>4
 OR NOT EXISTS(SELECT 1 FROM content.moderation_audit WHERE target_id=10300004 AND action='thread-options'
  AND snapshot_name='Retained name' AND snapshot_subject='Retained subject' AND snapshot_comment='Retained body'
  AND snapshot_comment_format=127 AND snapshot_staff_authorized_limits AND snapshot_wordfiltered
  AND snapshot_image_spoiler AND snapshot_filename='removed.png' AND before_mask=0 AND after_mask=31)
 OR NOT EXISTS(SELECT 1 FROM content.moderation_audit WHERE target_id=10300004 AND action='force-archive'
  AND snapshot_version=1 AND snapshot_comment='Saved before archive' AND before_mask IS NULL AND after_mask IS NULL)
 THEN RAISE EXCEPTION 'Historical or target-independent saved evidence changed'; END IF;
END $$;
-- Exercise actual SQL denial under each runtime/helper role, not just catalogs.
DO $$ DECLARE r record; BEGIN
 FOR r IN SELECT rolname FROM pg_roles WHERE rolname LIKE 'board_%' AND rolname NOT IN('board_staff','board_migrator') LOOP
  EXECUTE format('SET LOCAL ROLE %I',r.rolname);
  BEGIN
   PERFORM snapshot_comment FROM content.moderation_audit LIMIT 1;
   RAISE EXCEPTION 'Other role read snapshot: %',r.rolname USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  RESET ROLE;
 END LOOP;
END $$;
SQL
  "${admin[@]}" -At > "$cluster/$mode-$phase.fingerprint" <<'SQL'
SELECT relation,md5(value::text) FROM public.capture_rows() ORDER BY relation,value::text;
SELECT * FROM public.functions ORDER BY nspname,signature;
SELECT * FROM public.audit_dependencies ORDER BY object,referenced,deptype;
SELECT pg_get_viewdef('public.audit_snapshot_dependency'::regclass,true);

-- pg_dump can reorder ACL array entries. Compare the complete privilege set
-- semantically, including grantor, grantee, grant option and implicit defaults.
SELECT n.nspname,c.relname,pg_get_userbyid(c.relowner),
 (SELECT jsonb_agg(jsonb_build_array(pg_get_userbyid(a.grantor),
   CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
   ORDER BY pg_get_userbyid(a.grantor),CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
  FROM aclexplode(coalesce(c.relacl,acldefault(CASE WHEN c.relkind='S' THEN 's'::"char" ELSE 'r'::"char" END,c.relowner))) a),
 c.reloptions,c.relrowsecurity,c.relforcerowsecurity
FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission') ORDER BY 1,2;
SELECT table_schema,table_name,column_name,grantor,grantee,privilege_type,is_grantable FROM information_schema.column_privileges WHERE table_schema IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission') ORDER BY 1,2,3,4,5,6,7;
SELECT c.conrelid::regclass,c.conname,pg_get_constraintdef(c.oid,true) FROM pg_constraint c WHERE c.conrelid IN('content.threads'::regclass,'content.moderation_audit'::regclass) ORDER BY c.conrelid::regclass::text,c.conname;
SELECT indexname,indexdef FROM pg_indexes WHERE schemaname='content' AND tablename='threads' ORDER BY indexname;
SELECT pg_get_viewdef('content.visible_threads'::regclass,true);
SELECT schemaname,tablename,policyname,roles,cmd,qual,with_check FROM pg_policies ORDER BY 1,2,3;
SELECT last_value,is_called FROM content.moderation_audit_id_seq;
SELECT * FROM public.unchanged_metadata ORDER BY kind,value::text;
SELECT rolname,rolsuper,rolinherit,rolcreaterole,rolcreatedb,rolcanlogin,rolreplication,rolbypassrls,rolconfig FROM pg_roles WHERE rolname LIKE 'board_%' ORDER BY rolname;
SQL
 done
 cmp -s "$cluster/$mode-live.fingerprint" "$cluster/$mode-restored.fingerprint" || {
  printf 'Force-archive dump/restore fingerprint mismatch (%s); bounded synthetic schema/row-hash diff follows.\n' "$mode" >&3
  diff -u --label live --label restored "$cluster/$mode-live.fingerprint" "$cluster/$mode-restored.fingerprint" > "$cluster/fingerprint.diff" || true
  sed -n '1,100p' "$cluster/fingerprint.diff" >&3
  exit 1
 }
 printf '%s force-archive migration, role separation and administrator dump/restore passed.\n' "$mode" >&3
done
printf '0105 force-archive audit vocabulary and snapshots qualified without rewriting history or expanding authority.\n' >&3
