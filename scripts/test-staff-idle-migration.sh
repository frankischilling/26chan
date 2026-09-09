#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root against the disposable development cluster.' >&2; exit 1; }
source .local/database.env
cluster=$(cat .local/cluster-path)
[[ $cluster =~ ^/tmp/board-postgres\.[[:alnum:]]+$ && -d $cluster ]] || { echo 'Expected a disposable cluster path.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
admin=(runuser -u postgres -- "$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h /tmp -p 55432 -d postgres)
actual=$("${admin[@]}" -At -c 'SHOW data_directory')
[[ $actual = "$cluster" ]] || { echo 'Port 55432 belongs to a different cluster.' >&2; exit 1; }
upgrade_db="imageboard_idle_upgrade_$(date +%s)_$RANDOM"
[[ $upgrade_db =~ ^imageboard_idle_upgrade_[0-9]+_[0-9]+$ ]] || exit 1
created=0
cleanup() {
  if [[ $created = 1 ]]; then
    "${admin[@]}" -v upgrade_db="$upgrade_db" <<'SQL'
DROP DATABASE :"upgrade_db";
SQL
  fi
}
trap cleanup EXIT
"${admin[@]}" -v upgrade_db="$upgrade_db" <<'SQL'
CREATE DATABASE :"upgrade_db" OWNER board_migrator;
REVOKE ALL ON DATABASE :"upgrade_db" FROM PUBLIC;
GRANT CONNECT ON DATABASE :"upgrade_db" TO board_migrator;
SQL
created=1
upgrade_url="${MIGRATION_DATABASE_URL%/imageboard}/$upgrade_db"
db=("$pg_bin/psql" "$upgrade_url" -Xq -v ON_ERROR_STOP=1)
for migration in migrations/0001_content.sql migrations/0002_media_jobs.sql migrations/0003_media_queue_expiration.sql migrations/0004_staff.sql migrations/0005_staff_backup_eligibility.sql; do
  "${db[@]}" --single-transaction -f "$migration"
done
"${db[@]}" <<'SQL'
INSERT INTO staff_identity.accounts(role,username) VALUES ('moderator','synthetic_idle_upgrade');
INSERT INTO staff_identity.credentials(id,account_id,credential)
  SELECT decode('01','hex'),id,'{}'::jsonb FROM staff_identity.accounts;
INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id,authenticated_at,expires_at)
  SELECT decode(repeat('11',32),'hex'),decode(repeat('33',32),'hex'),id,decode('01','hex'),clock_timestamp()-interval '1 hour',clock_timestamp()+interval '7 hours' FROM staff_identity.accounts;
INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id,authenticated_at,expires_at)
  SELECT decode(repeat('22',32),'hex'),decode(repeat('44',32),'hex'),id,decode('01','hex'),clock_timestamp()-interval '2 minutes',clock_timestamp()+interval '7 hours' FROM staff_identity.accounts;
CREATE TABLE public.idle_upgrade_baseline AS SELECT token_hash,authenticated_at,expires_at FROM staff_identity.sessions;
SQL
"${db[@]}" --single-transaction -f migrations/0006_staff_idle.sql
"${db[@]}" <<'SQL'
DO $$
BEGIN
  IF (SELECT count(*) FROM staff_identity.sessions) <> 2 OR EXISTS (
    SELECT 1 FROM staff_identity.sessions s JOIN public.idle_upgrade_baseline b USING(token_hash)
    WHERE s.last_activity_at IS DISTINCT FROM b.authenticated_at
      OR s.authenticated_at IS DISTINCT FROM b.authenticated_at
      OR s.expires_at IS DISTINCT FROM b.expires_at
  ) THEN RAISE EXCEPTION 'Migration did not preserve historical session deadlines'; END IF;
  IF (SELECT count(*) FROM staff_identity.sessions WHERE last_activity_at>clock_timestamp()-interval '15 minutes') <> 1
  THEN RAISE EXCEPTION 'Migration revived an idle session or expired a live session'; END IF;
END $$;
INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id)
  SELECT decode(repeat('55',32),'hex'),decode(repeat('66',32),'hex'),id,decode('01','hex') FROM staff_identity.accounts;
DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM staff_identity.sessions WHERE token_hash=decode(repeat('55',32),'hex') AND last_activity_at>clock_timestamp()-interval '1 minute')
  THEN RAISE EXCEPTION 'New session activity default is missing'; END IF;
END $$;
SQL
cleanup
created=0
printf 'Staff migration exercise passed: 0005 sessions retain authentication and absolute deadlines; old activity stays expired; new sessions receive activity timestamps. Disposable database removed.\n'
