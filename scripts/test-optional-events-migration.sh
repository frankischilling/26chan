#!/usr/bin/env bash
# Upgrade an owned 0250 database, reject drift, and preserve prior data/authority.
set -euo pipefail
cd "$(dirname "$0")/.."
pg_bin=${PG_BIN:-/usr/lib/postgresql/16/bin}
port=${OPTIONAL_EVENTS_TEST_PORT:-15499}
[[ $port =~ ^[0-9]+$ && $port -ge 1024 && $port -le 65535 ]] || exit 2
cluster=$(mktemp -d /tmp/board-optional-events.XXXXXXXX)
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
  [[ $cluster =~ ^/tmp/board-optional-events\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
  [[ $(readlink -f "$cluster") = "$cluster" ]] || exit 1
  rm -rf -- "$cluster"
}
trap cleanup EXIT
"${as_pg[@]}" "$pg_bin/initdb" -D "$cluster/data" --auth=trust --encoding=UTF8 --no-locale >"$cluster/init.log"
"${as_pg[@]}" "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" -o "-h 127.0.0.1 -p $port -k ''" -w start >/dev/null
started=1
admin=("$pg_bin/psql" -X -v ON_ERROR_STOP=1 -h 127.0.0.1 -p "$port" -U "$pg_user" -d postgres)
"${admin[@]}" -f deploy/roles.sql >"$cluster/roles.log"
"${admin[@]}" -c 'CREATE DATABASE owned_optional_events OWNER board_migrator;' \
  -c 'REVOKE ALL ON DATABASE owned_optional_events FROM PUBLIC;' \
  -c 'GRANT CONNECT ON DATABASE owned_optional_events TO board_migrator,board_public;' >/dev/null
migrator=("$pg_bin/psql" -X -v ON_ERROR_STOP=1 -h 127.0.0.1 -p "$port" -U board_migrator -d owned_optional_events)
for file in migrations/[0-9][0-9][0-9][0-9]_*.sql; do
  [[ $file < migrations/0251_ ]] || break
  "${migrator[@]}" --single-transaction -f "$file" >>"$cluster/upgrade.log" 2>&1
done
"${migrator[@]}" -f fixtures/demo.sql >"$cluster/seed.log"
"${migrator[@]}" <<'SQL'
CREATE TABLE public.owned_events_before AS SELECT
 (SELECT jsonb_agg(to_jsonb(b) ORDER BY slug) FROM content.boards b) AS boards,
 (SELECT jsonb_agg(to_jsonb(p) ORDER BY id) FROM content.posts p) AS posts,
 (SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM content.threads t) AS threads;
SQL
for drift in "ALTER TABLE content.boards ALTER COLUMN title SET DEFAULT 'owned drift';" \
             "GRANT SELECT ON content.posts TO board_event_owner;"; do
  if "${migrator[@]}" --single-transaction -c "$drift" -f migrations/0251_optional_event_accounts.sql >"$cluster/rejected.log" 2>&1; then
    echo 'Optional event migration accepted pre-existing authority drift' >&2; exit 1
  fi
  grep -Eq 'Transfer authority drifted|Event owner has unexpected existing authority' "$cluster/rejected.log"
done
"${migrator[@]}" -c BEGIN -f migrations/0251_optional_event_accounts.sql -c ROLLBACK >"$cluster/rollback.log"
"${migrator[@]}" -c "DO \$\$ BEGIN IF to_regnamespace('event_private') IS NOT NULL THEN RAISE EXCEPTION 'Rollback retained event authority'; END IF; END \$\$;" >/dev/null
"${migrator[@]}" --single-transaction -f migrations/0251_optional_event_accounts.sql >"$cluster/accepted.log"
"${migrator[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM event_private.runs) THEN RAISE EXCEPTION 'Migration enabled an event'; END IF;
END $$;
INSERT INTO event_private.runs(id,kind,starts_at,ends_at,account_count) VALUES(1,1,1,253402300799,1),(2,2,1,253402300799,1);
INSERT INTO event_private.accounts(run_id,kind,actor_hash,created_at,balance,last_action_at)
 VALUES(1,1,decode(repeat('11',32),'hex'),100,1234,101),(2,2,decode(repeat('22',32),'hex'),100,100,100);
UPDATE event_private.accounts SET owned=ARRAY[108,109]::smallint[] WHERE run_id=2;
CREATE TABLE public.owned_events_accounts AS SELECT * FROM event_private.accounts;
SQL
"$pg_bin/psql" -X -v ON_ERROR_STOP=1 -h 127.0.0.1 -p "$port" -U "$pg_user" -d owned_optional_events -f scripts/test-optional-events-upgrade.sql
# Compare the catalog preimage if administrator restore changes readiness.
"${migrator[@]}" -At -c "SELECT prosrc FROM pg_proc WHERE oid='event_private.catalog_hash()'::regprocedure" >"$cluster/catalog.sql"
python3 - "$cluster/catalog.sql" <<'PYCODE'
import sys
p=sys.argv[1]
s=open(p).read().replace('SELECT sha256(convert_to(jsonb_build_object(', 'SELECT jsonb_build_object(', 1)
s=s.replace(")::text,'UTF8'))", ")::text;")
open(p,'w').write('SET search_path=pg_catalog,pg_temp;\n'+s)
PYCODE
"${migrator[@]}" -At -f "$cluster/catalog.sql" >"$cluster/before.json"
"$pg_bin/pg_dump" -h 127.0.0.1 -p "$port" -U "$pg_user" -d owned_optional_events -Fc >"$cluster/events.dump"
"${admin[@]}" -c 'CREATE DATABASE owned_optional_events_restore OWNER board_migrator;' >/dev/null
"$pg_bin/pg_restore" -h 127.0.0.1 -p "$port" -U "$pg_user" -d owned_optional_events_restore --exit-on-error <"$cluster/events.dump"
"$pg_bin/psql" -XAt -v ON_ERROR_STOP=1 -h 127.0.0.1 -p "$port" -U "$pg_user" -d owned_optional_events_restore -f "$cluster/catalog.sql" >"$cluster/after.json"
python3 - "$cluster/before.json" "$cluster/after.json" <<'PYCODE'
import sys,json
before,after=(json.loads(open(p).read().splitlines()[-1]) for p in sys.argv[1:])
for key in before:
 if before[key]!=after[key]: raise SystemExit('Catalog restore changed '+key)
PYCODE
"$pg_bin/psql" -X -v ON_ERROR_STOP=1 -h 127.0.0.1 -p "$port" -U "$pg_user" -d owned_optional_events_restore -f scripts/test-optional-events-upgrade.sql
printf 'Optional event rollback, upgrade, authority drift, retained history and administrator restore passed.\n'
