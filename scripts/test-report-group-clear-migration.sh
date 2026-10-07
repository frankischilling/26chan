#!/usr/bin/env bash
# Owned synthetic clusters only. Bound qualification to 0107 -> 0108.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-report-group-clear.XXXXXXXX)
exec 3>&1
started=0
cleanup() {
    status=$?
    trap - EXIT
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null || status=1
    fi
    [[ $cluster =~ ^/tmp/board-report-group-clear\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
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
trap 'printf "Report-group-clear qualification failed at line %s (private diagnostics removed during cleanup).\n" "$LINENO" >&3' ERR
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
# Extract current application readiness; no old application executes here.
python3 - "$cluster" <<'PYREADINESS'
import pathlib,re,sys
root=pathlib.Path(sys.argv[1])
match=re.search(r'pub const READINESS_SQL: &str = r#"(.*?)"#;',pathlib.Path('crates/store/src/report_admission.rs').read_text(),re.S)
if not match: raise SystemExit('Cannot extract report readiness')
query=match.group(1)
if 'content.clear_report_group(text,bigint,bigint)' not in query: raise SystemExit('Missing group-clear readiness')
(root/'readiness.sql').write_text("DO $check$ BEGIN IF ("+query+") IS DISTINCT FROM true THEN RAISE EXCEPTION 'Current readiness failed'; END IF; END $check$;")
changes=[
 'ALTER FUNCTION content.clear_report_group(text,bigint,bigint) SECURITY INVOKER',
 'ALTER FUNCTION content.clear_report_group(text,bigint,bigint) SET search_path=public',
 'ALTER FUNCTION content.clear_report_group(text,bigint,bigint) OWNER TO board_migrator',
 'GRANT EXECUTE ON FUNCTION content.clear_report_group(text,bigint,bigint) TO PUBLIC',
 'GRANT EXECUTE ON FUNCTION content.clear_report_group(text,bigint,bigint) TO board_auth',
 'REVOKE EXECUTE ON FUNCTION content.clear_report_group(text,bigint,bigint) FROM board_staff',
 'GRANT UPDATE(group_cleared_at) ON content.reports TO board_staff',
 'GRANT INSERT(group_cleared_by) ON content.reports TO board_staff',
 'REVOKE UPDATE(group_cleared_at) ON content.reports FROM board_report_admission_owner',
 'ALTER TABLE content.reports ALTER COLUMN group_clear_inherited SET DEFAULT false',
 'ALTER TABLE content.reports DROP CONSTRAINT reports_group_clear_complete',
 'ALTER TABLE post_secrets.report_group DROP CONSTRAINT report_group_clear_complete',
 'ALTER TABLE post_secrets.report_group ALTER COLUMN cleared_by SET DEFAULT 1',
 'ALTER TABLE content.moderation_audit DROP CONSTRAINT moderation_audit_group_clear_count',
 'ALTER TABLE content.moderation_audit ALTER COLUMN group_clear_count SET DEFAULT 1',
 'DROP INDEX content.reports_group_clear_history',
]
with (root/'drift.sql').open('w') as out:
 for i,change in enumerate(changes):
  out.write('BEGIN; '+change+';\n')
  for role in ('board_public','board_staff'):
   out.write(f"SET LOCAL ROLE {role}; DO $check$ BEGIN IF ({query}) IS DISTINCT FROM false THEN RAISE EXCEPTION 'Readiness accepted group-clear drift {i}'; END IF; END $check$; RESET ROLE;\n")
  out.write('ROLLBACK;\n')
PYREADINESS
cat > "$cluster/fixture.sql" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('groupclear','Owned clear fixture','Synthetic',1000,100,100,100,10),('groupother','Other fixture','Synthetic',1000,100,100,100,10);
BEGIN;
SET LOCAL ROLE board_migrator;
SELECT set_config('board.posting_actor',repeat('11',32),true);
INSERT INTO content.threads(id,board) VALUES(10800001,'groupclear'),(10800010,'groupother');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
SELECT 10800000+i,'groupclear',10800001,'Retained name','Retained subject','Retained body' FROM generate_series(1,5) i;
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(10800010,'groupother',10800010,'Other name','','Other body');
COMMIT;
SET ROLE board_migrator;
SELECT content.import_report_catalog('{"version":1,"categories":[{"id":1,"board":"","op_only":false,"reply_only":false,"image_only":false,"exclude_boards":null,"title":"Synthetic half","weight":0.5,"filtered":0}]}');
RESET ROLE;
-- Two complete weights, unknown numeric evidence, missing evidence, detached
-- retained history and a distinct board. No production catalog or identity.
INSERT INTO content.reports(id,board,post_id,reason,state,category_revision,category_id,category_kind,category_base_weight,created_at)
OVERRIDING SYSTEM VALUE
SELECT 10801000+i,CASE WHEN i=6 THEN 'groupother' ELSE 'groupclear' END,
CASE i WHEN 1 THEN 10800001 WHEN 2 THEN 10800001 WHEN 3 THEN 10800002 WHEN 4 THEN 10800003 WHEN 5 THEN 10800001 ELSE 10800010 END,
'Retained report',CASE i WHEN 2 THEN 'resolved' WHEN 5 THEN 'dismissed' ELSE 'open' END,1,1,1,0.5,'2020-01-01 UTC'
FROM generate_series(1,6) i;
BEGIN;
SELECT slug FROM content.boards ORDER BY slug FOR UPDATE;
INSERT INTO post_secrets.report_membership(report_id,actor_hash,board,post_id,thread_id,reported_at)
SELECT id,decode(repeat(lpad((id-10801000)::text,2,'0'),32),'hex'),board,post_id,
CASE WHEN board='groupclear' THEN 10800001 ELSE 10800010 END,'2020-01-01 UTC'
FROM content.reports WHERE id BETWEEN 10801001 AND 10801006 AND id<>10801005;
COMMIT;
INSERT INTO post_secrets.report_weight_evidence(report_id,evaluator_version,known_or_verified,effective_weight,numeric_proof,evaluated_at)
SELECT id,1,false,CASE WHEN id<>10801003 THEN 0.5 END,
CASE WHEN id<>10801003 THEN 'BaseEqualsFallback' END,'2020-01-01 UTC'
FROM content.reports WHERE id BETWEEN 10801001 AND 10801006 AND id<>10801004;
INSERT INTO content.moderation_audit(account_id,board,target_id,action,created_at)
VALUES(1,'groupclear',10800001,'resolve','2020-01-01 UTC');
INSERT INTO content.moderation_audit(account_id,board,target_id,action,reporter_clear_count,created_at)
VALUES(1,'groupclear',10801005,'reporter-clear',1,'2020-01-01 UTC');
SQL
for mode in upgrade fresh; do
 database="report_group_clear_$mode"
 create_database "$database"
 migrator=("${psql[@]}" -U board_migrator -d "$database")
 admin=(runuser -u postgres -- "${psql[@]}" -d "$database")
 for migration in migrations/*.sql; do
  [[ $migration < migrations/0108_report_group_clear.sql ]] || break
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
CREATE VIEW public.old_shape AS SELECT relation,CASE relation WHEN 'content.reports' THEN value-'group_cleared_at'-'group_cleared_by'-'group_clear_inherited' WHEN 'content.moderation_audit' THEN value-'group_clear_count' WHEN 'post_secrets.report_group' THEN value-'cleared_at'-'cleared_by' ELSE value END value FROM public.capture_rows();
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
AND NOT (n.nspname='content' AND c.relname='reports_group_clear_history')
UNION ALL
SELECT 'column-grant',to_jsonb(g)-'table_catalog' FROM information_schema.column_privileges g
WHERE table_schema IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission')
AND NOT (table_schema='content' AND table_name='reports' AND column_name IN ('board','post_id') AND grantee='board_report_admission_owner' AND privilege_type='SELECT')
AND NOT ((table_schema='content' AND ((table_name='reports' AND column_name IN ('group_cleared_at','group_cleared_by','group_clear_inherited')) OR (table_name='moderation_audit' AND column_name='group_clear_count'))) OR (table_schema='post_secrets' AND table_name='report_group' AND column_name IN ('cleared_at','cleared_by')))
UNION ALL
SELECT 'foreign-key',jsonb_build_array(c.conrelid::regclass::text,c.conname,pg_get_constraintdef(c.oid,true))
FROM pg_constraint c JOIN pg_namespace n ON n.oid=c.connamespace
WHERE c.contype='f' AND n.nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission')
UNION ALL
SELECT 'old-constraint',jsonb_build_array(c.conrelid::regclass::text,c.conname,pg_get_constraintdef(c.oid,true))
FROM pg_constraint c JOIN pg_namespace n ON n.oid=c.connamespace
WHERE n.nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission')
AND NOT (c.conrelid='content.moderation_audit'::regclass AND c.conname IN ('moderation_audit_action_check','moderation_audit_group_clear_count'))
AND c.conname NOT IN ('reports_group_clear_complete','report_group_clear_complete')
UNION ALL
SELECT 'old-column',jsonb_build_array(n.nspname,c.relname,a.attname,format_type(a.atttypid,a.atttypmod),
 a.attnotnull,a.attidentity,a.attgenerated,pg_get_expr(d.adbin,d.adrelid))
FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace
LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
WHERE a.attnum>0 AND NOT a.attisdropped
AND NOT (n.nspname='content' AND c.relname='reports_group_clear_history')
AND NOT ((n.nspname='content' AND ((c.relname='reports' AND a.attname IN ('group_cleared_at','group_cleared_by','group_clear_inherited')) OR (c.relname='moderation_audit' AND a.attname='group_clear_count'))) OR (n.nspname='post_secrets' AND c.relname='report_group' AND a.attname IN ('cleared_at','cleared_by')))
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
AND d.refobjsubid NOT IN (SELECT attnum FROM pg_attribute WHERE attrelid='content.moderation_audit'::regclass AND attname='group_clear_count')
AND NOT (d.classid='pg_constraint'::regclass AND d.objid IN
 (SELECT oid FROM pg_constraint WHERE conrelid='content.moderation_audit'::regclass
  AND conname IN('moderation_audit_action_check','moderation_audit_group_clear_count')));
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
 "${migrator[@]}" --single-transaction -f - < migrations/0108_report_group_clear.sql
 "${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(TABLE public.before_rows EXCEPT ALL TABLE public.old_shape)
 OR EXISTS(TABLE public.old_shape EXCEPT ALL TABLE public.before_rows)
 THEN RAISE EXCEPTION '0108 changed retained rows'; END IF;
 IF EXISTS(TABLE public.before_metadata EXCEPT ALL TABLE public.unchanged_metadata)
 OR EXISTS(TABLE public.unchanged_metadata EXCEPT ALL TABLE public.before_metadata)
 OR EXISTS(TABLE public.before_dependencies EXCEPT ALL TABLE public.audit_dependencies)
 OR EXISTS(TABLE public.audit_dependencies EXCEPT ALL TABLE public.before_dependencies)
 THEN RAISE EXCEPTION '0108 changed existing authority or metadata'; END IF;
 IF EXISTS(SELECT nspname,signature,owner,acl,config FROM public.before_functions EXCEPT ALL
 SELECT nspname,signature,owner,acl,config FROM public.functions WHERE signature<>'content.clear_report_group(text,bigint,bigint)')
 OR EXISTS(SELECT nspname,signature,owner,acl,config FROM public.functions WHERE signature<>'content.clear_report_group(text,bigint,bigint)' EXCEPT ALL
 SELECT nspname,signature,owner,acl,config FROM public.before_functions)
 OR EXISTS(SELECT * FROM public.before_functions WHERE signature<>'post_secrets.increment_report_group()' EXCEPT ALL
 SELECT * FROM public.functions WHERE signature NOT IN ('post_secrets.increment_report_group()','content.clear_report_group(text,bigint,bigint)'))
 OR EXISTS(SELECT * FROM public.functions WHERE signature NOT IN ('post_secrets.increment_report_group()','content.clear_report_group(text,bigint,bigint)') EXCEPT ALL
 SELECT * FROM public.before_functions WHERE signature<>'post_secrets.increment_report_group()')
 THEN RAISE EXCEPTION '0108 changed unrelated functions or existing function authority'; END IF;
 IF EXISTS(SELECT 1 FROM content.reports WHERE group_cleared_at IS NOT NULL OR group_cleared_by IS NOT NULL OR group_clear_inherited IS NOT NULL)
 OR EXISTS(SELECT 1 FROM content.moderation_audit WHERE group_clear_count IS NOT NULL)
 OR EXISTS(SELECT 1 FROM post_secrets.report_group WHERE cleared_at IS NOT NULL OR cleared_by IS NOT NULL)
 THEN RAISE EXCEPTION '0108 invented clear history'; END IF;
END $$;
DROP TABLE public.before_rows,public.before_metadata,public.before_functions,public.before_dependencies;
SQL
 if [[ $mode = fresh ]]; then "${admin[@]}" -f - < "$cluster/fixture.sql"; fi
 "${admin[@]}" <<'SQL'
CREATE TABLE public.before_clear AS SELECT * FROM public.capture_rows();
CREATE TABLE public.retained AS SELECT * FROM public.old_shape
WHERE relation NOT IN ('post_secrets.report_group','post_secrets.report_admission_gate');
SQL
 "${psql[@]}" -U board_staff -d "$database" <<'SQL'
BEGIN;
DO $$ BEGIN
 IF content.clear_report_group('groupclear',10800001,7) IS DISTINCT FROM 2::bigint THEN RAISE EXCEPTION 'Wrong rollback clear count'; END IF;
 INSERT INTO content.moderation_audit(account_id,board,target_id,action,group_clear_count)
 VALUES(7,'groupclear',10800001,'report-group-clear',2);
END $$;
ROLLBACK;
BEGIN ISOLATION LEVEL REPEATABLE READ;
DO $$ BEGIN
 BEGIN
  PERFORM content.clear_report_group('groupclear',10800001,7);
  RAISE EXCEPTION 'Non-RC clear accepted' USING ERRCODE='ZX001';
 EXCEPTION WHEN invalid_parameter_value THEN NULL; END;
END $$;
ROLLBACK;
DO $$ DECLARE target bigint; BEGIN
 FOREACH target IN ARRAY ARRAY[10800002,10800003]::bigint[] LOOP
  BEGIN
   PERFORM content.clear_report_group('groupclear',target,7);
   RAISE EXCEPTION 'Unknown weight accepted' USING ERRCODE='ZX001';
  EXCEPTION WHEN SQLSTATE 'P0108' THEN NULL; END;
 END LOOP;
 -- An audit error must unwind the helper in the same subtransaction.
 BEGIN
  PERFORM content.clear_report_group('groupclear',10800001,7);
  INSERT INTO content.moderation_audit(account_id,board,target_id,action,group_clear_count)
  VALUES(7,'groupclear',10800001,'report-group-clear',0);
  RAISE EXCEPTION 'Invalid audit accepted' USING ERRCODE='ZX001';
 EXCEPTION WHEN check_violation THEN NULL; END;
 IF content.clear_report_group('groupother',10800001,7) IS NOT NULL
 OR content.clear_report_group('missing',10800001,7) IS NOT NULL
 OR content.clear_report_group('groupclear',10800005,7) IS NOT NULL
 THEN RAISE EXCEPTION 'Missing group matched'; END IF;
END $$;
SQL
 "${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(TABLE public.before_clear EXCEPT ALL SELECT * FROM public.capture_rows())
 OR EXISTS(SELECT * FROM public.capture_rows() EXCEPT ALL TABLE public.before_clear)
 THEN RAISE EXCEPTION 'Rollback or rejected clear changed state'; END IF;
END $$;
SQL
 "${psql[@]}" -U board_staff -d "$database" <<'SQL'
BEGIN;
DO $$ DECLARE n bigint; BEGIN
 n:=content.clear_report_group('groupclear',10800001,7);
 IF n IS DISTINCT FROM 2::bigint THEN RAISE EXCEPTION 'Wrong committed clear count'; END IF;
 INSERT INTO content.moderation_audit(account_id,board,target_id,action,group_clear_count)
 VALUES(7,'groupclear',10800001,'report-group-clear',n);
 IF content.clear_report_group('groupclear',10800001,8) IS DISTINCT FROM 0::bigint THEN RAISE EXCEPTION 'Clear was not idempotent'; END IF;
END $$;
COMMIT;
-- First overload: authenticated legacy staff report, no numeric evidence.
SELECT content.admit_report('groupclear',10800001,'Inherited legacy',decode(repeat('41',32),'hex'));
SQL
 # Second overload remains free-text until the controlled catalog switch.
 "${psql[@]}" -U board_public -d "$database" <<'SQL'
SELECT content.admit_report('groupclear',10800001,'Inherited anonymous',decode(repeat('42',32),'hex'),
 decode(repeat('52',32),'hex'),decode(repeat('62',32),'hex'),decode(repeat('72',32),'hex'),decode(repeat('82',32),'hex'),true,extract(epoch FROM clock_timestamp())::bigint);
SQL
 "${migrator[@]}" -c 'SELECT content.set_report_catalog_active(1)'
 "${psql[@]}" -U board_public -d "$database" <<'SQL'
SELECT content.admit_categorical_report('groupclear',10800001,1,1,decode(repeat('43',32),'hex'),
 decode(repeat('53',32),'hex'),decode(repeat('63',32),'hex'),decode(repeat('73',32),'hex'),decode(repeat('83',32),'hex'),true,extract(epoch FROM clock_timestamp())::bigint);
SQL
 "${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF (SELECT count(*) FROM content.reports WHERE group_clear_inherited=false)<>2
 OR (SELECT count(*) FROM content.reports WHERE group_clear_inherited=true)<>3
 OR EXISTS(SELECT 1 FROM content.reports r JOIN post_secrets.report_group g ON g.board=r.board AND g.post_id=r.post_id
 WHERE r.group_cleared_at IS NOT NULL AND (r.group_cleared_at IS DISTINCT FROM g.cleared_at OR r.group_cleared_by IS DISTINCT FROM 7::bigint))
 OR EXISTS(SELECT 1 FROM content.reports WHERE id IN(10801003,10801004,10801005,10801006) AND group_cleared_at IS NOT NULL)
 OR (SELECT count(*) FROM post_secrets.report_membership WHERE board='groupclear' AND post_id=10800001)<>5
 OR (SELECT count(*) FROM content.moderation_audit WHERE action='report-group-clear' AND group_clear_count=2 AND account_id=7)<>1
 THEN RAISE EXCEPTION 'Clear scope, exact count, original actor/time or three-overload inheritance failed'; END IF;
 IF EXISTS(TABLE public.retained EXCEPT ALL TABLE public.old_shape)
 THEN RAISE EXCEPTION 'Clearing rewrote preexisting report/evidence/audit/session history'; END IF;
END $$;
DROP TABLE public.before_clear,public.retained;
SQL
 for phase in live restored; do
  if [[ $phase = restored ]]; then
   runuser -u postgres -- "$pg_bin/pg_dump" -h "$cluster" -U postgres -d "$database" --format=custom > "$cluster/current.dump"
   create_database "${database}_restore"
   runuser -u postgres -- "$pg_bin/pg_restore" -h "$cluster" --dbname="${database}_restore" --single-transaction --exit-on-error < "$cluster/current.dump"
   database="${database}_restore"
   admin=(runuser -u postgres -- "${psql[@]}" -d "$database")
  fi
  for role in board_public board_staff board_auth; do
   "${psql[@]}" -U "$role" -d "$database" <<'SQL'
DO $$ DECLARE relation_name text; BEGIN
 FOREACH relation_name IN ARRAY ARRAY['post_secrets.report_group','post_secrets.report_membership','post_secrets.report_weight_evidence','post_secrets.anonymous_sessions','post_secrets.anonymous_reports'] LOOP
  BEGIN
   EXECUTE 'SELECT * FROM '||relation_name||' LIMIT 1';
   RAISE EXCEPTION 'Runtime private report access' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 END LOOP;
 BEGIN
  UPDATE content.reports SET group_cleared_at=clock_timestamp(),group_cleared_by=1,group_clear_inherited=false WHERE false;
  RAISE EXCEPTION 'Runtime raw clear update' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 IF current_user<>'board_staff' THEN
  BEGIN
   PERFORM content.clear_report_group('groupclear',10800001,7);
   RAISE EXCEPTION 'Unauthorized group clear' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 END IF;
END $$;
SQL
  done
  "${psql[@]}" -U board_staff -d "$database" <<'SQL'
BEGIN;
DO $$ DECLARE patch jsonb; BEGIN
 IF content.clear_report_group('groupclear',10800001,8) IS DISTINCT FROM 0::bigint
 THEN RAISE EXCEPTION 'Inherited unknown evidence broke idempotency'; END IF;
 INSERT INTO content.moderation_audit(id,account_id,board,target_id,action,group_clear_count)
 OVERRIDING SYSTEM VALUE VALUES(10809991,7,'groupclear',10800001,'report-group-clear',1),
 (10809992,7,'groupclear',10800001,'report-group-clear',10000);
 FOR patch IN SELECT value FROM jsonb_array_elements('[
 {"group_clear_count":null},{"group_clear_count":0},{"group_clear_count":-1},{"group_clear_count":10001},
 {"action":"resolve"},{"reporter_clear_count":1},{"before_mask":0},{"after_mask":1},
 {"snapshot_version":1},{"snapshot_name":"Invalid"},
 {"snapshot_version":1,"snapshot_name":"","snapshot_subject":"","snapshot_comment":"","snapshot_comment_format":0,"snapshot_staff_authorized_limits":false,"snapshot_wordfiltered":false,"snapshot_image_spoiler":false}
 ]'::jsonb) LOOP
  BEGIN
   INSERT INTO content.moderation_audit OVERRIDING SYSTEM VALUE
   SELECT * FROM jsonb_populate_record(NULL::content.moderation_audit,
    '{"id":10809993,"account_id":7,"board":"groupclear","target_id":10800001,"action":"report-group-clear","group_clear_count":2,"created_at":"2020-01-01 UTC"}'::jsonb||patch);
   RAISE EXCEPTION 'Invalid group-clear audit accepted' USING ERRCODE='ZX001';
  EXCEPTION WHEN check_violation THEN NULL; END;
 END LOOP;
 BEGIN
  UPDATE content.moderation_audit SET group_clear_count=1 WHERE action='report-group-clear';
  RAISE EXCEPTION 'Staff changed immutable audit' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
ROLLBACK;
SQL
  # Purging only membership preserves clear metadata/history. Empty lifetime
  # loses its clear; a fresh owner-maintenance report never borrows old history.
  "${admin[@]}" <<'SQL'
BEGIN;
SELECT slug FROM content.boards ORDER BY slug FOR UPDATE;
CREATE TEMP TABLE preserved_history ON COMMIT DROP AS
 SELECT * FROM public.capture_rows() WHERE relation NOT IN ('post_secrets.report_membership','post_secrets.report_group');
CREATE TEMP TABLE cleared_group ON COMMIT DROP AS
 SELECT * FROM post_secrets.report_group WHERE board='groupclear' AND post_id=10800001;
DELETE FROM post_secrets.report_membership WHERE report_id=10801001;
DO $$ BEGIN
 IF EXISTS(TABLE cleared_group EXCEPT ALL SELECT * FROM post_secrets.report_group)
 THEN RAISE EXCEPTION 'Partial retirement reset lifetime'; END IF;
END $$;
DELETE FROM post_secrets.report_membership WHERE board='groupclear' AND post_id=10800001;
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM post_secrets.report_group WHERE board='groupclear' AND post_id=10800001)
 OR EXISTS(TABLE preserved_history EXCEPT ALL SELECT * FROM public.capture_rows())
 OR EXISTS(SELECT * FROM public.capture_rows() WHERE relation NOT IN ('post_secrets.report_membership','post_secrets.report_group') EXCEPT ALL TABLE preserved_history)
 THEN RAISE EXCEPTION 'Empty retirement retained lifetime or rewrote history'; END IF;
END $$;
INSERT INTO content.reports(id,board,post_id,reason) OVERRIDING SYSTEM VALUE
VALUES(10809990,'groupclear',10800001,'Fresh lifetime unknown');
INSERT INTO post_secrets.report_membership(report_id,actor_hash,board,post_id,thread_id,reported_at)
VALUES(10809990,decode(repeat('90',32),'hex'),'groupclear',10800001,10800001,clock_timestamp());
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM post_secrets.report_group WHERE board='groupclear' AND post_id=10800001 AND cleared_at IS NULL AND cleared_by IS NULL)
 OR NOT EXISTS(SELECT 1 FROM content.reports WHERE id=10809990 AND group_cleared_at IS NULL AND group_cleared_by IS NULL AND group_clear_inherited IS NULL)
 THEN RAISE EXCEPTION 'New lifetime inherited retained history'; END IF;
END $$;
SET LOCAL ROLE board_staff;
DO $$ BEGIN
 BEGIN
  PERFORM content.clear_report_group('groupclear',10800001,7);
  RAISE EXCEPTION 'Fresh unknown lifetime clear accepted' USING ERRCODE='ZX001';
 EXCEPTION WHEN SQLSTATE 'P0108' THEN NULL; END;
END $$;
ROLLBACK;
-- Enforce the row cap before unknown-weight handling and without partial writes.
BEGIN;
SELECT slug FROM content.boards ORDER BY slug FOR UPDATE;
INSERT INTO content.reports(id,board,post_id,reason,created_at) OVERRIDING SYSTEM VALUE
SELECT 10820000+i,'groupclear',10800004,'Bounded cap fixture','2020-01-01 UTC' FROM generate_series(1,10001) i;
INSERT INTO post_secrets.report_membership(report_id,actor_hash,board,post_id,thread_id,reported_at)
SELECT 10820000+i,decode(repeat('ff',32),'hex'),'groupclear',10800004,10800001,'2020-01-01 UTC' FROM generate_series(1,10001) i;
CREATE TEMP TABLE cap_before ON COMMIT DROP AS SELECT * FROM public.capture_rows();
SET LOCAL ROLE board_staff;
DO $$ BEGIN
 BEGIN
  PERFORM content.clear_report_group('groupclear',10800004,7);
  RAISE EXCEPTION 'Group clear cap not enforced' USING ERRCODE='ZX001';
 EXCEPTION WHEN SQLSTATE '54000' THEN NULL; END;
END $$;
RESET ROLE;
DO $$ BEGIN
 IF EXISTS(TABLE cap_before EXCEPT ALL SELECT * FROM public.capture_rows())
 OR EXISTS(SELECT * FROM public.capture_rows() EXCEPT ALL TABLE cap_before)
 THEN RAISE EXCEPTION 'Cap failure changed group/report/audit state'; END IF;
END $$;
ROLLBACK;
SQL
  for role in board_public board_staff; do
   "${psql[@]}" -U "$role" -d "$database" -f - < "$cluster/readiness.sql"
  done
  "${admin[@]}" -f - < "$cluster/drift.sql"
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
SELECT last_value,is_called FROM content.reports_id_seq;
SELECT last_value,is_called FROM content.moderation_audit_id_seq;
SELECT rolname,rolsuper,rolinherit,rolcreaterole,rolcreatedb,rolcanlogin,rolreplication,rolbypassrls,rolconfig FROM pg_roles WHERE rolname LIKE 'board_%' ORDER BY rolname;
SQL
 done
 cmp -s "$cluster/$mode-live.fingerprint" "$cluster/$mode-restored.fingerprint" || {
  printf 'Report-group-clear dump/restore fingerprint mismatch (%s).\n' "$mode" >&3; exit 1;
 }
 printf '%s group-clear migration, inheritance, runtime authority and administrator dump/restore passed.\n' "$mode" >&3
done
printf '0108 report group clearing qualified with retained nullable history and bounded lifetime semantics.\n' >&3
