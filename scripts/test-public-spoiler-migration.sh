#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-public-spoilers.XXXXXXXX)
started=0
cleanup() {
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null
        started=0
    fi
    [[ $cluster =~ ^/tmp/board-public-spoilers\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
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
CREATE DATABASE public_spoiler_upgrade OWNER board_migrator;
REVOKE ALL ON DATABASE public_spoiler_upgrade FROM PUBLIC;
GRANT CONNECT ON DATABASE public_spoiler_upgrade TO board_migrator,board_public,board_staff,board_auth,
    board_media,board_media_read,board_media_intake,board_monitor;
ALTER ROLE board_staff LOGIN;
ALTER ROLE board_auth LOGIN;
ALTER ROLE board_media_read LOGIN;
ALTER ROLE board_media_intake LOGIN;
ALTER ROLE board_monitor LOGIN;
SQL
migrator=("${psql[@]}" -U board_migrator -d public_spoiler_upgrade)
for migration in migrations/*.sql; do
    [[ $migration != migrations/0080_public_post_spoilers.sql ]] || break
    "${migrator[@]}" --single-transaction -f "$migration"
done
"${migrator[@]}" <<'SQL'
INSERT INTO content.boards(slug,title,description,comment_spoiler_cleanup,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,image_limit)
VALUES('oldson','Owned spoiler upgrade','Synthetic',true,1000,100,100,100,10,100),
      ('oldsoff','Owned disabled upgrade','Synthetic',false,1000,100,100,100,10,100);
INSERT INTO content.threads(id,board,created_at,modified_at,undead)
VALUES(8808001,'oldson','2026-01-01Z','2026-01-02Z',true),(8808002,'oldsoff','2026-01-01Z','2026-01-02Z',false);
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at,image_spoiler)
VALUES(8808001,'oldson',8808001,'Owned','SPOILER<>User literal','Owned body','2026-01-01Z',true),
      (8808002,'oldsoff',8808002,'Owned','Historical disabled','Owned body','2026-01-01Z',true);
INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES(8808001,'owned-upgrade-hash');
INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at)
VALUES(repeat('a',32),repeat('a',32),repeat('a',32),repeat('a',64),100,500,300,'approved',clock_timestamp());
INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler,file_deleted)
VALUES(8808002,repeat('a',32),repeat('a',32),'owned-spoiler.png',100,500,300,true,true);
INSERT INTO content.moderation_audit(account_id,board,target_id,action)
VALUES(42,'oldson',8808001,'undead'),(42,'oldsoff',8808002,'spoiler');
CREATE VIEW public.owned_spoiler_rows AS
    SELECT 'boards' AS relation,to_jsonb(b) AS value FROM content.boards b
    UNION ALL SELECT 'posts',to_jsonb(p) FROM content.posts p
    UNION ALL SELECT 'threads',to_jsonb(t) FROM content.threads t
    UNION ALL SELECT 'media',to_jsonb(m) FROM content.post_media m
    UNION ALL SELECT 'assets',to_jsonb(a) FROM media.assets a
    UNION ALL SELECT 'audit',to_jsonb(a) FROM content.moderation_audit a
    UNION ALL SELECT 'deletion',to_jsonb(d) FROM post_secrets.deletion d;
CREATE TABLE public.owned_spoiler_rows_before AS TABLE public.owned_spoiler_rows;
CREATE TABLE public.owned_spoiler_policies_before AS SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy;
CREATE TABLE public.owned_spoiler_functions_before AS SELECT p.oid,p.proowner,p.proacl::text AS acl,p.prosecdef,p.proconfig,p.prosrc
    FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname<>'pg_catalog' AND n.nspname<>'information_schema';
CREATE TABLE public.owned_spoiler_tables_before AS SELECT c.oid,c.relowner,c.relacl::text AS acl FROM pg_class c
    JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname IN ('content','media','post_secrets','staff_identity');
CREATE TABLE public.owned_spoiler_columns_before AS SELECT a.attrelid,a.attnum,a.attname,a.attacl::text AS acl FROM pg_attribute a
    WHERE a.attrelid IN(SELECT oid FROM public.owned_spoiler_tables_before);
SQL
"${migrator[@]}" --single-transaction -f migrations/0080_public_post_spoilers.sql
"${migrator[@]}" <<'SQL'
DO $$ BEGIN
  IF EXISTS(TABLE public.owned_spoiler_rows_before EXCEPT TABLE public.owned_spoiler_rows)
     OR EXISTS(TABLE public.owned_spoiler_rows EXCEPT TABLE public.owned_spoiler_rows_before)
     OR EXISTS(SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy EXCEPT TABLE public.owned_spoiler_policies_before)
     OR EXISTS(TABLE public.owned_spoiler_policies_before EXCEPT SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy)
     OR EXISTS(SELECT p.oid,p.proowner,p.proacl::text,p.prosecdef,p.proconfig,p.prosrc FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
         WHERE n.nspname<>'pg_catalog' AND n.nspname<>'information_schema'
           AND p.oid NOT IN('content.initial_public_image_spoiler()'::regprocedure,'content.initial_attachment_spoiler()'::regprocedure)
         EXCEPT TABLE public.owned_spoiler_functions_before)
     OR EXISTS(TABLE public.owned_spoiler_functions_before EXCEPT SELECT p.oid,p.proowner,p.proacl::text,p.prosecdef,p.proconfig,p.prosrc FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname<>'pg_catalog' AND n.nspname<>'information_schema')
     OR EXISTS(SELECT oid,relowner,relacl::text FROM pg_class WHERE oid IN(SELECT oid FROM public.owned_spoiler_tables_before) EXCEPT TABLE public.owned_spoiler_tables_before)
     OR EXISTS(TABLE public.owned_spoiler_tables_before EXCEPT SELECT oid,relowner,relacl::text FROM pg_class WHERE oid IN(SELECT oid FROM public.owned_spoiler_tables_before))
     OR EXISTS(SELECT attrelid,attnum,attname,attacl::text FROM pg_attribute WHERE attrelid IN(SELECT oid FROM public.owned_spoiler_tables_before) EXCEPT TABLE public.owned_spoiler_columns_before)
     OR EXISTS(TABLE public.owned_spoiler_columns_before EXCEPT SELECT attrelid,attnum,attname,attacl::text FROM pg_attribute WHERE attrelid IN(SELECT oid FROM public.owned_spoiler_tables_before)) THEN
    RAISE EXCEPTION 'Public spoiler upgrade changed historical rows, privacy, existing functions or grants';
  END IF;
  IF EXISTS(SELECT 1 FROM pg_proc p JOIN pg_roles r ON r.oid=p.proowner
      WHERE p.oid IN('content.initial_public_image_spoiler()'::regprocedure,'content.initial_attachment_spoiler()'::regprocedure)
        AND (r.rolname<>'board_migrator' OR p.prosecdef OR p.proconfig IS DISTINCT FROM ARRAY['search_path=pg_catalog, pg_temp'])) THEN
    RAISE EXCEPTION 'Initial spoiler trigger authority differs';
  END IF;
END $$;
SQL
for role in board_public board_staff board_auth board_media board_media_read board_media_intake board_monitor; do
    "${psql[@]}" -U "$role" -d public_spoiler_upgrade <<'SQL'
DO $$ BEGIN
  BEGIN UPDATE content.posts SET image_spoiler=true WHERE false;
    RAISE EXCEPTION 'Runtime wrote spoiler state' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  BEGIN INSERT INTO content.posts(id,board,thread_id,name,subject,comment,image_spoiler)
      SELECT 1,'oldson',8808001,'Owned','','Owned',true WHERE false;
    RAISE EXCEPTION 'Runtime inserted spoiler column' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  BEGIN PERFORM content.initial_public_image_spoiler();
    RAISE EXCEPTION 'Runtime executed initial post trigger' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  BEGIN PERFORM content.initial_attachment_spoiler();
    RAISE EXCEPTION 'Runtime executed initial attachment trigger' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SQL
done
"${psql[@]}" -U board_public -d public_spoiler_upgrade <<'SQL'
BEGIN;
SELECT set_config('board.post_image_spoiler','true',true);
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(8808003,'oldson',8808001,'Owned','New enabled','Owned new reply',clock_timestamp()),
      (8808004,'oldsoff',8808002,'Owned','New disabled','Owned new reply',clock_timestamp());
DO $$ BEGIN
  IF NOT (SELECT image_spoiler FROM content.posts WHERE id=8808003)
      OR (SELECT image_spoiler FROM content.posts WHERE id=8808004) THEN
    RAISE EXCEPTION 'Real public insert did not follow board spoiler policy';
  END IF;
END $$;
ROLLBACK;
SQL
cleanup
trap - EXIT
printf 'Public spoiler populated upgrade passed: history, privacy, existing functions and all grants preserved; seven runtime logins denied state/trigger authority, real public inserts followed policy. Private cluster removed.\n'
