#!/usr/bin/env bash
# Owned disposable PostgreSQL: populated upgrade and runtime policy authority.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
pg_bin=${PG_BIN:-/usr/lib/postgresql/16/bin}
runner=()
if [[ $(id -u) = 0 ]]; then runner=(runuser -u postgres --); fi
cluster=$(mktemp -d /tmp/board-verification-upgrade.XXXXXXXX)
started=0
cleanup() {
    status=$?
    trap - EXIT
    if [[ $started = 1 ]]; then
        "${runner[@]}" "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop >/dev/null || status=1
    fi
    if [[ $started = 1 ]]; then
        if "${runner[@]}" "$pg_bin/pg_ctl" -D "$cluster/data" status >/dev/null 2>&1; then exit 1; else stopped=$?; fi
        [[ $stopped = 3 ]] || exit 1
    fi
    [[ $cluster =~ ^/tmp/board-verification-upgrade\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
    [[ $(readlink -f "$cluster") = "$cluster" ]] || exit 1
    rm -rf -- "$cluster"
    exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
if [[ $(id -u) = 0 ]]; then chown postgres:postgres "$cluster"; fi
"${runner[@]}" "$pg_bin/initdb" -D "$cluster/data" --username=postgres --auth=trust --encoding=UTF8 --no-locale >/dev/null
connection=(-U postgres -h "$cluster")
network="-c listen_addresses='' -c unix_socket_directories='$cluster'"
if [[ -n ${PG_TEST_TCP_PORT:-} ]]; then
    [[ $PG_TEST_TCP_PORT =~ ^[0-9]+$ ]] || exit 1
    connection=(-U postgres -h 127.0.0.1 -p "$PG_TEST_TCP_PORT")
    network="-c listen_addresses=127.0.0.1 -c unix_socket_directories='' -p $PG_TEST_TCP_PORT"
fi
started=1
"${runner[@]}" "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" \
    -o "$network -c statement_timeout=30000 -c lock_timeout=5000" -w start >/dev/null || { cat "$cluster/server.log" >&2; exit 1; }
started=1
psql=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 "${connection[@]}")
"${runner[@]}" "${psql[@]}" -d postgres -f - < deploy/roles.sql
exec 3>&1
exec > "$cluster/qualification.log" 2>&1
trap 'printf "Verification upgrade/restore failed at line %s.\n" "$LINENO" >&3' ERR
"${runner[@]}" "${psql[@]}" -d postgres <<'SQL'
CREATE DATABASE verification_upgrade OWNER board_migrator;
REVOKE ALL ON DATABASE verification_upgrade FROM PUBLIC;
GRANT CONNECT ON DATABASE verification_upgrade TO board_migrator,board_public,board_verifier,board_mailer;
SQL
migrator=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 "${connection[@]}" -U board_migrator -d verification_upgrade)
for migration in migrations/*.sql; do
    [[ $(basename "$migration") < 0158_ ]] || break
    "${migrator[@]}" --single-transaction -f - < "$migration"
done
"${migrator[@]}" <<'SQL'
INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,created_at,network_at,address_at,environment_at,activity_at,expires_at,posts)
SELECT decode(repeat('a1',32),'hex'),decode(repeat('b2',32),'hex'),decode(repeat('c3',32),'hex'),decode(repeat('d4',32),'hex'),n-1000,n-900,n-800,n-700,n-10,n+31536000,9
FROM (SELECT extract(epoch FROM statement_timestamp())::bigint n) now;
SQL
"${migrator[@]}" -Atc "SELECT to_jsonb(s) FROM post_secrets.anonymous_sessions s ORDER BY token_hash" > "$cluster/before.json"
for migration in migrations/*.sql; do
    [[ $(basename "$migration") < 0158_ ]] && continue
    [[ $(basename "$migration") < 0174_ ]] || break
    "${migrator[@]}" --single-transaction -f - < "$migration"
    # The verifier's readiness predicate must distinguish the old two-column
    # credit result from the upgraded lifetime-aware wire contract.
    if [[ $(basename "$migration") = 0169_* ]]; then
        "${migrator[@]}" -Atc "SELECT pg_get_function_result('content.verification_credit_receipt(bytea,bytea,bytea,bytea,bytea,bytea,text,bigint,text,bytea,bigint)'::regprocedure)='TABLE(status text, remaining smallint)'" | grep -qx t
    elif [[ $(basename "$migration") = 0170_* ]]; then
        "${migrator[@]}" -Atc "SELECT pg_get_function_result('content.verification_credit_receipt(bytea,bytea,bytea,bytea,bytea,bytea,text,bigint,text,bytea,bigint)'::regprocedure)='TABLE(status text, remaining smallint, ttl_seconds integer)'" | grep -qx t
    fi
done
"${migrator[@]}" -Atc "SELECT to_jsonb(s) FROM post_secrets.anonymous_sessions s ORDER BY token_hash" > "$cluster/after.json"
cmp "$cluster/before.json" "$cluster/after.json"
"${migrator[@]}" <<'SQL'
INSERT INTO verification_private.blocked_recipients(recipient_hash) VALUES(decode(repeat('b7',32),'hex'));
INSERT INTO verification_private.peer_risk(peer_hash,score,expires_at) VALUES(decode(repeat('b8',32),'hex'),79,clock_timestamp()+interval '1 hour');
INSERT INTO verification_private.identities(email_hash) VALUES(decode(repeat('e5',32),'hex'));
INSERT INTO verification_private.browser_identities(session_hash,identity_id)
SELECT decode(repeat('a1',32),'hex'),id FROM verification_private.identities;
INSERT INTO verification_private.email_requests(id,token_hash,email_hash,recipient_hash,peer_hash,country,created_at,expires_at,captcha_failures,captcha_failure_limit)
VALUES ('12345678-1234-1234-1234-123456789abc',decode(repeat('f6',32),'hex'),decode(repeat('e5',32),'hex'),decode(repeat('17',32),'hex'),decode(repeat('28',32),'hex'),'US',statement_timestamp(),statement_timestamp()+interval '24 hours',1,3);
-- Opaque synthetic ciphertext exercises byte preservation only. Authenticated
-- encryption/decryption and delivery are covered by the service integration tests.
INSERT INTO verification_private.mail_outbox(request_id,recipient_hash,encrypted_payload,created_at,expires_at,retry_after,attempts)
VALUES ('12345678-1234-1234-1234-123456789abc',decode(repeat('17',32),'hex'),decode(repeat('39',128),'hex'),statement_timestamp(),statement_timestamp()+interval '1 hour',statement_timestamp()+interval '60 seconds',2);
SQL
"${runner[@]}" "$pg_bin/pg_dump" "${connection[@]}" -Fc -d verification_upgrade -f "$cluster/verification.dump"
"${runner[@]}" "${psql[@]}" -d postgres <<'SQL'
CREATE DATABASE verification_restored OWNER board_migrator;
REVOKE ALL ON DATABASE verification_restored FROM PUBLIC;
GRANT CONNECT ON DATABASE verification_restored TO board_migrator,board_public,board_verifier,board_mailer;
SQL
"${runner[@]}" "$pg_bin/pg_restore" "${connection[@]}" --exit-on-error --single-transaction -d verification_restored "$cluster/verification.dump"
for database in verification_upgrade verification_restored; do
    admin=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 "${connection[@]}" -U board_migrator -d "$database")
    "${admin[@]}" <<'SQL'
SET ROLE board_restriction_owner;
DO $$ BEGIN IF NOT content.restrictions_ready() THEN RAISE EXCEPTION 'Restriction catalog drift'; END IF; END $$;
RESET ROLE;
SET ROLE board_thread_transfer_owner;
DO $$ BEGIN IF NOT content.text_thread_transfer_ready() THEN RAISE EXCEPTION 'Transfer catalog drift'; END IF; END $$;
RESET ROLE;
DO $$ DECLARE role_name text; BEGIN
 FOREACH role_name IN ARRAY ARRAY['board_public','board_staff','board_auth','board_verifier','board_mailer'] LOOP
  IF has_schema_privilege(role_name,'verification_private','USAGE') OR EXISTS(
   SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
   WHERE n.nspname='verification_private' AND c.relkind='r'
   AND has_table_privilege(role_name,c.oid,'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER'))
  THEN RAISE EXCEPTION 'Runtime received private verification table authority'; END IF;
 END LOOP;
 IF NOT has_function_privilege('board_verifier','content.verification_email_state(bytea)','EXECUTE')
 OR has_function_privilege('board_public','content.verification_email_state(bytea)','EXECUTE')
 OR NOT has_function_privilege('board_mailer','content.verification_claim_mail(bytea,integer)','EXECUTE')
 OR has_function_privilege('board_verifier','content.verification_claim_mail(bytea,integer)','EXECUTE')
 THEN RAISE EXCEPTION 'Issuer or delivery grants drifted'; END IF;
END $$;
SQL
    "${runner[@]}" "${psql[@]}" -d "$database" -f - < scripts/test-challenge-tickets.sql
    "${runner[@]}" "${psql[@]}" -d "$database" -f - < scripts/test-report-challenge-history.sql
    "${runner[@]}" "${psql[@]}" -d "$database" -f - < scripts/test-report-challenge-epochs.sql
    "${runner[@]}" "${psql[@]}" -d "$database" -f - < scripts/test-protected-challenge-receipts.sql
    "${runner[@]}" "${psql[@]}" -d "$database" -f - < scripts/test-challenge-credit-profile.sql
    "${admin[@]}" -At > "$cluster/$database.fingerprint" <<'SQL'
SELECT 'session',to_jsonb(s) FROM post_secrets.anonymous_sessions s ORDER BY token_hash;
SELECT 'identity',to_jsonb(i) FROM verification_private.identities i ORDER BY id;
SELECT 'browser',to_jsonb(b) FROM verification_private.browser_identities b ORDER BY session_hash;
SELECT 'request',to_jsonb(r) FROM verification_private.email_requests r ORDER BY id;
SELECT 'outbox',to_jsonb(q) FROM verification_private.mail_outbox q ORDER BY request_id;
SELECT 'risk',to_jsonb(r) FROM verification_private.peer_risk r ORDER BY peer_hash;
SELECT 'blocked_recipient',to_jsonb(b) FROM verification_private.blocked_recipients b ORDER BY recipient_hash;
SELECT 'function',n.nspname,p.proname,pg_get_function_identity_arguments(p.oid),p.proowner::regrole,p.prosecdef,p.proconfig,p.proacl,md5(pg_get_functiondef(p.oid))
FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
WHERE n.nspname='verification_private' OR (n.nspname='content' AND p.proname LIKE 'verification_%')
ORDER BY n.nspname,p.proname,pg_get_function_identity_arguments(p.oid);
SELECT 'trigger',c.relname,t.tgname,t.tgenabled,pg_get_triggerdef(t.oid)
FROM pg_trigger t JOIN pg_class c ON c.oid=t.tgrelid JOIN pg_namespace n ON n.oid=c.relnamespace
WHERE n.nspname='verification_private' AND NOT t.tgisinternal ORDER BY c.relname,t.tgname;
SQL
 done
cmp "$cluster/verification_upgrade.fingerprint" "$cluster/verification_restored.fingerprint"
printf 'Public verification populated upgrade and restore preserve browser activity, private identity, token state, encrypted queue, grants, and sealed consumers.\n' >&3
