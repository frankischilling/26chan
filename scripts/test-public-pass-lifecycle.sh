#!/usr/bin/env bash
# Owned disposable cluster; no application configuration or production data.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
pg_bin=${PG_BIN:-/usr/lib/postgresql/16/bin}
cluster=$(mktemp -d /tmp/board-pass-lifecycle.XXXXXXXX)
runner=()
if [[ $(id -u) = 0 ]]; then
    chown postgres:postgres "$cluster"
    runner=(runuser -u postgres --)
fi
started=0
cleanup() {
    status=$?
    trap - EXIT
    [[ $cluster =~ ^/tmp/board-pass-lifecycle\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
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
"${runner[@]}" "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" -o "-c listen_addresses='127.0.0.1' -c port=15453 -c unix_socket_directories='' -c statement_timeout=30000 -c lock_timeout=5000" -w start >/dev/null || { cat "$cluster/server.log" >&2; exit 1; }
psql=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h 127.0.0.1 -p 15453 -U postgres)
"${psql[@]}" -d postgres -f - < deploy/roles.sql >/dev/null
"${psql[@]}" -d postgres <<'SQL'
CREATE DATABASE pass_lifecycle OWNER board_migrator;
REVOKE ALL ON DATABASE pass_lifecycle FROM PUBLIC;
GRANT CONNECT ON DATABASE pass_lifecycle TO board_migrator,board_verifier,board_public;
SQL
migrator=("${psql[@]}" -U board_migrator -d pass_lifecycle)
for migration in migrations/*.sql; do
    [[ $(basename "$migration") < 0216_ ]] || break
    "${migrator[@]}" --single-transaction -f - < "$migration" >/dev/null
done
"${migrator[@]}" -c BEGIN -f migrations/0216_public_pass_authority.sql -c ROLLBACK
"${psql[@]}" -d pass_lifecycle <<'SQL'
DO $$ BEGIN IF to_regclass('verification_private.pass_entitlements') IS NOT NULL
 OR to_regprocedure('content.public_pass_ready()') IS NOT NULL THEN
 RAISE EXCEPTION 'Membership rollback left authority behind'; END IF; END $$;
SQL
"${migrator[@]}" --single-transaction -f migrations/0216_public_pass_authority.sql
"${migrator[@]}" -c BEGIN -f migrations/0217_pass_post_challenge.sql -c ROLLBACK
"${psql[@]}" -d pass_lifecycle <<'SQL'
DO $$ BEGIN
 IF to_regclass('verification_private.pass_post_handoffs') IS NOT NULL
  OR content.public_pass_ready() IS DISTINCT FROM true
  OR content.public_challenge_ready() IS DISTINCT FROM true
  OR content.restrictions_ready() IS DISTINCT FROM true
  OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Pass post rollback changed existing authority'; END IF;
END $$;
SQL
"${migrator[@]}" --single-transaction -f migrations/0217_pass_post_challenge.sql
"${migrator[@]}" -c BEGIN -f migrations/0218_pass_only_boards.sql -c ROLLBACK
"${psql[@]}" -d pass_lifecycle <<'SQL'
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM pg_attribute WHERE attrelid='content.boards'::regclass AND attname='pass_post_only' AND NOT attisdropped)
  OR content.public_pass_ready() IS DISTINCT FROM true
  OR content.public_challenge_ready() IS DISTINCT FROM true
  OR content.restrictions_ready() IS DISTINCT FROM true
  OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Pass-only rollback changed existing authority'; END IF;
END $$;
SQL
"${migrator[@]}" -c 'SET ROLE board_anonymous_owner' -c "DO \$\$ BEGIN IF post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass-only rollback changed classifier history'; END IF; END \$\$;"
"${migrator[@]}" --single-transaction -f migrations/0218_pass_only_boards.sql
"${psql[@]}" -d pass_lifecycle <<'SQL'
DO $$ BEGIN
 IF (SELECT pass_post_only FROM content.boards WHERE slug='vip') IS DISTINCT FROM true
  OR EXISTS(SELECT 1 FROM content.boards WHERE slug<>'vip' AND pass_post_only)
  OR has_column_privilege('board_public','content.boards','pass_post_only','UPDATE')
  OR has_column_privilege('board_verifier','content.boards','pass_post_only','UPDATE') THEN
  RAISE EXCEPTION 'Pass-only source board configuration differs'; END IF;
END $$;
SQL
"${migrator[@]}" -c BEGIN -f migrations/0219_pass_reply_cooldowns.sql -c ROLLBACK
"${migrator[@]}" -c 'SET ROLE board_anonymous_owner' -c "DO \$\$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true OR post_secrets.classifier_history_ready() IS DISTINCT FROM true OR to_regprocedure('content.pass_check_posting_cooldown(bytea,text,bigint,boolean,bigint)') IS NOT NULL OR has_function_privilege('board_anonymous_owner','content.check_posting_cooldown_core(bytea,text,bigint,boolean,bigint,boolean)','EXECUTE') THEN RAISE EXCEPTION 'Cooldown rollback changed authority'; END IF; END \$\$;"
"${migrator[@]}" --single-transaction -f migrations/0219_pass_reply_cooldowns.sql
"${migrator[@]}" -c BEGIN -f migrations/0220_pass_purchase_year.sql -c ROLLBACK
"${psql[@]}" -d pass_lifecycle <<'SQL'
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM pg_attribute WHERE attrelid='content.posts'::regclass AND attname='since4pass' AND NOT attisdropped)
  OR content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
  OR content.restrictions_ready() IS DISTINCT FROM true OR content.text_thread_transfer_ready() IS DISTINCT FROM true
  OR content.report_snapshots_ready() IS DISTINCT FROM true OR content.report_profile_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Purchase year rollback changed authority'; END IF;
END $$;
SQL
"${migrator[@]}" --single-transaction -f migrations/0220_pass_purchase_year.sql
"${psql[@]}" -d pass_lifecycle <<'SQL'
DO $$ BEGIN
 IF has_column_privilege('board_public','content.posts','since4pass','INSERT,UPDATE')
 OR has_column_privilege('board_staff','content.posts','since4pass','INSERT,UPDATE') THEN
 RAISE EXCEPTION 'Runtime can forge a public purchase year'; END IF;
END $$;
SQL
"${migrator[@]}" -c BEGIN -f migrations/0221_private_pass_post_identity.sql -c ROLLBACK
"${psql[@]}" -d pass_lifecycle <<'SQL'
DO $$ BEGIN
 IF to_regclass('verification_private.pass_posts') IS NOT NULL
 OR content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR content.text_thread_transfer_ready() IS DISTINCT FROM true OR content.restrictions_ready() IS DISTINCT FROM true
 OR content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Private Pass capture rollback changed authority'; END IF;
END $$;
SQL
"${migrator[@]}" --single-transaction -f migrations/0221_private_pass_post_identity.sql
"${migrator[@]}" -c BEGIN -f migrations/0222_pass_duplicate_image_cooldown.sql -c ROLLBACK
"${psql[@]}" -d pass_lifecycle <<'SQL'
DO $$ BEGIN
 IF to_regprocedure('content.pass_check_duplicate_image(bytea,text,bigint,bigint,text,text)') IS NOT NULL
 OR content.public_pass_ready() IS DISTINCT FROM true OR content.text_thread_transfer_ready() IS DISTINCT FROM true
 OR content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Duplicate-image rollback changed authority'; END IF;
END $$;
SQL
"${migrator[@]}" --single-transaction -f migrations/0222_pass_duplicate_image_cooldown.sql
"${migrator[@]}" -c BEGIN -f migrations/0223_pass_posting_restrictions.sql -c ROLLBACK
"${psql[@]}" -d pass_lifecycle <<'SQL'
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM pg_attribute WHERE attrelid='restrictions.bans'::regclass AND attname='pass_identity_hash' AND NOT attisdropped)
 OR content.public_pass_ready() IS DISTINCT FROM true OR content.text_thread_transfer_ready() IS DISTINCT FROM true
 OR content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass restriction rollback changed authority'; END IF;
END $$;
SQL
"${migrator[@]}" --single-transaction -f migrations/0223_pass_posting_restrictions.sql
"${migrator[@]}" -c BEGIN -f migrations/0224_pass_warning_receipts.sql -c ROLLBACK
"${psql[@]}" -d pass_lifecycle <<'SQL'
DO $$ BEGIN
 IF to_regprocedure('content.pass_restriction_status(bytea,bytea,text,bigint)') IS NOT NULL
 OR content.public_pass_ready() IS DISTINCT FROM true OR content.restrictions_ready() IS DISTINCT FROM true
 THEN RAISE EXCEPTION 'Pass warning rollback changed authority'; END IF;
END $$;
SQL
"${migrator[@]}" --single-transaction -f migrations/0224_pass_warning_receipts.sql
"${migrator[@]}" -c BEGIN -f migrations/0225_private_pass_report_identity.sql -c ROLLBACK
"${psql[@]}" -d pass_lifecycle <<'SQL'
DO $$ BEGIN IF to_regclass('verification_private.pass_reports') IS NOT NULL
 OR content.public_pass_ready() IS DISTINCT FROM true OR content.report_profile_ready() IS DISTINCT FROM true
 THEN RAISE EXCEPTION 'Report identity rollback changed authority'; END IF; END $$;
SQL
"${migrator[@]}" --single-transaction -f migrations/0225_private_pass_report_identity.sql
"${migrator[@]}" -c BEGIN -f migrations/0226_pass_report_quotas.sql -c ROLLBACK
"${psql[@]}" -d pass_lifecycle <<'SQL'
DO $$ BEGIN IF to_regprocedure('content.pass_report_ids(bytea,text,bigint)') IS NOT NULL
 OR content.public_pass_ready() IS DISTINCT FROM true OR content.report_profile_ready() IS DISTINCT FROM true
 THEN RAISE EXCEPTION 'Report quota rollback changed authority'; END IF; END $$;
SQL
"${migrator[@]}" --single-transaction -f migrations/0226_pass_report_quotas.sql
"${migrator[@]}" -c BEGIN -f migrations/0227_pass_report_restrictions.sql -c ROLLBACK
"${psql[@]}" -d pass_lifecycle <<'SQL'
DO $$ BEGIN IF to_regprocedure('verification_private.pass_report_identity(bytea,bytea,text,bigint)') IS NOT NULL
 OR content.public_pass_ready() IS DISTINCT FROM true OR content.restrictions_ready() IS DISTINCT FROM true
 THEN RAISE EXCEPTION 'Report boundary rollback changed authority'; END IF; END $$;
SQL
"${migrator[@]}" --single-transaction -f migrations/0227_pass_report_restrictions.sql
"${migrator[@]}" -c BEGIN -f migrations/0228_pass_report_network.sql -c ROLLBACK
"${psql[@]}" -d pass_lifecycle <<'SQL'
DO $$ BEGIN IF to_regprocedure('content.pass_report_network_bypass(bytea,bytea,text,bigint)') IS NOT NULL
 OR content.public_pass_ready() IS DISTINCT FROM true OR content.restrictions_ready() IS DISTINCT FROM true
 THEN RAISE EXCEPTION 'Report boundary rollback changed authority'; END IF; END $$;
SQL
"${migrator[@]}" --single-transaction -f migrations/0228_pass_report_network.sql
"${migrator[@]}" -c BEGIN -f migrations/0229_pass_report_clear_history.sql -c ROLLBACK
"${psql[@]}" -d pass_lifecycle <<'SQL'
DO $$ BEGIN IF to_regclass('verification_private.pass_report_clear_events') IS NOT NULL
 OR content.public_pass_ready() IS DISTINCT FROM true OR content.report_profile_ready() IS DISTINCT FROM true
 THEN RAISE EXCEPTION 'Clear history rollback changed authority'; END IF; END $$;
SQL
"${migrator[@]}" --single-transaction -f migrations/0229_pass_report_clear_history.sql
"${migrator[@]}" -c BEGIN -f migrations/0230_pass_report_history_decisions.sql -c ROLLBACK
"${psql[@]}" -d pass_lifecycle <<'SQL'
DO $$ BEGIN IF to_regprocedure('content.pass_report_history_filtered(text,bigint,bytea,bytea,bigint,bigint)') IS NOT NULL
 OR content.public_pass_ready() IS DISTINCT FROM true OR content.restrictions_ready() IS DISTINCT FROM true
 THEN RAISE EXCEPTION 'History decision rollback changed authority'; END IF; END $$;
SQL
"${migrator[@]}" --single-transaction -f migrations/0230_pass_report_history_decisions.sql
"${psql[@]}" -d pass_lifecycle -f scripts/test-pass-report-identity.sql
"${psql[@]}" -d pass_lifecycle -f scripts/test-pass-report-quotas.sql
"${psql[@]}" -d pass_lifecycle -f scripts/test-pass-report-restrictions.sql
"${psql[@]}" -d pass_lifecycle -f scripts/test-pass-report-network.sql
"${psql[@]}" -d pass_lifecycle -f scripts/test-pass-report-clear-history.sql
"${psql[@]}" -d pass_lifecycle -f scripts/test-pass-report-history-decisions.sql
"${psql[@]}" -d pass_lifecycle -f scripts/test-pass-warning-receipts.sql
"${psql[@]}" -d pass_lifecycle -f scripts/test-pass-restrictions.sql
"${psql[@]}" -d pass_lifecycle -f scripts/test-public-pass-lifecycle.sql
"${psql[@]}" -d pass_lifecycle -f scripts/test-pass-reply-cooldowns.sql
"${psql[@]}" -d pass_lifecycle -f scripts/test-pass-post-identity.sql
"${psql[@]}" -d pass_lifecycle <<'SQL'
SET ROLE board_anonymous_owner;
INSERT INTO verification_private.pass_entitlements(token_hash,pin_hash,status,purchased_at,expires_at)
VALUES(sha256('restore-owned-pass'),repeat('owned-phc-placeholder',3),0,statement_timestamp()-interval '2 years',statement_timestamp()+interval '30 days');
SET ROLE board_verifier;
SELECT status FROM content.pass_begin_login(sha256('restore-owned-pass'),sha256('restore-peer'),sha256('restore-ticket'));
SELECT status FROM content.pass_finish_login(sha256('restore-ticket'),sha256('restore-peer'),sha256('restore-login-scope'),sha256('restore-posting-scope'),true,sha256('restore-session'),false);
RESET ROLE;
SQL
"${psql[@]}" -d pass_lifecycle <<'SQL'
SET ROLE board_migrator;
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
 VALUES('passrst','Owned identity restore','',2000,100,100,100,10);
WITH parent AS (INSERT INTO content.threads(board) VALUES('passrst') RETURNING id,board)
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
 SELECT id,board,id,'Anonymous','Owned restore','Owned private identity restore fixture' FROM parent;
SET ROLE board_anonymous_owner;
INSERT INTO verification_private.pass_posts(post_id,identity_hash)
 SELECT id,sha256('owned-restored-private-pass') FROM content.posts WHERE board='passrst';
DO $$ BEGIN
 IF has_table_privilege('board_public','verification_private.pass_posts','SELECT,INSERT,UPDATE,DELETE')
 OR has_table_privilege('board_staff','verification_private.pass_posts','SELECT,INSERT,UPDATE,DELETE')
 OR has_table_privilege('board_verifier','verification_private.pass_posts','SELECT,INSERT,UPDATE,DELETE')
 OR has_column_privilege('board_thread_transfer_owner','verification_private.pass_posts','identity_hash','UPDATE') THEN
 RAISE EXCEPTION 'Private posting membership leaked to a runtime'; END IF;
END $$;
RESET ROLE;
SQL
"${psql[@]}" -d pass_lifecycle <<'SQL'
SET ROLE board_staff;
SELECT content.admit_report('passrst',(SELECT id FROM content.posts WHERE board='passrst' ORDER BY id LIMIT 1),'Owned report restore',sha256('owned-restored-reporter'));
RESET ROLE;
INSERT INTO verification_private.pass_reports(report_id,identity_hash)
 SELECT id,sha256('owned-restored-report-pass') FROM content.reports WHERE board='passrst';
SELECT set_config('board.fixture_restore_report',(SELECT id::text FROM content.reports WHERE board='passrst'),false);
SET ROLE board_report_admission_owner;
SELECT content.pass_record_report_clears(ARRAY[current_setting('board.fixture_restore_report')::bigint],ARRAY[sha256('owned-restored-reporter')],ARRAY[NULL::uuid],clock_timestamp());
RESET ROLE;
SQL
"$pg_bin/pg_dump" -h 127.0.0.1 -p 15453 -U postgres -d pass_lifecycle --format=custom -f "$cluster/owned.dump"
"${psql[@]}" -d postgres -c 'CREATE DATABASE pass_restored OWNER board_migrator'
"$pg_bin/pg_restore" -h 127.0.0.1 -p 15453 -U postgres -d pass_restored --exit-on-error "$cluster/owned.dump"
"${psql[@]}" -d pass_restored <<'SQL'
SET ROLE board_anonymous_owner;
DO $$ BEGIN IF (SELECT count(*) FROM verification_private.pass_reports WHERE identity_hash=sha256('owned-restored-report-pass'))<>1
 OR (SELECT count(*) FROM verification_private.pass_report_clear_events WHERE identity_hash=sha256('owned-restored-report-pass') AND actor_hash=sha256('owned-restored-reporter'))<>1
 OR EXISTS(SELECT 1 FROM verification_private.pass_report_handoffs) THEN RAISE EXCEPTION 'Restored report identity differs'; END IF; END $$;
RESET ROLE;
SQL
"${psql[@]}" -d pass_restored -f scripts/test-pass-warning-receipts.sql
"${psql[@]}" -d pass_restored <<'SQL'
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Restored membership authority differs'; END IF; END $$;
SET ROLE board_anonymous_owner;
DO $$ BEGIN IF post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Restored classifier history differs'; END IF; END $$;
DO $$ BEGIN IF (SELECT count(*) FROM verification_private.pass_posts WHERE identity_hash=sha256('owned-restored-private-pass'))<>1 THEN
 RAISE EXCEPTION 'Restored private membership differs'; END IF; END $$;
RESET ROLE;
SET ROLE board_verifier;
DO $$ BEGIN IF (SELECT status FROM content.pass_session_state(sha256('restore-session')))<>'ready' THEN
 RAISE EXCEPTION 'Restored owned session failed'; END IF; END $$;
SQL
echo 'Membership rollback, state transitions, scope hold, quota, revocation and populated restore passed.'
