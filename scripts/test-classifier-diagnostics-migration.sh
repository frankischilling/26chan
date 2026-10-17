#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run against the owned disposable development cluster.' >&2; exit 1; }
source .local/database.env
binary=$(realpath target/debug/board-classifier-retention)
[[ -x $binary ]] || { echo 'Build board-classifier-retention first.' >&2; exit 1; }
cluster=$(cat .local/cluster-path)
[[ $cluster =~ ^/tmp/board-postgres\.[[:alnum:]]+$ && -d $cluster ]] || exit 1
pg_bin=/usr/lib/postgresql/16/bin
admin=(runuser -u postgres -- "$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h /tmp -p 55432 -d postgres)
actual=$("${admin[@]}" -At -c 'SHOW data_directory')
[[ $actual = "$cluster" ]] || { echo 'Unexpected database cluster.' >&2; exit 1; }
upgrade_db="imageboard_classifier_diagnostics_$(date +%s)_$RANDOM"
[[ $upgrade_db =~ ^imageboard_classifier_diagnostics_[0-9]+_[0-9]+$ ]] || exit 1
created=0
restore_created=0
restore_db="${upgrade_db}_restore"
dump_file="$cluster/classifier-diagnostics-$RANDOM.dump"
cleanup() {
  if [[ $restore_created = 1 ]]; then
    "${admin[@]}" -v restore_db="$restore_db" <<'SQL'
DROP DATABASE :"restore_db";
SQL
  fi
  rm -f -- "$dump_file"
  if [[ $created = 1 ]]; then
    "${admin[@]}" -v upgrade_db="$upgrade_db" <<'SQL'
DROP DATABASE :"upgrade_db";
SQL
    created=0
  fi
}
trap cleanup EXIT
"${admin[@]}" -v upgrade_db="$upgrade_db" <<'SQL'
CREATE DATABASE :"upgrade_db" OWNER board_migrator TEMPLATE template0 ENCODING 'UTF8';
REVOKE ALL ON DATABASE :"upgrade_db" FROM PUBLIC;
GRANT CONNECT ON DATABASE :"upgrade_db" TO board_migrator,board_public,board_moderation_retention;
SQL
created=1
"${admin[@]}" -c 'ALTER ROLE board_moderation_retention LOGIN'
upgrade_url="${MIGRATION_DATABASE_URL%/imageboard}/$upgrade_db"
db=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 --dbname="$upgrade_url")
[[ -f migrations/0199_classifier_diagnostics.sql ]] || exit 1
for migration in migrations/*.sql; do
  [[ $migration != migrations/0199_classifier_diagnostics.sql ]] || break
  "${db[@]}" --single-transaction -f "$migration"
done
"${db[@]}" -f fixtures/demo.sql
"${db[@]}" -f scripts/test-classifier-diagnostics-predecessor.sql
"${db[@]}" -c BEGIN -f migrations/0199_classifier_diagnostics.sql -c ROLLBACK
"${db[@]}" <<'SQL'
DO $$ BEGIN
 IF to_regclass('post_secrets.classifier_events') IS NOT NULL
 OR to_regprocedure('content.record_posting_classifier_events(text,bigint,text[],text,double precision)') IS NOT NULL
 OR (SELECT catalog_hash FROM public.owned_classifier_before) IS DISTINCT FROM
    (SELECT definition_hash FROM post_secrets.classifier_history_catalog WHERE singleton) THEN
  RAISE EXCEPTION 'Diagnostic ledger rollback changed predecessor authority'; END IF;
END $$;
SQL
"${db[@]}" -f scripts/test-classifier-history-upgrade.sql
for mutation in \
 "ALTER TABLE content.post_media DISABLE TRIGGER zz_retain_attachment_digest" \
 "SET ROLE board_anonymous_owner; ALTER FUNCTION post_secrets.read_classifier_history(bytea,boolean) COST 99; RESET ROLE"; do
 if "${db[@]}" --single-transaction -c "$mutation" -f migrations/0199_classifier_diagnostics.sql > "$cluster/classifier-diagnostics-drift.log" 2>&1; then
  echo 'Diagnostic ledger accepted predecessor drift.' >&2; exit 1
 fi
 grep -Eq 'Transfer authority drifted|Classifier history drifted' "$cluster/classifier-diagnostics-drift.log"
done
"${db[@]}" --single-transaction -f migrations/0199_classifier_diagnostics.sql
"${db[@]}" <<'SQL'
INSERT INTO post_secrets.classifier_events(created_at,kind,board,thread_id,country,threat_score)
 VALUES(floor(extract(epoch FROM clock_timestamp()))::bigint,'country_changed','owned',0,'US',NULL),
 (floor(extract(epoch FROM clock_timestamp()))::bigint,'block_country_changed','owned',0,NULL,NULL);
CREATE TABLE public.owned_diagnostics_before AS
 SELECT jsonb_agg(to_jsonb(e) ORDER BY id) AS rows FROM post_secrets.classifier_events e;
SQL
"${db[@]}" -f scripts/test-classifier-diagnostics-upgrade.sql
retention_url="${upgrade_url/board_migrator/board_moderation_retention}"
"${db[@]}" <<'SQL'
INSERT INTO post_secrets.classifier_events(created_at,kind,board,thread_id)
 SELECT floor(extract(epoch FROM clock_timestamp()))::bigint-691200,'log_mask_changed','owned',0
 FROM generate_series(1,1005);
SQL
output=$(MODERATION_RETENTION_DATABASE_URL="$retention_url" "$binary" run)
[[ $output = '{"expired_classifier_events":1005,"batch_limit_reached":false}' ]]
"${db[@]}" -f scripts/test-classifier-diagnostics-upgrade.sql
if "$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 --dbname="$retention_url" -c 'SELECT * FROM post_secrets.classifier_events' > "$cluster/classifier-private.log" 2>&1; then
 echo 'Retention login could read diagnostic payloads.' >&2; exit 1
fi
grep -q 'permission denied' "$cluster/classifier-private.log"
# These mutations are scoped to transactions in this owned test cluster.
for mutation in \
 "GRANT SELECT ON post_secrets.classifier_events TO board_staff" \
 "GRANT USAGE ON SEQUENCE post_secrets.classifier_events_id_seq TO board_staff" \
 "SET LOCAL ROLE board_anonymous_owner; ALTER FUNCTION post_secrets.prune_classifier_events() COST 99; RESET ROLE"; do
 "${db[@]}" -c BEGIN -c "$mutation" -f - -c ROLLBACK <<'SQL'
DO $$ BEGIN
 BEGIN
  PERFORM content.expire_classifier_events();
  RAISE EXCEPTION 'Diagnostic retention ignored unavailable authority';
 EXCEPTION WHEN SQLSTATE 'P199U' THEN NULL; END;
END $$;
SQL
done
for mutation in \
 "ALTER ROLE board_moderation_retention BYPASSRLS" \
 "GRANT board_migrator TO board_moderation_retention WITH INHERIT FALSE, SET TRUE"; do
 "${admin[@]}" -d "$upgrade_db" -c BEGIN -c "$mutation" -c 'SET LOCAL ROLE board_anonymous_owner' -f - -c ROLLBACK <<'SQL'
DO $$ BEGIN IF post_secrets.classifier_retention_ready() IS DISTINCT FROM false THEN
 RAISE EXCEPTION 'Classifier retention ignored worker role drift'; END IF; END $$;
SQL
done
runuser -u postgres -- "$pg_bin/pg_dump" -h /tmp -p 55432 -d "$upgrade_db" -Fc > "$dump_file"
"${admin[@]}" -v restore_db="$restore_db" <<'SQL'
CREATE DATABASE :"restore_db" OWNER board_migrator TEMPLATE template0 ENCODING 'UTF8';
SQL
restore_created=1
runuser -u postgres -- "$pg_bin/pg_restore" -h /tmp -p 55432 -d "$restore_db" --exit-on-error < "$dump_file"
"$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 --dbname="${MIGRATION_DATABASE_URL%/imageboard}/$restore_db" -f scripts/test-classifier-diagnostics-upgrade.sql
printf 'Classifier diagnostics rollback, scoped authority, dependency drift and populated restore passed.\n'
