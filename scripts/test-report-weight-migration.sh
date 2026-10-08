#!/usr/bin/env bash
# Owned synthetic clusters only. Qualify 0101 -> 0102, then upgrade to current
# before exercising the current readiness contract and dump/restore.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-report-weight.XXXXXXXX)
started=0
cleanup() {
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null
    fi
    [[ $cluster =~ ^/tmp/board-report-weight\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
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
trap 'printf "Report-weight qualification failed at line %s (private diagnostics removed during cleanup).\n" "$LINENO" >&3' ERR
# All fixture SQL is root-owned mode 0600. The parent shell opens stdin before
# runuser drops privileges; never pass these private paths to postgres psql.
runuser -u postgres -- "${psql[@]}" -d postgres -f - < deploy/roles.sql
runuser -u postgres -- "${psql[@]}" -d postgres -c 'ALTER ROLE board_staff LOGIN; ALTER ROLE board_auth LOGIN'
create_database() {
 runuser -u postgres -- "${psql[@]}" -d postgres -v database="$1" <<'SQL'
CREATE DATABASE :"database" OWNER board_migrator;
REVOKE ALL ON DATABASE :"database" FROM PUBLIC;
GRANT CONNECT ON DATABASE :"database" TO board_migrator,board_public,board_staff,board_auth;
SQL
}
# The current public/staff readiness contract is exercised through real logins.
python3 - "$cluster" <<'PYREADINESS'
import pathlib,re,sys
root=pathlib.Path(sys.argv[1])
match=re.search(r'pub const READINESS_SQL: &str = r#"(.*?)"#;',pathlib.Path('crates/store/src/report_admission.rs').read_text(),re.S)
if not match: raise SystemExit('Cannot extract current report readiness')
query=match.group(1)
(root/'readiness.sql').write_text("DO $check$ BEGIN IF ("+query+") IS DISTINCT FROM true THEN RAISE EXCEPTION 'Current readiness failed'; END IF; END $check$;")
table='post_secrets.report_weight_evidence'
changes=[
 'ALTER TABLE '+table+' RENAME TO missing_report_weight_evidence',
 'ALTER TABLE '+table+' OWNER TO board_migrator',
 'ALTER TABLE '+table+' ALTER COLUMN evaluated_at DROP NOT NULL',
 'ALTER TABLE '+table+' ALTER COLUMN known_or_verified SET DEFAULT false',
 'ALTER TABLE '+table+' ADD COLUMN unexpected text',
 'ALTER TABLE '+table+' ENABLE ROW LEVEL SECURITY',
] + ['GRANT SELECT ON '+table+' TO '+role for role in ('PUBLIC','board_public','board_staff','board_auth','board_migrator')]
changes += ['GRANT INSERT(known_or_verified) ON '+table+' TO board_staff']
for name in ('pkey','report_id_fkey','evaluator_version_check','authenticated_janitor_or_higher_check','threat_at_least_point_four_check','history_filtered_check','effective_weight_check','source_reason_check','numeric_pair_check'):
 changes.append('ALTER TABLE '+table+' DROP CONSTRAINT report_weight_evidence_'+name)
changes += [
 "DO $$ DECLARE t record; BEGIN FOR t IN SELECT tgname FROM pg_trigger WHERE tgrelid='post_secrets.report_weight_evidence'::regclass AND tgisinternal LOOP EXECUTE format('ALTER TABLE post_secrets.report_weight_evidence DISABLE TRIGGER %I',t.tgname); END LOOP; END $$",
 "DO $$ DECLARE t record; BEGIN FOR t IN SELECT tgname FROM pg_trigger WHERE tgrelid='content.reports'::regclass AND tgconstraint=(SELECT oid FROM pg_constraint WHERE conrelid='post_secrets.report_weight_evidence'::regclass AND contype='f') LOOP EXECUTE format('ALTER TABLE content.reports DISABLE TRIGGER %I',t.tgname); END LOOP; END $$",
 'ALTER TABLE '+table+' DROP CONSTRAINT report_weight_evidence_numeric_pair_check; ALTER TABLE '+table+" ADD CONSTRAINT report_weight_evidence_numeric_pair_check CHECK(effective_weight IS NULL OR effective_weight=0.5)",
 "CREATE FUNCTION public.weight_drift_trigger() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NEW; END $$; CREATE TRIGGER unexpected_weight_trigger BEFORE INSERT ON "+table+' FOR EACH ROW EXECUTE FUNCTION public.weight_drift_trigger()',
]
with (root/'drift.sql').open('w') as out:
 for i,change in enumerate(changes):
  out.write('BEGIN; '+change+';\n')
  for role in ('board_public','board_staff'):
   out.write(f"SET LOCAL ROLE {role}; DO $check$ BEGIN IF ({query}) IS DISTINCT FROM false THEN RAISE EXCEPTION 'Readiness accepted evidence drift {i}'; END IF; END $check$; RESET ROLE;\n")
  out.write('ROLLBACK;\n')
PYREADINESS
cat > "$cluster/fixture.sql" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('weightobs','Owned weight evidence fixture','Synthetic',1000,100,100,100,10);
BEGIN;
SELECT set_config('board.posting_actor',repeat('11',32),true);
INSERT INTO content.threads(id,board) VALUES(10200001,'weightobs');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
SELECT 10200000+i,'weightobs',10200001,'Synthetic','','Retained body' FROM generate_series(1,12) i;
COMMIT;
SELECT content.import_report_catalog('{"version":1,"categories":[
 {"id":1,"board":"","op_only":false,"reply_only":false,"image_only":false,"exclude_boards":null,"title":"Synthetic half","weight":0.5,"filtered":0},
 {"id":31,"board":"","op_only":false,"reply_only":false,"image_only":false,"exclude_boards":null,"title":"Synthetic other","weight":2.0,"filtered":0}]}');
SQL
cat > "$cluster/admit-free.sql" <<'SQL'
SELECT content.admit_report('weightobs',10200001,'Synthetic anonymous free report',decode(repeat('41',32),'hex'),
 decode(repeat('51',32),'hex'),decode(repeat('21',32),'hex'),decode(repeat('22',32),'hex'),decode(repeat('23',32),'hex'),true,extract(epoch FROM clock_timestamp())::bigint);
SQL
for mode in upgrade fresh; do
 database="report_weight_$mode"
 create_database "$database"
 migrator=("${psql[@]}" -U board_migrator -d "$database")
 admin=(runuser -u postgres -- "${psql[@]}" -d "$database")
 for migration in migrations/*.sql; do
  [[ $migration < migrations/0102_report_weight_evidence.sql ]] || break
  "${migrator[@]}" --single-transaction -f - < "$migration"
 done
 "${admin[@]}" <<'SQL'
CREATE FUNCTION public.capture_known_rows() RETURNS TABLE(relation text,value jsonb)
LANGUAGE plpgsql AS $$ DECLARE r record; BEGIN
 FOR r IN SELECT n.nspname,c.relname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission') AND c.relkind='r' LOOP
  RETURN QUERY EXECUTE format('SELECT %L,to_jsonb(r) FROM %I.%I r',r.nspname||'.'||r.relname,r.nspname,r.relname);
 END LOOP;
END $$;
REVOKE ALL ON FUNCTION public.capture_known_rows() FROM PUBLIC;
CREATE VIEW public.known_functions AS
SELECT n.nspname,p.oid::regprocedure::text signature,pg_get_userbyid(p.proowner) owner,
 (SELECT jsonb_agg(jsonb_build_array(pg_get_userbyid(a.grantor),
 CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
 ORDER BY pg_get_userbyid(a.grantor),CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
 FROM aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a) acl,
 p.proconfig::text config,md5(pg_get_functiondef(p.oid)) definition
FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
WHERE n.nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission');
SQL
 if [[ $mode = upgrade ]]; then
  "${migrator[@]}" -f - < "$cluster/fixture.sql"
  "${psql[@]}" -U board_public -d "$database" -f - < "$cluster/admit-free.sql"
  "${psql[@]}" -U board_staff -d "$database" -c "SELECT content.admit_report('weightobs',10200002,'Synthetic legacy staff report',decode(repeat('42',32),'hex'))"
  "${admin[@]}" -c "INSERT INTO content.moderation_audit(account_id,board,target_id,action) VALUES(1,'weightobs',10200002,'resolve')"
  "${migrator[@]}" -c 'SELECT content.set_report_catalog_active(1)'
  "${psql[@]}" -U board_public -d "$database" <<'SQL'
SELECT content.admit_categorical_report('weightobs',10200003,1,1,decode(repeat('43',32),'hex'),
 decode(repeat('53',32),'hex'),decode(repeat('21',32),'hex'),decode(repeat('22',32),'hex'),decode(repeat('23',32),'hex'),true,extract(epoch FROM clock_timestamp())::bigint);
SQL
 fi
 "${admin[@]}" -c 'CREATE TABLE public.before_install AS SELECT * FROM public.capture_known_rows(); CREATE TABLE public.before_functions AS TABLE public.known_functions'
 "${migrator[@]}" --single-transaction -f - < migrations/0102_report_weight_evidence.sql
 "${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM post_secrets.report_weight_evidence)
 OR EXISTS(TABLE public.before_install EXCEPT ALL SELECT * FROM public.capture_known_rows())
 OR EXISTS(SELECT * FROM public.capture_known_rows() EXCEPT ALL TABLE public.before_install)
 THEN RAISE EXCEPTION '0102 backfilled evidence or mutated retained rows'; END IF;
 -- Only definitions of the two anonymous admissions may change. ACL/owner/
 -- configuration for even those two remain identical, as do all other helpers.
 IF EXISTS(SELECT nspname,signature,owner,acl,config FROM public.before_functions EXCEPT ALL
 SELECT nspname,signature,owner,acl,config FROM public.known_functions)
 OR EXISTS(SELECT nspname,signature,owner,acl,config FROM public.known_functions EXCEPT ALL
 SELECT nspname,signature,owner,acl,config FROM public.before_functions)
 OR (SELECT count(*) FROM public.before_functions b JOIN public.known_functions a USING(signature) WHERE a.definition<>b.definition)<>2
 OR EXISTS(SELECT 1 FROM public.before_functions b JOIN public.known_functions a USING(signature)
 WHERE a.definition<>b.definition AND a.signature NOT IN(
 'content.admit_report(text,bigint,text,bytea,bytea,bytea,bytea,bytea,boolean,bigint)',
 'content.admit_categorical_report(text,bigint,bigint,bigint,bytea,bytea,bytea,bytea,bytea,boolean,bigint)'))
 THEN RAISE EXCEPTION '0102 changed unintended functions or privileges'; END IF;
END $$;
DROP TABLE public.before_functions;
CREATE TABLE public.retained_history AS SELECT * FROM public.capture_known_rows()
WHERE relation IN('content.reports','content.moderation_audit','post_secrets.anonymous_sessions','post_secrets.anonymous_reports');
DROP TABLE public.before_install;
SQL
 if [[ $mode = fresh ]]; then
  "${migrator[@]}" -f - < "$cluster/fixture.sql"
 fi
 "${migrator[@]}" -c 'SELECT content.set_report_catalog_active(NULL)'
 "${psql[@]}" -U board_public -d "$database" <<'SQL'
SELECT content.admit_report('weightobs',10200004,'New synthetic free report',decode(repeat('44',32),'hex'),
 decode(repeat('54',32),'hex'),decode(repeat('21',32),'hex'),decode(repeat('22',32),'hex'),decode(repeat('23',32),'hex'),true,extract(epoch FROM clock_timestamp())::bigint);
SQL
 "${migrator[@]}" -c 'SELECT content.set_report_catalog_active(1)'
 "${admin[@]}" <<'SQL'
-- Known pre-report state, with no inferred staff, threat or history authority.
INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,
 created_at,network_at,address_at,environment_at,activity_at,action_at,expires_at)
SELECT decode(repeat('55',32),'hex'),decode(repeat('21',32),'hex'),decode(repeat('22',32),'hex'),decode(repeat('23',32),'hex'),
 at-7200,at-7200,at-7200,at-7200,at-1,at-1,at+86400
FROM (SELECT extract(epoch FROM clock_timestamp())::bigint at) now;
SQL
 "${psql[@]}" -U board_public -d "$database" <<'SQL'
SELECT content.admit_categorical_report('weightobs',10200005,1,1,decode(repeat('45',32),'hex'),
 decode(repeat('55',32),'hex'),decode(repeat('21',32),'hex'),decode(repeat('22',32),'hex'),decode(repeat('23',32),'hex'),false,extract(epoch FROM clock_timestamp())::bigint);
SELECT content.admit_categorical_report('weightobs',10200006,31,1,decode(repeat('46',32),'hex'),
 decode(repeat('56',32),'hex'),decode(repeat('21',32),'hex'),decode(repeat('22',32),'hex'),decode(repeat('23',32),'hex'),true,extract(epoch FROM clock_timestamp())::bigint);
SQL
 "${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF (SELECT count(*) FROM post_secrets.report_weight_evidence)<>3
 OR EXISTS(SELECT 1 FROM post_secrets.report_weight_evidence e JOIN content.reports r ON r.id=e.report_id
 WHERE r.post_id NOT IN(10200004,10200005,10200006) OR e.evaluator_version<>1
 OR e.known_or_verified IS DISTINCT FROM (r.post_id=10200005)
 OR e.authenticated_janitor_or_higher IS NOT NULL OR e.threat_at_least_point_four IS NOT NULL
 OR e.history_filtered IS NOT NULL OR e.source_reason IS NOT NULL
 OR e.evaluated_at IS NULL
 OR e.effective_weight IS DISTINCT FROM CASE WHEN r.post_id=10200005 THEN 0.5::double precision END
 OR e.numeric_proof IS DISTINCT FROM CASE WHEN r.post_id=10200005 THEN 'BaseEqualsFallback' END)
 THEN RAISE EXCEPTION 'Captured known/base-half evidence differs'; END IF;
 IF EXISTS(TABLE public.retained_history EXCEPT ALL SELECT * FROM public.capture_known_rows())
 THEN RAISE EXCEPTION 'New admission rewrote retained historical records'; END IF;
END $$;
CREATE VIEW public.weight_schema AS
SELECT 'column' kind,a.attname name,jsonb_build_array(a.atttypid::regtype::text,a.attnotnull,a.atthasdef,a.attidentity,a.attgenerated) value
FROM pg_attribute a WHERE a.attrelid='post_secrets.report_weight_evidence'::regclass AND a.attnum>0 AND NOT a.attisdropped
UNION ALL SELECT 'constraint',conname,jsonb_build_array(contype,convalidated,condeferrable,condeferred,pg_get_constraintdef(oid))
FROM pg_constraint WHERE conrelid='post_secrets.report_weight_evidence'::regclass
UNION ALL SELECT 'table',relname,jsonb_build_array(pg_get_userbyid(relowner),relrowsecurity,relforcerowsecurity,
 (SELECT jsonb_agg(jsonb_build_array(pg_get_userbyid(a.grantor),CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
 ORDER BY pg_get_userbyid(a.grantor),CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
 FROM aclexplode(coalesce(c.relacl,acldefault('r',c.relowner))) a))
FROM pg_class c WHERE c.oid='post_secrets.report_weight_evidence'::regclass;
CREATE TABLE public.before_current_evidence AS TABLE post_secrets.report_weight_evidence;
CREATE TABLE public.before_current_weight_schema AS TABLE public.weight_schema;
SQL
 # Keep the historical 0102 assertions above at their real migration boundary.
 # Current readiness also requires later migrations (including reporter clear
 # in 0106); never weaken the runtime query to fit a historical schema.
 for migration in migrations/*.sql; do
  [[ $migration > migrations/0102_report_weight_evidence.sql ]] || continue
  "${migrator[@]}" --single-transaction -f - < "$migration"
 done
 "${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(TABLE public.before_current_evidence EXCEPT ALL TABLE post_secrets.report_weight_evidence)
 OR EXISTS(TABLE post_secrets.report_weight_evidence EXCEPT ALL TABLE public.before_current_evidence)
 OR EXISTS(TABLE public.before_current_weight_schema EXCEPT ALL TABLE public.weight_schema)
 OR EXISTS(TABLE public.weight_schema EXCEPT ALL TABLE public.before_current_weight_schema)
 THEN RAISE EXCEPTION 'Current upgrade changed captured weight evidence or its contract'; END IF;
END $$;
DROP TABLE public.before_current_evidence;
DROP TABLE public.before_current_weight_schema;
SQL
 for phase in live restored; do
  if [[ $phase = restored ]]; then
   runuser -u postgres -- "$pg_bin/pg_dump" -h "$cluster" -U postgres -d "$database" --format=custom > "$cluster/current.dump"
   create_database "${database}_restore"
   runuser -u postgres -- "$pg_bin/pg_restore" -h "$cluster" --dbname="${database}_restore" --single-transaction --exit-on-error < "$cluster/current.dump"
   database="${database}_restore"
   admin=(runuser -u postgres -- "${psql[@]}" -d "$database")
  fi
  for role in board_public board_staff; do
   "${psql[@]}" -U "$role" -d "$database" -f - < "$cluster/readiness.sql"
  done
  "${admin[@]}" -f - < "$cluster/drift.sql"
  for role in board_public board_staff board_auth board_migrator; do
   "${psql[@]}" -U "$role" -d "$database" <<'SQL'
DO $$ BEGIN
 BEGIN
  PERFORM * FROM post_secrets.report_weight_evidence;
  RAISE EXCEPTION 'Runtime read private evidence' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN
  UPDATE post_secrets.report_weight_evidence SET known_or_verified=true WHERE false;
  RAISE EXCEPTION 'Runtime could modify evidence' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SQL
  done
  "${admin[@]}" -At > "$cluster/$mode-$phase.fingerprint" <<'SQL'
SELECT relation,md5(value::text) FROM public.capture_known_rows() ORDER BY relation,value::text;
SELECT * FROM public.known_functions ORDER BY nspname,signature;
SELECT kind,name,value FROM public.weight_schema ORDER BY kind,name;
SQL
 done
 cmp -s "$cluster/$mode-live.fingerprint" "$cluster/$mode-restored.fingerprint" || {
  printf 'Report-weight dump/restore fingerprint mismatch (%s).\n' "$mode" >&3; exit 1;
 }
 printf '%s private report-weight evidence migration and administrator dump/restore passed.\n' "$mode" >&3
done
printf '0102 private captured evidence and no-backfill migration qualified; current upgrade, readiness drift and dump/restore preserved evidence.\n' >&3
