#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-ordinary-staff.XXXXXXXX)
[[ $cluster =~ ^/tmp/board-ordinary-staff\.[[:alnum:]]+$ && -d $cluster && ! -L $cluster ]] || exit 1
started=0
cleanup() {
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null
    fi
    [[ $cluster =~ ^/tmp/board-ordinary-staff\.[[:alnum:]]+$ && -d $cluster && ! -L $cluster ]] || exit 1
    [[ $(readlink -f "$cluster") = "$cluster" ]] || exit 1
    rm -rf -- "$cluster"
}
trap cleanup EXIT
chown postgres:postgres "$cluster"
runuser -u postgres -- "$pg_bin/initdb" -D "$cluster/data" --auth=trust --encoding=UTF8 --no-locale > /dev/null
runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" \
    -o "-c listen_addresses='' -c unix_socket_directories='$cluster'" -w start > /dev/null
started=1
psql=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h "$cluster")
runuser -u postgres -- "${psql[@]}" -d postgres -f deploy/roles.sql
runuser -u postgres -- "${psql[@]}" -d postgres <<'SQL'
CREATE DATABASE ordinary_staff_upgrade OWNER board_migrator;
REVOKE ALL ON DATABASE ordinary_staff_upgrade FROM PUBLIC;
GRANT CONNECT ON DATABASE ordinary_staff_upgrade TO board_migrator,board_public,board_auth,board_staff;
-- Logins exist only in this owned trust-authenticated qualification cluster.
ALTER ROLE board_auth LOGIN;
ALTER ROLE board_staff LOGIN;
SQL
migrator=("${psql[@]}" -U board_migrator -d ordinary_staff_upgrade)
auth=("${psql[@]}" -U board_auth -d ordinary_staff_upgrade)
staff=("${psql[@]}" -U board_staff -d ordinary_staff_upgrade)
for migration in migrations/*.sql; do
    [[ $migration != migrations/0077_ordinary_staff_posts.sql ]] || break
    "${migrator[@]}" --single-transaction -f "$migration"
done
"${migrator[@]}" <<'SQL'
INSERT INTO content.boards(slug,title,description,user_ids,max_comment_chars,max_authorized_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('oldplain','Owned ordinary staff upgrade','Synthetic',false,2000,10000,100,100,100,10);
INSERT INTO content.threads(id,board,created_at,modified_at) VALUES(880771,'oldplain','2026-01-01Z','2026-01-02Z');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(880771,'oldplain',880771,'Historical ordinary','','Owned old ordinary','2026-01-01Z'),
    (880772,'oldplain',880771,'Historical badge','','Owned old badge','2026-01-01Z');
UPDATE content.posts SET poster_id='Ab12+/CD' WHERE id=880771;
UPDATE content.posts SET capcode='mod',poster_id='Mod' WHERE id=880772;
UPDATE content.boards SET user_ids=true WHERE slug='oldplain';
INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES(880771,'owned-historical-derived-hash');
INSERT INTO post_secrets.op_peers(thread_id,peer) VALUES(880771,'192.0.2.77');
DO $$ DECLARE actor bigint; BEGIN
    INSERT INTO staff_identity.accounts(role,flags,allow_boards,deny_boards)
    VALUES('admin',ARRAY['capcode','capcodename'],ARRAY['all'],ARRAY[]::text[]) RETURNING id INTO actor;
    INSERT INTO staff_identity.credentials(id,account_id,credential) VALUES(convert_to('owned-ordinary-upgrade-key','UTF8'),actor,'{}');
    INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id)
    VALUES(decode(repeat('11',32),'hex'),decode(repeat('22',32),'hex'),actor,convert_to('owned-ordinary-upgrade-key','UTF8'));
END $$;
SQL
"${auth[@]}" <<'SQL'
SELECT staff_identity.issue_source_post_authority(decode(repeat('33',32),'hex'),decode(repeat('11',32),'hex'),decode(repeat('22',32),'hex'),900,false,
    880773,'oldplain',880771,'Owned prepared staff','','Owned existing source proof','2026-01-01Z',true,10000,NULL,NULL,'capcode_admin_hl','!ozOtJW9BFA',true);
SQL
"${migrator[@]}" <<'SQL'
CREATE TABLE public.owned_ordinary_posts_before AS SELECT to_jsonb(p) AS value FROM content.posts p;
CREATE TABLE public.owned_ordinary_threads_before AS SELECT to_jsonb(t) AS value FROM content.threads t;
CREATE TABLE public.owned_ordinary_intents_before AS SELECT to_jsonb(i) AS value FROM post_secrets.staff_post_intents i;
CREATE TABLE public.owned_ordinary_boards_before AS SELECT to_jsonb(b) AS value FROM content.boards b;
CREATE TABLE public.owned_ordinary_deletion_before AS SELECT to_jsonb(d) AS value FROM post_secrets.deletion d;
CREATE TABLE public.owned_ordinary_peers_before AS SELECT to_jsonb(p) AS value FROM post_secrets.op_peers p;
CREATE TABLE public.owned_ordinary_policies_before AS SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy;
CREATE TABLE public.owned_ordinary_consumer_before AS SELECT oid,proowner,prosecdef,proconfig,prosrc FROM pg_proc
    WHERE oid='content.consume_staff_post_authority(bytea,bigint,text,bigint,text,text,text,timestamptz)'::regprocedure;
SQL
"${migrator[@]}" --single-transaction -f migrations/0077_ordinary_staff_posts.sql
"${migrator[@]}" <<'SQL'
DO $$ DECLARE pair text[]; changed boolean; runtime text; BEGIN
    FOREACH pair SLICE 1 IN ARRAY ARRAY[
        ARRAY['content.posts','public.owned_ordinary_posts_before'],
        ARRAY['content.threads','public.owned_ordinary_threads_before'],
        ARRAY['content.boards','public.owned_ordinary_boards_before'],
        ARRAY['post_secrets.deletion','public.owned_ordinary_deletion_before'],
        ARRAY['post_secrets.op_peers','public.owned_ordinary_peers_before']]
    LOOP
        EXECUTE format('SELECT EXISTS(SELECT to_jsonb(v) FROM %s v EXCEPT SELECT value FROM %s) OR EXISTS(SELECT value FROM %s EXCEPT SELECT to_jsonb(v) FROM %s v)',pair[1],pair[2],pair[2],pair[1]) INTO changed;
        IF changed THEN RAISE EXCEPTION 'Ordinary staff upgrade changed historical rows in %',pair[1]; END IF;
    END LOOP;
    IF EXISTS(SELECT to_jsonb(i)-ARRAY['ordinary','ordinary_context','ordinary_policy'] FROM post_secrets.staff_post_intents i EXCEPT SELECT value FROM public.owned_ordinary_intents_before)
       OR EXISTS(SELECT value FROM public.owned_ordinary_intents_before EXCEPT SELECT to_jsonb(i)-ARRAY['ordinary','ordinary_context','ordinary_policy'] FROM post_secrets.staff_post_intents i)
       OR EXISTS(SELECT 1 FROM post_secrets.staff_post_intents WHERE ordinary OR ordinary_context IS NOT NULL OR ordinary_policy IS NOT NULL) THEN
        RAISE EXCEPTION 'Ordinary staff upgrade changed an existing badge proof';
    END IF;
    IF EXISTS(SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy EXCEPT SELECT * FROM public.owned_ordinary_policies_before)
       OR EXISTS(SELECT * FROM public.owned_ordinary_policies_before EXCEPT SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy)
       OR NOT EXISTS(SELECT 1 FROM pg_proc p JOIN public.owned_ordinary_consumer_before old USING(oid)
           WHERE p.oid='content.consume_badged_post_authority(bytea,bigint,text,bigint,text,text,text,timestamptz)'::regprocedure
           AND p.proowner=old.proowner AND p.prosecdef=old.prosecdef AND p.proconfig IS NOT DISTINCT FROM old.proconfig AND p.prosrc=old.prosrc) THEN
        RAISE EXCEPTION 'Ordinary staff upgrade changed privacy policies or the retained badge consumer';
    END IF;
    FOREACH runtime IN ARRAY ARRAY['board_staff','board_auth'] LOOP
        IF has_table_privilege(runtime,'post_secrets.op_peers','SELECT,INSERT,UPDATE,DELETE')
           OR has_table_privilege(runtime,'post_secrets.op_replies','SELECT,INSERT,UPDATE,DELETE')
           OR has_table_privilege(runtime,'post_secrets.deletion','SELECT,INSERT,UPDATE,DELETE')
           OR has_table_privilege(runtime,'post_secrets.staff_post_intents','SELECT,INSERT,UPDATE,DELETE') THEN
            RAISE EXCEPTION 'A runtime gained bulk private posting authority';
        END IF;
    END LOOP;
    IF EXISTS(SELECT 1 FROM pg_roles WHERE rolname='board_staff_post_owner'
        AND (rolcanlogin OR rolsuper OR rolcreatedb OR rolcreaterole OR rolbypassrls))
       OR has_function_privilege('board_staff','content.consume_badged_post_authority(bytea,bigint,text,bigint,text,text,text,timestamptz)','EXECUTE') THEN
        RAISE EXCEPTION 'Private badge consumer ownership or access changed';
    END IF;
END $$;
SQL
"${staff[@]}" <<'SQL'
BEGIN;
SELECT set_config('board.staff_post_ticket',repeat('33',32),true),set_config('board.post_trip','!ozOtJW9BFA',true),
    set_config('board.staff_ordinary_post','true',true),set_config('board.peer','192.0.2.77',true),
    set_config('board.deletion_hash','owned-forged-derived-hash',true),set_config('board.poster_fingerprint',repeat('aa',32),true),
    set_config('board.poster_epoch',repeat('bb',32),true);
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(880773,'oldplain',880771,'Owned prepared staff','','Owned existing source proof','2026-01-01Z');
DO $$ BEGIN
    BEGIN
        INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
        VALUES(880774,'oldplain',880771,'Owned prepared staff','','Owned existing source proof','2026-01-01Z');
        RAISE EXCEPTION 'Consumed badge proof replayed' USING ERRCODE='ZX001';
    EXCEPTION WHEN SQLSTATE '28000' THEN NULL; END;
END $$;
COMMIT;
SQL
"${migrator[@]}" <<'SQL'
DO $$ BEGIN
    IF NOT EXISTS(SELECT 1 FROM content.posts WHERE id=880773 AND capcode='admin_highlight' AND poster_id='Admin'
        AND trip='!ozOtJW9BFA' AND name='Owned prepared staff' AND staff_authorized_limits)
       OR EXISTS(SELECT 1 FROM post_secrets.staff_post_intents)
       OR EXISTS(SELECT 1 FROM post_secrets.poster_contexts WHERE post_id=880773)
       OR EXISTS(SELECT 1 FROM post_secrets.deletion WHERE post_id=880773)
       OR EXISTS(SELECT 1 FROM post_secrets.op_replies WHERE post_id=880773)
       OR (SELECT count(*) FROM content.moderation_audit WHERE board='oldplain' AND action='staff-post')<>1 THEN
        RAISE EXCEPTION 'Retained badge proof lost its identity or invented ordinary secrets';
    END IF;
END $$;
SQL
echo 'Ordinary staff upgrade passed: historical rows and badge proofs preserved; retained proof consumes once; forged ordinary markers create no private records; policies and runtime private-table boundaries retained.'
