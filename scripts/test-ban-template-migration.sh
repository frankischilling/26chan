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
upgrade_db="imageboard_ban_templates_$(date +%s)_$RANDOM"
[[ $upgrade_db =~ ^imageboard_ban_templates_[0-9]+_[0-9]+$ ]] || exit 1
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
[[ -f migrations/0195_staff_ban_templates.sql ]] || exit 1
for migration in migrations/*.sql; do
  [[ $migration != migrations/0195_staff_ban_templates.sql ]] || break
  "${db[@]}" --single-transaction -f "$migration"
done
"${db[@]}" -f fixtures/demo.sql
"${db[@]}" -f scripts/test-file-restriction-predecessor.sql
"${db[@]}" -c BEGIN -f migrations/0195_staff_ban_templates.sql -c ROLLBACK
"${db[@]}" <<'SQL'
SET ROLE board_restriction_owner;
DO $$ BEGIN
 IF to_regclass('restrictions.templates') IS NOT NULL
 OR EXISTS(SELECT 1 FROM pg_attribute WHERE attrelid='restrictions.bans'::regclass
   AND attname='template_id' AND NOT attisdropped)
 OR content.restrictions_ready() IS DISTINCT FROM true
 OR (SELECT bans FROM public.owned_file_restrictions_before) IS DISTINCT FROM
    (SELECT jsonb_agg(to_jsonb(b) ORDER BY id) FROM restrictions.bans b) THEN
  RAISE EXCEPTION 'Template rollback changed predecessor state'; END IF;
END $$;
RESET ROLE;
SQL
for mutation in \
 "ALTER TABLE content.post_media DISABLE TRIGGER zz_retain_attachment_digest" \
 "SET ROLE board_restriction_owner; GRANT SELECT ON restrictions.bans TO board_public; RESET ROLE"; do
 if "${db[@]}" --single-transaction -c "$mutation" -f migrations/0195_staff_ban_templates.sql > "$cluster/ban-template-drift.log" 2>&1; then
  echo 'Ban template migration accepted predecessor drift.' >&2; exit 1
 fi
 grep -Eq 'Restriction catalog drifted|Content catalog drifted' "$cluster/ban-template-drift.log"
done
"${db[@]}" --single-transaction -f migrations/0195_staff_ban_templates.sql
"${db[@]}" -f scripts/test-ban-template-upgrade.sql
printf 'Ban template upgrade preserves existing restrictions without inferred history.\n'
