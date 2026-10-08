#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run against the owned disposable development cluster as root.' >&2; exit 1; }
source .local/database.env
source .local/staff.env
cluster=$(cat .local/cluster-path)
[[ $cluster =~ ^/tmp/board-postgres\.[[:alnum:]]+$ && -d $cluster && ! -L $cluster ]] || exit 1
port=${BOARD_TEST_PORT:-55432}
[[ $port =~ ^[0-9]{1,5}$ && $port -gt 0 && $port -lt 65536 ]] || exit 1
pg_bin=/usr/lib/postgresql/16/bin
admin=(runuser -u postgres -- "$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h /tmp -p "$port" -d postgres)
actual=$("${admin[@]}" -At -c 'SHOW data_directory')
[[ $actual = "$cluster" ]] || { echo 'The selected port belongs to another cluster.' >&2; exit 1; }
upgrade_db="imageboard_staff_identity_$(date +%s)_$RANDOM"
[[ $upgrade_db =~ ^imageboard_staff_identity_[0-9]+_[0-9]+$ ]] || exit 1
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
GRANT CONNECT ON DATABASE :"upgrade_db" TO board_migrator,board_public,board_auth,board_staff;
SQL
created=1
export OWNED_STAFF_IDENTITY_UPGRADE_DATABASE="$upgrade_db" OWNED_STAFF_IDENTITY_UPGRADE_PORT="$port"
owned_url() {
  OWNED_ROLE_URL_KEY="$1" python3 - <<'PY'
import os, urllib.parse
value = urllib.parse.urlsplit(os.environ[os.environ['OWNED_ROLE_URL_KEY']])
if value.hostname != '127.0.0.1' or value.port != int(os.environ['OWNED_STAFF_IDENTITY_UPGRADE_PORT']):
    raise ValueError('Owned development credential points to another cluster.')
print(urllib.parse.urlunsplit(value._replace(path='/' + os.environ['OWNED_STAFF_IDENTITY_UPGRADE_DATABASE'])))
PY
}
db=("$pg_bin/psql" "$(owned_url MIGRATION_DATABASE_URL)" -Xq -v ON_ERROR_STOP=1)
auth=("$pg_bin/psql" "$(owned_url AUTH_DATABASE_URL)" -Xq -v ON_ERROR_STOP=1)
staff=("$pg_bin/psql" "$(owned_url STAFF_DATABASE_URL)" -Xq -v ON_ERROR_STOP=1)
for migration in migrations/*.sql; do
  [[ $migration != migrations/0071_staff_capcode_identity.sql ]] || break
  "${db[@]}" --single-transaction -f "$migration"
done
"${db[@]}" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,max_authorized_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('oldident','Owned historical identity','Synthetic',2000,10000,100,100,100,10);
INSERT INTO content.threads(id,board,created_at,modified_at) VALUES(888101,'oldident','2026-01-01Z','2026-01-02Z');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(888101,'oldident',888101,'Historical <identity>','Historical subject','Historical body','2026-01-01Z');
DO $$ DECLARE actor bigint; idx integer; BEGIN
  FOR idx IN 1..3 LOOP
    INSERT INTO staff_identity.accounts(role,flags,allow_boards,deny_boards)
    VALUES('moderator',CASE WHEN idx=3 THEN ARRAY[]::text[] ELSE ARRAY['capcode','capcodename'] END,ARRAY['all'],ARRAY['j']) RETURNING id INTO actor;
    INSERT INTO staff_identity.credentials(id,account_id,credential)
      VALUES(convert_to('owned-identity-key-'||idx,'UTF8'),actor,'{}');
    INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id)
      VALUES(decode(repeat((20+idx)::text,32),'hex'),decode(repeat((30+idx)::text,32),'hex'),actor,convert_to('owned-identity-key-'||idx,'UTF8'));
  END LOOP;
END $$;
CREATE TABLE content._owned_identity_history AS
SELECT (SELECT jsonb_agg(to_jsonb(p) ORDER BY id) FROM content.posts p WHERE board='oldident') AS posts,
    (SELECT jsonb_agg(to_jsonb(a) ORDER BY id) FROM staff_identity.accounts a) AS accounts;
CREATE TABLE content._owned_identity_functions AS
SELECT oid,proowner,proacl FROM pg_proc WHERE oid IN (
    'staff_identity.issue_post_authority(bytea,bytea,bytea,integer,boolean,bigint,text,bigint,text,text,text,timestamptz)'::regprocedure,
    'staff_identity.issue_limited_post_authority(bytea,bytea,bytea,integer,boolean,bigint,text,bigint,text,text,text,timestamptz,boolean,integer,bytea,text)'::regprocedure,
    'content.consume_staff_post_authority(bytea,bigint,text,bigint,text,text,text,timestamptz)'::regprocedure,
    'content.apply_staff_capcode()'::regprocedure,'content.apply_post_trip()'::regprocedure,'content.apply_forced_anonymous()'::regprocedure);
SQL
"${auth[@]}" <<'SQL'
SELECT staff_identity.issue_post_authority(decode(repeat('41',32),'hex'),decode(repeat('21',32),'hex'),decode(repeat('31',32),'hex'),900,false,
    888102,'oldident',888101,'Legacy named staff','','Owned old proof','2026-01-01Z');
SELECT staff_identity.issue_limited_post_authority(decode(repeat('42',32),'hex'),decode(repeat('22',32),'hex'),decode(repeat('32',32),'hex'),900,false,
    888103,'oldident',888101,'Extended named staff','','Owned extended proof','2026-01-01Z',true,10000,NULL,NULL);
SELECT staff_identity.issue_post_authority(decode(repeat('43',32),'hex'),decode(repeat('23',32),'hex'),decode(repeat('33',32),'hex'),900,false,
    888104,'oldident',888101,'Old unprivileged name','','Owned unavailable proof','2026-01-01Z');
SQL
"${db[@]}" <<'SQL'
CREATE TABLE content._owned_identity_intents AS SELECT token_hash,to_jsonb(i) AS payload FROM post_secrets.staff_post_intents i;
SQL
"${db[@]}" --single-transaction -f migrations/0071_staff_capcode_identity.sql
"${db[@]}" <<'SQL'
DO $$ BEGIN
  IF (SELECT posts FROM content._owned_identity_history) IS DISTINCT FROM
      (SELECT jsonb_agg(to_jsonb(p) ORDER BY id) FROM content.posts p WHERE board='oldident')
    OR (SELECT accounts FROM content._owned_identity_history) IS DISTINCT FROM
      (SELECT jsonb_agg(to_jsonb(a) ORDER BY id) FROM staff_identity.accounts a)
    OR EXISTS(SELECT 1 FROM content._owned_identity_functions old LEFT JOIN pg_proc p ON p.oid=old.oid
      WHERE p.oid IS NULL OR p.proowner IS DISTINCT FROM old.proowner OR p.proacl IS DISTINCT FROM old.proacl)
    OR (SELECT count(*) FROM content._owned_identity_functions)<>6
    OR (SELECT count(*) FROM post_secrets.staff_post_intents)<>3
    OR EXISTS(SELECT 1 FROM content._owned_identity_intents old LEFT JOIN post_secrets.staff_post_intents i USING(token_hash)
      WHERE i.token_hash IS NULL OR old.payload IS DISTINCT FROM (to_jsonb(i)-'source_options'-'prepared_trip'-'source_name_allowed'))
    OR EXISTS(SELECT 1 FROM post_secrets.staff_post_intents WHERE source_options IS NOT NULL OR prepared_trip IS NOT NULL OR source_name_allowed IS NOT NULL) THEN
    RAISE EXCEPTION 'Identity upgrade rewrote history, permissions, proof payloads or function authority';
  END IF;
END $$;
SQL
"${staff[@]}" <<'SQL'
BEGIN;
SELECT set_config('board.staff_post_ticket',repeat('41',32),true),set_config('board.post_trip','!0123456789',true);
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(888102,'oldident',888101,'Legacy named staff','','Owned old proof','2026-01-01Z');
COMMIT;
BEGIN;
SELECT set_config('board.staff_post_ticket',repeat('42',32),true);
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(888103,'oldident',888101,'Extended named staff','','Owned extended proof','2026-01-01Z');
COMMIT;
BEGIN;
SELECT set_config('board.staff_post_ticket',repeat('43',32),true);
DO $$ BEGIN
  BEGIN
    INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
    VALUES(888104,'oldident',888101,'Old unprivileged name','','Owned unavailable proof','2026-01-01Z');
    RAISE EXCEPTION 'Legacy named proof bypassed the source permission';
  EXCEPTION WHEN SQLSTATE '28000' THEN NULL; END;
END $$;
COMMIT;
SQL
"${auth[@]}" <<'SQL'
SELECT staff_identity.issue_source_post_authority(decode(repeat('51',32),'hex'),decode(repeat('21',32),'hex'),decode(repeat('31',32),'hex'),900,false,
    888105,'oldident',888101,'','','Owned trip-only proof','2026-01-01Z',true,10000,NULL,NULL,'capcode_mod','!0123456789',true);
SQL
"${staff[@]}" <<'SQL'
BEGIN;
SELECT set_config('board.staff_post_ticket',repeat('51',32),true),set_config('board.post_trip','!0123456789',true);
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(888105,'oldident',888101,'','','Owned trip-only proof','2026-01-01Z');
COMMIT;
SQL
"${db[@]}" <<'SQL'
DO $$ BEGIN
  IF NOT EXISTS(SELECT 1 FROM content.posts WHERE id=888102 AND name='Legacy named staff' AND trip IS NULL AND capcode='mod' AND NOT staff_authorized_limits)
    OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=888103 AND name='Extended named staff' AND trip IS NULL AND capcode='mod' AND staff_authorized_limits)
    OR EXISTS(SELECT 1 FROM content.posts WHERE id=888104)
    OR NOT EXISTS(SELECT 1 FROM post_secrets.staff_post_intents WHERE post_id=888104 AND source_options IS NULL)
    OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=888105 AND name='' AND trip='!0123456789' AND capcode='mod' AND staff_authorized_limits)
    OR (SELECT count(*) FROM content.moderation_audit WHERE board='oldident' AND action='staff-post')<>3
    OR (SELECT count(*) FROM post_secrets.staff_post_intents)<>1 THEN
    RAISE EXCEPTION 'Upgraded legacy/source proofs or audit outcomes differ';
  END IF;
END $$;
SQL
cleanup
trap - EXIT
printf 'Staff identity upgrade passed: history and account permissions retained; old function identity/grants preserved; eligible old proofs consume without trip authority; missing source permission rejects; new trip-only proof binds its hash. Owned database removed.\n'
