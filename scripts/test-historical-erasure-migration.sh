#!/usr/bin/env bash
# Owned synthetic clusters only. Never connects to an application database.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
binary=$(realpath target/debug/board-deletion-cleanup)
[[ -x "$binary" ]] || { echo 'Build board-deletion-cleanup first.' >&2; exit 1; }
cluster=$(mktemp -d /tmp/board-historical-erasure.XXXXXXXX)
exec 3>&1
started=0
cleanup() {
    status=$?
    trap - EXIT
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null || status=1
    fi
    [[ $cluster =~ ^/tmp/board-historical-erasure\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
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
trap 'printf "Historical-erasure qualification failed at line %s (private diagnostics removed during cleanup).\n" "$LINENO" >&3' ERR
runuser -u postgres -- "${psql[@]}" -d postgres -f - < deploy/roles.sql
runuser -u postgres -- "${psql[@]}" -d postgres -c 'ALTER ROLE board_staff LOGIN; ALTER ROLE board_auth LOGIN; ALTER ROLE board_media_read LOGIN; ALTER ROLE board_media_intake LOGIN; ALTER ROLE board_monitor LOGIN'
create_database() {
 runuser -u postgres -- "${psql[@]}" -d postgres -v database="$1" <<'SQL'
CREATE DATABASE :"database" OWNER board_migrator;
REVOKE ALL ON DATABASE :"database" FROM PUBLIC;
GRANT CONNECT ON DATABASE :"database" TO board_migrator,board_public,board_staff,board_auth,board_media,board_media_read,board_media_intake,board_monitor;
SQL
}
create_database historical_erasure
migrator=("${psql[@]}" -U board_migrator -d historical_erasure)
for migration in migrations/*.sql; do
 [[ $(basename "$migration") < 0129_ ]] || break
 "${migrator[@]}" --single-transaction -f - < "$migration"
done
runuser -u postgres -- "${psql[@]}" -d historical_erasure -f - < scripts/historical-erasure-fixture.sql
for migration in migrations/*.sql; do
 [[ $(basename "$migration") > 0128_poll_voting.sql ]] || continue
 "${migrator[@]}" --single-transaction -f - < "$migration"
done
# Installation must not silently perform the irreversible operator action.
[[ $("${migrator[@]}" -Atc "SELECT count(*) FROM content.posts WHERE board='hclean' AND NOT content_erased AND comment<>''") = 133 ]]
BOARD_TEST_CLUSTER="$cluster" ERASURE_TEST_PSQL="$pg_bin/psql" ERASURE_TEST_HOST="$cluster" \
 ERASURE_TEST_PORT=5432 ERASURE_TEST_DATABASE=historical_erasure ERASURE_TEST_OWNER=postgres \
 python3 scripts/test-historical-erasure.py "$binary"

# Dump/restore retains tombstones, independent records, role contracts and seals.
runuser -u postgres -- "$pg_bin/pg_dump" -h "$cluster" -Fc historical_erasure > "$cluster/restore.dump"
create_database historical_restore
runuser -u postgres -- "$pg_bin/pg_restore" -h "$cluster" --exit-on-error -d historical_restore < "$cluster/restore.dump"
for database in historical_erasure historical_restore; do
 MIGRATION_DATABASE_URL="postgresql://board_migrator@localhost/$database?host=$cluster" \
 "$binary" preview hclean > "$cluster/$database.json"
 python3 - "$cluster/$database.json" <<'PY'
import json,sys
value=json.load(open(sys.argv[1]))
assert value["report_ids"]==value["post_ids"]==value["thread_ids"]==[]
assert not value["applied"]
PY
 runuser -u postgres -- "${psql[@]}" -d "$database" -Atc \
 "SELECT encode(sha256(convert_to(jsonb_agg(jsonb_build_array(relation,value) ORDER BY relation,value)::text,'UTF8')),'hex') FROM public.historical_rows()" > "$cluster/$database.rows"
done
cmp "$cluster/historical_erasure.rows" "$cluster/historical_restore.rows"

create_database historical_fresh
for migration in migrations/*.sql; do
 "${psql[@]}" -U board_migrator -d historical_fresh --single-transaction -f - < "$migration"
done
"${psql[@]}" -U board_migrator -d historical_fresh -c "INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES('fresh','Owned fresh cleanup','Synthetic',2000,100,100,100,10)"
MIGRATION_DATABASE_URL="postgresql://board_migrator@localhost/historical_fresh?host=$cluster" \
 "$binary" preview fresh > "$cluster/fresh.json"
python3 - "$cluster/fresh.json" <<'PY'
import json,sys
value=json.load(open(sys.argv[1]))
assert value["report_ids"]==value["post_ids"]==value["thread_ids"]==[]
PY

# The migration must reject existing drift rather than bless it in the seal.
create_database historical_drift
for migration in migrations/*.sql; do
 [[ $(basename "$migration") < 0141_ ]] || break
 "${psql[@]}" -U board_migrator -d historical_drift --single-transaction -f - < "$migration"
done
"${psql[@]}" -U board_migrator -d historical_drift -c 'ALTER TABLE content.posts DISABLE TRIGGER z_guard_post_erasure'
if "${psql[@]}" -U board_migrator -d historical_drift --single-transaction -f - \
 < migrations/0141_historical_content_erasure.sql > "$cluster/drift.log" 2>&1; then
 echo 'Historical erasure migration accepted existing drift.' >&2; exit 1
fi
grep -q 'Text transfer catalog is not ready before historical erasure' "$cluster/drift.log"
printf 'Historical-erasure fresh/populated upgrade, bounded operator CLI, unchanged independent data, role/drift refusals and restore passed.\n' >&3
