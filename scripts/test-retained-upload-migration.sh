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
upgrade_db="imageboard_retained_digest_$(date +%s)_$RANDOM"
[[ $upgrade_db =~ ^imageboard_retained_digest_[0-9]+_[0-9]+$ ]] || exit 1
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
[[ -f migrations/0194_retained_upload_digest.sql ]] || exit 1
for migration in migrations/*.sql; do
  [[ $migration != migrations/0194_retained_upload_digest.sql ]] || break
  "${db[@]}" --single-transaction -f "$migration"
done
"${db[@]}" -f fixtures/demo.sql
"${db[@]}" -f scripts/test-retained-upload-predecessor.sql
"${db[@]}" -c BEGIN -f migrations/0194_retained_upload_digest.sql -c ROLLBACK
"${db[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM pg_attribute WHERE attrelid='content.post_media'::regclass
   AND attname='normalized_md5' AND NOT attisdropped)
 OR to_regprocedure('content.retain_attachment_digest()') IS NOT NULL
 OR (SELECT rows FROM public.owned_upload_digest_before) IS DISTINCT FROM
    (SELECT jsonb_agg(to_jsonb(m) ORDER BY post_id) FROM content.post_media m) THEN
  RAISE EXCEPTION 'Retained upload rollback changed predecessor state'; END IF;
END $$;
SQL
for mutation in \
 "ALTER TABLE content.post_media DISABLE TRIGGER z_guard_media_erasure" \
 "GRANT SELECT ON content.post_media TO board_public"; do
 if "${db[@]}" --single-transaction -c "$mutation" -f migrations/0194_retained_upload_digest.sql > "$cluster/retained-upload-drift.log" 2>&1; then
  echo 'Retained upload migration accepted predecessor drift.' >&2; exit 1
 fi
 grep -q 'Content catalog is unavailable before retained upload identity' "$cluster/retained-upload-drift.log"
done
"${db[@]}" --single-transaction -f migrations/0194_retained_upload_digest.sql
"${db[@]}" -f scripts/test-retained-upload-upgrade.sql
printf 'Retained upload upgrade preserves old rows without invented digests.\n'
