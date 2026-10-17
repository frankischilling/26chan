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
upgrade_db="imageboard_capability_file_$(date +%s)_$RANDOM"
[[ $upgrade_db =~ ^imageboard_capability_file_[0-9]+_[0-9]+$ ]] || exit 1
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
[[ -f migrations/0192_capability_file_effects.sql ]] || exit 1
for migration in migrations/*.sql; do
  [[ $migration != migrations/0192_capability_file_effects.sql ]] || break
  "${db[@]}" --single-transaction -f "$migration"
done
"${db[@]}" -f fixtures/demo.sql
"${db[@]}" -f scripts/test-capability-file-predecessor.sql
"${db[@]}" -c BEGIN -f migrations/0192_capability_file_effects.sql -c ROLLBACK
"${db[@]}" -c "SET ROLE board_restriction_owner; DO \$\$ BEGIN IF to_regclass('restrictions.file_clock') IS NOT NULL OR to_regprocedure('content.finalize_file_filter(bytea,inet,bytea,text,bigint,text,text,bigint,bigint,bytea,jsonb,bytea,bytea,integer,jsonb)') IS NOT NULL OR content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Capability file rollback changed predecessor state.'; END IF; END \$\$; RESET ROLE; SET ROLE board_thread_transfer_owner; DO \$\$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Capability file rollback changed transfer state.'; END IF; END \$\$; RESET ROLE"
# The previous seal did not cover these cross-owner helpers. The new migration
# must reject their drift rather than bless it while extending the seal.
for mutation in \
 "SET ROLE board_admission_owner; ALTER FUNCTION admission.file_effect_exempt(inet) SET search_path=pg_catalog,public,pg_temp; RESET ROLE" \
 "SET ROLE board_admission_owner; REVOKE EXECUTE ON FUNCTION admission.file_effect_dmca(bytea) FROM board_restriction_owner; RESET ROLE"; do
 if "${db[@]}" --single-transaction -c "$mutation" -f migrations/0192_capability_file_effects.sql > "$cluster/capability-file-drift.log" 2>&1; then
  echo 'Capability file migration accepted a changed predecessor dependency.' >&2; exit 1
 fi
 grep -q 'File effect dependency drifted:' "$cluster/capability-file-drift.log"
done
"${db[@]}" --single-transaction -f migrations/0192_capability_file_effects.sql
"${db[@]}" -f scripts/test-capability-file-upgrade.sql
printf 'Capability file upgrade preserves policy, warning and restriction state.\n'
