#!/usr/bin/env bash
# Upgrade an owned 0248 database, reject drift, and preserve prior data/authority.
set -euo pipefail
cd "$(dirname "$0")/.."
pg_bin=${PG_BIN:-/usr/lib/postgresql/16/bin}
port=${SWF_UPGRADE_TEST_PORT:-15492}
[[ $port =~ ^[0-9]+$ && $port -ge 1024 && $port -le 65535 ]] || exit 2
cluster=$(mktemp -d /tmp/board-swf-upgrade.XXXXXXXX)
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
  [[ $cluster =~ ^/tmp/board-swf-upgrade\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
  [[ $(readlink -f "$cluster") = "$cluster" ]] || exit 1
  rm -rf -- "$cluster"
}
trap cleanup EXIT
"${as_pg[@]}" "$pg_bin/initdb" -D "$cluster/data" --auth=trust --encoding=UTF8 --no-locale >"$cluster/init.log"
"${as_pg[@]}" "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" -o "-h 127.0.0.1 -p $port -k ''" -w start >/dev/null
started=1
admin=("$pg_bin/psql" -X -v ON_ERROR_STOP=1 -h 127.0.0.1 -p "$port" -U "$pg_user" -d postgres)
"${admin[@]}" -f deploy/roles.sql >"$cluster/roles.log"
"${admin[@]}" -c 'CREATE DATABASE owned_swf_upgrade OWNER board_migrator;' \
  -c 'REVOKE ALL ON DATABASE owned_swf_upgrade FROM PUBLIC;' \
  -c 'GRANT CONNECT ON DATABASE owned_swf_upgrade TO board_migrator,board_public;' >/dev/null
migrator=("$pg_bin/psql" -X -v ON_ERROR_STOP=1 -h 127.0.0.1 -p "$port" -U board_migrator -d owned_swf_upgrade)
for file in migrations/[0-9][0-9][0-9][0-9]_*.sql; do
  [[ $file < migrations/0249_ ]] || break
  "${migrator[@]}" --single-transaction -f "$file" >>"$cluster/upgrade.log" 2>&1
done
"${migrator[@]}" >"$cluster/seed.log" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
 VALUES('ownjson','Owned JSON upgrade','Historical policy',1000,100,50,10,10);
INSERT INTO content.threads(id,board) VALUES(8891001,'ownjson');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
 VALUES(8891001,'ownjson',8891001,'Owned','Historical subject','Historical comment');
INSERT INTO post_secrets.poster_contexts(post_id,thread_id,fingerprint,epoch)
 VALUES(8891001,8891001,decode(repeat('11',32),'hex'),decode(repeat('22',32),'hex'));
CREATE TABLE public.owned_json_before AS SELECT
 (SELECT jsonb_agg(to_jsonb(b) ORDER BY slug) FROM content.boards b) AS boards,
 (SELECT jsonb_agg(to_jsonb(p) ORDER BY id) FROM content.posts p WHERE board='ownjson') AS posts,
 (SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM content.threads t WHERE board='ownjson') AS threads,
 (SELECT jsonb_agg(to_jsonb(c) ORDER BY post_id) FROM post_secrets.poster_contexts c WHERE thread_id=8891001) AS contexts;
CREATE TABLE public.owned_json_acl_before AS SELECT oid,relowner,relacl::text FROM pg_class WHERE relnamespace IN('content'::regnamespace,'media'::regnamespace);
SQL
for migration in migrations/0249_swf_asset_manifest.sql migrations/0250_upload_board_admission.sql; do
  for drift in "ALTER TABLE content.boards ALTER COLUMN title SET DEFAULT 'owned drift';" \
               "REVOKE SELECT ON content.posts FROM board_public;"; do
    printf '%s\n' "$drift" >"$cluster/drift.sql"
    if "${migrator[@]}" --single-transaction -f "$cluster/drift.sql" -f "$migration" >"$cluster/rejected.log" 2>&1; then
      echo 'SWF upgrade accepted pre-existing authority drift' >&2; exit 1
    fi
    grep -q 'Transfer authority drifted before' "$cluster/rejected.log"
  done
  "${migrator[@]}" --single-transaction -f "$migration" >>"$cluster/accepted.log"
done
cat >"$cluster/check.sql" <<'SQL'
DO $$
DECLARE before_row record;
BEGIN
 SELECT * INTO before_row FROM public.owned_json_before;
 IF before_row.boards IS DISTINCT FROM (SELECT jsonb_agg(to_jsonb(b) ORDER BY slug) FROM content.boards b)
 OR before_row.posts IS DISTINCT FROM (SELECT jsonb_agg(to_jsonb(p) ORDER BY id) FROM content.posts p WHERE board='ownjson')
 OR before_row.threads IS DISTINCT FROM (SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM content.threads t WHERE board='ownjson')
 OR before_row.contexts IS DISTINCT FROM (SELECT jsonb_agg(to_jsonb(c) ORDER BY post_id) FROM post_secrets.poster_contexts c WHERE thread_id=8891001) THEN
  RAISE EXCEPTION 'SWF upgrade changed historical data'; END IF;
 IF has_table_privilege('board_public','content.upload_board_names','SELECT')
 OR has_table_privilege('board_migrator','content.upload_board_names','SELECT')
 OR has_function_privilege('board_public','content.sync_upload_board_name(bigint,boolean)','EXECUTE')
 OR has_function_privilege('board_migrator','content.require_upload_board_file()','EXECUTE') THEN
  RAISE EXCEPTION 'SWF filename authority escaped its owner'; END IF;
END $$;
SET ROLE board_thread_transfer_owner;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'SWF upgrade left content.text_thread_transfer_ready() unavailable'; END IF; END $$;
RESET ROLE;
SET ROLE board_report_snapshot_owner;
DO $$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'SWF upgrade left content.report_snapshots_ready() unavailable'; END IF; END $$;
RESET ROLE;
SET ROLE board_restriction_owner;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'SWF upgrade left content.restrictions_ready() unavailable'; END IF; END $$;
RESET ROLE;
SET ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'SWF upgrade left content.public_pass_ready() unavailable'; END IF; END $$;
RESET ROLE;
SET ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_vip_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'SWF upgrade left content.public_vip_ready() unavailable'; END IF; END $$;
RESET ROLE;
SET ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_challenge_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'SWF upgrade left content.public_challenge_ready() unavailable'; END IF; END $$;
RESET ROLE;
SET ROLE board_anonymous_owner;
DO $$ BEGIN IF post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'SWF upgrade left post_secrets.classifier_history_ready() unavailable'; END IF; END $$;
RESET ROLE;
SQL
"${migrator[@]}" -f "$cluster/check.sql"
"${migrator[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM public.owned_json_acl_before b JOIN pg_class c ON c.oid=b.oid
  WHERE b.relowner<>c.relowner OR b.relacl IS DISTINCT FROM c.relacl::text) THEN
  RAISE EXCEPTION 'SWF upgrade changed existing table ownership or grants'; END IF;
END $$;
SQL
# Restore with the same administrator procedure used by the backup exercise.
"$pg_bin/pg_dump" -h 127.0.0.1 -p "$port" -U "$pg_user" -d owned_swf_upgrade -Fc -f "$cluster/owned.dump"
"${admin[@]}" -c 'CREATE DATABASE owned_swf_restored OWNER board_migrator;' >/dev/null
"$pg_bin/pg_restore" -h 127.0.0.1 -p "$port" -U "$pg_user" -d owned_swf_restored --exit-on-error "$cluster/owned.dump"
"$pg_bin/psql" -X -v ON_ERROR_STOP=1 -h 127.0.0.1 -p "$port" -U board_migrator -d owned_swf_restored -f "$cluster/check.sql"
echo 'SWF upgrade, drift rollback, historical preservation, private authority and administrator restore passed.'
