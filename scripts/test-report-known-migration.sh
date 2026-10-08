#!/usr/bin/env bash
# Owned synthetic clusters only. Qualify 0100 -> 0101 and frozen 0100 dump/restore.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-report-known.XXXXXXXX)
started=0
cleanup() {
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null
    fi
    [[ $cluster =~ ^/tmp/board-report-known\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
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
trap 'printf "Report-known qualification failed at line %s (private diagnostics removed during cleanup).\n" "$LINENO" >&3' ERR
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
# Frozen 0101 startup SQL; this historical qualification stops before 0102.
# Drift probes use serving-role catalog access without private-schema grants.
python3 - "$cluster" <<'PYREADINESS'
import pathlib,sys
root=pathlib.Path(sys.argv[1])
query=pathlib.Path('scripts/fixtures/report-known-0101-readiness.sql').read_text().strip().rstrip(';')
(root/'readiness.sql').write_text("DO $check$ BEGIN IF ("+query+") IS DISTINCT FROM true THEN RAISE EXCEPTION '0101 readiness failed'; END IF; END $check$;")
signature='post_secrets.report_known_or_verified(bytea,bytea,bytea,bytea,boolean,bigint)'
changes=[
 'DROP FUNCTION '+signature,
 'ALTER FUNCTION '+signature+' OWNER TO board_report_admission_owner',
 'ALTER FUNCTION '+signature+' SECURITY INVOKER',
 'ALTER FUNCTION '+signature+' SET search_path=public',
 'ALTER FUNCTION '+signature+' STABLE',
 'REVOKE EXECUTE ON FUNCTION '+signature+' FROM board_report_admission_owner',
] + ['GRANT EXECUTE ON FUNCTION '+signature+' TO '+role for role in ('PUBLIC','board_public','board_staff','board_auth','board_migrator')]
with (root/'drift.sql').open('w') as out:
 for i,change in enumerate(changes):
  out.write('BEGIN; '+change+';\n')
  for role in ('board_public','board_staff'):
   out.write(f"SET LOCAL ROLE {role}; DO $check$ BEGIN IF ({query}) IS DISTINCT FROM false THEN RAISE EXCEPTION 'Readiness accepted helper drift {i}'; END IF; END $check$; RESET ROLE;\n")
  out.write('ROLLBACK;\n')
PYREADINESS
cat > "$cluster/fixture.sql" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('knownobs','Owned observation fixture','Synthetic',1000,100,100,100,10);
-- Deliberately synthetic owner-maintenance session; no capability is allocated
-- by the helper. The UUID is a fixture marker, not a source identity mapping.
INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,
 created_at,network_at,address_at,environment_at,activity_at,action_at,expires_at,
 verified_level,posts,images,threads,reports,pending,change_score,automatic_identity)
SELECT decode(repeat('11',32),'hex'),decode(repeat('21',32),'hex'),decode(repeat('22',32),'hex'),decode(repeat('23',32),'hex'),
 at-7200,at-7200,at-7200,at-7200,at-1,at-1,at+86400,
 0,3,0,0,9,8,0,'10000000-0000-4000-8000-000000000101'::uuid
FROM (SELECT extract(epoch FROM clock_timestamp())::bigint at) now;
SQL
cat > "$cluster/contract.sql" <<'SQL'
DO $$ DECLARE p record; BEGIN
 SELECT f.* INTO STRICT p FROM pg_proc f JOIN pg_namespace n ON n.oid=f.pronamespace
 WHERE n.nspname='post_secrets' AND f.proname='report_known_or_verified'
 AND f.oid='post_secrets.report_known_or_verified(bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure;
 IF pg_get_userbyid(p.proowner)<>'board_anonymous_owner' OR NOT p.prosecdef
 OR p.prorettype<>'boolean'::regtype OR p.proretset OR p.provolatile<>'v'
 OR p.proconfig IS DISTINCT FROM ARRAY['search_path=pg_catalog, pg_temp']::text[]
 OR p.prolang<>(SELECT oid FROM pg_language WHERE lanname='plpgsql')
 THEN RAISE EXCEPTION 'Private helper metadata differs'; END IF;
 IF (SELECT count(*) FROM aclexplode(p.proacl))<>2 OR EXISTS(
 SELECT 1 FROM aclexplode(p.proacl) a WHERE a.privilege_type<>'EXECUTE' OR a.is_grantable
 OR pg_get_userbyid(a.grantor)<>'board_anonymous_owner'
 OR a.grantee NOT IN(SELECT oid FROM pg_roles WHERE rolname IN('board_anonymous_owner','board_report_admission_owner')))
 THEN RAISE EXCEPTION 'Private helper ACL differs'; END IF;
END $$;
SQL
cat > "$cluster/observe.sql" <<'SQL'
BEGIN;
-- Match the private caller's board -> gate -> session lock prerequisites.
SELECT slug FROM content.boards WHERE slug='knownobs' FOR UPDATE;
SELECT singleton FROM post_secrets.report_admission_gate FOR UPDATE;
SET LOCAL ROLE board_report_admission_owner;
DO $$ DECLARE at bigint:=extract(epoch FROM clock_timestamp())::bigint; BEGIN
 IF post_secrets.report_known_or_verified(decode(repeat('11',32),'hex'),
 decode(repeat('21',32),'hex'),decode(repeat('22',32),'hex'),decode(repeat('23',32),'hex'),false,at) IS DISTINCT FROM true
 THEN RAISE EXCEPTION 'Known synthetic snapshot refused'; END IF;
 IF post_secrets.report_known_or_verified(decode(repeat('99',32),'hex'),
 decode(repeat('21',32),'hex'),decode(repeat('22',32),'hex'),decode(repeat('23',32),'hex'),true,at) IS DISTINCT FROM false
 THEN RAISE EXCEPTION 'Absent minted snapshot became known'; END IF;
END $$;
COMMIT;
SQL
for mode in upgrade fresh; do
 database="report_known_$mode"
 create_database "$database"
 migrator=("${psql[@]}" -U board_migrator -d "$database")
 admin=(runuser -u postgres -- "${psql[@]}" -d "$database")
 for migration in migrations/*.sql; do
  [[ $migration < migrations/0101_report_known_or_verified.sql ]] || break
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
  "${admin[@]}" -f - < "$cluster/fixture.sql"
 fi
 "${admin[@]}" -c 'CREATE TABLE public.before_install AS SELECT * FROM public.capture_known_rows(); CREATE TABLE public.before_functions AS TABLE public.known_functions'
 "${migrator[@]}" --single-transaction -f - < migrations/0101_report_known_or_verified.sql
 "${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(TABLE public.before_install EXCEPT ALL SELECT * FROM public.capture_known_rows())
 OR EXISTS(SELECT * FROM public.capture_known_rows() EXCEPT ALL TABLE public.before_install)
 THEN RAISE EXCEPTION '0101 mutated retained rows'; END IF;
 IF EXISTS(TABLE public.before_functions EXCEPT ALL TABLE public.known_functions)
 OR EXISTS(SELECT * FROM public.known_functions
 WHERE signature<>'post_secrets.report_known_or_verified(bytea,bytea,bytea,bytea,boolean,bigint)'
 EXCEPT ALL TABLE public.before_functions)
 THEN RAISE EXCEPTION '0101 changed existing functions or public admission wiring'; END IF;
END $$;
DROP TABLE public.before_install,public.before_functions;
SQL
 if [[ $mode = fresh ]]; then
  "${admin[@]}" -f - < "$cluster/fixture.sql"
 fi
 for phase in live restored; do
  if [[ $phase = restored ]]; then
   runuser -u postgres -- "$pg_bin/pg_dump" -h "$cluster" -U postgres -d "$database" --format=custom > "$cluster/current.dump"
   create_database "${database}_restore"
   runuser -u postgres -- "$pg_bin/pg_restore" -h "$cluster" --dbname="${database}_restore" --single-transaction --exit-on-error < "$cluster/current.dump"
   database="${database}_restore"
   admin=(runuser -u postgres -- "${psql[@]}" -d "$database")
  fi
  "${admin[@]}" -f - < "$cluster/contract.sql"
  for role in board_public board_staff; do
   "${psql[@]}" -U "$role" -d "$database" -f - < "$cluster/readiness.sql"
  done
  "${admin[@]}" -f - < "$cluster/drift.sql"
  "${admin[@]}" -c 'CREATE TABLE public.before_observe AS SELECT * FROM public.capture_known_rows()'
  "${admin[@]}" -f - < "$cluster/observe.sql"
  for role in board_public board_staff board_auth board_migrator; do
   "${psql[@]}" -U "$role" -d "$database" <<'SQL'
DO $$ BEGIN
 BEGIN
  PERFORM post_secrets.report_known_or_verified(decode(repeat('11',32),'hex'),
   decode(repeat('21',32),'hex'),decode(repeat('22',32),'hex'),decode(repeat('23',32),'hex'),
   false,extract(epoch FROM clock_timestamp())::bigint);
  RAISE EXCEPTION 'Non-owner directly called private observation' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SQL
  done
  "${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(TABLE public.before_observe EXCEPT ALL SELECT * FROM public.capture_known_rows())
 OR EXISTS(SELECT * FROM public.capture_known_rows() EXCEPT ALL TABLE public.before_observe)
 THEN RAISE EXCEPTION 'Observation or rejected call mutated durable state'; END IF;
END $$;
DROP TABLE public.before_observe;
SQL
  "${admin[@]}" -At > "$cluster/$mode-$phase.fingerprint" <<'SQL'
SELECT relation,md5(value::text) FROM public.capture_known_rows() ORDER BY relation,value::text;
-- Includes the helper definition, owner, configuration and exact ACL.
SELECT * FROM public.known_functions ORDER BY nspname,signature;
SQL
 done
 cmp -s "$cluster/$mode-live.fingerprint" "$cluster/$mode-restored.fingerprint" || {
  printf 'Report-known dump/restore fingerprint mismatch (%s).\n' "$mode" >&3; exit 1;
 }
 printf '%s private report-known migration and administrator dump/restore passed.\n' "$mode" >&3
done
printf '0101 private helper metadata, read-only observation, runtime denial and readiness qualified; no public policy wiring.\n' >&3
