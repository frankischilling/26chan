#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-source-flags.XXXXXXXX)
started=0
cleanup() {
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null
        started=0
    fi
    [[ $cluster =~ ^/tmp/board-source-flags\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
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
CREATE DATABASE source_flag_upgrade OWNER board_migrator;
REVOKE ALL ON DATABASE source_flag_upgrade FROM PUBLIC;
GRANT CONNECT ON DATABASE source_flag_upgrade TO board_migrator,board_public,board_staff,board_auth,
    board_media,board_media_read,board_media_intake,board_monitor;
ALTER ROLE board_staff LOGIN;
ALTER ROLE board_auth LOGIN;
ALTER ROLE board_media LOGIN;
ALTER ROLE board_media_read LOGIN;
ALTER ROLE board_media_intake LOGIN;
ALTER ROLE board_monitor LOGIN;
SQL
migrator=("${psql[@]}" -U board_migrator -d source_flag_upgrade)
for migration in migrations/*.sql; do
    [[ $migration != migrations/0083_source_board_flags.sql ]] || break
    "${migrator[@]}" --single-transaction -f "$migration"
done
"${migrator[@]}" <<'SQL'
UPDATE content.boards SET comment_spoiler_cleanup=false WHERE slug='a';
INSERT INTO content.boards(slug,title,description,comment_spoiler_cleanup,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('owncustom','Owned custom-count upgrade','Synthetic',true,1000,100,100,100,10);
INSERT INTO content.threads(id,board,created_at,modified_at,undead)
VALUES(8810001,'owncustom','2026-01-01Z','2026-01-02Z',true);
UPDATE content.boards SET board_flags=ARRAY['CM'] WHERE slug='owncustom';
SELECT set_config('board.flag','CM',false);
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at,image_spoiler)
VALUES(8810001,'owncustom',8810001,'Owned','Historical custom metadata','Owned body','2026-01-01Z',true);
SELECT set_config('board.flag','',false);
INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES(8810001,'owned-custom-upgrade-hash');
INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at)
VALUES(repeat('b',32),repeat('b',32),repeat('b',32),repeat('b',64),100,500,300,'approved',clock_timestamp());
INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler,file_deleted)
VALUES(8810001,repeat('b',32),repeat('b',32),'owned-custom.png',100,500,300,true,true);
INSERT INTO content.moderation_audit(account_id,board,target_id,action) VALUES(42,'owncustom',8810001,'spoiler');
CREATE VIEW public.owned_custom_rows AS
 SELECT 'boards' AS relation,to_jsonb(b)-'board_flags' AS value FROM content.boards b
 UNION ALL SELECT 'posts',to_jsonb(p) FROM content.posts p
 UNION ALL SELECT 'threads',to_jsonb(t) FROM content.threads t
 UNION ALL SELECT 'attachments',to_jsonb(m) FROM content.post_media m
 UNION ALL SELECT 'assets',to_jsonb(a) FROM media.assets a
 UNION ALL SELECT 'deletion',to_jsonb(d) FROM post_secrets.deletion d
 UNION ALL SELECT 'audit',to_jsonb(a) FROM content.moderation_audit a;
CREATE TABLE public.owned_custom_rows_before AS TABLE public.owned_custom_rows;
CREATE TABLE public.owned_custom_policies_before AS SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy;
CREATE TABLE public.owned_custom_functions_before AS SELECT p.oid,p.proowner,p.proacl::text AS acl,p.prosecdef,p.proconfig,CASE WHEN p.proname='apply_post_flag' THEN '' ELSE p.prosrc END AS prosrc FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname NOT IN ('pg_catalog','information_schema');
CREATE TABLE public.owned_custom_tables_before AS SELECT c.oid,c.relowner,c.relacl::text AS acl FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname IN ('content','media','post_secrets','staff_identity');
CREATE TABLE public.owned_custom_columns_before AS SELECT a.attrelid,a.attnum,a.attname,a.attacl::text AS acl FROM pg_attribute a WHERE a.attrelid IN(SELECT oid FROM public.owned_custom_tables_before);
DROP VIEW public.owned_custom_rows;
SQL
"${migrator[@]}" --single-transaction -f migrations/0083_source_board_flags.sql
"${migrator[@]}" <<'SQL'
CREATE VIEW public.owned_custom_rows AS
 SELECT 'boards' AS relation,to_jsonb(b)-'board_flag_type'-'board_flags' AS value FROM content.boards b
 UNION ALL SELECT 'posts',to_jsonb(p)-'board_flag_type' FROM content.posts p
 UNION ALL SELECT 'threads',to_jsonb(t) FROM content.threads t
 UNION ALL SELECT 'attachments',to_jsonb(m) FROM content.post_media m
 UNION ALL SELECT 'assets',to_jsonb(a) FROM media.assets a
 UNION ALL SELECT 'deletion',to_jsonb(d) FROM post_secrets.deletion d
 UNION ALL SELECT 'audit',to_jsonb(a) FROM content.moderation_audit a;
DO $$ BEGIN
 IF EXISTS(TABLE public.owned_custom_rows_before EXCEPT TABLE public.owned_custom_rows)
 OR EXISTS(TABLE public.owned_custom_rows EXCEPT TABLE public.owned_custom_rows_before)
 OR EXISTS(TABLE public.owned_custom_policies_before EXCEPT SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy)
 OR EXISTS(SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy EXCEPT TABLE public.owned_custom_policies_before)
 OR EXISTS(TABLE public.owned_custom_functions_before EXCEPT SELECT p.oid,p.proowner,p.proacl::text,p.prosecdef,p.proconfig,CASE WHEN p.proname='apply_post_flag' THEN '' ELSE p.prosrc END FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname NOT IN ('pg_catalog','information_schema'))
 OR EXISTS(SELECT p.oid,p.proowner,p.proacl::text,p.prosecdef,p.proconfig,CASE WHEN p.proname='apply_post_flag' THEN '' ELSE p.prosrc END FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname NOT IN ('pg_catalog','information_schema') AND p.proname NOT IN ('board_flag_codes','board_flag_label') EXCEPT TABLE public.owned_custom_functions_before)
 OR EXISTS(TABLE public.owned_custom_tables_before EXCEPT SELECT oid,relowner,relacl::text FROM pg_class WHERE oid IN(SELECT oid FROM public.owned_custom_tables_before))
 OR EXISTS(SELECT oid,relowner,relacl::text FROM pg_class WHERE oid IN(SELECT oid FROM public.owned_custom_tables_before) EXCEPT TABLE public.owned_custom_tables_before)
 OR EXISTS(TABLE public.owned_custom_columns_before EXCEPT SELECT attrelid,attnum,attname,attacl::text FROM pg_attribute WHERE attrelid IN(SELECT oid FROM public.owned_custom_tables_before))
 OR EXISTS(SELECT attrelid,attnum,attname,attacl::text FROM pg_attribute WHERE attrelid IN(SELECT oid FROM public.owned_custom_tables_before) AND attname<>'board_flag_type' EXCEPT TABLE public.owned_custom_columns_before)
 THEN RAISE EXCEPTION 'Flag policy upgrade changed historical records, privacy or existing authority'; END IF;
 IF cardinality((SELECT board_flags FROM content.boards WHERE slug='mlp'))<>83
 OR cardinality((SELECT board_flags FROM content.boards WHERE slug='pol'))<>25
 OR cardinality((SELECT board_flags FROM content.boards WHERE slug='lgbt'))<>0
 OR cardinality((SELECT board_flags FROM content.boards WHERE slug='test'))<>0
 OR (SELECT board_flag_type FROM content.boards WHERE slug='mlp')<>'mlp'
 OR (SELECT board_flags FROM content.boards WHERE slug='owncustom')<>ARRAY['CM']
 OR (SELECT flag_name FROM content.posts WHERE id=8810001)<>'Communist'
 OR (SELECT board_flag_type FROM content.posts WHERE id=8810001)<>'pol'
 OR content.board_flag_label('pol','CM')<>'Commie'
 OR content.board_flag_label('mlp','AN')<>'Anon'
 OR content.board_flag_label('pol','AN')<>'Anarchist'
 OR content.board_flag_label('lgbt','ACE')<>'Asexual'
 OR content.board_flag_label('test','FL1')<>'Flag 1'
 OR content.board_flag_label('mlp','ac') IS NOT NULL
 OR (SELECT count(*) FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
     WHERE n.nspname='content' AND p.proname IN ('board_flag_label','board_flag_codes')
       AND NOT p.prosecdef AND p.provolatile='i' AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp'])<>2
 THEN RAISE EXCEPTION 'Source flag choices, display labels, defaults or function authority differ'; END IF;
 BEGIN UPDATE content.boards SET board_flag_type='unknown' WHERE slug='owncustom';
   RAISE EXCEPTION 'Unknown type accepted' USING ERRCODE='ZX001';
 EXCEPTION WHEN check_violation THEN NULL; END;
 BEGIN UPDATE content.boards SET board_flags=ARRAY['TWI'] WHERE slug='owncustom';
   RAISE EXCEPTION 'Cross-type code accepted' USING ERRCODE='ZX001';
 EXCEPTION WHEN check_violation THEN NULL; END;
 BEGIN UPDATE content.boards SET board_flags=ARRAY[NULL]::text[] WHERE slug='owncustom';
   RAISE EXCEPTION 'NULL element accepted' USING ERRCODE='ZX001';
 EXCEPTION WHEN check_violation THEN NULL; END;
 BEGIN UPDATE content.boards SET board_flags=array_fill('CM'::text,ARRAY[84]) WHERE slug='owncustom';
   RAISE EXCEPTION 'Oversized policy accepted' USING ERRCODE='ZX001';
 EXCEPTION WHEN check_violation THEN NULL; END;
 BEGIN UPDATE content.boards SET board_flags='[0:0]={CM}'::text[] WHERE slug='owncustom';
   RAISE EXCEPTION 'Noncanonical array accepted' USING ERRCODE='ZX001';
 EXCEPTION WHEN check_violation THEN NULL; END;
 BEGIN UPDATE content.boards SET board_flags=ARRAY[ARRAY['CM']] WHERE slug='owncustom';
   RAISE EXCEPTION 'Multidimensional array accepted' USING ERRCODE='ZX001';
 EXCEPTION WHEN check_violation THEN NULL; END;
END $$;
SQL
for role in board_public board_staff board_auth board_media board_media_read board_media_intake board_monitor; do
    "${psql[@]}" -U "$role" -d source_flag_upgrade <<'SQL'
DO $$ BEGIN
 BEGIN UPDATE content.boards SET board_flag_type='pol',board_flags=ARRAY['CM'] WHERE false;
   RAISE EXCEPTION 'Runtime wrote flag policy' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN INSERT INTO content.boards(slug,title,description,board_flag_type,board_flags) SELECT 'denied','Owned','Synthetic','pol',ARRAY['CM'] WHERE false;
   RAISE EXCEPTION 'Runtime inserted flag policy' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SQL
done
cleanup
trap - EXIT
printf 'Source flag populated upgrade passed: historical content and authority preserved; source choices, distinct labels, array constraints and seven runtime write denials verified. Private cluster removed.\n'
