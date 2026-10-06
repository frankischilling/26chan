#!/usr/bin/env bash
# Owned synthetic clusters only. Qualify 0096 -> 0097 and current dump/restore.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-session-report.XXXXXXXX)
started=0
cleanup() {
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null
    fi
    [[ $cluster =~ ^/tmp/board-session-report\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
    [[ $(readlink -f "$cluster") = "$cluster" ]] || exit 1
    rm -rf -- "$cluster"
}
trap cleanup EXIT
chown postgres:postgres "$cluster"
runuser -u postgres -- "$pg_bin/initdb" -D "$cluster/data" --auth=trust --encoding=UTF8 --no-locale > /dev/null
runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" \
    -o "-c listen_addresses='' -c unix_socket_directories='$cluster' -c statement_timeout=30000 -c lock_timeout=5000" -w start > /dev/null
started=1
psql=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h "$cluster")
# SQL diagnostics and synthetic fixture values stay in the private directory.
exec 3>&1
exec > "$cluster/qualification.log" 2>&1
trap 'printf "Session-report qualification failed at line %s (private diagnostics removed during cleanup).\n" "$LINENO" >&3' ERR
# All fixture SQL is root-owned mode 0600. The parent shell opens stdin before
# runuser drops privileges; never pass these private paths to postgres psql.
runuser -u postgres -- "${psql[@]}" -d postgres -f - < deploy/roles.sql
runuser -u postgres -- "${psql[@]}" -d postgres -c 'ALTER ROLE board_staff LOGIN'
create_database() {
 runuser -u postgres -- "${psql[@]}" -d postgres -v database="$1" <<'SQL'
CREATE DATABASE :"database" OWNER board_migrator;
REVOKE ALL ON DATABASE :"database" FROM PUBLIC;
GRANT CONNECT ON DATABASE :"database" TO board_migrator,board_public,board_staff;
SQL
}
cat > "$cluster/fixtures.sql" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('sessreport','Session report fixture','Synthetic',1000,100,100,100,10);
BEGIN;
SELECT set_config('board.posting_actor',repeat('11',32),true);
INSERT INTO content.threads(id,board) VALUES(9700001,'sessreport');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
SELECT 9700000+i,'sessreport',9700001,'Synthetic','','Retained body' FROM generate_series(1,100) i;
INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES(9700001,'owned-deletion-hash');
COMMIT;
SQL
# Pull the exact readiness SQL used by each current runtime. Catalog drift is
# introduced only by the owned administrator, and always rolled back. Staff
# receives no private-schema USAGE or other qualification-only grants.
python3 - "$cluster/readiness.sql" <<'PYREADINESS'
import pathlib, re, sys
profiles = {
 'report_admission': [
  'DROP FUNCTION content.check_report_admission(text,bigint,bytea,bytea,bigint)',
  'DROP FUNCTION content.admit_report(text,bigint,text,bytea,bytea,bytea,bytea,bytea,boolean,bigint)',
  'GRANT EXECUTE ON FUNCTION content.admit_report(text,bigint,text,bytea) TO board_public',
  'REVOKE EXECUTE ON FUNCTION content.admit_report(text,bigint,text,bytea) FROM board_staff',
  'ALTER FUNCTION content.admit_report(text,bigint,text,bytea,bytea,bytea,bytea,bytea,boolean,bigint) SECURITY INVOKER',
  'ALTER FUNCTION content.check_report_admission(text,bigint,bytea,bytea,bigint) SET search_path=public',
  'GRANT SELECT(automatic_identity) ON post_secrets.report_membership TO board_public',
  'GRANT INSERT ON content.reports TO board_staff',
  'ALTER ROLE board_report_admission_owner LOGIN',
 ],
 'automatic_admission': [
  'REVOKE EXECUTE ON FUNCTION content.register_anonymous_report(bytea,bytea,bytea,bytea,boolean,text,bigint,bigint) FROM board_report_admission_owner',
  'REVOKE EXECUTE ON FUNCTION post_secrets.resolve_automatic_identity(bytea,boolean,bigint,boolean) FROM board_report_admission_owner',
  'REVOKE EXECUTE ON FUNCTION post_secrets.lookup_automatic_identity(bytea,bigint) FROM board_report_admission_owner',
  'GRANT SELECT(automatic_identity) ON post_secrets.anonymous_sessions TO board_public',
  'GRANT UPDATE(automatic_identity) ON post_secrets.posting_history TO board_staff',
  'ALTER TABLE post_secrets.report_membership ALTER COLUMN registration_xid DROP DEFAULT',
  'ALTER TABLE post_secrets.anonymous_sessions ALTER COLUMN automatic_identity SET DEFAULT gen_random_uuid()',
  'ALTER TABLE post_secrets.anonymous_sessions DROP CONSTRAINT anonymous_sessions_automatic_identity_key',
  'ALTER FUNCTION post_secrets.lookup_automatic_identity(bytea,bigint) SECURITY INVOKER',
  'ALTER ROLE board_anonymous_owner LOGIN',
 ]}
with open(sys.argv[1], 'w') as out:
 for profile, changes in profiles.items():
  source = pathlib.Path(f'crates/store/src/{profile}.rs').read_text()
  match = re.search(r'pub const READINESS_SQL: &str = r#"(.*?)"#;', source, re.S)
  if not match:
   raise SystemExit(f'Cannot extract actual {profile} READINESS_SQL')
  query = match.group(1)
  for case, (change, expected) in enumerate([('', True), *[(c, False) for c in changes]]):
   out.write('BEGIN;\n' + (change + ';\n' if change else ''))
   for role in ('board_public', 'board_staff'):
    out.write(f'SET LOCAL ROLE {role};\nDO $readiness$ BEGIN IF ({query}) IS DISTINCT FROM {str(expected).lower()} THEN RAISE EXCEPTION \'{profile} readiness case {case} for {role} failed\'; END IF; END $readiness$;\nRESET ROLE;\n')
   out.write('ROLLBACK;\n')
PYREADINESS
for mode in upgrade fresh; do
 database="session_report_$mode"
 create_database "$database"
 migrator=("${psql[@]}" -U board_migrator -d "$database")
 admin=(runuser -u postgres -- "${psql[@]}" -d "$database")
 for migration in migrations/*.sql; do
  [[ $migration < migrations/0097_automatic_report_admission.sql ]] || break
  "${migrator[@]}" --single-transaction -f - < "$migration"
 done
 if [[ $mode = upgrade ]]; then
  "${migrator[@]}" -f - < "$cluster/fixtures.sql"
  # Genuine 0096 public mutation/registration captures pre-upgrade UUID rows.
  "${psql[@]}" -U board_public -d "$database" <<'SQL'
BEGIN;
SELECT content.lock_posting_actor(decode(repeat('12',32),'hex'),true);
SELECT set_config('board.posting_actor',repeat('12',32),true);
INSERT INTO content.threads(id,board) VALUES(9700200,'sessreport');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(9700200,'sessreport',9700200,'Synthetic','','Pre-upgrade registered OP');
INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES(9700200,'owned-deletion-hash');
SELECT content.register_anonymous_post(decode(repeat('01',32),'hex'),decode(repeat('02',32),'hex'),
 decode(repeat('03',32),'hex'),decode(repeat('04',32),'hex'),true,'sessreport',9700200,extract(epoch FROM clock_timestamp())::bigint);
SELECT content.admit_report('sessreport',9700001,'Retained report',decode(repeat('21',32),'hex')) AS report_id \gset
SELECT content.register_anonymous_report(decode(repeat('01',32),'hex'),decode(repeat('02',32),'hex'),
 decode(repeat('03',32),'hex'),decode(repeat('04',32),'hex'),false,'sessreport',:report_id,extract(epoch FROM clock_timestamp())::bigint);
COMMIT;
SQL
 fi
 "${admin[@]}" <<'SQL'
CREATE TABLE public.report_rows_before(relation text,value jsonb);
CREATE FUNCTION public.capture_report_rows() RETURNS TABLE(relation text,value jsonb)
LANGUAGE plpgsql AS $$ DECLARE r record; BEGIN
 FOR r IN SELECT n.nspname,c.relname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname IN ('content','post_secrets','staff_identity','media','media_intake','deployment','admission') AND c.relkind='r' LOOP
  RETURN QUERY EXECUTE format('SELECT %L,to_jsonb(r) FROM %I.%I r',r.nspname||'.'||r.relname,r.nspname,r.relname);
 END LOOP;
END $$;
INSERT INTO public.report_rows_before SELECT * FROM public.capture_report_rows();
-- Canonical names, rather than database OIDs, also support dump/restore comparison.
-- acldefault uses lowercase s for sequences (uppercase S means foreign server),
-- unlike pg_class.relkind. Expand NULL ACLs to the correct object-type defaults.
-- Dump/restore can replace
-- explicit owner-only ACLs with NULL and reorder entries without changing rights.
-- Exploded table/column tuples are compared as sets and sorted in fingerprints;
-- nested schema/function tuples are sorted explicitly. Owners remain separate.
CREATE VIEW public.report_authority AS
SELECT 'relation' kind,n.nspname||'.'||c.relname object,
 jsonb_build_array(pg_get_userbyid(c.relowner),c.relrowsecurity,c.relforcerowsecurity) value
 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname NOT LIKE 'pg_%' AND n.nspname NOT IN ('information_schema','public')
UNION ALL SELECT 'table grant',n.nspname||'.'||c.relname,
 jsonb_build_array(pg_get_userbyid(a.grantor),CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace,
 LATERAL aclexplode(coalesce(c.relacl,acldefault(CASE WHEN c.relkind='S' THEN 's'::"char" ELSE 'r'::"char" END,c.relowner))) a
 WHERE c.relkind IN ('r','p','v','m','f','S')
 AND n.nspname NOT LIKE 'pg_%' AND n.nspname NOT IN ('information_schema','public')
UNION ALL SELECT 'column grant',n.nspname||'.'||c.relname||'.'||att.attname,
 jsonb_build_array(pg_get_userbyid(a.grantor),CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
 FROM pg_attribute att JOIN pg_class c ON c.oid=att.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace,LATERAL aclexplode(att.attacl) a
 WHERE n.nspname NOT LIKE 'pg_%' AND n.nspname NOT IN ('information_schema','public')
UNION ALL SELECT 'function',p.oid::regprocedure::text,
 jsonb_build_array(pg_get_userbyid(p.proowner),coalesce((
  SELECT jsonb_agg(jsonb_build_array(pg_get_userbyid(a.grantor),
   CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
   ORDER BY pg_get_userbyid(a.grantor),CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
  FROM aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a),'[]'::jsonb),pg_get_functiondef(p.oid))
 FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
 WHERE n.nspname NOT LIKE 'pg_%' AND n.nspname NOT IN ('information_schema','public')
UNION ALL SELECT 'policy',p.polrelid::regclass::text||'.'||p.polname,
 jsonb_build_array(p.polcmd,p.polpermissive,ARRAY(SELECT pg_get_userbyid(x) FROM unnest(p.polroles) x ORDER BY x),pg_get_expr(p.polqual,p.polrelid),pg_get_expr(p.polwithcheck,p.polrelid)) FROM pg_policy p
UNION ALL SELECT 'trigger',t.tgrelid::regclass::text||'.'||t.tgname,
 jsonb_build_array(t.tgenabled,pg_get_triggerdef(t.oid)) FROM pg_trigger t WHERE NOT t.tgisinternal
UNION ALL SELECT 'schema',n.nspname,jsonb_build_array(pg_get_userbyid(n.nspowner),coalesce((
 SELECT jsonb_agg(jsonb_build_array(pg_get_userbyid(a.grantor),
  CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
  ORDER BY pg_get_userbyid(a.grantor),CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
 FROM aclexplode(coalesce(n.nspacl,acldefault('n',n.nspowner))) a),'[]'::jsonb))
 FROM pg_namespace n WHERE n.nspname NOT LIKE 'pg_%' AND n.nspname NOT IN ('information_schema','public')
UNION ALL SELECT 'role',rolname,
 jsonb_build_array(rolsuper,rolinherit,rolcreaterole,rolcreatedb,rolcanlogin,rolreplication,
  rolbypassrls,rolconnlimit,rolvaliduntil,rolconfig)
 FROM pg_roles WHERE rolname LIKE 'board_%'
UNION ALL SELECT 'membership',pg_get_userbyid(roleid)||'.'||pg_get_userbyid(member),
 jsonb_build_array(pg_get_userbyid(grantor),admin_option,inherit_option,set_option)
 FROM pg_auth_members WHERE pg_get_userbyid(roleid) LIKE 'board_%' OR pg_get_userbyid(member) LIKE 'board_%'
UNION ALL SELECT 'default grant',pg_get_userbyid(d.defaclrole)||'.'||coalesce(n.nspname,'*')||'.'||d.defaclobjtype::text,
 jsonb_build_array(pg_get_userbyid(a.grantor),CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
 FROM pg_default_acl d LEFT JOIN pg_namespace n ON n.oid=d.defaclnamespace,
 LATERAL aclexplode(d.defaclacl) a;
CREATE TABLE public.report_authority_before AS TABLE public.report_authority;
CREATE VIEW public.report_columns AS
SELECT n.nspname||'.'||c.relname relation,a.attname,a.attnum,a.atttypid::regtype::text type,
 a.attnotnull,coalesce(pg_get_expr(d.adbin,d.adrelid),'') default_value
FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace
LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
WHERE n.nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission')
-- Index attributes belong to the index contract, not table/view columns.
AND c.relkind IN('r','p','v','m','f') AND a.attnum>0 AND NOT a.attisdropped;
CREATE TABLE public.report_columns_before AS TABLE public.report_columns;
CREATE VIEW public.report_constraints AS
SELECT conrelid::regclass::text relation,conname,contype,convalidated,condeferrable,condeferred,
 connoinherit,pg_get_constraintdef(oid,true) definition FROM pg_constraint
WHERE connamespace IN(SELECT oid FROM pg_namespace WHERE nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission'));
CREATE TABLE public.report_constraints_before AS TABLE public.report_constraints;
CREATE VIEW public.report_indexes AS
SELECT schemaname,tablename,indexname,indexdef FROM pg_indexes
WHERE schemaname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission');
CREATE TABLE public.report_indexes_before AS TABLE public.report_indexes;
SQL
 "${migrator[@]}" --single-transaction -f - < migrations/0097_automatic_report_admission.sql
 "${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(TABLE public.report_rows_before EXCEPT ALL SELECT * FROM public.capture_report_rows())
 OR EXISTS(SELECT * FROM public.capture_report_rows() EXCEPT ALL TABLE public.report_rows_before)
 THEN RAISE EXCEPTION '0097 rewrote report/session/history/UUID rows'; END IF;
 IF EXISTS(TABLE public.report_columns_before EXCEPT TABLE public.report_columns)
 OR EXISTS(TABLE public.report_columns EXCEPT TABLE public.report_columns_before)
 OR EXISTS(TABLE public.report_constraints_before EXCEPT TABLE public.report_constraints)
 OR EXISTS(TABLE public.report_constraints EXCEPT TABLE public.report_constraints_before)
 OR EXISTS(TABLE public.report_indexes_before EXCEPT TABLE public.report_indexes)
 OR EXISTS(TABLE public.report_indexes EXCEPT TABLE public.report_indexes_before)
 THEN RAISE EXCEPTION '0097 changed column, constraint or index contracts'; END IF;
END $$;
-- Only these precise function contracts may change; current readiness validates
-- their new owners, safe paths, and exact runtime/helper EXECUTE ACLs.
CREATE FUNCTION public.session_report_changed_function(object text) RETURNS boolean
LANGUAGE sql IMMUTABLE AS $$ SELECT object IN (
 'post_secrets.report_target(text,bigint)',
 'post_secrets.report_target(text,bigint,timestamp with time zone)',
 'post_secrets.check_report_limits(text,bigint,bytea,timestamp with time zone)',
 'post_secrets.check_report_limits(text,bigint,bytea,uuid,timestamp with time zone)',
 'content.check_report_admission(text,bigint,bytea,bytea,bigint)',
 'content.admit_report(text,bigint,text,bytea)',
 'content.admit_report(text,bigint,text,bytea,bytea,bytea,bytea,bytea,boolean,bigint)',
 'content.register_anonymous_report(bytea,bytea,bytea,bytea,boolean,text,bigint,bigint)') $$;
DO $$ BEGIN
 IF EXISTS(SELECT * FROM public.report_authority_before WHERE NOT(kind='function' AND public.session_report_changed_function(object))
 EXCEPT SELECT * FROM public.report_authority WHERE NOT(kind='function' AND public.session_report_changed_function(object)))
 OR EXISTS(SELECT * FROM public.report_authority WHERE NOT(kind='function' AND public.session_report_changed_function(object))
 EXCEPT SELECT * FROM public.report_authority_before WHERE NOT(kind='function' AND public.session_report_changed_function(object)))
 THEN RAISE EXCEPTION '0097 changed unrelated authority or function'; END IF;
 -- Registration gains one helper EXECUTE grant, never a new body or owner.
 IF (SELECT value->2 FROM public.report_authority_before WHERE kind='function'
 AND object='content.register_anonymous_report(bytea,bytea,bytea,bytea,boolean,text,bigint,bigint)') IS DISTINCT FROM
 (SELECT value->2 FROM public.report_authority WHERE kind='function'
 AND object='content.register_anonymous_report(bytea,bytea,bytea,bytea,boolean,text,bigint,bigint)')
 THEN RAISE EXCEPTION '0097 changed registration body'; END IF;
END $$;
DROP FUNCTION public.session_report_changed_function(text);
DROP TABLE public.report_rows_before,public.report_authority_before,public.report_columns_before,
 public.report_constraints_before,public.report_indexes_before;
SQL
 if [[ $mode = fresh ]]; then
  "${migrator[@]}" -f - < "$cluster/fixtures.sql"
  "${psql[@]}" -U board_public -d "$database" <<'SQL'
BEGIN;
SELECT content.admit_report('sessreport',9700001,'Retained report',decode(repeat('21',32),'hex'),
 decode(repeat('01',32),'hex'),decode(repeat('02',32),'hex'),decode(repeat('03',32),'hex'),
 decode(repeat('04',32),'hex'),true,extract(epoch FROM clock_timestamp())::bigint);
COMMIT;
SQL
 fi
 # Capture the exact activity/ownership baseline before the current success.
 # reports is a four-hour activity-bucket counter, not a raw report count:
 # a first action queues bit 8 and does not yet increment that bucket counter.
 "${admin[@]}" <<'SQL'
CREATE TABLE public.current_report_baseline AS
SELECT coalesce((SELECT reports FROM post_secrets.anonymous_sessions
 WHERE token_hash=decode(repeat('62',32),'hex')),0) AS reports,
 coalesce((SELECT pending FROM post_secrets.anonymous_sessions
 WHERE token_hash=decode(repeat('62',32),'hex')),0) AS pending,
 (SELECT count(*) FROM post_secrets.anonymous_reports
 WHERE token_hash=decode(repeat('62',32),'hex')) AS ownership_count,
 extract(epoch FROM clock_timestamp())::bigint AS request_floor;
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM post_secrets.anonymous_sessions WHERE token_hash=decode(repeat('62',32),'hex'))
 THEN RAISE EXCEPTION 'Current minted-session fixture unexpectedly has a session baseline'; END IF;
END $$;
SQL
 # Real restricted logins prove both mutation contracts, without SET ROLE's
 # administrator authority masking an invoker mistake.
 "${psql[@]}" -U board_public -d "$database" <<'SQL'
BEGIN;
DO $$ BEGIN
 BEGIN
  PERFORM content.admit_report('sessreport',9700002,'Forbidden old API',decode(repeat('61',32),'hex'));
  RAISE EXCEPTION 'Public old mutation accepted' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SELECT content.admit_report('sessreport',9700002,'Current session report',decode(repeat('61',32),'hex'),
 decode(repeat('62',32),'hex'),decode(repeat('02',32),'hex'),decode(repeat('03',32),'hex'),
 decode(repeat('04',32),'hex'),true,extract(epoch FROM clock_timestamp())::bigint);
COMMIT;
SQL
 "${psql[@]}" -U board_staff -d "$database" <<'SQL'
SELECT content.admit_report('sessreport',9700003,'Staff IP-only report',decode(repeat('63',32),'hex'));
SQL
 for phase in live restored; do
  if [[ $phase = restored ]]; then
   # Private report membership requires the owned administrator, not extra grants.
   runuser -u postgres -- "$pg_bin/pg_dump" -h "$cluster" -U postgres -d "$database" --format=custom > "$cluster/current.dump"
   create_database "${database}_restore"
   runuser -u postgres -- "$pg_bin/pg_restore" -h "$cluster" --dbname="${database}_restore" --single-transaction --exit-on-error < "$cluster/current.dump"
   database="${database}_restore"
   admin=(runuser -u postgres -- "${psql[@]}" -d "$database")
  fi
  "${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM content.reports r JOIN post_secrets.report_membership m ON m.report_id=r.id
 JOIN post_secrets.anonymous_reports a ON a.report_id=r.id
 JOIN post_secrets.anonymous_sessions s ON s.token_hash=a.token_hash
 WHERE r.reason='Current session report' AND a.token_hash=decode(repeat('62',32),'hex')
 AND m.automatic_identity=s.automatic_identity AND m.automatic_identity IS NOT NULL)
 THEN RAISE EXCEPTION 'Current report/session ownership or UUID capture failed'; END IF;
 IF NOT EXISTS(SELECT 1 FROM content.reports r JOIN post_secrets.report_membership m ON m.report_id=r.id
 WHERE r.reason='Current session report' AND m.registration_xid IS NOT NULL)
 THEN RAISE EXCEPTION 'Current report registration provenance missing'; END IF;
 IF NOT EXISTS(SELECT 1 FROM post_secrets.anonymous_sessions s CROSS JOIN public.current_report_baseline b
 WHERE s.token_hash=decode(repeat('62',32),'hex') AND s.reports=b.reports
 AND s.pending=(b.pending | 8) AND s.activity_at>=b.request_floor
 AND s.action_at=s.activity_at AND s.created_at=s.activity_at
 AND (SELECT count(*) FROM post_secrets.anonymous_reports a WHERE a.token_hash=s.token_hash)=b.ownership_count+1)
 THEN RAISE EXCEPTION 'Current report did not advance baseline ownership by one and queue its activity bucket'; END IF;
 IF NOT EXISTS(SELECT 1 FROM content.reports r JOIN post_secrets.report_membership m ON m.report_id=r.id
 WHERE r.reason='Staff IP-only report' AND m.automatic_identity IS NULL)
 THEN RAISE EXCEPTION 'Retained staff IP-only report membership failed'; END IF;
END $$;
-- GET succeeds with absent, unknown and valid context and is genuinely read-only.
BEGIN;
CREATE TEMP TABLE rows_before AS SELECT * FROM public.capture_report_rows();
UPDATE post_secrets.report_membership SET reported_at=clock_timestamp()-interval '2 days';
CREATE TEMP TABLE get_before AS SELECT * FROM public.capture_report_rows();
SET LOCAL ROLE board_public;
DO $$ DECLARE n bigint:=extract(epoch FROM clock_timestamp())::bigint; BEGIN
 PERFORM content.check_report_admission('sessreport',9700004,decode(repeat('70',32),'hex'),NULL,n);
 PERFORM content.check_report_admission('sessreport',9700004,decode(repeat('70',32),'hex'),decode(repeat('71',32),'hex'),n);
 PERFORM content.check_report_admission('sessreport',9700004,decode(repeat('70',32),'hex'),decode(repeat('01',32),'hex'),n);
END $$;
RESET ROLE;
DO $$ BEGIN
 IF EXISTS(TABLE get_before EXCEPT ALL SELECT * FROM public.capture_report_rows())
 OR EXISTS(SELECT * FROM public.capture_report_rows() EXCEPT ALL TABLE get_before)
 THEN RAISE EXCEPTION 'Advisory GET wrote activity, identity or membership'; END IF;
END $$;
-- Reports never apply the thread quota source_new exemption: same-second and
-- seven-day-idle sessions retain duplicate protection after changing IP.
SELECT extract(epoch FROM clock_timestamp())::bigint AS request_at \gset
SELECT set_config('test.request_at',:'request_at',true);
UPDATE post_secrets.anonymous_sessions SET created_at=:request_at,activity_at=:request_at;
SET LOCAL ROLE board_public;
DO $$ BEGIN
 BEGIN
  PERFORM content.check_report_admission('sessreport',9700001,decode(repeat('70',32),'hex'),decode(repeat('01',32),'hex'),current_setting('test.request_at')::bigint);
  RAISE EXCEPTION 'Same-second session duplicate bypass' USING ERRCODE='ZX001';
 EXCEPTION WHEN SQLSTATE 'P0001' THEN
  IF SQLERRM<>'You have already reported this post.' THEN RAISE; END IF;
 END;
END $$;
RESET ROLE;
UPDATE post_secrets.anonymous_sessions SET created_at=:request_at-700000,activity_at=:request_at-604801;
SET LOCAL ROLE board_public;
DO $$ BEGIN
 BEGIN
  PERFORM content.admit_report('sessreport',9700001,'Idle duplicate',decode(repeat('70',32),'hex'),
   decode(repeat('01',32),'hex'),decode(repeat('02',32),'hex'),decode(repeat('03',32),'hex'),
   decode(repeat('04',32),'hex'),false,current_setting('test.request_at')::bigint);
  RAISE EXCEPTION 'Idle session duplicate bypass' USING ERRCODE='ZX001';
 EXCEPTION WHEN SQLSTATE 'P0001' THEN
  IF SQLERRM<>'You have already reported this post.' THEN RAISE; END IF;
 END;
END $$;
RESET ROLE;
ROLLBACK;
SQL
  # OR predicates must combine separate IP and session rows while counting each
  # matching row once. Exercise hourly and daily thresholds without sleeps.
  for window in hour day; do
   if [[ $window = hour ]]; then limit=30; age='2 minutes'; else limit=80; age='2 hours'; fi
   "${admin[@]}" -v quota="$limit" -v age="$age" <<'SQL'
BEGIN;
DELETE FROM post_secrets.report_membership;
SELECT automatic_identity AS identity FROM post_secrets.anonymous_sessions WHERE token_hash=decode(repeat('01',32),'hex') \gset
INSERT INTO content.reports(board,post_id,reason)
SELECT 'sessreport',9700000+i,'OR fixture '||i FROM generate_series(1,:quota-1) i;
INSERT INTO post_secrets.report_membership(report_id,actor_hash,board,post_id,thread_id,reported_at,automatic_identity)
SELECT id,decode(repeat('75',32),'hex'),board,post_id,9700001,clock_timestamp()-:'age'::interval,:'identity'::uuid
FROM content.reports WHERE reason LIKE 'OR fixture %';
SET LOCAL ROLE board_public;
-- All 29/79 rows match BOTH branches. A sum of separate branch counts fails.
SELECT content.check_report_admission('sessreport',9700100,decode(repeat('75',32),'hex'),
 decode(repeat('01',32),'hex'),extract(epoch FROM clock_timestamp())::bigint);
RESET ROLE;
-- Split matches across disjoint branches; their union still reaches the cap.
UPDATE post_secrets.report_membership SET automatic_identity=NULL WHERE post_id%2=0;
UPDATE post_secrets.report_membership SET actor_hash=decode(repeat('76',32),'hex') WHERE post_id%2=1;
INSERT INTO content.reports(board,post_id,reason) VALUES('sessreport',9700099,'OR threshold') RETURNING id AS extra_id \gset
INSERT INTO post_secrets.report_membership(report_id,actor_hash,board,post_id,thread_id,reported_at,automatic_identity)
VALUES(:extra_id,decode(repeat('76',32),'hex'),'sessreport',9700099,9700001,clock_timestamp()-:'age'::interval,:'identity'::uuid);
SET LOCAL ROLE board_public;
DO $$ BEGIN
 BEGIN
  PERFORM content.check_report_admission('sessreport',9700100,decode(repeat('75',32),'hex'),decode(repeat('01',32),'hex'),extract(epoch FROM clock_timestamp())::bigint);
  RAISE EXCEPTION 'Combined OR threshold failed open' USING ERRCODE='ZX001';
 EXCEPTION WHEN SQLSTATE 'P0001' THEN
  IF SQLERRM<>'You have to wait a while before reporting another post.' THEN RAISE; END IF;
 END;
 BEGIN
  PERFORM content.admit_report('sessreport',9700100,'Combined OR rejection',decode(repeat('75',32),'hex'),
   decode(repeat('01',32),'hex'),decode(repeat('02',32),'hex'),decode(repeat('03',32),'hex'),
   decode(repeat('04',32),'hex'),false,extract(epoch FROM clock_timestamp())::bigint);
  RAISE EXCEPTION 'Mutation combined OR threshold failed open' USING ERRCODE='ZX001';
 EXCEPTION WHEN SQLSTATE 'P0001' THEN
  IF SQLERRM<>'You have to wait a while before reporting another post.' THEN RAISE; END IF;
 END;
END $$;
RESET ROLE;
ROLLBACK;
SQL
  done
  "${admin[@]}" <<'SQL'
BEGIN;
CREATE TEMP TABLE rollback_before AS SELECT * FROM public.capture_report_rows();
SET LOCAL ROLE board_public;
DO $$ DECLARE n bigint:=extract(epoch FROM clock_timestamp())::bigint; BEGIN
 BEGIN
  PERFORM content.admit_report('sessreport',9700004,'Downstream rollback',decode(repeat('81',32),'hex'),
   decode(repeat('82',32),'hex'),decode(repeat('02',32),'hex'),decode(repeat('03',32),'hex'),decode(repeat('04',32),'hex'),true,n);
  RAISE EXCEPTION 'Synthetic downstream failure' USING ERRCODE='ZX002';
 EXCEPTION WHEN SQLSTATE 'ZX002' THEN NULL; END;
 -- Invalid registration fingerprint fails after insertion, rolling back the
 -- report, membership, identity allocation, proof and session activity.
 BEGIN
  PERFORM content.admit_report('sessreport',9700004,'Registration rollback',decode(repeat('83',32),'hex'),
   decode(repeat('84',32),'hex'),decode('01','hex'),decode(repeat('03',32),'hex'),decode(repeat('04',32),'hex'),true,n);
  RAISE EXCEPTION 'Invalid registration context accepted' USING ERRCODE='ZX001';
 EXCEPTION WHEN check_violation THEN NULL; END;
 BEGIN
  PERFORM content.admit_report('sessreport',9700004,'Unknown retained token',decode(repeat('85',32),'hex'),
   decode(repeat('86',32),'hex'),decode(repeat('02',32),'hex'),decode(repeat('03',32),'hex'),decode(repeat('04',32),'hex'),false,n);
  RAISE EXCEPTION 'Unknown retained token accepted' USING ERRCODE='ZX001';
 EXCEPTION WHEN invalid_authorization_specification THEN NULL; END;
END $$;
RESET ROLE;
DO $$ BEGIN
 IF EXISTS(TABLE rollback_before EXCEPT ALL SELECT * FROM public.capture_report_rows())
 OR EXISTS(SELECT * FROM public.capture_report_rows() EXCEPT ALL TABLE rollback_before)
 THEN RAISE EXCEPTION 'Rejected mutation leaked report/session/history/UUID rows'; END IF;
END $$;
ROLLBACK;
-- Cooldown captures session equality across IP changes, even if source_new.
BEGIN;
UPDATE post_secrets.anonymous_sessions SET created_at=extract(epoch FROM clock_timestamp())::bigint;
UPDATE post_secrets.report_membership SET reported_at=clock_timestamp()+interval '1 minute';
SET LOCAL ROLE board_public;
DO $$ BEGIN
 BEGIN
  PERFORM content.check_report_admission('sessreport',9700004,decode(repeat('90',32),'hex'),decode(repeat('01',32),'hex'),extract(epoch FROM clock_timestamp())::bigint);
  RAISE EXCEPTION 'Automatic cooldown bypass' USING ERRCODE='ZX001';
 EXCEPTION WHEN SQLSTATE 'P0001' THEN
  IF SQLERRM<>'You have to wait a while before reporting another post.' THEN RAISE; END IF;
 END;
END $$;
RESET ROLE;
ROLLBACK;
SQL
  "${psql[@]}" -U board_public -d "$database" <<'SQL'
BEGIN READ ONLY;
DO $$ DECLARE n bigint:=extract(epoch FROM clock_timestamp())::bigint; BEGIN
 PERFORM content.check_report_admission('sessreport',9700004,decode(repeat('70',32),'hex'),NULL,n);
 PERFORM content.check_report_admission('sessreport',9700004,decode(repeat('70',32),'hex'),decode(repeat('71',32),'hex'),n);
 BEGIN
  PERFORM content.check_report_admission('sessreport',9700004,decode(repeat('70',32),'hex'),decode(repeat('01',32),'hex'),n);
 EXCEPTION WHEN SQLSTATE 'P0001' THEN
  IF SQLERRM<>'You have to wait a while before reporting another post.' THEN RAISE; END IF;
 END;
END $$;
ROLLBACK;
SQL
  "${admin[@]}" -f - < "$cluster/readiness.sql"
  "${admin[@]}" -At > "$cluster/$mode-$phase.fingerprint" <<'SQL'
SELECT relation,md5(value::text) FROM public.capture_report_rows() ORDER BY relation,value::text;
SELECT kind,object,md5(value::text) FROM public.report_authority ORDER BY kind,object,value::text;
SELECT * FROM public.report_columns ORDER BY relation,attnum;
SELECT * FROM public.report_constraints ORDER BY relation,conname;
SELECT * FROM public.report_indexes ORDER BY schemaname,tablename,indexname;
SQL
 done
 cmp -s "$cluster/$mode-live.fingerprint" "$cluster/$mode-restored.fingerprint" || {
  printf 'Session-report current dump/restore fingerprint mismatch (%s).\n' "$mode" >&3; exit 1;
 }
 printf '%s session-report migration and current dump/restore passed.\n' "$mode" >&3
done
printf '0096 rows and UUIDs preserved; 0097 public/session and staff/IP contracts, OR limits, rollback, read-only GET and both readiness profiles qualified.\n' >&3
