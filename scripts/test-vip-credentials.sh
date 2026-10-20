#!/usr/bin/env bash
# Owned disposable cluster; no application configuration or production data.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
pg_bin=${PG_BIN:-/usr/lib/postgresql/16/bin}
cluster=$(mktemp -d /tmp/board-vip-credentials.XXXXXXXX)
runner=()
if [[ $(id -u) = 0 ]]; then
    chown postgres:postgres "$cluster"
    runner=(runuser -u postgres --)
fi
started=0
cleanup() {
    status=$?
    trap - EXIT
    [[ $cluster =~ ^/tmp/board-vip-credentials\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
    [[ $(readlink -f "$cluster") = "$cluster" ]] || exit 1
    if [[ $started = 1 ]]; then
        "${runner[@]}" "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop >/dev/null 2>&1 || status=1
        if "${runner[@]}" "$pg_bin/pg_ctl" -D "$cluster/data" status >/dev/null 2>&1; then server_status=0; else server_status=$?; fi
        if [[ $server_status != 3 ]]; then echo "Cluster stop unconfirmed; preserved $cluster" >&2; exit 1; fi
    fi
    rm -rf -- "$cluster"
    exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
"${runner[@]}" "$pg_bin/initdb" -D "$cluster/data" --username=postgres --auth=trust --encoding=UTF8 --no-locale >/dev/null
started=1
"${runner[@]}" "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" -o "-c listen_addresses='127.0.0.1' -c port=15455 -c unix_socket_directories='' -c statement_timeout=30000 -c lock_timeout=5000" -w start >/dev/null || { cat "$cluster/server.log" >&2; exit 1; }
psql=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h 127.0.0.1 -p 15455 -U postgres)
"${psql[@]}" -d postgres -f - < deploy/roles.sql >/dev/null
"${psql[@]}" -d postgres <<'SQL'
CREATE DATABASE vip_credentials OWNER board_migrator;
REVOKE ALL ON DATABASE vip_credentials FROM PUBLIC;
GRANT CONNECT ON DATABASE vip_credentials TO board_migrator,board_verifier,board_public,board_moderation_retention;
ALTER ROLE board_moderation_retention LOGIN;
SQL
migrator=("${psql[@]}" -U board_migrator -d vip_credentials)
for migration in migrations/*.sql; do
    [[ $(basename "$migration") < 0238_ ]] || break
    "${migrator[@]}" --single-transaction -f - < "$migration" >/dev/null
done
"${migrator[@]}" -c BEGIN -f migrations/0238_vip_credentials.sql -c ROLLBACK
"${psql[@]}" -d vip_credentials <<'SQL'
DO $$ BEGIN
 IF to_regclass('verification_private.vip_accounts') IS NOT NULL
  OR to_regprocedure('content.public_vip_ready()') IS NOT NULL
  OR content.public_pass_ready() IS DISTINCT FROM true
  OR content.public_challenge_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'VIP rollback changed existing authority'; END IF;
END $$;
SQL
"${migrator[@]}" --single-transaction -f migrations/0238_vip_credentials.sql
"${migrator[@]}" -c BEGIN -f migrations/0239_vip_posting_receipts.sql -c ROLLBACK
"${psql[@]}" -d vip_credentials <<'SQL'
DO $$ BEGIN
 IF to_regclass('verification_private.vip_post_handoffs') IS NOT NULL
  OR content.public_vip_ready() IS DISTINCT FROM true
  OR content.public_pass_ready() IS DISTINCT FROM true
  OR content.public_challenge_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'VIP posting rollback changed authority'; END IF;
END $$;
SQL
"${migrator[@]}" --single-transaction -f migrations/0239_vip_posting_receipts.sql
"${migrator[@]}" -c BEGIN -f migrations/0240_vip_attachment_admission.sql -c ROLLBACK
"${migrator[@]}" --single-transaction -f migrations/0240_vip_attachment_admission.sql
"${migrator[@]}" -c BEGIN -f migrations/0241_source_image_duplicates.sql -c ROLLBACK
"${psql[@]}" -d vip_credentials <<'SQL'
DO $$ BEGIN
 IF to_regprocedure('content.image_duplicate_candidate(text,text,text,bigint,boolean,boolean)') IS NOT NULL
  OR EXISTS(SELECT 1 FROM pg_attribute WHERE attrelid='content.boards'::regclass AND attname='image_repost_limit' AND NOT attisdropped)
  OR content.text_thread_transfer_ready() IS DISTINCT FROM true
  OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Image duplicate rollback changed existing authority'; END IF;
END $$;
SQL
"${migrator[@]}" --single-transaction -f migrations/0241_source_image_duplicates.sql
"${migrator[@]}" -c BEGIN -f migrations/0242_source_upload_image_history.sql -c ROLLBACK
"${psql[@]}" -d vip_credentials <<'SQL'
DO $$ BEGIN
 IF to_regclass('post_secrets.image_upload_history') IS NOT NULL
 OR to_regprocedure('content.expire_image_upload_history()') IS NOT NULL
 OR content.text_thread_transfer_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Upload history rollback changed existing authority'; END IF;
END $$;
SQL
"${migrator[@]}" --single-transaction -f migrations/0242_source_upload_image_history.sql
"${psql[@]}" -d vip_credentials -f scripts/test-image-upload-history.sql
"${psql[@]}" -d vip_credentials -f scripts/test-vip-credentials.sql
"${psql[@]}" -d vip_credentials -c "INSERT INTO post_secrets.image_upload_history(post_id,scope,digest,accepted_at) VALUES(9000001005,'f_md5',decode(repeat('12',16),'hex'),clock_timestamp()-interval '49 hours')"
binary="$PWD/target/debug/board-image-history-retention"
[[ -x $binary ]] || { echo 'Build board-image-history-retention before qualification.' >&2; exit 1; }
result=$(MODERATION_RETENTION_DATABASE_URL=postgres://board_moderation_retention@127.0.0.1:15455/vip_credentials "$binary" run)
[[ $result == '{"expired_image_history_rows":1,"batch_limit_reached":false}' ]] || { echo 'Image history CLI expiry differs.' >&2; exit 1; }
if MODERATION_RETENTION_DATABASE_URL=postgres://board_migrator@127.0.0.1:15455/vip_credentials "$binary" run; then
 echo 'Image history CLI accepted a migration identity.' >&2; exit 1
fi
"$pg_bin/pg_dump" -h 127.0.0.1 -p 15455 -U postgres -d vip_credentials -Fc -f "$cluster/vip.dump"
"${psql[@]}" -d postgres -c 'CREATE DATABASE vip_restored OWNER board_migrator'
"$pg_bin/pg_restore" -h 127.0.0.1 -p 15455 -U postgres -d vip_restored --exit-on-error "$cluster/vip.dump"
"${psql[@]}" -d vip_restored <<'SQL'
DO $$ BEGIN
 IF content.public_vip_ready() IS DISTINCT FROM true
  OR content.public_pass_ready() IS DISTINCT FROM true
  OR content.public_challenge_ready() IS DISTINCT FROM true
  OR content.text_thread_transfer_ready() IS DISTINCT FROM true
  OR (SELECT count(*) FROM post_secrets.image_upload_history)<>2
  OR (SELECT count(*) FROM verification_private.vip_accounts)<>2
  OR (SELECT count(*) FROM verification_private.vip_receipts)<>1
  OR NOT EXISTS(SELECT 1 FROM verification_private.vip_accounts WHERE name='Renamed Owned VIP' AND credential_version=2) THEN
  RAISE EXCEPTION 'Populated VIP restore differs'; END IF;
END $$;
SQL
printf '%s\n' 'VIP credential upgrade, rollback, authority and populated restore passed.'
