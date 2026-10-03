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
upgrade_db="imageboard_authorized_post_$(date +%s)_$RANDOM"
[[ $upgrade_db =~ ^imageboard_authorized_post_[0-9]+_[0-9]+$ ]] || exit 1
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
export OWNED_AUTHORIZED_UPGRADE_DATABASE="$upgrade_db"
owned_url() {
  OWNED_ROLE_URL_KEY="$1" python3 - <<'PY'
import os, urllib.parse
value = urllib.parse.urlsplit(os.environ[os.environ['OWNED_ROLE_URL_KEY']])
print(urllib.parse.urlunsplit(value._replace(path='/' + os.environ['OWNED_AUTHORIZED_UPGRADE_DATABASE'])))
PY
}
db=("$pg_bin/psql" "$(owned_url MIGRATION_DATABASE_URL)" -Xq -v ON_ERROR_STOP=1)
auth=("$pg_bin/psql" "$(owned_url AUTH_DATABASE_URL)" -Xq -v ON_ERROR_STOP=1)
staff=("$pg_bin/psql" "$(owned_url STAFF_DATABASE_URL)" -Xq -v ON_ERROR_STOP=1)
runtime=("$pg_bin/psql" "$(owned_url TEST_PUBLIC_DATABASE_URL)" -Xq -v ON_ERROR_STOP=1)
for migration in migrations/*.sql; do
  [[ $migration != migrations/0069_authorized_comment_policy.sql ]] || break
  "${db[@]}" --single-transaction -f "$migration"
done
"${db[@]}" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,comment_max_lines,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('oldlim','Owned historical budget','Synthetic',3210,26,100,100,100,10);
INSERT INTO content.threads(id,board,created_at,modified_at)
VALUES(887001,'oldlim','2026-01-01Z','2026-01-02Z');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(887001,'oldlim',887001,'Historical identity',repeat('s',400),repeat('x',16000),'2026-01-01Z');
UPDATE content.boards SET word_filter_enabled=true WHERE slug='oldlim';
BEGIN;
SELECT set_config('board.wordfilter_payload','5746303100000001ffff0000000000104f776e656420686973746f726963616c',true);
SELECT set_config('board.wordfilter_search','Owned historical',true);
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(887002,'oldlim',887001,'Historical filtered','','Owned historical','2026-01-01Z');
COMMIT;
UPDATE content.boards SET word_filter_enabled=false WHERE slug='oldlim';
DO $$ DECLARE account bigint; BEGIN
  INSERT INTO staff_identity.accounts(role) VALUES('moderator') RETURNING id INTO account;
  INSERT INTO staff_identity.credentials(id,account_id,credential)
    VALUES(convert_to('owned-upgrade-passkey','UTF8'),account,'{}');
  INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id)
    VALUES(decode(repeat('21',32),'hex'),decode(repeat('31',32),'hex'),account,convert_to('owned-upgrade-passkey','UTF8'));
END $$;
SQL
"${auth[@]}" <<'SQL'
SELECT staff_identity.issue_post_authority(decode(repeat('41',32),'hex'),
    decode(repeat('21',32),'hex'),decode(repeat('31',32),'hex'),900,false,
    887003,'oldlim',887001,'Legacy staff','','Owned legacy proof','2026-01-01Z');
SQL
"${db[@]}" <<'SQL'
CREATE TABLE public.owned_limits_posts_before AS SELECT to_jsonb(p) AS value FROM content.posts p;
CREATE TABLE public.owned_limits_threads_before AS SELECT * FROM content.threads;
CREATE TABLE public.owned_limits_boards_before AS SELECT to_jsonb(b) AS value FROM content.boards b;
CREATE TABLE public.owned_limits_intents_before AS SELECT to_jsonb(i) AS value FROM post_secrets.staff_post_intents i;
CREATE TABLE public.owned_limits_functions_before AS
SELECT oid,proowner,prosecdef,proacl::text AS acl,proconfig FROM pg_proc
WHERE oid IN ('content.consume_staff_post_authority(bytea,bigint,text,bigint,text,text,text,timestamptz)'::regprocedure,
    'content.apply_staff_capcode()'::regprocedure,'content.stamp_wordfilter_payload()'::regprocedure);
CREATE TABLE public.owned_limits_acls_before AS
SELECT oid,relacl::text AS acl FROM pg_class WHERE oid IN ('content.posts'::regclass,'content.threads'::regclass);
SQL
"${db[@]}" --single-transaction -f migrations/0069_authorized_comment_policy.sql
"${db[@]}" --single-transaction -f migrations/0070_authorized_post_bounds.sql
"${db[@]}" <<'SQL'
DO $$ BEGIN
  IF EXISTS(SELECT to_jsonb(p)-'staff_authorized_limits' FROM content.posts p EXCEPT SELECT value FROM public.owned_limits_posts_before)
    OR EXISTS(SELECT value FROM public.owned_limits_posts_before EXCEPT SELECT to_jsonb(p)-'staff_authorized_limits' FROM content.posts p)
    OR EXISTS(SELECT * FROM content.threads EXCEPT SELECT * FROM public.owned_limits_threads_before)
    OR EXISTS(SELECT * FROM public.owned_limits_threads_before EXCEPT SELECT * FROM content.threads)
    OR EXISTS(SELECT to_jsonb(b)-'max_authorized_comment_chars' FROM content.boards b EXCEPT SELECT value FROM public.owned_limits_boards_before)
    OR EXISTS(SELECT value FROM public.owned_limits_boards_before EXCEPT SELECT to_jsonb(b)-'max_authorized_comment_chars' FROM content.boards b)
    OR EXISTS(SELECT to_jsonb(i)-ARRAY['authorized_limits','comment_limit'] FROM post_secrets.staff_post_intents i EXCEPT SELECT value FROM public.owned_limits_intents_before)
    OR EXISTS(SELECT value FROM public.owned_limits_intents_before EXCEPT SELECT to_jsonb(i)-ARRAY['authorized_limits','comment_limit'] FROM post_secrets.staff_post_intents i)
    OR EXISTS(SELECT 1 FROM content.posts WHERE staff_authorized_limits)
    OR EXISTS(SELECT 1 FROM post_secrets.staff_post_intents WHERE authorized_limits OR comment_limit IS NOT NULL)
  THEN RAISE EXCEPTION 'Authorized upgrade changed historical content, clocks, operator settings or pending proofs'; END IF;
  IF EXISTS(SELECT oid,proowner,prosecdef,proacl::text,proconfig FROM pg_proc WHERE oid IN (SELECT oid FROM public.owned_limits_functions_before)
      EXCEPT SELECT * FROM public.owned_limits_functions_before)
    OR EXISTS(SELECT oid,relacl::text FROM pg_class WHERE oid IN (SELECT oid FROM public.owned_limits_acls_before)
      EXCEPT SELECT * FROM public.owned_limits_acls_before)
  THEN RAISE EXCEPTION 'Authorized upgrade changed existing function identity, owner, ACL, search path or content grants'; END IF;
  IF (SELECT count(*) FROM content.boards WHERE source_order<1000 AND max_authorized_comment_chars=10000)<>81
    OR (SELECT max_authorized_comment_chars FROM content.boards WHERE slug='j')<>50000
    OR (SELECT max_authorized_comment_chars FROM content.boards WHERE slug='oldlim')<>10000
  THEN RAISE EXCEPTION 'Authorized source budgets or synthetic default differ'; END IF;
END $$;
SQL
"${staff[@]}" <<'SQL'
BEGIN;
SELECT set_config('board.staff_post_ticket',repeat('41',32),true);
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(887003,'oldlim',887001,'Legacy staff','','Owned legacy proof','2026-01-01Z');
COMMIT;
SQL
"${auth[@]}" <<'SQL'
SELECT staff_identity.issue_limited_post_authority(decode(repeat('51',32),'hex'),
    decode(repeat('21',32),'hex'),decode(repeat('31',32),'hex'),900,false,
    887004,'oldlim',887001,'Bound moderator',repeat('s',1020),repeat(' ',39999)||'Z','2026-01-01Z',true,10000,NULL,NULL);
SQL
"${staff[@]}" <<'SQL'
BEGIN;
SELECT set_config('board.staff_post_ticket',repeat('51',32),true);
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(887004,'oldlim',887001,'Bound moderator',repeat('s',1020),repeat(' ',39999)||'Z','2026-01-01Z');
COMMIT;
SQL
"${runtime[@]}" <<'SQL'
DO $$ BEGIN
  IF NOT EXISTS(SELECT 1 FROM content.posts WHERE id=887003 AND capcode='mod' AND NOT staff_authorized_limits)
    OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=887004 AND capcode='mod' AND staff_authorized_limits
        AND octet_length(subject)=1020 AND char_length(comment)=40000)
    OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=887002 AND substring(wordfilter_payload FROM 1 FOR 4)=decode('57463031','hex') AND NOT staff_authorized_limits)
  THEN RAISE EXCEPTION 'Legacy proof, historical format or new authorized control failed'; END IF;
  PERFORM set_config('board.staff_authorized_limits','true',true);
  BEGIN
    INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
    VALUES(887005,'oldlim',887001,'Anonymous',repeat('s',401),'Forged setting');
    RAISE EXCEPTION 'Public setting enlarged the ordinary subject bound' USING ERRCODE='ZX001';
  EXCEPTION WHEN check_violation THEN NULL; END;
  BEGIN
    INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
    VALUES(887006,'oldlim',887001,'Anonymous','',repeat('x',16001));
    RAISE EXCEPTION 'Public setting enlarged the ordinary comment bound' USING ERRCODE='ZX001';
  EXCEPTION WHEN check_violation THEN NULL; END;
  BEGIN
    UPDATE content.boards SET max_authorized_comment_chars=50000 WHERE false;
    RAISE EXCEPTION 'Public changed staff budget' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SQL
"${db[@]}" <<'SQL'
DO $$ BEGIN
  IF EXISTS(SELECT 1 FROM post_secrets.staff_post_intents)
    OR (SELECT count(*) FROM content.moderation_audit WHERE board='oldlim' AND action='staff-post')<>2
    OR EXISTS(SELECT 1 FROM content.posts WHERE id IN (887005,887006))
  THEN RAISE EXCEPTION 'Upgrade controls left proofs, duplicate audit or rejected posts'; END IF;
END $$;
SQL
cleanup
echo 'Authorized-post upgrade passed: all source budgets, historical bodies/formats/clocks/settings/grants and pending ordinary proofs preserved; new proof consumed once; public settings retain ordinary bounds.'
