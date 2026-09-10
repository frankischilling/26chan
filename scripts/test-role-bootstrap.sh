#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-role-bootstrap.XXXXXXXX)
[[ $cluster =~ ^/tmp/board-role-bootstrap\.[[:alnum:]]+$ && -d $cluster && ! -L $cluster ]] || exit 1
started=0
cleanup() {
  if [[ $started = 1 ]]; then
    runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null
    started=0
  fi
  [[ $cluster =~ ^/tmp/board-role-bootstrap\.[[:alnum:]]+$ && -d $cluster && ! -L $cluster ]] || exit 1
  rm -rf -- "$cluster"
}
trap cleanup EXIT
chown postgres:postgres "$cluster"
runuser -u postgres -- "$pg_bin/initdb" -D "$cluster/data" --auth=trust --encoding=UTF8 --no-locale > /dev/null
# Only a Unix socket inside this private generated directory; no TCP listener.
runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" \
  -o "-c listen_addresses='' -c unix_socket_directories='$cluster'" -w start > /dev/null
started=1
db=(runuser -u postgres -- "$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h "$cluster")
"${db[@]}" -d postgres -f deploy/roles.sql
"${db[@]}" -d postgres <<'SQL'
CREATE DATABASE bootstrap_test OWNER board_migrator;
REVOKE ALL ON DATABASE bootstrap_test FROM PUBLIC;
GRANT CONNECT ON DATABASE bootstrap_test TO board_migrator,board_public;
SQL
for migration in migrations/*.sql; do
  "${db[@]}" -d bootstrap_test --single-transaction -c 'SET ROLE board_migrator' -f "$migration"
done
"${db[@]}" -d bootstrap_test <<'SQL'
DO $$
BEGIN
  IF (SELECT rolcanlogin FROM pg_roles WHERE rolname='board_media_read') THEN
    RAISE EXCEPTION 'Unqualified staging reader is login-enabled';
  END IF;
  IF NOT has_table_privilege('board_media_read','media.approved_assets','SELECT')
     OR has_table_privilege('board_media_read','media.assets','SELECT')
     OR has_table_privilege('board_media_read','media.jobs','SELECT') THEN
    RAISE EXCEPTION 'Bootstrap reader grants differ';
  END IF;
  IF (SELECT rolcanlogin FROM pg_roles WHERE rolname='board_monitor') THEN
    RAISE EXCEPTION 'Unqualified observer is login-enabled';
  END IF;
  IF NOT has_schema_privilege('board_monitor','monitoring','USAGE')
     OR NOT has_table_privilege('board_monitor','monitoring.media_queue','SELECT')
     OR has_schema_privilege('board_monitor','media','USAGE')
     OR has_table_privilege('board_monitor','media.jobs','SELECT,INSERT,UPDATE,DELETE')
     OR has_table_privilege('board_monitor','media.queue_policy','SELECT,UPDATE') THEN
    RAISE EXCEPTION 'Bootstrap observer grants differ';
  END IF;
END $$;
DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname IN ('board_media_intake','board_media_intake_owner')
      AND (rolcanlogin OR rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls)) THEN
    RAISE EXCEPTION 'Unqualified intake roles have login or elevated flags';
  END IF;
  IF EXISTS (SELECT 1 FROM pg_auth_members m JOIN pg_roles r ON r.oid = m.member
      WHERE r.rolname IN ('board_media_intake','board_media_intake_owner')) THEN
    RAISE EXCEPTION 'Intake roles have membership';
  END IF;
  IF NOT has_schema_privilege('board_media_intake','media_intake','USAGE')
     OR has_schema_privilege('board_media_intake','media_intake','CREATE')
     OR has_schema_privilege('board_media_intake','media','USAGE')
     OR has_any_column_privilege('board_media_intake','media_intake.handles','SELECT,INSERT,UPDATE,REFERENCES')
     OR has_any_column_privilege('board_media_intake','media.jobs','SELECT,INSERT,UPDATE,REFERENCES')
     OR NOT has_function_privilege('board_media_intake','media_intake.reserve(text)','EXECUTE') THEN
    RAISE EXCEPTION 'Bootstrap intake grants differ';
  END IF;
  IF has_column_privilege('board_media_intake_owner','media.jobs','lease_token','SELECT,INSERT,UPDATE')
     OR has_column_privilege('board_media_intake_owner','media.jobs','attempts','SELECT,INSERT,UPDATE')
     OR has_column_privilege('board_media_intake_owner','media.jobs','output_sha256','SELECT,INSERT,UPDATE')
     OR has_any_column_privilege('board_media_intake_owner','media.assets','INSERT,UPDATE,REFERENCES')
     OR has_table_privilege('board_media_intake_owner','media.jobs','DELETE,TRUNCATE,TRIGGER')
     OR has_schema_privilege('board_media_intake_owner','media_intake','CREATE') THEN
    RAISE EXCEPTION 'Intake owner exceeds required authority';
  END IF;
END $$;
SET ROLE board_media_intake;
SELECT media_intake.ready();
SELECT id IS NOT NULL AND capability IS NOT NULL AS reserved FROM media_intake.reserve('bootstrap-synthetic.png');
RESET ROLE;
SET ROLE board_media_read;
SELECT count(*) AS initially_approved FROM media.approved_assets;
RESET ROLE;
SET ROLE board_monitor;
SELECT capacity, receiving, queued, processing FROM monitoring.media_queue;
SQL
cleanup
trap - EXIT
printf 'Fresh role bootstrap passed: all migrations applied as owner; reader, observer and intake remain NOLOGIN with restricted grants. Private cluster removed.\n'
