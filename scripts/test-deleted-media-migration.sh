#!/usr/bin/env bash
# Fresh/populated/restore qualification on an owned disposable PostgreSQL host.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
binary=$(realpath target/debug/board-deletion-cleanup)
[[ -x $binary ]] || { echo 'Build board-deletion-cleanup first.' >&2; exit 1; }
cluster=$(mktemp -d /tmp/board-deletion-cleanup.XXXXXXXX)
started=0
cleanup() {
 if [[ $started = 1 ]]; then runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop >/dev/null; fi
 [[ $cluster =~ ^/tmp/board-deletion-cleanup\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
 [[ $(readlink -f "$cluster") = "$cluster" ]] || exit 1
 rm -rf -- "$cluster"
}
trap cleanup EXIT
chown postgres:postgres "$cluster"
runuser -u postgres -- "$pg_bin/initdb" -D "$cluster/data" --auth=trust --encoding=UTF8 --no-locale >/dev/null
runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" \
 -o "-c listen_addresses='' -c unix_socket_directories='$cluster' -c statement_timeout=30000 -c lock_timeout=5000" -w start >/dev/null
started=1
psql=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h "$cluster")
exec 3>&1
exec > "$cluster/qualification.log" 2>&1
trap 'printf "Deleted media qualification failed at line %s.\n" "$LINENO" >&3' ERR
runuser -u postgres -- "${psql[@]}" -d postgres -f deploy/roles.sql
runuser -u postgres -- "${psql[@]}" -d postgres -c 'ALTER ROLE board_moderation_retention LOGIN; ALTER ROLE board_staff LOGIN; ALTER ROLE board_auth LOGIN;'
create_database() {
 runuser -u postgres -- "${psql[@]}" -d postgres -v database="$1" <<'SQL'
CREATE DATABASE :"database" OWNER board_migrator;
REVOKE ALL ON DATABASE :"database" FROM PUBLIC;
GRANT CONNECT ON DATABASE :"database" TO board_migrator,board_public,board_staff,board_auth,board_moderation_retention;
SQL
}
create_database media_erasure
migrator=("${psql[@]}" -U board_migrator -d media_erasure)
for migration in migrations/*.sql; do
 [[ $migration != migrations/0143* ]] || break
 "${migrator[@]}" --single-transaction -f "$migration"
done
"${migrator[@]}" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
 VALUES('merase','Owned media erasure','Synthetic',1000,100,100,100,10);
BEGIN;
SELECT set_config('board.posting_actor',repeat('42',32),true);
INSERT INTO content.threads(id,board) VALUES(42000001,'merase');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
 SELECT n,'merase',42000001,'Owned name','','Owned body' FROM generate_series(42000001,42000071) n;
INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler)
 SELECT n,md5(n::text||'job'),md5(n::text||'asset'),'owned-private.png',4321,120,80,false FROM generate_series(42000001,42000071) n;
SELECT slug FROM content.boards WHERE slug='merase' FOR UPDATE;
UPDATE content.posts SET deleted=true WHERE id>42000001;
COMMIT;
SQL
"${migrator[@]}" --single-transaction -f migrations/0143_deleted_media_metadata.sql
[[ $("${migrator[@]}" -Atc "SELECT count(*) FROM content.post_media WHERE filename='owned-private.png' AND NOT metadata_erased") = 71 ]]
export MIGRATION_DATABASE_URL="postgresql://board_migrator@localhost/media_erasure?host=$cluster"
"$binary" preview-media merase > "$cluster/preview.json"
receipt=$(python3 - "$cluster/preview.json" <<'PY'
import json,sys
p=json.load(open(sys.argv[1]));assert len(p['post_ids'])==64 and not p['applied'];print(p['receipt'])
PY
)
# Change a selected row after preview. No old receipt may erase a new state.
"${migrator[@]}" -c "UPDATE content.post_media SET metadata_erased=true WHERE post_id=42000002"
if "$binary" apply-media merase --confirm-permanent-erasure "$receipt" --writers-stopped; then exit 1; fi
[[ $("${migrator[@]}" -Atc 'SELECT count(*) FROM content.post_media WHERE metadata_erased') = 1 ]]
for expected in 64 5 0; do
 "$binary" preview-media merase > "$cluster/preview.json"
 receipt=$(python3 - "$cluster/preview.json" "$expected" <<'PY'
import json,sys
p=json.load(open(sys.argv[1]));assert len(p['post_ids'])==int(sys.argv[2]);print(p['receipt'])
PY
 )
 "$binary" apply-media merase --confirm-permanent-erasure "$receipt" --writers-stopped > "$cluster/applied.json"
done
[[ $("${migrator[@]}" -Atc "SELECT count(*) FROM content.post_media WHERE metadata_erased AND filename='[deleted]' AND bytes=1 AND width=1 AND height=1 AND file_deleted AND NOT spoiler AND created_at='epoch'") = 70 ]]
[[ $("${migrator[@]}" -Atc "SELECT count(*) FROM content.post_media WHERE job_id=md5(post_id::text||'job') AND asset_id=md5(post_id::text||'asset')") = 71 ]]
[[ $("${migrator[@]}" -Atc "SELECT filename='owned-private.png' AND bytes=4321 AND NOT metadata_erased FROM content.post_media WHERE post_id=42000001") = t ]]
for role in board_public board_staff board_auth; do
 if MIGRATION_DATABASE_URL="postgresql://$role@localhost/media_erasure?host=$cluster" "$binary" preview-media merase; then exit 1; fi
done
"${migrator[@]}" -c 'ALTER TABLE content.post_media DISABLE TRIGGER z_guard_media_erasure'
if "$binary" preview-media merase; then exit 1; fi
"${migrator[@]}" -c 'ALTER TABLE content.post_media ENABLE TRIGGER z_guard_media_erasure'
runuser -u postgres -- "$pg_bin/pg_dump" -h "$cluster" -Fc media_erasure > "$cluster/restore.dump"
create_database media_restore
runuser -u postgres -- "$pg_bin/pg_restore" -h "$cluster" --exit-on-error -d media_restore < "$cluster/restore.dump"
MIGRATION_DATABASE_URL="postgresql://board_migrator@localhost/media_restore?host=$cluster" "$binary" preview-media merase > "$cluster/restored.json"
python3 - "$cluster/restored.json" <<'PY'
import json,sys
assert json.load(open(sys.argv[1]))['post_ids']==[]
PY
printf 'Qualified deleted-media upgrade, bounded receipts, role refusals, drift and restore.\n' >&3
