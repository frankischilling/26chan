#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-attachment-id-policy.XXXXXXXX)
[[ $cluster =~ ^/tmp/board-attachment-id-policy\.[[:alnum:]]+$ && -d $cluster && ! -L $cluster ]] || exit 1
started=0
cleanup() {
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null
    fi
    [[ $cluster =~ ^/tmp/board-attachment-id-policy\.[[:alnum:]]+$ && -d $cluster && ! -L $cluster ]] || exit 1
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
CREATE DATABASE attachment_id_policy_upgrade OWNER board_migrator;
REVOKE ALL ON DATABASE attachment_id_policy_upgrade FROM PUBLIC;
SQL
for migration in migrations/*.sql; do
    [[ $migration != migrations/0076_attachment_poster_id_policy.sql ]] || break
    "${db[@]}" -d attachment_id_policy_upgrade --single-transaction -c 'SET ROLE board_migrator' -f "$migration"
done
"${db[@]}" -d attachment_id_policy_upgrade <<'SQL'
SET ROLE board_migrator;
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('oldattach','Owned attachment ID policy','Synthetic',1000,100,100,100,10);
INSERT INTO content.threads(id,board) VALUES(8807601,'oldattach');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(8807601,'oldattach',8807601,'Owned historical name','','Owned historical body');
CREATE TABLE public.owned_attachment_id_posts_before AS SELECT to_jsonb(p) AS value FROM content.posts p;
CREATE TABLE public.owned_attachment_id_boards_before AS SELECT to_jsonb(b) AS value FROM content.boards b;
CREATE TABLE public.owned_attachment_id_policies_before AS SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy;
CREATE TABLE public.owned_attachment_id_table_acl_before AS SELECT oid,relowner,relacl::text AS acl FROM pg_class
    WHERE oid IN ('content.boards'::regclass,'content.posts'::regclass,'content.threads'::regclass,'post_secrets.staff_post_intents'::regclass);
CREATE TABLE public.owned_attachment_id_column_acl_before AS SELECT attrelid,attnum,attacl::text AS acl FROM pg_attribute
    WHERE attrelid='content.boards'::regclass AND attname NOT IN ('meta_board','poster_id_no_heaven');
CREATE TABLE public.owned_attachment_id_functions_before AS SELECT oid,proowner,proacl::text AS acl,prosecdef,proconfig FROM pg_proc
    WHERE oid IN ('content.apply_poster_id()'::regprocedure,'content.insert_post_attachment(bigint,text,bigint,text,text,text,text,text,boolean,timestamptz)'::regprocedure);
SET ROLE board_attachment_owner;
DO $$ BEGIN
    BEGIN
        INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
        VALUES(8807602,'oldattach',8807601,'Owned denied insert','','Owned pre-upgrade denial');
        RAISE EXCEPTION 'Missing policy grants did not reject owner insertion' USING ERRCODE='ZX001';
    EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SQL
"${db[@]}" -d attachment_id_policy_upgrade --single-transaction -c 'SET ROLE board_migrator' -f migrations/0076_attachment_poster_id_policy.sql
"${db[@]}" -d attachment_id_policy_upgrade <<'SQL'
SET ROLE board_migrator;
DO $$ DECLARE runtime text; BEGIN
    IF EXISTS(SELECT to_jsonb(p) FROM content.posts p EXCEPT SELECT value FROM public.owned_attachment_id_posts_before)
       OR EXISTS(SELECT value FROM public.owned_attachment_id_posts_before EXCEPT SELECT to_jsonb(p) FROM content.posts p)
       OR EXISTS(SELECT to_jsonb(b) FROM content.boards b EXCEPT SELECT value FROM public.owned_attachment_id_boards_before)
       OR EXISTS(SELECT value FROM public.owned_attachment_id_boards_before EXCEPT SELECT to_jsonb(b) FROM content.boards b)
       OR EXISTS(SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy EXCEPT SELECT * FROM public.owned_attachment_id_policies_before)
       OR EXISTS(SELECT * FROM public.owned_attachment_id_policies_before EXCEPT SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy)
       OR EXISTS(SELECT oid,relowner,relacl::text FROM pg_class WHERE oid IN (SELECT oid FROM public.owned_attachment_id_table_acl_before) EXCEPT SELECT * FROM public.owned_attachment_id_table_acl_before)
       OR EXISTS(SELECT * FROM public.owned_attachment_id_table_acl_before EXCEPT SELECT oid,relowner,relacl::text FROM pg_class WHERE oid IN (SELECT oid FROM public.owned_attachment_id_table_acl_before))
       OR EXISTS(SELECT attrelid,attnum,attacl::text FROM pg_attribute WHERE attrelid='content.boards'::regclass AND attname NOT IN ('meta_board','poster_id_no_heaven') EXCEPT SELECT * FROM public.owned_attachment_id_column_acl_before)
       OR EXISTS(SELECT * FROM public.owned_attachment_id_column_acl_before EXCEPT SELECT attrelid,attnum,attacl::text FROM pg_attribute WHERE attrelid='content.boards'::regclass AND attname NOT IN ('meta_board','poster_id_no_heaven'))
       OR EXISTS(SELECT oid,proowner,proacl::text,prosecdef,proconfig FROM pg_proc WHERE oid IN (SELECT oid FROM public.owned_attachment_id_functions_before) EXCEPT SELECT * FROM public.owned_attachment_id_functions_before)
       OR EXISTS(SELECT * FROM public.owned_attachment_id_functions_before EXCEPT SELECT oid,proowner,proacl::text,prosecdef,proconfig FROM pg_proc WHERE oid IN (SELECT oid FROM public.owned_attachment_id_functions_before)) THEN
        RAISE EXCEPTION 'Attachment policy upgrade changed history, privacy policy, table ACLs, unrelated column ACLs or function metadata';
    END IF;
    IF (SELECT rolcanlogin FROM pg_roles WHERE rolname='board_attachment_owner')
       OR has_table_privilege('board_attachment_owner','content.boards','SELECT')
       OR NOT has_column_privilege('board_attachment_owner','content.boards','meta_board','SELECT')
       OR NOT has_column_privilege('board_attachment_owner','content.boards','poster_id_no_heaven','SELECT')
       OR has_column_privilege('board_attachment_owner','content.boards','meta_board','UPDATE')
       OR has_column_privilege('board_attachment_owner','content.boards','poster_id_no_heaven','UPDATE')
       OR has_table_privilege('board_attachment_owner','post_secrets.staff_post_intents','SELECT') THEN
        RAISE EXCEPTION 'Attachment owner policy access exceeds its two read-only columns';
    END IF;
    FOREACH runtime IN ARRAY ARRAY['board_public','board_staff','board_auth','board_media','board_media_read','board_media_intake','board_monitor'] LOOP
        IF has_column_privilege(runtime,'content.boards','meta_board','UPDATE')
           OR has_column_privilege(runtime,'content.boards','poster_id_no_heaven','UPDATE') THEN
            RAISE EXCEPTION 'A runtime gained source policy mutation authority';
        END IF;
    END LOOP;
END $$;
BEGIN;
UPDATE content.boards SET user_ids=true WHERE slug='oldattach';
SELECT set_config('board.poster_id','Ab12+/CD',true),set_config('board.post_sage','true',true);
INSERT INTO content.threads(id,board) VALUES(8807603,'oldattach');
SET LOCAL ROLE board_attachment_owner;
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(8807603,'oldattach',8807603,'Owned new OP','','Owned attachment-owner Heaven OP'),
    (8807604,'oldattach',8807603,'Owned new reply','','Owned attachment-owner Heaven reply');
SET LOCAL ROLE board_migrator;
DO $$ BEGIN
    IF NOT EXISTS(SELECT 1 FROM content.posts WHERE id=8807603 AND poster_id='Heaven' AND json_op_poster_id='Ab12+/CD')
       OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=8807604 AND poster_id='Heaven' AND json_op_poster_id IS NULL) THEN
        RAISE EXCEPTION 'Restricted attachment owner lost saved Heaven or ordinary OP network fields';
    END IF;
END $$;
ROLLBACK;
SQL
echo 'Attachment policy upgrade preserves history and function metadata; restricted owner reads only both policy columns and inserts source Heaven OP/reply fields; runtime policy writes remain denied.'
