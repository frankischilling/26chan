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
upgrade_db="imageboard_credit_request_$(date +%s)_$RANDOM"
[[ $upgrade_db =~ ^imageboard_credit_request_[0-9]+_[0-9]+$ ]] || exit 1
created=0
cleanup() {
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
[[ -f migrations/0193_captcha_credit_request.sql ]] || exit 1
for migration in migrations/*.sql; do
  [[ $migration != migrations/0193_captcha_credit_request.sql ]] || break
  "${db[@]}" --single-transaction -f "$migration"
done
"${db[@]}" -f fixtures/demo.sql
"${db[@]}" -f scripts/test-credit-request-predecessor.sql
"${db[@]}" -c BEGIN -f migrations/0193_captcha_credit_request.sql -c ROLLBACK
"${db[@]}" <<'SQL'
DO $$ BEGIN
 IF (SELECT rows FROM public.owned_credit_request_before) IS DISTINCT FROM public.owned_credit_request_rows()
 OR (SELECT source FROM public.owned_credit_request_before) IS DISTINCT FROM
    (SELECT prosrc FROM pg_proc WHERE oid='content.check_public_post_challenge()'::regprocedure) THEN
  RAISE EXCEPTION 'Credit request rollback changed predecessor state'; END IF;
END $$;
SQL
for mutation in \
 "SET ROLE board_anonymous_owner; ALTER FUNCTION content.check_public_post_challenge() SET search_path=pg_catalog,public,pg_temp; RESET ROLE" \
 "SET ROLE board_anonymous_owner; GRANT EXECUTE ON FUNCTION content.check_public_post_challenge() TO PUBLIC; RESET ROLE"; do
 if "${db[@]}" --single-transaction -c "$mutation" -f migrations/0193_captcha_credit_request.sql > "$cluster/credit-request-drift.log" 2>&1; then
  echo 'Credit request migration accepted predecessor drift.' >&2; exit 1
 fi
 grep -Eq 'catalog drifted|consumer drifted|challenge authority is unavailable' "$cluster/credit-request-drift.log"
done
cat > "$cluster/credit-request-body-drift.sql" <<'SQL'
GRANT CREATE ON SCHEMA content TO board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
DO $drift$ DECLARE definition text; BEGIN
 SELECT pg_get_functiondef('content.check_public_post_challenge()'::regprocedure) INTO definition;
 EXECUTE replace(definition,'AS $function$','AS $function$'||chr(10)||'-- owned predecessor drift');
END $drift$;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_anonymous_owner;
SQL
if "${db[@]}" --single-transaction -f "$cluster/credit-request-body-drift.sql" -f migrations/0193_captcha_credit_request.sql > "$cluster/credit-request-body-drift.log" 2>&1; then
 echo 'Credit request migration accepted a changed consumer body.' >&2; exit 1
fi
grep -Eq 'catalog drifted|consumer drifted|challenge authority is unavailable' "$cluster/credit-request-body-drift.log"
"${db[@]}" --single-transaction -f migrations/0193_captcha_credit_request.sql
"${db[@]}" -f scripts/test-credit-request-upgrade.sql
printf 'Credit request upgrade preserves receipts, credits and sessions.\n'
