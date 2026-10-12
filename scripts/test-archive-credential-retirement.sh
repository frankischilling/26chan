#!/usr/bin/env bash
# Owned disposable PostgreSQL: populated upgrade and runtime policy authority.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
pg_bin=${PG_BIN:-/usr/lib/postgresql/16/bin}
runner=()
if [[ $(id -u) = 0 ]]; then runner=(runuser -u postgres --); fi
cluster=$(mktemp -d /tmp/board-archive-credential-retirement.XXXXXXXX)
started=0
cleanup() {
    status=$?
    trap - EXIT
    if [[ $started = 1 ]]; then
        "${runner[@]}" "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop >/dev/null || status=1
    fi
    [[ $cluster =~ ^/tmp/board-archive-credential-retirement\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
    [[ $(readlink -f "$cluster") = "$cluster" ]] || exit 1
    rm -rf -- "$cluster"
    exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
if [[ $(id -u) = 0 ]]; then chown postgres:postgres "$cluster"; fi
"${runner[@]}" "$pg_bin/initdb" -D "$cluster/data" --auth=trust --encoding=UTF8 --no-locale >/dev/null
connection=(-h "$cluster")
network="-c listen_addresses='' -c unix_socket_directories='$cluster'"
if [[ -n ${PG_TEST_TCP_PORT:-} ]]; then
    [[ $PG_TEST_TCP_PORT =~ ^[0-9]+$ ]] || exit 1
    connection=(-h 127.0.0.1 -p "$PG_TEST_TCP_PORT")
    network="-c listen_addresses=127.0.0.1 -c unix_socket_directories='' -p $PG_TEST_TCP_PORT"
fi
"${runner[@]}" "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" \
    -o "$network -c statement_timeout=30000 -c lock_timeout=5000" -w start >/dev/null || { cat "$cluster/server.log" >&2; exit 1; }
started=1
psql=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 "${connection[@]}")
"${runner[@]}" "${psql[@]}" -d postgres -f - < deploy/roles.sql
"${runner[@]}" "${psql[@]}" -d postgres <<'SQL'
CREATE DATABASE archive_credential_upgrade OWNER board_migrator;
REVOKE ALL ON DATABASE archive_credential_upgrade FROM PUBLIC;
GRANT CONNECT ON DATABASE archive_credential_upgrade TO board_migrator,board_public;
SQL
migrator=("${psql[@]}" -U board_migrator -d archive_credential_upgrade)
for migration in migrations/*.sql; do
    [[ $(basename "$migration") < 0091_ ]] || break
    "${migrator[@]}" --single-transaction -f - < "$migration" >/dev/null
done
"${migrator[@]}" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,staff_only,archive_retention_seconds)
VALUES ('legacy','Owned legacy archive','',1000,100,100,100,10,false,86400),
       ('legacypriv','Owned private archive','',1000,100,100,100,10,true,0);
BEGIN;
SELECT set_config('board.posting_actor',repeat('11',32),true);
INSERT INTO content.threads(id,board,sticky,undead)
VALUES (9139000,'legacy',false,false),(9139010,'legacy',true,true),
       (9139020,'legacypriv',false,false),(9139030,'legacypriv',false,false);
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES (9139000,'legacy',9139000,'Anonymous','Archived OP','Owned synthetic'),
       (9139001,'legacy',9139000,'Anonymous','','Archived reply'),
       (9139002,'legacy',9139000,'Anonymous','','Deleted archived reply'),
       (9139010,'legacy',9139010,'Anonymous','Protected active OP','Keep authority'),
       (9139011,'legacy',9139010,'Anonymous','','Active reply'),
       (9139020,'legacypriv',9139020,'Anonymous','Disabled private archive','Keep body'),
       (9139021,'legacypriv',9139020,'Anonymous','','Private archived reply'),
       (9139030,'legacypriv',9139030,'Anonymous','Private active OP','Keep authority');
COMMIT;
UPDATE content.posts SET deleted=true WHERE id=9139002;
INSERT INTO post_secrets.deletion(post_id,password_hash)
SELECT id,'owned-synthetic-secret-'||id FROM content.posts;
INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,
 created_at,network_at,address_at,environment_at,expires_at,posts,threads)
VALUES(decode(repeat('01',32),'hex'),decode(repeat('02',32),'hex'),decode(repeat('03',32),'hex'),
 decode(repeat('04',32),'hex'),1,1,1,1,4102444800,2,2);
INSERT INTO post_secrets.anonymous_posts(post_id,token_hash,password_proof)
SELECT post_id,decode(repeat('01',32),'hex'),sha256(convert_to(password_hash,'UTF8'))
FROM post_secrets.deletion WHERE post_id IN (9139000,9139010);
INSERT INTO content.reports(board,post_id,reason) VALUES('legacy',9139000,'Owned retained archive evidence');
INSERT INTO content.moderation_audit(account_id,board,target_id,action) VALUES(42,'legacy',9139000,'remove-post');
UPDATE content.threads SET archived_at=statement_timestamp()-interval '1 hour',
 archive_expires_at=statement_timestamp()+interval '1 day' WHERE id IN (9139000,9139020);
SQL
for migration in migrations/*.sql; do
    [[ $(basename "$migration") > 0090_ ]] || continue
    [[ $(basename "$migration") < 0139_ ]] || break
    [[ $(basename "$migration") != 0090_* ]] || continue
    "${migrator[@]}" --single-transaction -f - < "$migration" >/dev/null
done
"${migrator[@]}" <<'SQL'
DO $$ BEGIN
 IF (SELECT count(*) FROM post_secrets.deletion)<>8
 THEN RAISE EXCEPTION 'Legacy fixture was not preserved through the prior migrations'; END IF;
END $$;
CREATE TABLE content.credential_upgrade_witness AS
SELECT 'thread' kind,to_jsonb(t) row FROM content.threads t UNION ALL
SELECT 'post',to_jsonb(p) FROM content.posts p UNION ALL SELECT 'proof',to_jsonb(a) FROM post_secrets.anonymous_posts a
 UNION ALL SELECT 'session',to_jsonb(a) FROM post_secrets.anonymous_sessions a
 UNION ALL SELECT 'report',to_jsonb(a) FROM content.reports a
 UNION ALL SELECT 'audit',to_jsonb(a) FROM content.moderation_audit a UNION ALL
SELECT 'secret',to_jsonb(d) FROM post_secrets.deletion d
JOIN content.posts p ON p.id=d.post_id JOIN content.threads t ON t.id=p.thread_id
WHERE t.archived_at IS NULL;
SQL
"${psql[@]}" -U board_public -d archive_credential_upgrade <<'SQL'
DO $$ BEGIN
 IF content.anonymous_post_proof(decode(repeat('01',32),'hex'),'legacy',9139000) IS NULL
 OR content.anonymous_post_proof(decode(repeat('01',32),'hex'),'legacy',9139010) IS NULL
 THEN RAISE EXCEPTION 'Legacy proof positive controls are not usable'; END IF;
END $$;
SQL
# Fixed snapshots must fail before retiring any authority.
if { printf 'BEGIN ISOLATION LEVEL REPEATABLE READ;\n'; cat migrations/0139_retire_legacy_archive_credentials.sql; printf 'COMMIT;\n'; } |
    "${migrator[@]}" >"$cluster/fixed.out" 2>"$cluster/fixed.err"; then
    echo 'Archive credential retirement accepted a fixed snapshot.' >&2; exit 1
fi
grep -Fq 'Archive credential retirement requires Read Committed.' "$cluster/fixed.err"
"${migrator[@]}" -Atc 'SELECT count(*) FROM post_secrets.deletion' | grep -Fxq 8
"${migrator[@]}" <<'SQL'
CREATE SEQUENCE content.owned_legacy_retirement_attempt;
CREATE FUNCTION content.reject_owned_legacy_retirement() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF nextval('content.owned_legacy_retirement_attempt')=2 THEN
  RAISE EXCEPTION 'Owned legacy retirement failure' USING ERRCODE='23514';
 END IF;
 RETURN OLD;
END $$;
CREATE TRIGGER zz_owned_legacy_retirement BEFORE DELETE ON post_secrets.deletion
FOR EACH ROW EXECUTE FUNCTION content.reject_owned_legacy_retirement();
SQL
if "${migrator[@]}" --single-transaction -f - < migrations/0139_retire_legacy_archive_credentials.sql >"$cluster/atomic.out" 2>"$cluster/atomic.err"; then
    echo 'Archive credential retirement ignored the owned failure.' >&2; exit 1
fi
grep -Fq 'Owned legacy retirement failure' "$cluster/atomic.err"
"${migrator[@]}" -Atc 'SELECT last_value FROM content.owned_legacy_retirement_attempt' | grep -Fxq 2
"${migrator[@]}" -Atc 'SELECT count(*) FROM post_secrets.deletion' | grep -Fxq 8
"${migrator[@]}" <<'SQL'
DROP TRIGGER zz_owned_legacy_retirement ON post_secrets.deletion;
DROP FUNCTION content.reject_owned_legacy_retirement();
DROP SEQUENCE content.owned_legacy_retirement_attempt;
SQL
"${migrator[@]}" --single-transaction -f - < migrations/0139_retire_legacy_archive_credentials.sql
"${migrator[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM post_secrets.deletion d JOIN content.posts p ON p.id=d.post_id
           JOIN content.threads t ON t.board=p.board AND t.id=p.thread_id WHERE t.archived_at IS NOT NULL)
 THEN RAISE EXCEPTION 'Archived password authority survived retirement'; END IF;
 IF (SELECT count(*) FROM post_secrets.deletion)<>3
 THEN RAISE EXCEPTION 'Active password authority changed'; END IF;
 IF EXISTS((SELECT kind,row FROM content.credential_upgrade_witness EXCEPT
    (SELECT 'thread',to_jsonb(t) FROM content.threads t UNION ALL SELECT 'post',to_jsonb(p) FROM content.posts p UNION ALL SELECT 'proof',to_jsonb(a) FROM post_secrets.anonymous_posts a
 UNION ALL SELECT 'session',to_jsonb(a) FROM post_secrets.anonymous_sessions a
 UNION ALL SELECT 'report',to_jsonb(a) FROM content.reports a
 UNION ALL SELECT 'audit',to_jsonb(a) FROM content.moderation_audit a
     UNION ALL SELECT 'secret',to_jsonb(d) FROM post_secrets.deletion d))
 UNION ALL ((SELECT 'thread',to_jsonb(t) FROM content.threads t UNION ALL SELECT 'post',to_jsonb(p) FROM content.posts p UNION ALL SELECT 'proof',to_jsonb(a) FROM post_secrets.anonymous_posts a
 UNION ALL SELECT 'session',to_jsonb(a) FROM post_secrets.anonymous_sessions a
 UNION ALL SELECT 'report',to_jsonb(a) FROM content.reports a
 UNION ALL SELECT 'audit',to_jsonb(a) FROM content.moderation_audit a
     UNION ALL SELECT 'secret',to_jsonb(d) FROM post_secrets.deletion d) EXCEPT
     SELECT kind,row FROM content.credential_upgrade_witness))
 THEN RAISE EXCEPTION 'Credential retirement changed unrelated content or active secrets'; END IF;
 BEGIN
  INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES(9139000,'owned-rejected-restoration');
  RAISE EXCEPTION 'Archived authority could be restored' USING ERRCODE='ZX001';
 EXCEPTION WHEN check_violation THEN NULL; END;
END $$;
SET ROLE board_thread_transfer_owner;
DO $$ BEGIN IF NOT content.text_thread_transfer_ready() THEN RAISE EXCEPTION 'Transfer readiness lost'; END IF; END $$;
RESET ROLE;
SET ROLE board_report_snapshot_owner;
DO $$ BEGIN IF NOT content.report_snapshots_ready() THEN RAISE EXCEPTION 'Report snapshot readiness lost'; END IF; END $$;
RESET ROLE;
DROP TABLE content.credential_upgrade_witness;
SQL
"${psql[@]}" -U board_public -d archive_credential_upgrade <<'SQL'
DO $$ BEGIN
 IF content.anonymous_post_proof(decode(repeat('01',32),'hex'),'legacy',9139000) IS NOT NULL
 OR content.lock_anonymous_post_proof(decode(repeat('01',32),'hex'),'legacy',9139000) IS NOT NULL
 THEN RAISE EXCEPTION 'Retained archive membership still authorizes deletion'; END IF;
 IF content.anonymous_post_proof(decode(repeat('01',32),'hex'),'legacy',9139010) IS NULL
 THEN RAISE EXCEPTION 'Active proof was revoked'; END IF;
END $$;
SQL
# Repeating the reconciliation is harmless, including after a backup restore.
"${migrator[@]}" --single-transaction -f - < migrations/0139_retire_legacy_archive_credentials.sql
"${runner[@]}" "$pg_bin/pg_dump" "${connection[@]}" -Fc -d archive_credential_upgrade -f "$cluster/archive.dump"
"${runner[@]}" "${psql[@]}" -d postgres -c 'CREATE DATABASE archive_credential_restored OWNER board_migrator'
"${runner[@]}" "$pg_bin/pg_restore" "${connection[@]}" --exit-on-error -d archive_credential_restored "$cluster/archive.dump"
"${psql[@]}" -U board_migrator -d archive_credential_restored -Atc 'SELECT count(*) FROM post_secrets.deletion' | grep -Fxq 3
printf 'Legacy archive credentials retired without changing content or active authority; fixed snapshots and restoration were refused, and dump/restore preserved retirement.\n'
"${runner[@]}" "${psql[@]}" -d postgres -c 'CREATE DATABASE archive_credential_fresh OWNER board_migrator'
for migration in migrations/*.sql; do
    [[ $(basename "$migration") < 0140_ ]] || break
    "${psql[@]}" -U board_migrator -d archive_credential_fresh --single-transaction -f - < "$migration" >/dev/null
done
"${psql[@]}" -U board_migrator -d archive_credential_fresh -Atc 'SELECT count(*) FROM post_secrets.deletion' | grep -Fxq 0
printf 'Fresh migration chain also passed without creating archive credentials.\n'
