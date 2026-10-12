#!/usr/bin/env bash
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
tree=$PWD
transfer=$tree
image=$tree
if [[ ${REPORT_SNAPSHOT_MIGRATION_ORDER:-} = '' ]]; then
 REPORT_SNAPSHOT_MIGRATION_ORDER=absent bash "$0"
 if [[ -f migrations/0131_text_thread_transfer.sql && -f migrations/0132_source_image_board_policy.sql ]]; then
  REPORT_SNAPSHOT_MIGRATION_ORDER=before bash "$0"
  REPORT_SNAPSHOT_MIGRATION_ORDER=after bash "$0"
 fi
 exit
fi
order=${REPORT_SNAPSHOT_MIGRATION_ORDER:-absent}
[[ $order = absent || $order = before || $order = after ]] || exit 1
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-report-snapshot.XXXXXXXX)
started=0
cleanup() {
 result=$?
 trap - EXIT
 [[ $cluster =~ ^/tmp/board-report-snapshot\.[[:alnum:]]{8}$ && ! -L $cluster && $(readlink -f "$cluster") = "$cluster" ]] || exit 1
 if [[ $started = 1 ]]; then
  runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop >/dev/null || exit 1
  if runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" status >/dev/null 2>&1; then exit 1; else [[ $? = 3 ]] || exit 1; fi
 fi
 rm -rf -- "$cluster"
 exit "$result"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
chown postgres:postgres "$cluster"
runuser -u postgres -- "$pg_bin/initdb" -D "$cluster/data" --auth=trust --encoding=UTF8 --no-locale >/dev/null
started=1
runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" -o "-c listen_addresses='' -c unix_socket_directories='$cluster'" -w start >/dev/null
db=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h "$cluster")
runuser -u postgres -- "${db[@]}" -d postgres -f - < "$tree/deploy/roles.sql"
runuser -u postgres -- "${db[@]}" -d postgres -c 'CREATE DATABASE report_snapshot OWNER board_migrator;'
runuser -u postgres -- "${db[@]}" -d postgres -c 'GRANT CONNECT ON DATABASE report_snapshot TO board_public,board_staff; ALTER ROLE board_staff LOGIN;'
migrator=("${db[@]}" -U board_migrator -d report_snapshot)
admin=(runuser -u postgres -- "${db[@]}" -d report_snapshot)
for migration in "$tree"/migrations/*.sql; do
 # Freeze this historical upgrade at its target. Later migrations can depend
 # on 0133 and must not run while this fixture deliberately withholds it.
 [[ $(basename "$migration") < 0133_ ]] || break
 if [[ $order = absent && $(basename "$migration") = 0131_* ]]; then continue; fi
 if [[ $order != absent && ( $(basename "$migration") = 0131_* || $(basename "$migration") = 0132_* ) ]]; then continue; fi
 "${migrator[@]}" --single-transaction -f - < "$migration" >/dev/null 2>"$cluster/migration.log" || { tail -n 7 "$cluster/migration.log"; exit 1; }
done
"${migrator[@]}" -f - < "$tree/fixtures/demo.sql" >/dev/null
if [[ $order = before ]]; then
 "${migrator[@]}" --single-transaction -f - < "$transfer/migrations/0131_text_thread_transfer.sql" >/dev/null
 "${migrator[@]}" --single-transaction -f - < "$image/migrations/0132_source_image_board_policy.sql" >/dev/null
 "${admin[@]}" -c "ALTER TABLE content.boards DISABLE ROW LEVEL SECURITY;" >/dev/null
 if "${migrator[@]}" --single-transaction -f - < "$tree/migrations/0133_report_post_snapshots.sql" > "$cluster/preflight.log" 2>&1; then
  echo 'Report capture accepted prior transfer drift'; exit 1
 fi
 grep -q 'Text transfer catalog is not ready before report capture' "$cluster/preflight.log" || { tail -n 5 "$cluster/preflight.log"; exit 1; }
 "${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF to_regclass('post_secrets.report_post_snapshots') IS NOT NULL
 OR EXISTS(SELECT 1 FROM pg_policy WHERE polname='report_snapshot_board_read')
 THEN RAISE EXCEPTION 'Failed capture migration leaked schema or board privileges'; END IF;
END $$;
ALTER TABLE content.boards ENABLE ROW LEVEL SECURITY;
SQL
fi
"${migrator[@]}" <<'SQL'
BEGIN;
SELECT slug FROM content.boards WHERE slug='demo' FOR UPDATE;
INSERT INTO content.reports(board,post_id,reason) VALUES('demo',1000001,'Legacy report');
COMMIT;
SQL
"${migrator[@]}" --single-transaction -f - < "$tree/migrations/0133_report_post_snapshots.sql" >/dev/null
if [[ $order = after ]]; then
 "${migrator[@]}" --single-transaction -f - < "$transfer/migrations/0131_text_thread_transfer.sql" >/dev/null
 "${migrator[@]}" --single-transaction -f - < "$image/migrations/0132_source_image_board_policy.sql" >/dev/null
fi
if [[ $order != absent ]]; then
 "${db[@]}" -U board_staff -d report_snapshot -c "DO \$\$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Combined capture/transfer/image readiness failed'; END IF; END \$\$;" >/dev/null
fi
"${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(TABLE post_secrets.report_post_snapshots)
 OR (SELECT used_bytes FROM post_secrets.report_snapshot_capacity)<>0
 OR (SELECT count(*) FROM content.reports)<>1
 THEN RAISE EXCEPTION 'Legacy report was backfilled or modified'; END IF;
END $$;
SQL
"${db[@]}" -U board_public -d report_snapshot -c "SELECT content.admit_report('demo',1000001,'Owned capture',decode(repeat('11',32),'hex'),decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),decode(repeat('44',32),'hex'),decode(repeat('55',32),'hex'),true,extract(epoch FROM clock_timestamp())::bigint);" >/dev/null
"${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF (SELECT count(*) FROM post_secrets.report_post_snapshots)<>1
 OR EXISTS(SELECT 1 FROM post_secrets.report_post_snapshots WHERE payload ?| ARRAY['host','pwd','ip','token_hash','password_hash','actor_hash','reason'])
 OR (SELECT used_bytes FROM post_secrets.report_snapshot_capacity)<>(
     SELECT sum(charged_bytes) FROM post_secrets.report_post_snapshots)
 THEN RAISE EXCEPTION 'Capture count, privacy or charge failed'; END IF;
END $$;
CREATE TABLE public.saved_snapshot AS TABLE post_secrets.report_post_snapshots;
CREATE TABLE public.saved_admission AS
 SELECT 'reports' AS source,to_jsonb(r) AS row FROM content.reports r
 UNION ALL SELECT 'membership',to_jsonb(r) FROM post_secrets.report_membership r
 UNION ALL SELECT 'groups',to_jsonb(r) FROM post_secrets.report_group r
 UNION ALL SELECT 'anonymous',to_jsonb(r) FROM post_secrets.anonymous_reports r
 UNION ALL SELECT 'sessions',to_jsonb(r) FROM post_secrets.anonymous_sessions r;
CREATE VIEW public.current_admission AS
 SELECT 'reports' AS source,to_jsonb(r) AS row FROM content.reports r
 UNION ALL SELECT 'membership',to_jsonb(r) FROM post_secrets.report_membership r
 UNION ALL SELECT 'groups',to_jsonb(r) FROM post_secrets.report_group r
 UNION ALL SELECT 'anonymous',to_jsonb(r) FROM post_secrets.anonymous_reports r
 UNION ALL SELECT 'sessions',to_jsonb(r) FROM post_secrets.anonymous_sessions r;
UPDATE post_secrets.report_snapshot_capacity SET byte_limit=used_bytes;
SQL
expect_rejected() {
 expected=$1
 if "${db[@]}" -U board_public -d report_snapshot -v VERBOSITY=verbose -c \
  "SELECT content.admit_report('demo',1000001,'Rejected capture',decode(repeat('61',32),'hex'),decode(repeat('62',32),'hex'),decode(repeat('63',32),'hex'),decode(repeat('64',32),'hex'),decode(repeat('65',32),'hex'),true,extract(epoch FROM clock_timestamp())::bigint);" > "$cluster/rejection.log" 2>&1; then
  echo "Expected $expected refusal succeeded."; exit 1
 fi
 grep -Eq "ERROR:  $expected:" "$cluster/rejection.log" || { tail -n 7 "$cluster/rejection.log"; exit 1; }
 "${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(TABLE public.saved_admission EXCEPT ALL TABLE public.current_admission)
 OR EXISTS(TABLE public.current_admission EXCEPT ALL TABLE public.saved_admission)
 OR EXISTS(TABLE public.saved_snapshot EXCEPT ALL TABLE post_secrets.report_post_snapshots)
 OR EXISTS(TABLE post_secrets.report_post_snapshots EXCEPT ALL TABLE public.saved_snapshot)
 OR (SELECT used_bytes FROM post_secrets.report_snapshot_capacity)<>(
     SELECT sum(charged_bytes) FROM post_secrets.report_post_snapshots)
 THEN RAISE EXCEPTION 'Denied admission changed reports, membership, activity or captures'; END IF;
END $$;
SQL
}
expect_rejected P0133
"${admin[@]}" -c 'UPDATE post_secrets.report_snapshot_capacity SET byte_limit=67108864; ALTER TABLE content.reports DISABLE TRIGGER capture_report_post;' >/dev/null
expect_rejected 42501
"${admin[@]}" -c 'ALTER TABLE content.reports ENABLE TRIGGER capture_report_post; GRANT SELECT ON post_secrets.report_post_snapshots TO board_staff;' >/dev/null
expect_rejected 42501
"${admin[@]}" -c 'REVOKE SELECT ON post_secrets.report_post_snapshots FROM board_staff;' >/dev/null
"${admin[@]}" -c 'GRANT board_public TO board_report_snapshot_owner WITH INHERIT FALSE, SET TRUE;' >/dev/null
expect_rejected 42501
"${admin[@]}" -c 'REVOKE board_public FROM board_report_snapshot_owner;' >/dev/null
"${db[@]}" -U board_staff -d report_snapshot <<'SQL'
DO $$ BEGIN
 IF content.report_snapshots_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Capture readiness did not recover'; END IF;
 IF NOT has_table_privilege(current_user,'content.report_post_snapshots','SELECT')
 OR has_table_privilege(current_user,'content.report_post_snapshots','INSERT')
 OR EXISTS(SELECT 1 FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
     WHERE n.nspname='post_secrets' AND p.proname='report_snapshot_catalog_hash'
       AND has_function_privilege(current_user,p.oid,'EXECUTE'))
 OR pg_has_role(current_user,'board_report_snapshot_owner','MEMBER')
 THEN RAISE EXCEPTION 'Staff capture boundary failed'; END IF;
END $$;
SQL
"${admin[@]}" <<'SQL'
UPDATE content.posts SET comment='Changed after admission',name='Changed name' WHERE id=1000001;
DO $$ BEGIN
 IF EXISTS(TABLE public.saved_snapshot EXCEPT ALL TABLE post_secrets.report_post_snapshots)
 OR EXISTS(TABLE post_secrets.report_post_snapshots EXCEPT ALL TABLE public.saved_snapshot)
 THEN RAISE EXCEPTION 'Captured post changed with live data'; END IF;
END $$;
SQL
"${db[@]}" -U board_public -d report_snapshot -c "SELECT content.delete_post_attachment('demo',1000001);" >/dev/null
"$pg_bin/pg_dump" -h "$cluster" -U postgres -Fc report_snapshot > "$cluster/capture.dump"
"${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(TABLE public.saved_snapshot EXCEPT ALL TABLE post_secrets.report_post_snapshots)
 THEN RAISE EXCEPTION 'Public file action removed snapshot'; END IF;
END $$;
DELETE FROM content.reports;
DO $$ BEGIN
 IF EXISTS(TABLE post_secrets.report_post_snapshots)
 OR (SELECT used_bytes FROM post_secrets.report_snapshot_capacity)<>0
 THEN RAISE EXCEPTION 'Snapshot cascade or charge release failed'; END IF;
END $$;
SQL
runuser -u postgres -- "${db[@]}" -d postgres -c 'CREATE DATABASE report_snapshot_restored OWNER board_migrator;'
runuser -u postgres -- "${db[@]}" -d postgres -c 'GRANT CONNECT ON DATABASE report_snapshot_restored TO board_public,board_staff;'
runuser -u postgres -- "$pg_bin/pg_restore" -h "$cluster" -d report_snapshot_restored < "$cluster/capture.dump"
original_oid=$("${admin[@]}" -At -c "SELECT 'post_secrets.report_post_snapshots'::regclass::oid;")
restored_oid=$(runuser -u postgres -- "${db[@]}" -d report_snapshot_restored -At -c "SELECT 'post_secrets.report_post_snapshots'::regclass::oid;")
[[ $original_oid != "$restored_oid" ]] || { echo 'Restore did not change snapshot relation OID'; exit 1; }
"${db[@]}" -U board_staff -d report_snapshot_restored -c "DO \$\$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restored capture readiness failed'; END IF; END \$\$;"
runuser -u postgres -- "${db[@]}" -d report_snapshot_restored <<'SQL'
DO $$ BEGIN
 IF EXISTS(TABLE public.saved_snapshot EXCEPT ALL TABLE post_secrets.report_post_snapshots)
 OR EXISTS(TABLE post_secrets.report_post_snapshots EXCEPT ALL TABLE public.saved_snapshot)
 OR (SELECT count(*) FROM post_secrets.report_post_snapshots)<>1
 THEN RAISE EXCEPTION 'Administrator restore changed retained capture'; END IF;
END $$;
SQL
"${db[@]}" -U board_public -d report_snapshot_restored -c "SELECT content.admit_report('demo',1000001,'Restored capture',decode(repeat('71',32),'hex'),decode(repeat('72',32),'hex'),decode(repeat('73',32),'hex'),decode(repeat('74',32),'hex'),decode(repeat('75',32),'hex'),true,extract(epoch FROM clock_timestamp())::bigint);" >/dev/null
runuser -u postgres -- "${db[@]}" -d report_snapshot_restored <<'SQL'
DO $$ BEGIN
 IF (SELECT count(*) FROM post_secrets.report_post_snapshots)<>2
 OR (SELECT used_bytes FROM post_secrets.report_snapshot_capacity)<>(
     SELECT sum(charged_bytes) FROM post_secrets.report_post_snapshots)
 THEN RAISE EXCEPTION 'Restored admission capture or accounting failed'; END IF;
END $$;
SQL
echo "Legacy preservation, capture privacy, storage refusal atomicity, trigger/ACL drift rejection, live-change stability, public retention, private cascade accounting and changed-object-OID restore passed (transfer order: $order)."
