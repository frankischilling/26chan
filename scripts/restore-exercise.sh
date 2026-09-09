#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root against the disposable cluster.' >&2; exit 1; }
source .local/database.env
source .local/media.env
source .local/staff.env
pg_bin=/usr/lib/postgresql/16/bin
restore_db="imageboard_restore_$(date +%s)_$RANDOM"
[[ $restore_db =~ ^imageboard_restore_[0-9]+_[0-9]+$ ]] || exit 1
mkdir -p .local/backups
backup=".local/backups/$restore_db.dump"
"$pg_bin/pg_dump" "$MIGRATION_DATABASE_URL" --format=custom --file="$backup"
admin=(runuser -u postgres -- "$pg_bin/psql" -X -v ON_ERROR_STOP=1 -h /tmp -p 55432)
"${admin[@]}" -v restore_db="$restore_db" <<'SQL'
CREATE DATABASE :"restore_db" OWNER board_migrator;
REVOKE ALL ON DATABASE :"restore_db" FROM PUBLIC;
GRANT CONNECT ON DATABASE :"restore_db" TO board_public, board_migrator, board_media, board_staff, board_auth;
SQL
restore_url="${MIGRATION_DATABASE_URL%/imageboard}/$restore_db"
"$pg_bin/pg_restore" --dbname="$restore_url" --exit-on-error "$backup"
fingerprint_sql="SELECT md5(string_agg(row_to_json(p)::text, '' ORDER BY id)) FROM content.posts p;"
before=$("$pg_bin/psql" "$MIGRATION_DATABASE_URL" -XAt -v ON_ERROR_STOP=1 -c "$fingerprint_sql")
after=$("$pg_bin/psql" "$restore_url" -XAt -v ON_ERROR_STOP=1 -c "$fingerprint_sql")
[[ -n $before && $before = "$after" ]] || { echo 'Restored post data differs.' >&2; exit 1; }
for table in content.boards content.threads content.reports content.moderation_audit post_secrets.deletion staff_identity.accounts staff_identity.credentials staff_identity.invitations staff_identity.ceremonies staff_identity.sessions deployment.settings public._sqlx_migrations media.jobs media.queue_policy; do
  before=$("$pg_bin/psql" "$MIGRATION_DATABASE_URL" -XAt -v ON_ERROR_STOP=1 -c "SELECT count(*) FROM $table")
  after=$("$pg_bin/psql" "$restore_url" -XAt -v ON_ERROR_STOP=1 -c "SELECT count(*) FROM $table")
  [[ $before = "$after" ]] || { echo "Restored row count differs: $table" >&2; exit 1; }
done
public_restore="${TEST_PUBLIC_DATABASE_URL%/imageboard}/$restore_db"
"$pg_bin/psql" "$public_restore" -XAt -v ON_ERROR_STOP=1 -c 'SELECT count(*) FROM content.boards' > .local/restored-public-check.txt
if "$pg_bin/psql" "$public_restore" -XAt -v ON_ERROR_STOP=1 -c 'SELECT * FROM staff_identity.credentials' > .local/restored-denial.txt 2>&1; then
  echo 'Restored public login could read protected staff data.' >&2; exit 1
fi
grep -q 'permission denied' .local/restored-denial.txt
media_restore="${MEDIA_DATABASE_URL%/imageboard}/$restore_db"
"$pg_bin/psql" "$media_restore" -XAt -v ON_ERROR_STOP=1 -c 'SELECT capacity FROM media.queue_policy' > .local/restored-media-check.txt
if "$pg_bin/psql" "$media_restore" -XAt -v ON_ERROR_STOP=1 -c 'SELECT * FROM content.posts' > .local/restored-media-denial.txt 2>&1; then
  echo 'Restored media login could read public content.' >&2; exit 1
fi
grep -q 'permission denied' .local/restored-media-denial.txt
auth_restore="${AUTH_DATABASE_URL%/imageboard}/$restore_db"
staff_restore="${STAFF_DATABASE_URL%/imageboard}/$restore_db"
"$pg_bin/psql" "$auth_restore" -XAt -v ON_ERROR_STOP=1 -c 'SELECT count(*) FROM staff_identity.accounts' > .local/restored-auth-check.txt
"$pg_bin/psql" "$staff_restore" -XAt -v ON_ERROR_STOP=1 -c 'SELECT count(*) FROM content.reports' > .local/restored-staff-check.txt
if "$pg_bin/psql" "$auth_restore" -XAt -v ON_ERROR_STOP=1 -c "UPDATE staff_identity.accounts SET role = 'admin' WHERE false" > .local/restored-auth-denial.txt 2>&1; then
  echo 'Restored authentication login could modify staff roles.' >&2; exit 1
fi
grep -q 'permission denied' .local/restored-auth-denial.txt
if "$pg_bin/psql" "$staff_restore" -XAt -v ON_ERROR_STOP=1 -c 'SELECT * FROM staff_identity.credentials' > .local/restored-staff-denial.txt 2>&1; then
  echo 'Restored moderation login could read credentials.' >&2; exit 1
fi
grep -q 'permission denied' .local/restored-staff-denial.txt
"${admin[@]}" -v restore_db="$restore_db" <<'SQL'
DROP DATABASE :"restore_db";
SQL
printf 'Restore exercise passed: post fingerprint, fourteen table counts, public/media/auth/staff reads and protected-operation denials. Disposable restored database removed.\n'
printf 'Source PostgreSQL: '; "$pg_bin/pg_dump" --version
