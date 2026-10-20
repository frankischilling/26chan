#!/usr/bin/env bash
# Owned disposable cluster; no application configuration or production data.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
pg_bin=${PG_BIN:-/usr/lib/postgresql/16/bin}
cluster=$(mktemp -d /tmp/board-vip-credentials.XXXXXXXX)
runner=()
if [[ $(id -u) = 0 ]]; then
    chown postgres:postgres "$cluster"
    runner=(runuser -u postgres --)
fi
started=0
cleanup() {
    status=$?
    trap - EXIT
    [[ $cluster =~ ^/tmp/board-vip-credentials\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
    [[ $(readlink -f "$cluster") = "$cluster" ]] || exit 1
    if [[ $started = 1 ]]; then
        "${runner[@]}" "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop >/dev/null 2>&1 || status=1
        if "${runner[@]}" "$pg_bin/pg_ctl" -D "$cluster/data" status >/dev/null 2>&1; then server_status=0; else server_status=$?; fi
        if [[ $server_status != 3 ]]; then echo "Cluster stop unconfirmed; preserved $cluster" >&2; exit 1; fi
    fi
    rm -rf -- "$cluster"
    exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
"${runner[@]}" "$pg_bin/initdb" -D "$cluster/data" --username=postgres --auth=trust --encoding=UTF8 --no-locale >/dev/null
started=1
"${runner[@]}" "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" -o "-c listen_addresses='127.0.0.1' -c port=15455 -c unix_socket_directories='' -c statement_timeout=30000 -c lock_timeout=5000" -w start >/dev/null || { cat "$cluster/server.log" >&2; exit 1; }
psql=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h 127.0.0.1 -p 15455 -U postgres)
"${psql[@]}" -d postgres -f - < deploy/roles.sql >/dev/null
"${psql[@]}" -d postgres <<'SQL'
CREATE DATABASE vip_credentials OWNER board_migrator;
REVOKE ALL ON DATABASE vip_credentials FROM PUBLIC;
GRANT CONNECT ON DATABASE vip_credentials TO board_migrator,board_verifier,board_public;
SQL
migrator=("${psql[@]}" -U board_migrator -d vip_credentials)
for migration in migrations/*.sql; do
    [[ $(basename "$migration") < 0238_ ]] || break
    "${migrator[@]}" --single-transaction -f - < "$migration" >/dev/null
done
"${migrator[@]}" -c BEGIN -f migrations/0238_vip_credentials.sql -c ROLLBACK
"${psql[@]}" -d vip_credentials <<'SQL'
DO $$ BEGIN
 IF to_regclass('verification_private.vip_accounts') IS NOT NULL
  OR to_regprocedure('content.public_vip_ready()') IS NOT NULL
  OR content.public_pass_ready() IS DISTINCT FROM true
  OR content.public_challenge_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'VIP rollback changed existing authority'; END IF;
END $$;
SQL
"${migrator[@]}" --single-transaction -f migrations/0238_vip_credentials.sql
"${psql[@]}" -d vip_credentials -f scripts/test-vip-credentials.sql
"$pg_bin/pg_dump" -h 127.0.0.1 -p 15455 -U postgres -d vip_credentials -Fc -f "$cluster/vip.dump"
"${psql[@]}" -d postgres -c 'CREATE DATABASE vip_restored OWNER board_migrator'
"$pg_bin/pg_restore" -h 127.0.0.1 -p 15455 -U postgres -d vip_restored --exit-on-error "$cluster/vip.dump"
"${psql[@]}" -d vip_restored <<'SQL'
DO $$ BEGIN
 IF content.public_vip_ready() IS DISTINCT FROM true
  OR content.public_pass_ready() IS DISTINCT FROM true
  OR content.public_challenge_ready() IS DISTINCT FROM true
  OR (SELECT count(*) FROM verification_private.vip_accounts)<>2
  OR (SELECT count(*) FROM verification_private.vip_receipts)<>1
  OR NOT EXISTS(SELECT 1 FROM verification_private.vip_accounts WHERE name='Renamed Owned VIP' AND credential_version=2) THEN
  RAISE EXCEPTION 'Populated VIP restore differs'; END IF;
END $$;
SQL
printf '%s\n' 'VIP credential upgrade, rollback, authority and populated restore passed.'
