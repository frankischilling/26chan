#!/usr/bin/env bash
# Owned disposable PostgreSQL: populated upgrade and runtime policy authority.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-autoarchive-policy.XXXXXXXX)
started=0
cleanup() {
    status=$?
    trap - EXIT
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop >/dev/null || status=1
    fi
    [[ $cluster =~ ^/tmp/board-autoarchive-policy\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
    [[ $(readlink -f "$cluster") = "$cluster" ]] || exit 1
    rm -rf -- "$cluster"
    exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
chown postgres:postgres "$cluster"
runuser -u postgres -- "$pg_bin/initdb" -D "$cluster/data" --auth=trust --encoding=UTF8 --no-locale >/dev/null
runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" \
    -o "-c listen_addresses='' -c unix_socket_directories='$cluster' -c statement_timeout=30000 -c lock_timeout=5000" -w start >/dev/null
started=1
psql=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h "$cluster")
runuser -u postgres -- "${psql[@]}" -d postgres -f - < deploy/roles.sql
runuser -u postgres -- "${psql[@]}" -d postgres <<'SQL'
CREATE DATABASE autoarchive_upgrade OWNER board_migrator;
REVOKE ALL ON DATABASE autoarchive_upgrade FROM PUBLIC;
GRANT CONNECT ON DATABASE autoarchive_upgrade TO board_migrator,board_public;
SQL
migrator=("${psql[@]}" -U board_migrator -d autoarchive_upgrade)
for migration in migrations/*.sql; do
    [[ $(basename "$migration") < 0137_ ]] || break
    "${migrator[@]}" --single-transaction -f - < "$migration" >/dev/null
done
"${migrator[@]}" -f - < fixtures/demo.sql >/dev/null
"${migrator[@]}" <<'SQL'
UPDATE content.boards SET reply_limit=800,bump_limit=300 WHERE slug='jp';
CREATE TABLE content.autoarchive_upgrade_witness AS SELECT to_jsonb(t) row FROM content.threads t;
SET ROLE board_thread_transfer_owner;
DO $$ BEGIN
 IF NOT content.text_thread_transfer_ready() THEN RAISE EXCEPTION 'Transfer precondition failed'; END IF;
END $$;
RESET ROLE;
SQL
catalog_before=$("${migrator[@]}" -Atc "SET ROLE board_thread_transfer_owner; SELECT encode(definition_hash,'hex') FROM post_secrets.thread_transfer_catalog WHERE singleton")
"${migrator[@]}" -c "SET ROLE board_thread_transfer_owner; UPDATE post_secrets.thread_transfer_catalog SET definition_hash=decode(repeat('00',32),'hex')"
if "${migrator[@]}" --single-transaction -f - < migrations/0137_source_autoarchive.sql >"$cluster/rejected.out" 2>"$cluster/rejected.err"; then
 echo 'Autoarchive migration accepted prior catalog drift.' >&2; exit 1
fi
grep -Fq 'Text transfer catalog is not ready for autoarchive policy.' "$cluster/rejected.err"
"${migrator[@]}" -v old_hash="$catalog_before" <<'SQL'
SET ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=decode(:'old_hash','hex') WHERE singleton;
RESET ROLE;
SQL
"${migrator[@]}" --single-transaction -f - < migrations/0137_source_autoarchive.sql
"${migrator[@]}" <<'SQL'
DO $$ BEGIN
 IF (SELECT autoarchive_cap<>1500 OR reply_limit<>800 FROM content.boards WHERE slug='jp')
 OR (SELECT autoarchive_cap<>1500 OR reply_limit<>1000 FROM content.boards WHERE slug='vt')
 OR EXISTS(SELECT 1 FROM content.boards WHERE slug NOT IN ('jp','vt') AND autoarchive_cap<>0)
 THEN RAISE EXCEPTION 'Source threshold import or operator ceiling differs'; END IF;
 IF EXISTS((SELECT row FROM content.autoarchive_upgrade_witness EXCEPT SELECT to_jsonb(t) FROM content.threads t)
 UNION ALL (SELECT to_jsonb(t) FROM content.threads t EXCEPT SELECT row FROM content.autoarchive_upgrade_witness))
 THEN RAISE EXCEPTION 'Migration changed historical threads'; END IF;

END $$;
DROP TABLE content.autoarchive_upgrade_witness;
SET ROLE board_thread_transfer_owner;
DO $$ BEGIN IF NOT content.text_thread_transfer_ready() THEN RAISE EXCEPTION 'Transfer readiness lost'; END IF; END $$;
RESET ROLE;
SQL
"${psql[@]}" -U board_public -d autoarchive_upgrade <<'SQL'
DO $$ BEGIN
 PERFORM autoarchive_cap FROM content.boards WHERE slug='vt';
 BEGIN
  UPDATE content.boards SET autoarchive_cap=0 WHERE slug='vt';
  RAISE EXCEPTION 'Public changed archive policy' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SQL
printf 'Autoarchive migration preserved operator ceilings and history, rejected drift, and retained runtime policy denials.\n'
