#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run against the owned disposable development cluster.' >&2; exit 1; }
source .local/database.env
cluster=$(cat .local/cluster-path)
[[ $cluster =~ ^/tmp/board-postgres\.[[:alnum:]]+$ && -d $cluster ]] || exit 1
pg_bin=/usr/lib/postgresql/16/bin
admin=(runuser -u postgres -- "$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h /tmp -p 55432 -d postgres)
actual=$("${admin[@]}" -At -c 'SHOW data_directory')
[[ $actual = "$cluster" ]] || { echo 'Unexpected database cluster.' >&2; exit 1; }
upgrade_db="imageboard_country_classifier_$(date +%s)_$RANDOM"
[[ $upgrade_db =~ ^imageboard_country_classifier_[0-9]+_[0-9]+$ ]] || exit 1
created=0
restore_created=0
restore_db="${upgrade_db}_restore"
dump_file="$cluster/country-classifier-$RANDOM.dump"
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
GRANT CONNECT ON DATABASE :"upgrade_db" TO board_migrator,board_public;
SQL
created=1
upgrade_url="${MIGRATION_DATABASE_URL%/imageboard}/$upgrade_db"
db=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 --dbname="$upgrade_url")
[[ -f migrations/0198_country_classifier_refusal.sql ]] || exit 1
for migration in migrations/*.sql; do
  [[ $migration != migrations/0198_country_classifier_refusal.sql ]] || break
  "${db[@]}" --single-transaction -f "$migration"
done
"${db[@]}" -f fixtures/demo.sql
"${db[@]}" -f scripts/test-country-classifier-predecessor.sql
"${db[@]}" -c BEGIN -f migrations/0198_country_classifier_refusal.sql -c ROLLBACK
"${db[@]}" <<'SQL'
DO $$ BEGIN
 IF to_regprocedure('content.posting_classifier_history(text,bytea,boolean)') IS NOT NULL
 OR to_regprocedure('content.record_country_classifier_refusal(text,bytea,bytea,bytea,bytea,boolean,bigint)') IS NOT NULL
 OR (SELECT catalog_hash FROM public.owned_classifier_before) IS DISTINCT FROM
    (SELECT definition_hash FROM post_secrets.classifier_history_catalog WHERE singleton) THEN
  RAISE EXCEPTION 'Country bridge rollback changed predecessor authority'; END IF;
END $$;
SQL
"${db[@]}" -f scripts/test-classifier-history-upgrade.sql
for mutation in \
 "ALTER TABLE content.post_media DISABLE TRIGGER zz_retain_attachment_digest" \
 "SET ROLE board_anonymous_owner; ALTER FUNCTION post_secrets.read_classifier_history(bytea,boolean) COST 99; RESET ROLE"; do
 if "${db[@]}" --single-transaction -c "$mutation" -f migrations/0198_country_classifier_refusal.sql > "$cluster/country-classifier-drift.log" 2>&1; then
  echo 'Country bridge accepted predecessor drift.' >&2; exit 1
 fi
 grep -Eq 'Transfer authority drifted|Classifier history drifted' "$cluster/country-classifier-drift.log"
done
"${db[@]}" --single-transaction -f migrations/0198_country_classifier_refusal.sql
"${db[@]}" -f scripts/test-country-classifier-upgrade.sql
# Role/RLS changes are scoped to a transaction in this owned test cluster.
for mutation in \
 "ALTER ROLE board_anonymous_owner LOGIN" \
 "GRANT board_anonymous_owner TO board_public WITH INHERIT FALSE, SET TRUE" \
 "CREATE POLICY owned_classifier_drift ON content.boards FOR SELECT TO board_anonymous_owner USING (true)" \
 "SET LOCAL ROLE board_anonymous_owner; ALTER FUNCTION post_secrets.resume_anonymous_refusal(bytea,bytea,bytea,bytea,boolean,bigint) COST 99; RESET ROLE"; do
 "${admin[@]}" -d "$upgrade_db" -c BEGIN -c "$mutation" -c 'SET LOCAL ROLE board_public' -f - -c ROLLBACK <<'SQL'
DO $$ BEGIN
 BEGIN
  PERFORM content.record_country_classifier_refusal(NULL,NULL,NULL,NULL,NULL,false,1);
  RAISE EXCEPTION 'Country issuer ignored unavailable dependency authority';
 EXCEPTION WHEN SQLSTATE 'P198U' THEN NULL; END;
END $$;
SQL
done
runuser -u postgres -- "$pg_bin/pg_dump" -h /tmp -p 55432 -d "$upgrade_db" -Fc > "$dump_file"
"${admin[@]}" -v restore_db="$restore_db" <<'SQL'
CREATE DATABASE :"restore_db" OWNER board_migrator TEMPLATE template0 ENCODING 'UTF8';
SQL
restore_created=1
runuser -u postgres -- "$pg_bin/pg_restore" -h /tmp -p 55432 -d "$restore_db" --exit-on-error < "$dump_file"
"$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 --dbname="${MIGRATION_DATABASE_URL%/imageboard}/$restore_db" -f scripts/test-country-classifier-upgrade.sql
printf 'Country refusal rollback, scoped authority, dependency drift and populated restore passed.\n'
