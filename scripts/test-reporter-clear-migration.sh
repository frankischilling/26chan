#!/usr/bin/env bash
# Owned synthetic clusters only. Bound qualification to 0105 -> 0106.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-reporter-clear.XXXXXXXX)
exec 3>&1
started=0
cleanup() {
    status=$?
    trap - EXIT
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null || status=1
    fi
    [[ $cluster =~ ^/tmp/board-reporter-clear\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
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
trap 'printf "Reporter-clear qualification failed at line %s (private diagnostics removed during cleanup).\n" "$LINENO" >&3' ERR
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
# Execute the application readiness query using both supported runtime roles.
python3 - "$cluster" <<'PYREADINESS'
import pathlib, re, sys
root = pathlib.Path(sys.argv[1])
source = pathlib.Path('crates/store/src/report_admission.rs').read_text()
match = re.search(r'pub const READINESS_SQL: &str = r#"(.*?)"#;', source, re.S)
if not match or 'content.clear_reporter(text,bigint)' not in match.group(1):
    raise SystemExit('Cannot extract 0106 report readiness')
query = match.group(1).strip().rstrip(';')
(root / 'readiness.sql').write_text("DO $check$ BEGIN IF (" + query + ") IS DISTINCT FROM true THEN RAISE EXCEPTION '0106 readiness failed'; END IF; END $check$;")
changes = [
    'ALTER FUNCTION content.clear_reporter(text,bigint) SECURITY INVOKER',
    'ALTER FUNCTION content.clear_reporter(text,bigint) SET search_path=public',
    'ALTER FUNCTION content.clear_reporter(text,bigint) OWNER TO board_migrator',
    'GRANT EXECUTE ON FUNCTION content.clear_reporter(text,bigint) TO board_public',
    'GRANT EXECUTE ON FUNCTION content.clear_reporter(text,bigint) TO board_auth',
    'REVOKE EXECUTE ON FUNCTION content.clear_reporter(text,bigint) FROM board_staff',
    'GRANT UPDATE(reporter_cleared_at) ON content.reports TO board_staff',
    'REVOKE UPDATE(reporter_cleared_at) ON content.reports FROM board_report_admission_owner',
    'ALTER TABLE content.reports ALTER COLUMN reporter_cleared_at SET DEFAULT clock_timestamp()',
    'ALTER TABLE content.moderation_audit DROP CONSTRAINT moderation_audit_reporter_clear_count',
]
with (root / 'readiness-drift.sql').open('w') as out:
    for i, change in enumerate(changes):
        out.write('BEGIN; ' + change + ';\n')
        for role in ('board_public', 'board_staff'):
            out.write(f"SET LOCAL ROLE {role}; DO $check$ BEGIN IF ({query}) IS DISTINCT FROM false THEN RAISE EXCEPTION '0106 readiness accepted drift {i}'; END IF; END $check$; RESET ROLE;\n")
        out.write('ROLLBACK;\n')
PYREADINESS
cat > "$cluster/fixture.sql" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('cleara','Clear fixture A','Synthetic',1000,100,100,100,10),('clearb','Clear fixture B','Synthetic',1000,100,100,100,10);
BEGIN;
SET LOCAL ROLE board_migrator;
SELECT set_config('board.posting_actor',repeat('11',32),true);
INSERT INTO content.threads(id,board) VALUES(10600001,'cleara'),(10600002,'clearb');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(10600001,'cleara',10600001,'Retained name','Retained subject','Retained body'),
(10600002,'clearb',10600002,'Second name','','Second body');
COMMIT;
SET ROLE board_migrator;
SELECT content.import_report_catalog('{"version":1,"categories":[{"id":31,"board":"","op_only":false,"reply_only":false,"image_only":false,"exclude_boards":null,"title":"Synthetic illegal","weight":1,"filtered":0}]}');
RESET ROLE;
-- Six reports: seed, same IP, same session, a nontransitive session neighbor,
-- an unrelated report and retained history without active membership.
INSERT INTO content.reports(id,board,post_id,reason,state,category_revision,category_id,category_kind,category_base_weight,created_at)
OVERRIDING SYSTEM VALUE
SELECT 10601000+i,CASE WHEN i=3 THEN 'clearb' ELSE 'cleara' END,
CASE WHEN i=3 THEN 10600002 ELSE 10600001 END,'Retained report',CASE WHEN i=2 THEN 'resolved' WHEN i=3 THEN 'dismissed' ELSE 'open' END,
CASE WHEN i IN(1,2,3) THEN 1 END,CASE WHEN i IN(1,2,3) THEN 31 END,
CASE WHEN i IN(1,2,3) THEN 2 END,CASE WHEN i IN(1,2,3) THEN 1.0 END,'2020-01-01 UTC'
FROM generate_series(1,6) i;
BEGIN;
SELECT slug FROM content.boards ORDER BY slug FOR UPDATE;
INSERT INTO post_secrets.report_membership(report_id,actor_hash,board,post_id,thread_id,reported_at,automatic_identity)
SELECT id,decode(repeat(CASE id WHEN 10601001 THEN 'aa' WHEN 10601002 THEN 'aa' WHEN 10601003 THEN 'bb' WHEN 10601004 THEN 'cc' ELSE 'dd' END,32),'hex'),
board,post_id,post_id,'2020-01-01 UTC',CASE id WHEN 10601001 THEN '00000000-0000-0000-0000-000000000001'::uuid WHEN 10601003 THEN '00000000-0000-0000-0000-000000000001'::uuid WHEN 10601002 THEN '00000000-0000-0000-0000-000000000002'::uuid WHEN 10601004 THEN '00000000-0000-0000-0000-000000000002'::uuid END
FROM content.reports WHERE id BETWEEN 10601001 AND 10601005;
COMMIT;
INSERT INTO post_secrets.report_weight_evidence(report_id,evaluator_version,known_or_verified,evaluated_at)
SELECT id,1,false,'2020-01-01 UTC' FROM content.reports;
INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,created_at,network_at,address_at,environment_at,expires_at,automatic_identity)
VALUES(decode(repeat('01',32),'hex'),decode(repeat('02',32),'hex'),decode(repeat('03',32),'hex'),decode(repeat('04',32),'hex'),1,1,1,1,4000000000,'00000000-0000-0000-0000-000000000001');
INSERT INTO post_secrets.anonymous_reports(report_id,token_hash)
VALUES(10601001,decode(repeat('01',32),'hex'));
INSERT INTO content.moderation_audit(account_id,board,target_id,action,created_at)
VALUES(1,'cleara',10600001,'resolve','2020-01-01 UTC');
INSERT INTO content.moderation_audit(account_id,board,target_id,action,before_mask,after_mask,created_at)
VALUES(1,'cleara',10600001,'thread-options',0,31,'2020-01-01 UTC');
INSERT INTO content.moderation_audit(account_id,board,target_id,action,snapshot_version,snapshot_name,snapshot_subject,snapshot_comment,snapshot_comment_format,snapshot_staff_authorized_limits,snapshot_wordfiltered,snapshot_image_spoiler,created_at)
VALUES(1,'cleara',10600001,'force-archive',1,'Saved name','Saved subject','Saved body',0,false,false,false,'2020-01-01 UTC');
SQL
# Freeze this migration boundary even when later migrations are added.
for mode in upgrade fresh; do
 database="reporter_clear_$mode"
 create_database "$database"
 migrator=("${psql[@]}" -U board_migrator -d "$database")
 admin=(runuser -u postgres -- "${psql[@]}" -d "$database")
 for migration in migrations/*.sql; do
  [[ $migration < migrations/0106_reporter_clear.sql ]] || break
  "${migrator[@]}" --single-transaction -f - < "$migration"
 done
 "${admin[@]}" <<'SQL'
CREATE FUNCTION public.capture_rows() RETURNS TABLE(relation text,value jsonb)
LANGUAGE plpgsql AS $$ DECLARE r record; BEGIN
 FOR r IN SELECT n.nspname,c.relname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission') AND c.relkind='r' LOOP
  RETURN QUERY EXECUTE format('SELECT %L,to_jsonb(r) FROM %I.%I r',r.nspname||'.'||r.relname,r.nspname,r.relname);
 END LOOP;
END $$;
REVOKE ALL ON FUNCTION public.capture_rows() FROM PUBLIC;
CREATE VIEW public.old_shape AS SELECT relation,CASE relation WHEN 'content.reports' THEN value-'reporter_cleared_at' WHEN 'content.moderation_audit' THEN value-'reporter_clear_count' ELSE value END value FROM public.capture_rows();
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
AND NOT (table_schema='content' AND ((table_name='reports' AND column_name='reporter_cleared_at') OR (table_name='moderation_audit' AND column_name='reporter_clear_count')))
UNION ALL
SELECT 'foreign-key',jsonb_build_array(c.conrelid::regclass::text,c.conname,pg_get_constraintdef(c.oid,true))
FROM pg_constraint c JOIN pg_namespace n ON n.oid=c.connamespace
WHERE c.contype='f' AND n.nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission')
UNION ALL
SELECT 'old-constraint',jsonb_build_array(c.conrelid::regclass::text,c.conname,pg_get_constraintdef(c.oid,true))
FROM pg_constraint c JOIN pg_namespace n ON n.oid=c.connamespace
WHERE n.nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission')
AND NOT (c.conrelid='content.moderation_audit'::regclass AND c.conname IN ('moderation_audit_action_check','moderation_audit_reporter_clear_count'))
UNION ALL
SELECT 'old-column',jsonb_build_array(n.nspname,c.relname,a.attname,format_type(a.atttypid,a.atttypmod),
 a.attnotnull,a.attidentity,a.attgenerated,pg_get_expr(d.adbin,d.adrelid))
FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace
LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
WHERE a.attnum>0 AND NOT a.attisdropped
AND NOT (n.nspname='content' AND ((c.relname='reports' AND a.attname='reporter_cleared_at') OR (c.relname='moderation_audit' AND a.attname='reporter_clear_count')))
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
AND d.refobjsubid NOT IN (SELECT attnum FROM pg_attribute WHERE attrelid='content.moderation_audit'::regclass AND attname='reporter_clear_count')
AND NOT (d.classid='pg_constraint'::regclass AND d.objid IN
 (SELECT oid FROM pg_constraint WHERE conrelid='content.moderation_audit'::regclass
  AND conname IN('moderation_audit_action_check','moderation_audit_reporter_clear_count')));
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
 if [[ $mode = upgrade ]]; then "${admin[@]}" -f - < "$cluster/fixture.sql"; fi
 "${admin[@]}" -c 'CREATE TABLE public.before_rows AS TABLE public.old_shape; CREATE TABLE public.before_metadata AS TABLE public.unchanged_metadata; CREATE TABLE public.before_functions AS TABLE public.functions; CREATE TABLE public.before_dependencies AS TABLE public.audit_dependencies'
 "${migrator[@]}" --single-transaction -f - < migrations/0106_reporter_clear.sql
 "${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(TABLE public.before_rows EXCEPT ALL TABLE public.old_shape)
 OR EXISTS(TABLE public.old_shape EXCEPT ALL TABLE public.before_rows)
 THEN RAISE EXCEPTION '0106 changed retained rows'; END IF;
 IF EXISTS(TABLE public.before_metadata EXCEPT ALL TABLE public.unchanged_metadata)
 OR EXISTS(TABLE public.unchanged_metadata EXCEPT ALL TABLE public.before_metadata)
 OR EXISTS(TABLE public.before_functions EXCEPT ALL SELECT * FROM public.functions WHERE signature<>'content.clear_reporter(text,bigint)')
 OR EXISTS(SELECT * FROM public.functions WHERE signature<>'content.clear_reporter(text,bigint)' EXCEPT ALL TABLE public.before_functions)
 OR EXISTS(TABLE public.before_dependencies EXCEPT ALL TABLE public.audit_dependencies)
 OR EXISTS(TABLE public.audit_dependencies EXCEPT ALL TABLE public.before_dependencies)
 THEN RAISE EXCEPTION '0106 changed existing authority or metadata'; END IF;
 IF EXISTS(SELECT 1 FROM content.reports WHERE reporter_cleared_at IS NOT NULL)
 OR EXISTS(SELECT 1 FROM content.moderation_audit WHERE reporter_clear_count IS NOT NULL)
 THEN RAISE EXCEPTION '0106 backfilled nullable fields'; END IF;
END $$;
DROP TABLE public.before_rows,public.before_metadata,public.before_functions,public.before_dependencies;
SQL
 if [[ $mode = fresh ]]; then "${admin[@]}" -f - < "$cluster/fixture.sql"; fi
 "${admin[@]}" <<'SQL'
CREATE TABLE public.retained AS SELECT * FROM public.old_shape WHERE relation NOT IN('post_secrets.report_membership','post_secrets.report_group');
CREATE TABLE public.groups_before AS TABLE post_secrets.report_group;
SQL
 # Rollback covers the helper's membership, marker and group mutations together.
 "${psql[@]}" -U board_staff -d "$database" <<'SQL'
BEGIN;
DO $$ BEGIN
 IF content.clear_reporter('cleara',10601001) IS DISTINCT FROM 3::bigint
 THEN RAISE EXCEPTION 'Unexpected clear count'; END IF;
END $$;
ROLLBACK;
BEGIN ISOLATION LEVEL REPEATABLE READ;
DO $$ BEGIN
 BEGIN
  PERFORM content.clear_reporter('cleara',10601001);
  RAISE EXCEPTION 'Non-RC clear accepted' USING ERRCODE='ZX001';
 EXCEPTION WHEN invalid_parameter_value THEN NULL; END;
END $$;
ROLLBACK;
SQL
 "${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF (SELECT count(*) FROM post_secrets.report_membership)<>5
 OR EXISTS(SELECT 1 FROM content.reports WHERE reporter_cleared_at IS NOT NULL)
 OR EXISTS(TABLE public.groups_before EXCEPT ALL TABLE post_secrets.report_group)
 OR EXISTS(TABLE post_secrets.report_group EXCEPT ALL TABLE public.groups_before)
 THEN RAISE EXCEPTION 'Clear rollback changed state'; END IF;
END $$;
SQL
 "${psql[@]}" -U board_staff -d "$database" <<'SQL'
BEGIN;
DO $$ DECLARE n bigint; BEGIN
 n:=content.clear_reporter('cleara',10601001);
 IF n IS DISTINCT FROM 3::bigint THEN RAISE EXCEPTION 'Wrong committed clear count'; END IF;
 INSERT INTO content.moderation_audit(account_id,board,target_id,action,reporter_clear_count)
 VALUES(1,'cleara',10601001,'reporter-clear',n);
END $$;
COMMIT;
SQL
 for phase in live restored; do
  if [[ $phase = restored ]]; then
   runuser -u postgres -- "$pg_bin/pg_dump" -h "$cluster" -U postgres -d "$database" --format=custom > "$cluster/current.dump"
   create_database "${database}_restore"
   runuser -u postgres -- "$pg_bin/pg_restore" -h "$cluster" --dbname="${database}_restore" --single-transaction --exit-on-error < "$cluster/current.dump"
   database="${database}_restore"
   admin=(runuser -u postgres -- "${psql[@]}" -d "$database")
  fi
  "${admin[@]}" <<'SQL'
DO $$ DECLARE role_name text; BEGIN
 IF (SELECT count(*) FROM pg_attribute WHERE (attrelid,attname) IN
  (('content.reports'::regclass,'reporter_cleared_at'),('content.moderation_audit'::regclass,'reporter_clear_count'))
  AND NOT attnotnull AND NOT atthasdef AND NOT attisdropped)<>2
 OR (SELECT atttypid FROM pg_attribute WHERE attrelid='content.reports'::regclass AND attname='reporter_cleared_at')<>'timestamptz'::regtype
 OR (SELECT atttypid FROM pg_attribute WHERE attrelid='content.moderation_audit'::regclass AND attname='reporter_clear_count')<>'bigint'::regtype
 THEN RAISE EXCEPTION 'Unexpected new columns'; END IF;
 IF NOT EXISTS(SELECT 1 FROM pg_proc WHERE oid='content.clear_reporter(text,bigint)'::regprocedure
  AND proowner='board_report_admission_owner'::regrole AND prosecdef
  AND proconfig=ARRAY['search_path=pg_catalog, pg_temp'] AND prorettype='bigint'::regtype)
 OR NOT has_function_privilege('board_staff','content.clear_reporter(text,bigint)','EXECUTE')
 OR has_schema_privilege('board_report_admission_owner','content','CREATE')
 OR NOT has_column_privilege('board_report_admission_owner','content.reports','reporter_cleared_at','UPDATE')
 OR NOT has_column_privilege('board_report_admission_owner','content.reports','reporter_cleared_at','SELECT')
 THEN RAISE EXCEPTION 'Helper ownership, path or authority changed'; END IF;
 FOREACH role_name IN ARRAY ARRAY['board_public','board_auth','board_migrator'] LOOP
  IF has_function_privilege(role_name,'content.clear_reporter(text,bigint)','EXECUTE')
  THEN RAISE EXCEPTION 'Unexpected helper execution grant'; END IF;
 END LOOP;
 IF EXISTS(SELECT 1 FROM aclexplode((SELECT proacl FROM pg_proc WHERE oid='content.clear_reporter(text,bigint)'::regprocedure)) WHERE grantee=0)
 THEN RAISE EXCEPTION 'PUBLIC helper execution granted'; END IF;
 IF (SELECT array_agg(id ORDER BY id) FROM content.reports WHERE reporter_cleared_at IS NOT NULL) IS DISTINCT FROM ARRAY[10601001,10601002,10601003]::bigint[]
 OR (SELECT array_agg(report_id ORDER BY report_id) FROM post_secrets.report_membership) IS DISTINCT FROM ARRAY[10601004,10601005]::bigint[]
 OR EXISTS(SELECT 1 FROM post_secrets.report_group WHERE board='clearb')
 OR NOT EXISTS(SELECT 1 FROM post_secrets.report_group WHERE board='cleara' AND illegal_count=2 AND incomplete)
 THEN RAISE EXCEPTION 'Wrong identity scope, marker set, partial count or empty group'; END IF;
 IF EXISTS(TABLE public.retained EXCEPT ALL SELECT * FROM public.old_shape)
 OR EXISTS(SELECT * FROM public.old_shape WHERE relation NOT IN('post_secrets.report_membership','post_secrets.report_group')
  AND NOT (relation='content.moderation_audit' AND value->>'action'='reporter-clear') EXCEPT ALL TABLE public.retained)
 THEN RAISE EXCEPTION 'Clearing changed reports, state, identity, evidence or history'; END IF;
 IF NOT EXISTS(SELECT 1 FROM content.moderation_audit WHERE action='reporter-clear' AND reporter_clear_count=3
 AND before_mask IS NULL AND after_mask IS NULL AND snapshot_version IS NULL)
 THEN RAISE EXCEPTION 'Missing clear audit'; END IF;
END $$;
SQL
  for role in board_public board_staff board_auth; do
   "${psql[@]}" -U "$role" -d "$database" <<'SQL'
DO $$ DECLARE relation_name text; BEGIN
 FOREACH relation_name IN ARRAY ARRAY['post_secrets.report_membership','post_secrets.report_group','post_secrets.report_weight_evidence','post_secrets.anonymous_sessions','post_secrets.anonymous_reports'] LOOP
  BEGIN
   EXECUTE 'SELECT * FROM '||relation_name||' LIMIT 1';
   RAISE EXCEPTION 'Runtime private identity access' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 END LOOP;
 BEGIN
  UPDATE content.reports SET reporter_cleared_at=clock_timestamp() WHERE false;
  RAISE EXCEPTION 'Runtime raw marker update' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 IF current_user<>'board_staff' THEN
  BEGIN
   PERFORM content.clear_reporter('cleara',10601004);
   RAISE EXCEPTION 'Runtime unauthorized clear' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 END IF;
END $$;
SQL
  done
  "${psql[@]}" -U board_staff -d "$database" <<'SQL'
BEGIN;
DO $$ DECLARE patch jsonb; BEGIN
 IF content.clear_reporter('cleara',10601001) IS NOT NULL
 OR content.clear_reporter('clearb',10601004) IS NOT NULL
 OR content.clear_reporter('cleara',10601006) IS NOT NULL
 THEN RAISE EXCEPTION 'Missing/wrong-board/historical seed did not return NULL'; END IF;
 -- Remaining NULL-session report matches only its own IP, never other NULLs.
 IF content.clear_reporter('cleara',10601005) IS DISTINCT FROM 1::bigint
 OR content.clear_reporter('cleara',10601004) IS DISTINCT FROM 1::bigint
 THEN RAISE EXCEPTION 'Remaining identity groups cleared incorrectly'; END IF;
 INSERT INTO content.moderation_audit(id,account_id,board,target_id,action,reporter_clear_count)
 OVERRIDING SYSTEM VALUE VALUES(10609991,1,'cleara',10601001,'reporter-clear',1),
 (10609992,1,'cleara',10601001,'reporter-clear',10000);
 FOR patch IN SELECT value FROM jsonb_array_elements('[
 {"reporter_clear_count":null},{"reporter_clear_count":0},{"reporter_clear_count":-1},{"reporter_clear_count":10001},
 {"action":"resolve"},{"action":"unknown"},{"before_mask":0},{"after_mask":1},
 {"snapshot_version":1},{"snapshot_name":"Invalid"},{"snapshot_trip":"!0123456789"},
 {"snapshot_version":1,"snapshot_name":"","snapshot_subject":"","snapshot_comment":"","snapshot_comment_format":0,"snapshot_staff_authorized_limits":false,"snapshot_wordfiltered":false,"snapshot_image_spoiler":false}
 ]'::jsonb) LOOP
  BEGIN
   INSERT INTO content.moderation_audit OVERRIDING SYSTEM VALUE
   SELECT * FROM jsonb_populate_record(NULL::content.moderation_audit,
    '{"id":10609993,"account_id":1,"board":"cleara","target_id":10601001,"action":"reporter-clear","reporter_clear_count":1,"created_at":"2020-01-01 UTC"}'::jsonb||patch);
   RAISE EXCEPTION 'Invalid clear audit shape accepted' USING ERRCODE='ZX001';
  EXCEPTION WHEN check_violation THEN NULL; END;
 END LOOP;
 BEGIN
  UPDATE content.moderation_audit SET reporter_clear_count=2 WHERE action='reporter-clear';
  RAISE EXCEPTION 'Staff changed immutable audit' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
ROLLBACK;
SQL
  for role in board_public board_staff; do
   "${psql[@]}" -U "$role" -d "$database" -f - < "$cluster/readiness.sql"
  done
  "${admin[@]}" -f - < "$cluster/readiness-drift.sql"
  # Cap fixtures stay inside rollback-only transactions; neither identity
  # sequence values nor the live/restore fingerprint acquire synthetic rows.
  "${admin[@]}" <<'SQL'
BEGIN;
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
SELECT 'cap'||lpad(i::text,4,'0'),'Board cap fixture','Synthetic',1000,100,100,100,10
FROM generate_series(1,513-(SELECT count(*) FROM content.boards)) i;
CREATE TEMP TABLE cap_rows_before ON COMMIT DROP AS SELECT * FROM public.capture_rows();
DO $$ BEGIN
 IF (SELECT count(*) FROM content.boards)<>513 THEN RAISE EXCEPTION 'Board cap fixture size changed'; END IF;
END $$;
SET LOCAL ROLE board_staff;
DO $$ BEGIN
 BEGIN
  PERFORM content.clear_reporter('cleara',10601004);
  RAISE EXCEPTION 'Board cap was not enforced' USING ERRCODE='ZX001';
 EXCEPTION WHEN SQLSTATE '54000' THEN NULL; END;
END $$;
RESET ROLE;
DO $$ BEGIN
 IF EXISTS(TABLE cap_rows_before EXCEPT ALL SELECT * FROM public.capture_rows())
 OR EXISTS(SELECT * FROM public.capture_rows() EXCEPT ALL TABLE cap_rows_before)
 THEN RAISE EXCEPTION 'Board cap failure changed retained or active state'; END IF;
END $$;
ROLLBACK;
BEGIN;
-- Owner bulk maintenance prelocks every affected board before report/member
-- writes. Normal membership triggers create the fixture group contribution.
SELECT slug FROM content.boards ORDER BY slug FOR UPDATE;
INSERT INTO content.reports(id,board,post_id,reason,created_at)
OVERRIDING SYSTEM VALUE
SELECT 10620000+i,'cleara',10600001,'Bounded cap fixture','2020-01-01 UTC'
FROM generate_series(1,10001) i;
INSERT INTO post_secrets.report_membership(report_id,actor_hash,board,post_id,thread_id,reported_at)
SELECT 10620000+i,decode(repeat('ff',32),'hex'),'cleara',10600001,10600001,'2020-01-01 UTC'
FROM generate_series(1,10001) i;
CREATE TEMP TABLE cap_rows_before ON COMMIT DROP AS SELECT * FROM public.capture_rows();
DO $$ BEGIN
 IF (SELECT count(*) FROM post_secrets.report_membership WHERE actor_hash=decode(repeat('ff',32),'hex'))<>10001
 THEN RAISE EXCEPTION 'Membership cap fixture size changed'; END IF;
END $$;
SET LOCAL ROLE board_staff;
DO $$ BEGIN
 BEGIN
  PERFORM content.clear_reporter('cleara',10620001);
  RAISE EXCEPTION 'Membership cap was not enforced' USING ERRCODE='ZX001';
 EXCEPTION WHEN SQLSTATE '54000' THEN NULL; END;
END $$;
RESET ROLE;
DO $$ BEGIN
 IF EXISTS(TABLE cap_rows_before EXCEPT ALL SELECT * FROM public.capture_rows())
 OR EXISTS(SELECT * FROM public.capture_rows() EXCEPT ALL TABLE cap_rows_before)
 THEN RAISE EXCEPTION 'Membership cap failure changed markers, audit, membership or evidence'; END IF;
END $$;
ROLLBACK;
SQL
  "${admin[@]}" -At > "$cluster/$mode-$phase.fingerprint" <<'SQL'
SELECT relation,md5(value::text) FROM public.capture_rows() ORDER BY relation,value::text;
SELECT * FROM public.functions ORDER BY nspname,signature;
SELECT * FROM public.audit_dependencies ORDER BY object,referenced,deptype;
SELECT * FROM public.unchanged_metadata ORDER BY kind,value::text;
SELECT table_schema,table_name,column_name,grantor,grantee,privilege_type,is_grantable
 FROM information_schema.column_privileges WHERE table_schema IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission') ORDER BY 1,2,3,4,5,6,7;
SELECT c.conrelid::regclass,c.conname,pg_get_constraintdef(c.oid,true) FROM pg_constraint c JOIN pg_namespace n ON n.oid=c.connamespace
 WHERE n.nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission') ORDER BY c.conrelid::regclass::text,c.conname;
SELECT schemaname,indexname,indexdef FROM pg_indexes WHERE schemaname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission') ORDER BY 1,2;
SELECT last_value,is_called FROM content.moderation_audit_id_seq;
SELECT rolname,rolsuper,rolinherit,rolcreaterole,rolcreatedb,rolcanlogin,rolreplication,rolbypassrls,rolconfig FROM pg_roles WHERE rolname LIKE 'board_%' ORDER BY rolname;
SQL
 done
 cmp -s "$cluster/$mode-live.fingerprint" "$cluster/$mode-restored.fingerprint" || {
  printf 'Reporter-clear dump/restore fingerprint mismatch (%s).\n' "$mode" >&3; exit 1;
 }
 printf '%s reporter-clear migration, runtime authority and administrator dump/restore passed.\n' "$mode" >&3
done
printf '0106 reporter clear qualified with retained history, bounded identity scope and strict audit shapes.\n' >&3
