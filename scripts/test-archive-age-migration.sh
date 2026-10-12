#!/usr/bin/env bash
# Owned disposable PostgreSQL: populated upgrade and runtime policy authority.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
pg_bin=${PG_BIN:-/usr/lib/postgresql/16/bin}
runner=()
if [[ $(id -u) = 0 ]]; then runner=(runuser -u postgres --); fi
cluster=$(mktemp -d /tmp/board-archive-age-policy.XXXXXXXX)
started=0
cleanup() {
    status=$?
    trap - EXIT
    if [[ $started = 1 ]]; then
        "${runner[@]}" "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop >/dev/null || status=1
    fi
    [[ $cluster =~ ^/tmp/board-archive-age-policy\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
    [[ $(readlink -f "$cluster") = "$cluster" ]] || exit 1
    rm -rf -- "$cluster"
    exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
if [[ $(id -u) = 0 ]]; then chown postgres:postgres "$cluster"; fi
"${runner[@]}" "$pg_bin/initdb" -D "$cluster/data" --auth=trust --encoding=UTF8 --no-locale >/dev/null
connection=(-h "$cluster")
network="-c listen_addresses='' -c unix_socket_directories='$cluster'"
if [[ -n ${PG_TEST_TCP_PORT:-} ]]; then
    [[ $PG_TEST_TCP_PORT =~ ^[0-9]+$ ]] || exit 1
    connection=(-h 127.0.0.1 -p "$PG_TEST_TCP_PORT")
    network="-c listen_addresses=127.0.0.1 -c unix_socket_directories='' -p $PG_TEST_TCP_PORT"
fi
"${runner[@]}" "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" \
    -o "$network -c statement_timeout=30000 -c lock_timeout=5000" -w start >/dev/null || { cat "$cluster/server.log" >&2; exit 1; }
started=1
psql=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 "${connection[@]}")
"${runner[@]}" "${psql[@]}" -d postgres -f - < deploy/roles.sql
"${runner[@]}" "${psql[@]}" -d postgres <<'SQL'
CREATE DATABASE archive_age_upgrade OWNER board_migrator;
REVOKE ALL ON DATABASE archive_age_upgrade FROM PUBLIC;
GRANT CONNECT ON DATABASE archive_age_upgrade TO board_migrator,board_public;
SQL
migrator=("${psql[@]}" -U board_migrator -d archive_age_upgrade)
for migration in migrations/*.sql; do
    [[ $(basename "$migration") < 0138_ ]] || break
    "${migrator[@]}" --single-transaction -f - < "$migration" >/dev/null
done
"${migrator[@]}" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,archive_retention_seconds)
VALUES ('agefinite','Owned finite','',2000,1000,300,10,10,7200),('ageoff','Owned disabled','',2000,1000,300,10,10,0),('agedeleted','Owned deleted','',2000,1000,300,10,10,7200);
INSERT INTO content.threads(board,archived_at,archive_expires_at)
SELECT slug,statement_timestamp()-interval '1 hour',statement_timestamp()+interval '8 hours'
FROM content.boards WHERE slug IN ('agefinite','ageoff','agedeleted');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
SELECT id,board,id,'Anonymous','Owned archive','Synthetic migration fixture'
FROM content.threads WHERE board IN ('agefinite','ageoff','agedeleted');
UPDATE content.threads SET deleted=true WHERE board='agedeleted';
CREATE TABLE content.archive_age_witness AS SELECT to_jsonb(t) row FROM content.threads t;
CREATE TABLE content.archive_age_view_witness AS SELECT attnum,attname,atttypid
FROM pg_attribute WHERE attrelid='content.visible_threads'::regclass AND attnum>0;
SQL
catalog_before=$("${migrator[@]}" -Atc "SET ROLE board_thread_transfer_owner; SELECT encode(definition_hash,'hex') FROM post_secrets.thread_transfer_catalog WHERE singleton")
"${migrator[@]}" -c "SET ROLE board_thread_transfer_owner; UPDATE post_secrets.thread_transfer_catalog SET definition_hash=decode(repeat('00',32),'hex')"
if "${migrator[@]}" --single-transaction -f - < migrations/0138_archive_age_policy.sql >"$cluster/rejected.out" 2>"$cluster/rejected.err"; then
 echo 'Archive age migration accepted prior catalog drift.' >&2; exit 1
fi
grep -Fq 'Text transfer catalog is not ready for archive age policy.' "$cluster/rejected.err"
"${migrator[@]}" -v old_hash="$catalog_before" <<'SQL'
SET ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=decode(:'old_hash','hex') WHERE singleton;
RESET ROLE;
SQL
snapshot_before=$("${migrator[@]}" -Atc "SET ROLE board_report_snapshot_owner; SELECT encode(definition_hash,'hex') FROM post_secrets.report_snapshot_catalog WHERE singleton")
"${migrator[@]}" -c "SET ROLE board_report_snapshot_owner; UPDATE post_secrets.report_snapshot_catalog SET definition_hash=decode(repeat('00',32),'hex')"
if "${migrator[@]}" --single-transaction -f - < migrations/0138_archive_age_policy.sql >"$cluster/snapshot-rejected.out" 2>"$cluster/snapshot-rejected.err"; then
 echo 'Archive age migration accepted prior report snapshot drift.' >&2; exit 1
fi
grep -Fq 'Report snapshot catalog is not ready before archive age policy.' "$cluster/snapshot-rejected.err"
"${migrator[@]}" -v old_hash="$snapshot_before" <<'SQL'
SET ROLE board_report_snapshot_owner;
UPDATE post_secrets.report_snapshot_catalog SET definition_hash=decode(:'old_hash','hex') WHERE singleton;
RESET ROLE;
SQL
"${migrator[@]}" --single-transaction -f - < migrations/0138_archive_age_policy.sql
"${migrator[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM content.threads WHERE board='agefinite'
           AND archive_expires_at<>archived_at+interval '2 hours')
 THEN RAISE EXCEPTION 'Historical live deadline was not reconciled'; END IF;
 IF EXISTS(SELECT 1 FROM content.threads t JOIN content.archive_age_witness w ON (w.row->>'id')::bigint=t.id
           WHERE (t.board IN ('ageoff','agedeleted') AND to_jsonb(t)<>w.row)
           OR (to_jsonb(t)-ARRAY['archive_expires_at','http_modified_at']<>w.row-ARRAY['archive_expires_at','http_modified_at'])
           OR t.http_modified_at<(w.row->>'http_modified_at')::timestamptz)
 THEN RAISE EXCEPTION 'Upgrade changed disabled storage or unrelated thread fields'; END IF;
 IF EXISTS((SELECT attnum,attname,atttypid FROM content.archive_age_view_witness EXCEPT
 SELECT attnum,attname,atttypid FROM pg_attribute WHERE attrelid='content.visible_threads'::regclass AND attnum>0)
 UNION ALL (SELECT attnum,attname,atttypid FROM pg_attribute WHERE attrelid='content.visible_threads'::regclass AND attnum>0
 EXCEPT SELECT attnum,attname,atttypid FROM content.archive_age_view_witness))
 THEN RAISE EXCEPTION 'Upgrade changed public view columns'; END IF;
END $$;
UPDATE content.boards SET archive_retention_seconds=-1 WHERE slug='agefinite';
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM content.threads WHERE board='agefinite' AND archive_expires_at<>'infinity'::timestamptz)
 THEN RAISE EXCEPTION 'Unlimited age did not update retained archives'; END IF;
END $$;
SET ROLE board_thread_transfer_owner;
DO $$ BEGIN IF NOT content.text_thread_transfer_ready() THEN RAISE EXCEPTION 'Transfer readiness lost'; END IF; END $$;
RESET ROLE;
SET ROLE board_report_snapshot_owner;
DO $$ BEGIN IF NOT content.report_snapshots_ready() THEN RAISE EXCEPTION 'Report snapshot readiness lost'; END IF; END $$;
RESET ROLE;
DROP TABLE content.archive_age_witness,content.archive_age_view_witness;
SQL
"${psql[@]}" -U board_public -d archive_age_upgrade <<'SQL'
DO $$ BEGIN
 IF (SELECT count(*) FROM content.visible_threads WHERE board='agefinite')<>1
 THEN RAISE EXCEPTION 'Unlimited archive is not public'; END IF;
 IF EXISTS(SELECT 1 FROM content.visible_threads WHERE board='ageoff')
 THEN RAISE EXCEPTION 'Disabled archive is public'; END IF;
 BEGIN
  UPDATE content.boards SET archive_retention_seconds=1 WHERE slug='agefinite';
  RAISE EXCEPTION 'Public changed archive policy' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SQL
"${runner[@]}" "$pg_bin/pg_dump" "${connection[@]}" -Fc -d archive_age_upgrade -f "$cluster/archive.dump"
"${runner[@]}" "${psql[@]}" -d postgres -c 'CREATE DATABASE archive_age_restored OWNER board_migrator'
"${runner[@]}" "$pg_bin/pg_restore" "${connection[@]}" --exit-on-error -d archive_age_restored "$cluster/archive.dump"
"${psql[@]}" -U board_migrator -d archive_age_restored <<'SQL'
SET ROLE board_thread_transfer_owner;
DO $$ BEGIN IF NOT content.text_thread_transfer_ready() THEN RAISE EXCEPTION 'Restored transfer readiness lost'; END IF; END $$;
RESET ROLE;
SET ROLE board_report_snapshot_owner;
DO $$ BEGIN IF NOT content.report_snapshots_ready() THEN RAISE EXCEPTION 'Restored report snapshot readiness lost'; END IF; END $$;
RESET ROLE;
UPDATE content.boards SET archive_retention_seconds=10800 WHERE slug='agefinite';
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM content.threads WHERE board='agefinite' AND archive_expires_at<>archived_at+interval '3 hours')
 THEN RAISE EXCEPTION 'Restored policy trigger did not update retained archives'; END IF;
 IF NOT EXISTS(SELECT 1 FROM content.threads WHERE board='agedeleted' AND deleted AND content_erased)
 THEN RAISE EXCEPTION 'Restore lost the erased-content state'; END IF;
END $$;
SQL
printf 'Archive age upgrade and restore preserved disabled/deleted storage, public view columns, sealed readiness and role denials; both catalog drift controls were rejected.\n'
