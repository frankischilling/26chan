#!/usr/bin/env bash
# Fresh/populated/restore qualification on an owned disposable PostgreSQL host.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
binary=$(realpath target/debug/board-moderation-retention)
[[ -x $binary ]] || { echo 'Build board-moderation-retention first.' >&2; exit 1; }
cluster=$(mktemp -d /tmp/board-moderation-retention.XXXXXXXX)
started=0
cleanup() {
 if [[ $started = 1 ]]; then runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop >/dev/null; fi
 [[ $cluster =~ ^/tmp/board-moderation-retention\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
 [[ $(readlink -f "$cluster") = "$cluster" ]] || exit 1
 rm -rf -- "$cluster"
}
trap cleanup EXIT
chown postgres:postgres "$cluster"
runuser -u postgres -- "$pg_bin/initdb" -D "$cluster/data" --auth=trust --encoding=UTF8 --no-locale >/dev/null
runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" \
 -o "-c listen_addresses='' -c unix_socket_directories='$cluster' -c statement_timeout=30000 -c lock_timeout=5000" -w start >/dev/null
started=1
psql=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h "$cluster")
exec 3>&1
exec > "$cluster/qualification.log" 2>&1
trap 'printf "Moderation retention qualification failed at line %s.\n" "$LINENO" >&3' ERR
runuser -u postgres -- "${psql[@]}" -d postgres -f deploy/roles.sql
runuser -u postgres -- "${psql[@]}" -d postgres -c 'ALTER ROLE board_moderation_retention LOGIN; ALTER ROLE board_staff LOGIN; ALTER ROLE board_auth LOGIN;'
create_database() {
 runuser -u postgres -- "${psql[@]}" -d postgres -v database="$1" <<'SQL'
CREATE DATABASE :"database" OWNER board_migrator;
REVOKE ALL ON DATABASE :"database" FROM PUBLIC;
GRANT CONNECT ON DATABASE :"database" TO board_migrator,board_public,board_staff,board_auth,board_moderation_retention;
SQL
}
for mode in fresh upgrade; do
 database="retention_$mode"
 create_database "$database"
 migrator=("${psql[@]}" -U board_migrator -d "$database")
 worker=("${psql[@]}" -U board_moderation_retention -d "$database")
 for migration in migrations/*.sql; do
  [[ $migration != migrations/0142* ]] || break
  "${migrator[@]}" --single-transaction -f "$migration"
 done
 if [[ $mode = fresh ]]; then "${migrator[@]}" --single-transaction -f migrations/0142_moderation_snapshot_retention.sql; fi
 "${migrator[@]}" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
 VALUES('retain','Owned retention fixture','Synthetic',1000,100,100,100,10);
INSERT INTO content.moderation_audit(account_id,board,target_id,action,snapshot_version,
 snapshot_name,snapshot_subject,snapshot_comment,snapshot_comment_format,snapshot_staff_authorized_limits,
 snapshot_wordfiltered,snapshot_image_spoiler,created_at)
 SELECT 42,'retain',n,'spoiler',CASE WHEN n=1 THEN 1 ELSE 2 END,
 'Owned name','Owned subject','Owned body',0,false,false,false,
 CASE WHEN n=1 THEN clock_timestamp()-interval '31 days'
      WHEN n=2 THEN clock_timestamp() ELSE '2100-01-01Z'::timestamptz END
 FROM generate_series(1,3) n;
INSERT INTO content.moderation_audit(account_id,board,target_id,action,created_at)
 VALUES(42,'retain',4,'remove-post',clock_timestamp()-interval '60 days');
SQL
 snapshot="SELECT jsonb_agg(to_jsonb(a)-'snapshot_expires_at' ORDER BY id)::text FROM content.moderation_audit a"
 before=$("${migrator[@]}" -Atc "$snapshot")
 if [[ $mode = upgrade ]]; then "${migrator[@]}" --single-transaction -f migrations/0142_moderation_snapshot_retention.sql; fi
 [[ $before = "$("${migrator[@]}" -Atc "$snapshot")" ]] || { echo 'Migration erased or rewrote existing audit data.'; exit 1; }
 [[ $("${worker[@]}" -Atc 'SELECT content.moderation_retention_ready()') = t ]]
 if [[ $mode = fresh ]]; then
  [[ $("${migrator[@]}" -Atc "SELECT count(*) FROM content.moderation_audit WHERE snapshot_expires_at BETWEEN clock_timestamp()+interval '719 hours' AND clock_timestamp()+interval '720 hours'") = 3 ]]
  # The old audit timestamp supplied after migration cannot create an early expiry.
  [[ $("${worker[@]}" -Atc 'SELECT content.expire_moderation_snapshots()') = 0 ]]
  "${migrator[@]}" -c "UPDATE content.moderation_audit SET snapshot_expires_at=clock_timestamp()-interval '1 second' WHERE target_id=1"
 else
  [[ $("${migrator[@]}" -Atc "SELECT count(*) FROM content.moderation_audit WHERE snapshot_expires_at<=clock_timestamp()") = 1 ]]
 fi
 # Backup still contains the expired snapshot. Restore qualification must sweep it.
 runuser -u postgres -- "$pg_bin/pg_dump" -h "$cluster" -d "$database" -Fc -f "$cluster/$mode.dump"
 result=$(MODERATION_RETENTION_DATABASE_URL="postgresql://board_moderation_retention@localhost/$database?host=$cluster" "$binary" run)
 [[ $result = '{"expired_snapshots":1,"batch_limit_reached":false}' ]]
 [[ $("${migrator[@]}" -Atc 'SELECT count(*) FROM content.moderation_audit') = 4 ]]
 [[ $("${migrator[@]}" -Atc 'SELECT count(snapshot_version) FROM content.moderation_audit') = 2 ]]
 [[ $("${worker[@]}" -Atc 'SELECT content.expire_moderation_snapshots()') = 0 ]]
 for role in board_public board_staff board_auth; do
  if "${psql[@]}" -U "$role" -d "$database" -c 'SELECT content.expire_moderation_snapshots()'; then echo 'Runtime gained expiry authority.'; exit 1; fi
 done
 for sql in 'SELECT snapshot_comment FROM content.moderation_audit' 'UPDATE content.moderation_audit SET snapshot_comment=NULL' 'DELETE FROM content.moderation_audit' 'SELECT * FROM post_secrets.moderation_retention_catalog'; do
  if "${worker[@]}" -c "$sql"; then echo 'Worker gained private/direct write access.'; exit 1; fi
 done
 "${migrator[@]}" -c 'ALTER TABLE content.moderation_audit DISABLE TRIGGER moderation_snapshot_deadline'
 if "${worker[@]}" -c 'SELECT content.expire_moderation_snapshots()'; then echo 'Disabled deadline guard was accepted.'; exit 1; fi
 "${migrator[@]}" -c 'ALTER TABLE content.moderation_audit ENABLE TRIGGER moderation_snapshot_deadline'
 [[ $("${worker[@]}" -Atc 'SELECT content.moderation_retention_ready()') = t ]]
 "${migrator[@]}" -c 'GRANT SELECT(snapshot_comment) ON content.moderation_audit TO board_moderation_retention'
 if "${worker[@]}" -c 'SELECT content.expire_moderation_snapshots()'; then echo 'Expanded worker permissions were accepted.'; exit 1; fi
 "${migrator[@]}" -c 'REVOKE SELECT(snapshot_comment) ON content.moderation_audit FROM board_moderation_retention'
 [[ $("${worker[@]}" -Atc 'SELECT content.moderation_retention_ready()') = t ]]
 restore="${database}_restored"
 create_database "$restore"
 runuser -u postgres -- "$pg_bin/pg_restore" -h "$cluster" -d "$restore" --exit-on-error "$cluster/$mode.dump"
 [[ $("${psql[@]}" -U board_moderation_retention -d "$restore" -Atc 'SELECT content.moderation_retention_ready()') = t ]]
 result=$(MODERATION_RETENTION_DATABASE_URL="postgresql://board_moderation_retention@localhost/$restore?host=$cluster" "$binary" run)
 [[ $result = '{"expired_snapshots":1,"batch_limit_reached":false}' ]]
 [[ $("${psql[@]}" -U board_migrator -d "$restore" -Atc 'SELECT count(snapshot_version) FROM content.moderation_audit') = 2 ]]
 if MODERATION_RETENTION_DATABASE_URL="postgresql://board_migrator@localhost/$database?host=$cluster" "$binary" run; then echo 'CLI accepted migration credentials.'; exit 1; fi
 echo "Qualified $mode retention, narrow login, drift refusal and restored expiry." >&3
done
