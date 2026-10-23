#!/usr/bin/env bash
# Upgrade an owned 0257 database, reject drift, and preserve prior data/authority.
set -euo pipefail
cd "$(dirname "$0")/.."
pg_bin=${PG_BIN:-/usr/lib/postgresql/16/bin}
port=${SITE_SUBMISSIONS_TEST_PORT:-15501}
[[ $port =~ ^[0-9]+$ && $port -ge 1024 && $port -le 65535 ]] || exit 2
cluster=$(mktemp -d /tmp/board-site-submissions.XXXXXXXX)
as_pg=()
pg_user=$(id -un)
if [[ $(id -u) = 0 ]]; then
  chown postgres:postgres "$cluster"
  as_pg=(runuser -u postgres --)
  pg_user=postgres
fi
started=0
cleanup() {
  if [[ $started = 1 ]]; then
    "${as_pg[@]}" "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop >/dev/null
  fi
  [[ $cluster =~ ^/tmp/board-site-submissions\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
  [[ $(readlink -f "$cluster") = "$cluster" ]] || exit 1
  rm -rf -- "$cluster"
}
trap cleanup EXIT
"${as_pg[@]}" "$pg_bin/initdb" -D "$cluster/data" --auth=trust --encoding=UTF8 --no-locale >"$cluster/init.log"
"${as_pg[@]}" "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" -o "-h 127.0.0.1 -p $port -k ''" -w start >/dev/null
started=1
admin=("$pg_bin/psql" -X -v ON_ERROR_STOP=1 -h 127.0.0.1 -p "$port" -U "$pg_user" -d postgres)
"${admin[@]}" -f deploy/roles.sql >"$cluster/roles.log"
"${admin[@]}" -c 'CREATE DATABASE owned_site_submissions OWNER board_migrator;' \
  -c 'REVOKE ALL ON DATABASE owned_site_submissions FROM PUBLIC;' \
  -c 'GRANT CONNECT ON DATABASE owned_site_submissions TO board_migrator,board_public;' >/dev/null
migrator=("$pg_bin/psql" -X -v ON_ERROR_STOP=1 -h 127.0.0.1 -p "$port" -U board_migrator -d owned_site_submissions)
for file in migrations/[0-9][0-9][0-9][0-9]_*.sql; do
  [[ $file < migrations/0258_ ]] || break
  "${migrator[@]}" --single-transaction -f "$file" >>"$cluster/upgrade.log" 2>&1
done
"${migrator[@]}" -f fixtures/demo.sql >"$cluster/seed.log"
"${migrator[@]}" <<'SQL'
CREATE TABLE public.owned_submissions_before AS SELECT
 (SELECT jsonb_agg(to_jsonb(b) ORDER BY slug) FROM content.boards b) AS boards,
 (SELECT jsonb_agg(to_jsonb(p) ORDER BY id) FROM content.posts p) AS posts,
 (SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM content.threads t) AS threads;
SQL
if "${migrator[@]}" --single-transaction -c 'GRANT SELECT ON content.posts TO board_submission_owner;' \
 -f migrations/0258_private_site_submissions.sql >"$cluster/rejected.log" 2>&1; then
 echo 'Submission migration accepted existing owner authority' >&2; exit 1
fi
grep -q 'Submission owner has unexpected existing authority' "$cluster/rejected.log"
"${migrator[@]}" -c BEGIN -f migrations/0258_private_site_submissions.sql -c ROLLBACK >"$cluster/rollback.log"
"${migrator[@]}" -c "DO \$\$ BEGIN IF to_regnamespace('submission_private') IS NOT NULL OR to_regnamespace('submission_api') IS NOT NULL THEN RAISE EXCEPTION 'Rollback retained submission schemas'; END IF; END \$\$;" >/dev/null
"${migrator[@]}" --single-transaction -f migrations/0258_private_site_submissions.sql >"$cluster/accepted.log"
"${migrator[@]}" -f scripts/test-site-submission-storage.sql >"$cluster/storage.log"
"${migrator[@]}" <<'SQL'
INSERT INTO submission_private.application_campaigns(id,starts_at,ends_at,boards,questions,field_limit,answer_limit,capacity)
VALUES(777,100,200,ARRAY['g'],ARRAY['Owned one','Owned two','Owned three','Owned four'],150,2000,100);
INSERT INTO submission_private.applications(campaign_id,age,timezone,hours,times,board1,answers,created_at,updated_at)
VALUES(777,25,-4,3,'16:00-19:00','g',ARRAY['one','two','three','four'],100,100);
INSERT INTO submission_private.application_identity(application_id,campaign_id,ownership_hash,actor_hash,firstname,handle,email,email_identity)
SELECT id,777,decode(repeat('a1',32),'hex'),decode(repeat('b2',32),'hex'),'Owned','Owned handle','owned@example.test','owned@example.test'
FROM submission_private.applications WHERE campaign_id=777;
CREATE TABLE public.owned_submission_identity_before AS SELECT * FROM submission_private.application_identity;
SQL
cat >"$cluster/check.sql" <<'SQL'
DO $$ BEGIN
 IF (SELECT boards FROM public.owned_submissions_before) IS DISTINCT FROM
  (SELECT jsonb_agg(to_jsonb(b) ORDER BY slug) FROM content.boards b)
 OR (SELECT posts FROM public.owned_submissions_before) IS DISTINCT FROM
  (SELECT jsonb_agg(to_jsonb(p) ORDER BY id) FROM content.posts p)
 OR (SELECT threads FROM public.owned_submissions_before) IS DISTINCT FROM
  (SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM content.threads t)
 OR EXISTS((SELECT * FROM submission_private.application_identity EXCEPT SELECT * FROM public.owned_submission_identity_before)
  UNION ALL (SELECT * FROM public.owned_submission_identity_before EXCEPT SELECT * FROM submission_private.application_identity))
 THEN RAISE EXCEPTION 'Submission migration or restore changed retained data'; END IF;
END $$;
SQL
"${migrator[@]}" -f "$cluster/check.sql"
public=("$pg_bin/psql" -XAt -v ON_ERROR_STOP=1 -h 127.0.0.1 -p "$port" -U board_public)
ready=$("${public[@]}" -d owned_site_submissions -c 'SELECT submission_api.ready();')
[[ $ready = t ]] || { echo "Submission readiness failed before dump: $ready" >&2; exit 1; }
echo 'Submission readiness passed before dump.'
"${migrator[@]}" -At -c "SELECT prosrc FROM pg_proc WHERE oid='submission_private.catalog_hash()'::regprocedure" >"$cluster/catalog.sql"
python3 - "$cluster/catalog.sql" <<'PYCODE'
import sys
p=sys.argv[1]
s=open(p).read().replace('SELECT sha256(convert_to(jsonb_build_object(', 'SELECT jsonb_build_object(', 1)
s=s.replace(")::text,'UTF8'))", ")::text;")
open(p,'w').write('SET search_path=pg_catalog,pg_temp;\n'+s)
PYCODE
"${migrator[@]}" -At -f "$cluster/catalog.sql" >"$cluster/before.json"
"$pg_bin/pg_dump" -h 127.0.0.1 -p "$port" -U "$pg_user" -d owned_site_submissions -Fc >"$cluster/submissions.dump"
"${admin[@]}" -c 'CREATE DATABASE owned_site_submissions_restore OWNER board_migrator;' \
 -c 'REVOKE ALL ON DATABASE owned_site_submissions_restore FROM PUBLIC;' \
 -c 'GRANT CONNECT ON DATABASE owned_site_submissions_restore TO board_migrator,board_public;' >/dev/null
"$pg_bin/pg_restore" -h 127.0.0.1 -p "$port" -U "$pg_user" -d owned_site_submissions_restore --exit-on-error <"$cluster/submissions.dump"
"$pg_bin/psql" -XAt -v ON_ERROR_STOP=1 -h 127.0.0.1 -p "$port" -U "$pg_user" -d owned_site_submissions_restore -f "$cluster/catalog.sql" >"$cluster/after.json"
python3 - "$cluster/before.json" "$cluster/after.json" <<'PYCODE'
import sys,json
before,after=(json.loads(open(p).read().splitlines()[-1]) for p in sys.argv[1:])
for key in before:
    if before[key]!=after[key]:
        print('Catalog restore changed '+key, file=sys.stderr)
        print(json.dumps({'before':before[key],'after':after[key]},indent=2),file=sys.stderr)
        raise SystemExit(1)
PYCODE
ready=$("${public[@]}" -d owned_site_submissions_restore -c 'SELECT submission_api.ready();')
[[ $ready = t ]] || { echo "Submission readiness failed after restore: $ready" >&2; exit 1; }
"$pg_bin/psql" -X -v ON_ERROR_STOP=1 -h 127.0.0.1 -p "$port" -U board_migrator -d owned_site_submissions_restore -f "$cluster/check.sql"
printf 'Submission storage rollback, authority drift, retained data and restore passed.\n'
