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
upgrade_db="imageboard_attachment_filter_$(date +%s)_$RANDOM"
[[ $upgrade_db =~ ^imageboard_attachment_filter_[0-9]+_[0-9]+$ ]] || exit 1
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
[[ -f migrations/0183_attachment_filter_preflight.sql ]] || exit 1
for migration in migrations/*.sql; do
  [[ $migration != migrations/0183_attachment_filter_preflight.sql ]] || break
  "${db[@]}" --single-transaction -f "$migration"
done
"${db[@]}" -f fixtures/demo.sql
"${db[@]}" -f scripts/test-input-receipts-predecessor.sql
"${db[@]}" -c BEGIN -f migrations/0183_attachment_filter_preflight.sql -c ROLLBACK
"${db[@]}" -c "DO \$\$ BEGIN IF to_regprocedure('content.attachment_upload_filter_metadata(text,text,text,bigint,boolean,boolean)') IS NOT NULL THEN RAISE EXCEPTION 'Attachment preflight rollback left a function.'; END IF; END \$\$"
"${db[@]}" --single-transaction -f migrations/0183_attachment_filter_preflight.sql
"${db[@]}" -f scripts/test-input-receipts-upgrade.sql
"${db[@]}" -f scripts/test-attachment-filter-upgrade.sql
printf 'Attachment preflight upgrade preserves historical media and scoped authority.\n'
