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
upgrade_db="imageboard_post_notices_$(date +%s)_$RANDOM"
[[ $upgrade_db =~ ^imageboard_post_notices_[0-9]+_[0-9]+$ ]] || exit 1
created=0
restore_created=0
restore_db="${upgrade_db}_restore"
dump_file="$cluster/post-notices-$RANDOM.dump"
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
[[ -f migrations/0203_staff_post_notices.sql ]] || exit 1
for migration in migrations/*.sql; do
 [[ $migration != migrations/0203_staff_post_notices.sql ]] || break
 "${db[@]}" --single-transaction -f "$migration"
done
"${db[@]}" -f fixtures/demo.sql
"${db[@]}" -f scripts/test-post-notices-predecessor.sql
"${db[@]}" -c BEGIN -f migrations/0203_staff_post_notices.sql -c ROLLBACK
"${db[@]}" <<'SQL'
DO $$ BEGIN IF to_regprocedure('content.post_notices_ready()') IS NOT NULL
 OR EXISTS(SELECT 1 FROM pg_attribute WHERE attrelid='content.posts'::regclass AND attname='staff_notices' AND NOT attisdropped)
 THEN RAISE EXCEPTION 'Post notice rollback retained new authority'; END IF; END $$;
SQL
"${db[@]}" --single-transaction -f migrations/0203_staff_post_notices.sql
"${db[@]}" -f scripts/test-post-notices-upgrade.sql
runuser -u postgres -- "$pg_bin/pg_dump" -h /tmp -p 55432 -d "$upgrade_db" -Fc > "$dump_file"
"${admin[@]}" -v restore_db="$restore_db" <<'SQL'
CREATE DATABASE :"restore_db" OWNER board_migrator TEMPLATE template0 ENCODING 'UTF8';
SQL
restore_created=1
runuser -u postgres -- "$pg_bin/pg_restore" -h /tmp -p 55432 -d "$restore_db" --exit-on-error < "$dump_file"
"$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 --dbname="${MIGRATION_DATABASE_URL%/imageboard}/$restore_db" -f scripts/test-post-notices-upgrade.sql
printf 'Post notice rollback, preserved content, authority and populated restore passed.\n'
