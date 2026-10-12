#!/usr/bin/env bash
# Qualify only owned synthetic clusters; no historical ownership is backfilled.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-staff-self.XXXXXXXX)
started=0
cleanup() {
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null
    fi
    [[ $cluster =~ ^/tmp/board-staff-self\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
    [[ $(readlink -f "$cluster") = "$cluster" ]] || exit 1
    rm -rf -- "$cluster"
}
trap cleanup EXIT
chown postgres:postgres "$cluster"
runuser -u postgres -- "$pg_bin/initdb" -D "$cluster/data" --auth=trust --encoding=UTF8 --no-locale > /dev/null
runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" \
    -o "-c listen_addresses='' -c unix_socket_directories='$cluster' -c statement_timeout=30000 -c lock_timeout=5000" -w start > /dev/null
started=1
psql=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h "$cluster")
# SQL diagnostics and synthetic fixture values stay in the private directory.
exec 3>&1
exec > "$cluster/qualification.log" 2>&1
trap 'printf "Staff-self qualification failed at line %s (private diagnostics removed during cleanup).\n" "$LINENO" >&3' ERR
runuser -u postgres -- "${psql[@]}" -d postgres -f deploy/roles.sql
runuser -u postgres -- "${psql[@]}" -d postgres <<'SQL'
ALTER ROLE board_staff LOGIN;
ALTER ROLE board_auth LOGIN;
ALTER ROLE board_media_read LOGIN;
ALTER ROLE board_media_intake LOGIN;
ALTER ROLE board_monitor LOGIN;
SQL
create_database() {
 runuser -u postgres -- "${psql[@]}" -d postgres -v database="$1" <<'SQL'
CREATE DATABASE :"database" OWNER board_migrator;
REVOKE ALL ON DATABASE :"database" FROM PUBLIC;
GRANT CONNECT ON DATABASE :"database" TO board_migrator,board_public,board_staff,board_auth,
 board_media,board_media_read,board_media_intake,board_monitor;
SQL
}
for mode in fresh upgrade; do
 database="staff_self_$mode"
 create_database "$database"
 migrator=("${psql[@]}" -U board_migrator -d "$database")
 for migration in migrations/*.sql; do
    [[ $migration != migrations/0140* ]] || break
    "${migrator[@]}" --single-transaction -f "$migration"
 done
 if [[ $mode = fresh ]]; then
    "${migrator[@]}" --single-transaction -f migrations/0140_staff_self_deletion.sql
 fi
 "${migrator[@]}" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('selfcheck','Owned self-deletion fixture','Synthetic',1000,100,100,100,10);
INSERT INTO content.threads(id,board) VALUES(9201000,'selfcheck');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(9201000,'selfcheck',9201000,'Synthetic','Owned OP','Keep body'),
      (9201001,'selfcheck',9201000,'Synthetic','','Keep reply');
INSERT INTO post_secrets.deletion(post_id,password_hash)
VALUES(9201000,'synthetic-op-hash'),(9201001,'synthetic-reply-hash');
INSERT INTO post_secrets.posting_history(post_id,board,thread_id,actor_hash,request_at)
VALUES(9201000,'selfcheck',9201000,decode(repeat('11',32),'hex'),1),
      (9201001,'selfcheck',9201000,decode(repeat('22',32),'hex'),2);
INSERT INTO content.reports(board,post_id,reason) VALUES('selfcheck',9201001,'Owned report');
INSERT INTO content.moderation_audit(account_id,board,target_id,action)
VALUES(42,'selfcheck',9201001,'remove-file');
SQL
 snapshot="SELECT json_build_array(
 (SELECT json_agg(p ORDER BY id) FROM content.posts p WHERE board='selfcheck'),
 (SELECT json_agg(r ORDER BY id) FROM content.reports r WHERE board='selfcheck'),
 (SELECT json_agg(a ORDER BY id) FROM content.moderation_audit a WHERE board='selfcheck'),
 (SELECT json_agg(d ORDER BY post_id) FROM post_secrets.deletion d),
 (SELECT json_agg(h ORDER BY post_id) FROM post_secrets.posting_history h))::text"
 before=$("${migrator[@]}" -Atc "$snapshot")
 if [[ $mode = upgrade ]]; then
    "${migrator[@]}" --single-transaction -f migrations/0140_staff_self_deletion.sql
 fi
 after=$("${migrator[@]}" -Atc "$snapshot")
 [[ $before = "$after" ]] || { echo 'Migration changed historical fixture data.' >&2; exit 1; }
 staff=("${psql[@]}" -U board_staff -d "$database")
 [[ $("${staff[@]}" -Atc "SELECT content.staff_post_deletion_hash('selfcheck',9201001)") = synthetic-reply-hash ]]
 [[ $("${staff[@]}" -Atc "SELECT content.staff_post_peer_matches('selfcheck',9201001,decode(repeat('22',32),'hex'))") = t ]]
 [[ $("${staff[@]}" -Atc "SELECT content.staff_post_peer_matches('selfcheck',9201001,decode(repeat('11',32),'hex'))") = f ]]
 [[ $("${staff[@]}" -Atc "SELECT content.staff_post_deletion_hash('wrong',9201001) IS NULL") = t ]]
 [[ $("${staff[@]}" -Atc "SELECT content.text_thread_transfer_ready()") = t ]]
 for role in board_public board_auth board_media board_media_read board_media_intake board_monitor; do
    for query in "SELECT content.staff_post_deletion_hash('selfcheck',9201001)" \
      "SELECT content.staff_post_peer_matches('selfcheck',9201001,NULL)"; do
        if "${psql[@]}" -U "$role" -d "$database" -c "$query" > "$cluster/denial.log" 2>&1; then
            echo "Unexpected helper access for $role" >&2; exit 1
        fi
        grep -q 'permission denied' "$cluster/denial.log"
    done
 done
 # Archive retirement cannot leave either helper claiming ownership.
 "${migrator[@]}" -c "UPDATE content.boards SET archive_retention_seconds=86400 WHERE slug='selfcheck'; UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 day' WHERE id=9201000;"
 [[ $("${staff[@]}" -Atc "SELECT content.staff_post_deletion_hash('selfcheck',9201001) IS NULL AND NOT content.staff_post_peer_matches('selfcheck',9201001,decode(repeat('22',32),'hex'))") = t ]]
 # Restore exact schema/role references and verify sealed transfer metadata.
 runuser -u postgres -- "$pg_bin/pg_dump" -h "$cluster" -Fc "$database" > "$cluster/restore.dump"
 restored="${database}_restored"
 create_database "$restored"
 runuser -u postgres -- "$pg_bin/pg_restore" -h "$cluster" --exit-on-error -d "$restored" "$cluster/restore.dump"
 [[ $("${psql[@]}" -U board_staff -d "$restored" -Atc 'SELECT content.text_thread_transfer_ready()') = t ]]
 printf 'Staff-self %s and restore qualification passed.\n' "$mode" >&3
done
# A preexisting catalog mismatch must fail before creating helpers or resealing it.
create_database staff_self_drift
migrator=("${psql[@]}" -U board_migrator -d staff_self_drift)
for migration in migrations/*.sql; do
 [[ $migration != migrations/0140* ]] || break
 "${migrator[@]}" --single-transaction -f "$migration"
done
"${migrator[@]}" -c 'ALTER TABLE content.moderation_audit DROP CONSTRAINT moderation_audit_action_check;'
if "${migrator[@]}" --single-transaction -f migrations/0140_staff_self_deletion.sql > "$cluster/drift.log" 2>&1; then
 echo 'Preexisting catalog drift was accepted.' >&2; exit 1
fi
grep -q 'Text transfer catalog is not ready before staff self deletion' "$cluster/drift.log"
[[ $("${migrator[@]}" -Atc "SELECT to_regprocedure('content.staff_post_peer_matches(text,bigint,bytea)') IS NULL") = t ]]
printf 'Staff-self preexisting drift rejection passed.\n' >&3
