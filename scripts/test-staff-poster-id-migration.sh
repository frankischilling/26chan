#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-staff-poster-id.XXXXXXXX)
[[ $cluster =~ ^/tmp/board-staff-poster-id\.[[:alnum:]]+$ && -d $cluster && ! -L $cluster ]] || exit 1
started=0
cleanup() {
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null
    fi
    [[ $cluster =~ ^/tmp/board-staff-poster-id\.[[:alnum:]]+$ && -d $cluster && ! -L $cluster ]] || exit 1
    [[ $(readlink -f "$cluster") = "$cluster" ]] || exit 1
    rm -rf -- "$cluster"
}
trap cleanup EXIT
chown postgres:postgres "$cluster"
runuser -u postgres -- "$pg_bin/initdb" -D "$cluster/data" --auth=trust --encoding=UTF8 --no-locale > /dev/null
runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" \
    -o "-c listen_addresses='' -c unix_socket_directories='$cluster'" -w start > /dev/null
started=1
db=(runuser -u postgres -- "$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h "$cluster")
"${db[@]}" -d postgres -f deploy/roles.sql
"${db[@]}" -d postgres <<'SQL'
CREATE DATABASE staff_id_upgrade OWNER board_migrator;
REVOKE ALL ON DATABASE staff_id_upgrade FROM PUBLIC;
GRANT CONNECT ON DATABASE staff_id_upgrade TO board_migrator,board_public,board_auth,board_staff;
SQL
for migration in migrations/*.sql; do
    [[ $migration != migrations/0073_staff_poster_id_labels.sql ]] || break
    "${db[@]}" -d staff_id_upgrade --single-transaction -c 'SET ROLE board_migrator' -f "$migration"
done
"${db[@]}" -d staff_id_upgrade <<'SQL'
SET ROLE board_migrator;
INSERT INTO content.boards(slug,title,description,user_ids,max_comment_chars,max_authorized_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('oldstaffid','Owned historical staff IDs','Synthetic',false,2000,10000,100,100,100,10);
INSERT INTO content.threads(id,board,created_at,modified_at) VALUES(880731,'oldstaffid','2026-01-01Z','2026-01-02Z');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(880731,'oldstaffid',880731,'Historical ordinary','','Owned old ordinary','2026-01-01Z'),
    (880732,'oldstaffid',880731,'Historical badge','','Owned old badge','2026-01-01Z'),
    (880733,'oldstaffid',880731,'Historical null badge','','Owned old null badge','2026-01-01Z');
UPDATE content.posts SET poster_id='Ab12+/CD' WHERE id IN (880731,880732);
UPDATE content.posts SET capcode='mod' WHERE id=880732;
UPDATE content.posts SET capcode='admin_highlight' WHERE id=880733;
UPDATE content.boards SET user_ids=true WHERE slug='oldstaffid';
DO $$ DECLARE actor bigint; BEGIN
    INSERT INTO staff_identity.accounts(role,flags,allow_boards,deny_boards)
    VALUES('admin',ARRAY['capcode','capcodename'],ARRAY['all'],ARRAY[]::text[]) RETURNING id INTO actor;
    INSERT INTO staff_identity.credentials(id,account_id,credential) VALUES(convert_to('owned-staff-id-key','UTF8'),actor,'{}');
    INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id)
    VALUES(decode(repeat('11',32),'hex'),decode(repeat('22',32),'hex'),actor,convert_to('owned-staff-id-key','UTF8'));
END $$;
SET ROLE board_auth;
SELECT staff_identity.issue_source_post_authority(decode(repeat('33',32),'hex'),decode(repeat('11',32),'hex'),decode(repeat('22',32),'hex'),900,false,
    880734,'oldstaffid',880731,'Owned prepared staff','','Owned existing source proof','2026-01-01Z',true,10000,NULL,NULL,'capcode_admin_hl','!ozOtJW9BFA',true);
SET ROLE board_migrator;
CREATE TABLE public.owned_staff_id_posts_before AS SELECT to_jsonb(p) AS value FROM content.posts p;
CREATE TABLE public.owned_staff_id_threads_before AS SELECT to_jsonb(t) AS value FROM content.threads t;
CREATE TABLE public.owned_staff_id_intents_before AS SELECT to_jsonb(i) AS value FROM post_secrets.staff_post_intents i;
CREATE TABLE public.owned_staff_id_boards_before AS SELECT to_jsonb(b) AS value FROM content.boards b;
CREATE TABLE public.owned_staff_id_policies_before AS SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy;
CREATE TABLE public.owned_staff_id_acls_before AS SELECT oid,relowner,relacl::text AS acl FROM pg_class
    WHERE oid IN ('content.boards'::regclass,'content.posts'::regclass,'content.threads'::regclass,'content.visible_threads'::regclass,'post_secrets.staff_post_intents'::regclass);
CREATE TABLE public.owned_staff_id_functions_before AS SELECT oid,proowner,proacl::text AS acl,prosecdef,proconfig,prorettype,proargtypes FROM pg_proc
    WHERE oid IN ('content.apply_poster_id()'::regprocedure,'content.apply_staff_capcode()'::regprocedure,
    'content.consume_staff_post_authority(bytea,bigint,text,bigint,text,text,text,timestamptz)'::regprocedure);
SQL
"${db[@]}" -d staff_id_upgrade --single-transaction -c 'SET ROLE board_migrator' -f migrations/0073_staff_poster_id_labels.sql
"${db[@]}" -d staff_id_upgrade <<'SQL'
SET ROLE board_migrator;
DO $$ DECLARE pair text[]; changed boolean; runtime text; BEGIN
    FOREACH pair SLICE 1 IN ARRAY ARRAY[
        ARRAY['content.posts','public.owned_staff_id_posts_before'],
        ARRAY['content.threads','public.owned_staff_id_threads_before'],
        ARRAY['content.boards','public.owned_staff_id_boards_before'],
        ARRAY['post_secrets.staff_post_intents','public.owned_staff_id_intents_before']]
    LOOP
        EXECUTE format('SELECT EXISTS(SELECT to_jsonb(v) FROM %s v EXCEPT SELECT value FROM %s) OR EXISTS(SELECT value FROM %s EXCEPT SELECT to_jsonb(v) FROM %s v)',pair[1],pair[2],pair[2],pair[1]) INTO changed;
        IF changed THEN RAISE EXCEPTION 'Static ID upgrade changed saved rows in %',pair[1]; END IF;
    END LOOP;
    IF EXISTS(SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy EXCEPT SELECT * FROM public.owned_staff_id_policies_before)
       OR EXISTS(SELECT * FROM public.owned_staff_id_policies_before EXCEPT SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy)
       OR EXISTS(SELECT oid,relowner,relacl::text FROM pg_class WHERE oid IN (SELECT oid FROM public.owned_staff_id_acls_before) EXCEPT SELECT * FROM public.owned_staff_id_acls_before)
       OR EXISTS(SELECT * FROM public.owned_staff_id_acls_before EXCEPT SELECT oid,relowner,relacl::text FROM pg_class WHERE oid IN (SELECT oid FROM public.owned_staff_id_acls_before))
       OR EXISTS(SELECT oid,proowner,proacl::text,prosecdef,proconfig,prorettype,proargtypes FROM pg_proc WHERE oid IN (SELECT oid FROM public.owned_staff_id_functions_before) EXCEPT SELECT * FROM public.owned_staff_id_functions_before)
       OR EXISTS(SELECT * FROM public.owned_staff_id_functions_before EXCEPT SELECT oid,proowner,proacl::text,prosecdef,proconfig,prorettype,proargtypes FROM pg_proc WHERE oid IN (SELECT oid FROM public.owned_staff_id_functions_before)) THEN
        RAISE EXCEPTION 'Static ID upgrade changed privacy policies, function OIDs, owners or grants';
    END IF;
    FOREACH runtime IN ARRAY ARRAY['board_public','board_staff','board_auth','board_media','board_media_read','board_media_intake','board_monitor'] LOOP
        IF has_column_privilege(runtime,'content.posts','poster_id','INSERT,UPDATE')
           OR has_function_privilege(runtime,'content.apply_poster_id()','EXECUTE') THEN
            RAISE EXCEPTION 'A runtime role gained direct static ID authority';
        END IF;
    END LOOP;
    BEGIN
        UPDATE content.posts SET poster_id='Admin' WHERE id=880731;
        RAISE EXCEPTION 'Ordinary static ID admitted' USING ERRCODE='ZX001';
    EXCEPTION WHEN check_violation THEN NULL; END;
    BEGIN
        UPDATE content.posts SET poster_id='Admin' WHERE id=880732;
        RAISE EXCEPTION 'Mismatched badge ID admitted' USING ERRCODE='ZX001';
    EXCEPTION WHEN check_violation THEN NULL; END;
END $$;
BEGIN;
SET LOCAL ROLE board_staff;
SELECT set_config('board.staff_post_ticket',repeat('33',32),true),set_config('board.poster_id','Founder',true),
    set_config('board.poster_fingerprint',repeat('aa',32),true),set_config('board.poster_epoch',repeat('bb',32),true),
    set_config('board.post_trip','!ozOtJW9BFA',true);
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(880734,'oldstaffid',880731,'Owned prepared staff','','Owned existing source proof','2026-01-01Z');
DO $$ BEGIN
    BEGIN
        INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
        VALUES(880735,'oldstaffid',880731,'Owned prepared staff','','Owned existing source proof','2026-01-01Z');
        RAISE EXCEPTION 'Consumed staff ID proof replayed' USING ERRCODE='ZX001';
    EXCEPTION WHEN SQLSTATE '28000' THEN NULL; END;
END $$;
SET LOCAL ROLE board_migrator;
DO $$ BEGIN
    IF NOT EXISTS(SELECT 1 FROM content.posts WHERE id=880734 AND capcode='admin_highlight' AND poster_id='Admin' AND trip='!ozOtJW9BFA' AND name='Owned prepared staff')
       OR EXISTS(SELECT 1 FROM post_secrets.staff_post_intents)
       OR EXISTS(SELECT 1 FROM post_secrets.poster_contexts) THEN
        RAISE EXCEPTION 'Existing proof lost its identity or invented a staff network context';
    END IF;
END $$;
ROLLBACK;
BEGIN;
SET LOCAL ROLE board_public;
SELECT set_config('board.poster_id','Admin',true),set_config('board.staff_is_admin','true',true),set_config('board.staff_post_ticket',repeat('33',32),true);
DO $$ BEGIN
    BEGIN
        INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
        VALUES(880736,'oldstaffid',880731,'Owned forged staff ID','','Owned public label forgery');
        RAISE EXCEPTION 'Public runtime forged a static staff ID' USING ERRCODE='ZX001';
    EXCEPTION WHEN check_violation THEN NULL; END;
END $$;
ROLLBACK;
SQL
echo 'Populated static staff ID upgrade preserves historical rows, source proofs, privacy policies, owners and grants; proof consumption and forgery controls pass.'
