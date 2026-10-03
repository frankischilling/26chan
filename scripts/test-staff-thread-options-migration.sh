#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-staff-options.XXXXXXXX)
started=0
cleanup() {
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null
        started=0
    fi
    [[ $cluster =~ ^/tmp/board-staff-options\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
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
CREATE DATABASE staff_options_upgrade OWNER board_migrator;
REVOKE ALL ON DATABASE staff_options_upgrade FROM PUBLIC;
GRANT CONNECT ON DATABASE staff_options_upgrade TO board_migrator,board_public,board_staff,board_auth,
    board_media,board_media_read,board_media_intake,board_monitor;
-- These runtime logins exist only in the private synthetic fixture.
ALTER ROLE board_staff LOGIN;
ALTER ROLE board_auth LOGIN;
ALTER ROLE board_media_read LOGIN;
ALTER ROLE board_media_intake LOGIN;
ALTER ROLE board_monitor LOGIN;
SQL
migrator=("${psql[@]}" -U board_migrator -d staff_options_upgrade)
for migration in migrations/*.sql; do
    [[ $migration != migrations/0079_staff_thread_options.sql ]] || break
    "${migrator[@]}" --single-transaction -f "$migration"
done
"${migrator[@]}" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,archive_retention_seconds)
VALUES('oldoption','Owned options upgrade','Synthetic',1000,100,100,100,10,3600);
INSERT INTO content.threads(id,board,permaage,undead,created_at,modified_at)
VALUES(8807901,'oldoption',true,false,'2026-01-01Z','2026-01-02Z'),
    (8807902,'oldoption',false,true,'2026-01-01Z','2026-01-02Z');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(8807901,'oldoption',8807901,'Owned','Owned options','Owned root','2026-01-01Z'),
    (8807902,'oldoption',8807902,'Owned','Owned Undead','Owned root','2026-01-01Z');
INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES(8807901,'owned-options-upgrade-hash');
INSERT INTO content.moderation_audit(account_id,board,target_id,action)
VALUES(42,'oldoption',8807901,'permaage'),(42,'oldoption',8807902,'spoiler');
CREATE TABLE public.owned_options_rows_before AS
    SELECT 'boards' AS relation,to_jsonb(b) AS value FROM content.boards b
    UNION ALL SELECT 'posts',to_jsonb(p) FROM content.posts p
    UNION ALL SELECT 'threads',to_jsonb(t) FROM content.threads t
    UNION ALL SELECT 'audit',to_jsonb(a) FROM content.moderation_audit a
    UNION ALL SELECT 'deletion',to_jsonb(d) FROM post_secrets.deletion d;
CREATE VIEW public.owned_options_rows_after AS
    SELECT 'boards' AS relation,to_jsonb(b) AS value FROM content.boards b
    UNION ALL SELECT 'posts',to_jsonb(p) FROM content.posts p
    UNION ALL SELECT 'threads',to_jsonb(t) FROM content.threads t
    UNION ALL SELECT 'audit',to_jsonb(a) FROM content.moderation_audit a
    UNION ALL SELECT 'deletion',to_jsonb(d) FROM post_secrets.deletion d;
CREATE TABLE public.owned_options_policies_before AS SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy;
CREATE TABLE public.owned_options_functions_before AS SELECT p.oid,p.proowner,p.proacl::text AS acl,p.prosecdef,p.proconfig,p.prosrc
    FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname<>'pg_catalog' AND n.nspname<>'information_schema';
CREATE TABLE public.owned_options_tables_before AS SELECT oid,relowner,relacl::text AS acl FROM pg_class
    WHERE oid IN ('content.threads'::regclass,'content.posts'::regclass,'content.moderation_audit'::regclass);
CREATE TABLE public.owned_options_columns_before AS SELECT attnum,attname,attacl::text AS acl FROM pg_attribute
    WHERE attrelid='content.threads'::regclass AND attname<>'undead';
SQL
"${migrator[@]}" --single-transaction -f migrations/0079_staff_thread_options.sql
"${migrator[@]}" <<'SQL'
DO $$ DECLARE v_role text; BEGIN
  IF EXISTS(TABLE public.owned_options_rows_before EXCEPT TABLE public.owned_options_rows_after)
     OR EXISTS(TABLE public.owned_options_rows_after EXCEPT TABLE public.owned_options_rows_before)
     OR EXISTS(SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy EXCEPT TABLE public.owned_options_policies_before)
     OR EXISTS(TABLE public.owned_options_policies_before EXCEPT SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy)
     OR EXISTS(SELECT p.oid,p.proowner,p.proacl::text,p.prosecdef,p.proconfig,p.prosrc FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname<>'pg_catalog' AND n.nspname<>'information_schema' EXCEPT TABLE public.owned_options_functions_before)
     OR EXISTS(TABLE public.owned_options_functions_before EXCEPT SELECT p.oid,p.proowner,p.proacl::text,p.prosecdef,p.proconfig,p.prosrc FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname<>'pg_catalog' AND n.nspname<>'information_schema')
     OR EXISTS(SELECT oid,relowner,relacl::text FROM pg_class WHERE oid IN(SELECT oid FROM public.owned_options_tables_before) EXCEPT TABLE public.owned_options_tables_before)
     OR EXISTS(TABLE public.owned_options_tables_before EXCEPT SELECT oid,relowner,relacl::text FROM pg_class WHERE oid IN(SELECT oid FROM public.owned_options_tables_before))
     OR EXISTS(SELECT attnum,attname,attacl::text FROM pg_attribute WHERE attrelid='content.threads'::regclass AND attname<>'undead' EXCEPT TABLE public.owned_options_columns_before)
     OR EXISTS(TABLE public.owned_options_columns_before EXCEPT SELECT attnum,attname,attacl::text FROM pg_attribute WHERE attrelid='content.threads'::regclass AND attname<>'undead') THEN
    RAISE EXCEPTION 'Thread options upgrade changed historical rows, privacy, functions or other grants';
  END IF;
  FOREACH v_role IN ARRAY ARRAY['board_public','board_staff','board_auth','board_media','board_media_read','board_media_intake','board_monitor'] LOOP
    IF has_column_privilege(v_role,'content.threads','undead','UPDATE') IS DISTINCT FROM (v_role='board_staff') THEN
      RAISE EXCEPTION 'Undead mutation is not confined to staff';
    END IF;
  END LOOP;
END $$;
SQL
for role in board_public board_staff board_auth board_media board_media_read board_media_intake board_monitor; do
    "${psql[@]}" -U "$role" -d staff_options_upgrade <<'SQL'
DO $$ BEGIN
  IF current_user='board_staff' THEN
    UPDATE content.threads SET undead=true WHERE false;
  ELSE
    BEGIN UPDATE content.threads SET undead=true WHERE false;
      RAISE EXCEPTION 'Non-staff runtime wrote Undead' USING ERRCODE='ZX001';
    EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  END IF;
END $$;
SQL
done
"${psql[@]}" -U board_staff -d staff_options_upgrade <<'SQL'
BEGIN;
UPDATE content.threads SET undead=true WHERE board='oldoption' AND id=8807901;
INSERT INTO content.moderation_audit(account_id,board,target_id,action)
VALUES(42,'oldoption',8807901,'undead'),(42,'oldoption',8807901,'unundead');
DO $$ BEGIN
  IF NOT (SELECT undead AND permaage FROM content.threads WHERE id=8807901) THEN
    RAISE EXCEPTION 'Staff action changed another thread option';
  END IF;
END $$;
ROLLBACK;
SQL
cleanup
trap - EXIT
printf 'Staff options populated upgrade passed: historical values, privacy, functions and other grants preserved; all seven runtime connections qualified. Private cluster removed.\n'
