#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-drawing-policy.XXXXXXXX)
started=0
cleanup() {
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null
        started=0
    fi
    [[ $cluster =~ ^/tmp/board-drawing-policy\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
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
CREATE DATABASE drawing_policy_upgrade OWNER board_migrator;
REVOKE ALL ON DATABASE drawing_policy_upgrade FROM PUBLIC;
GRANT CONNECT ON DATABASE drawing_policy_upgrade TO board_migrator,board_public,board_staff,board_auth,
    board_media,board_media_read,board_media_intake,board_monitor;
ALTER ROLE board_staff LOGIN;
ALTER ROLE board_auth LOGIN;
ALTER ROLE board_media LOGIN;
ALTER ROLE board_media_read LOGIN;
ALTER ROLE board_media_intake LOGIN;
ALTER ROLE board_monitor LOGIN;
SQL
migrator=("${psql[@]}" -U board_migrator -d drawing_policy_upgrade)
for migration in migrations/*.sql; do
    [[ $migration != migrations/0114_board_drawing_policy.sql ]] || break
    "${migrator[@]}" --single-transaction -f "$migration"
done
"${migrator[@]}" <<'SQL'
DO $$ BEGIN
 IF (SELECT count(*) FROM content.boards)<>82
 THEN RAISE EXCEPTION 'The full source board import is missing'; END IF;
 -- The new readiness projection must fail before the additive schema upgrade.
 BEGIN
   PERFORM oekaki,oekaki_replays,oekaki_width,oekaki_height FROM content.boards LIMIT 0;
   RAISE EXCEPTION 'Drawing policy already exists before migration 0114' USING ERRCODE='ZX001';
 EXCEPTION WHEN undefined_column THEN NULL; END;
END $$;
UPDATE content.boards SET title='Operator title',image_limit=0,math_tags=true WHERE slug='qst';
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('ownedraw','Owned drawing upgrade','Synthetic',1000,100,100,100,10);
INSERT INTO content.threads(id,board,created_at,modified_at,undead)
VALUES(1140001,'ownedraw','2026-01-01Z','2026-01-02Z',true),
      (1140002,'j','2026-01-01Z','2026-01-02Z',false);
-- Seed historical rows through the existing wordfilter admission context.
BEGIN;
SELECT set_config('board.wordfilter_payload','5746303100000001ffff0000001b4c69746572616c205b6d6174685d785b2f6d6174685d20626f6479',true);
SELECT set_config('board.wordfilter_search','Literal [math]x[/math] body',true);
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(1140001,'ownedraw',1140001,'Owned','Historical drawing attachment','Literal [math]x[/math] body','2026-01-01Z');
COMMIT;
BEGIN;
SELECT set_config('board.wordfilter_payload','5746303100000001ffff000000155072697661746520626f6172642066697874757265',true);
SELECT set_config('board.wordfilter_search','Private board fixture',true);
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(1140002,'j',1140002,'Owned','Private historical post','Private board fixture','2026-01-01Z');
COMMIT;
INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES(1140001,'owned-drawing-upgrade-hash');
INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at)
VALUES(repeat('b',32),repeat('b',32),repeat('b',32),repeat('b',64),100,500,300,'approved',clock_timestamp());
INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler,file_deleted)
VALUES(1140001,repeat('b',32),repeat('b',32),'owned-drawing.png',100,500,300,false,false);
INSERT INTO content.moderation_audit(account_id,board,target_id,action) VALUES(42,'ownedraw',1140001,'spoiler');
CREATE VIEW public.owned_drawing_rows AS
 SELECT 'boards' AS relation,to_jsonb(b) AS value FROM content.boards b
 UNION ALL SELECT 'posts',to_jsonb(p) FROM content.posts p
 UNION ALL SELECT 'threads',to_jsonb(t) FROM content.threads t
 UNION ALL SELECT 'attachments',to_jsonb(m) FROM content.post_media m
 UNION ALL SELECT 'assets',to_jsonb(a) FROM media.assets a
 UNION ALL SELECT 'deletion',to_jsonb(d) FROM post_secrets.deletion d
 UNION ALL SELECT 'audit',to_jsonb(a) FROM content.moderation_audit a;
CREATE TABLE public.owned_drawing_rows_before AS TABLE public.owned_drawing_rows;
CREATE TABLE public.owned_drawing_policies_before AS SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy;
CREATE TABLE public.owned_drawing_functions_before AS SELECT p.oid,p.proowner,p.proacl::text AS acl,p.prosecdef,p.proconfig,p.prosrc FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname NOT IN ('pg_catalog','information_schema');
CREATE TABLE public.owned_drawing_tables_before AS SELECT c.oid,c.relowner,c.relacl::text AS acl,c.relrowsecurity,c.relforcerowsecurity FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname IN ('content','media','post_secrets','staff_identity');
CREATE TABLE public.owned_drawing_columns_before AS SELECT a.attrelid,a.attnum,a.attname,a.attacl::text AS acl FROM pg_attribute a WHERE a.attrelid IN(SELECT oid FROM public.owned_drawing_tables_before);
CREATE TABLE public.owned_drawing_triggers_before AS SELECT t.oid,t.tgrelid,t.tgname,t.tgfoid,t.tgtype,t.tgenabled,t.tgisinternal,t.tgqual::text,t.tgargs FROM pg_trigger t WHERE t.tgrelid IN(SELECT oid FROM public.owned_drawing_tables_before);
DROP VIEW public.owned_drawing_rows;
SQL
"${migrator[@]}" --single-transaction -f migrations/0114_board_drawing_policy.sql
"${migrator[@]}" <<'SQL'
CREATE VIEW public.owned_drawing_rows AS
 SELECT 'boards' AS relation,to_jsonb(b)-'oekaki'-'oekaki_replays'-'oekaki_width'-'oekaki_height' AS value FROM content.boards b
 UNION ALL SELECT 'posts',to_jsonb(p) FROM content.posts p
 UNION ALL SELECT 'threads',to_jsonb(t) FROM content.threads t
 UNION ALL SELECT 'attachments',to_jsonb(m) FROM content.post_media m
 UNION ALL SELECT 'assets',to_jsonb(a) FROM media.assets a
 UNION ALL SELECT 'deletion',to_jsonb(d) FROM post_secrets.deletion d
 UNION ALL SELECT 'audit',to_jsonb(a) FROM content.moderation_audit a;
DO $$ BEGIN
 IF (SELECT count(*) FROM content.boards)<>83
 OR EXISTS(SELECT 1 FROM content.boards WHERE oekaki IS DISTINCT FROM (slug IN ('i','qst','vip'))
      OR oekaki_replays IS DISTINCT FROM (slug='i') OR oekaki_width<>400 OR oekaki_height<>400)
 THEN RAISE EXCEPTION 'Drawing source policies or global defaults differ'; END IF;
 IF EXISTS(TABLE public.owned_drawing_rows_before EXCEPT TABLE public.owned_drawing_rows)
 OR EXISTS(TABLE public.owned_drawing_rows EXCEPT TABLE public.owned_drawing_rows_before)
 OR EXISTS(TABLE public.owned_drawing_policies_before EXCEPT SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy)
 OR EXISTS(SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy EXCEPT TABLE public.owned_drawing_policies_before)
 OR EXISTS(TABLE public.owned_drawing_functions_before EXCEPT SELECT p.oid,p.proowner,p.proacl::text,p.prosecdef,p.proconfig,p.prosrc FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname NOT IN ('pg_catalog','information_schema'))
 OR EXISTS(SELECT p.oid,p.proowner,p.proacl::text,p.prosecdef,p.proconfig,p.prosrc FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname NOT IN ('pg_catalog','information_schema') EXCEPT TABLE public.owned_drawing_functions_before)
 OR EXISTS(TABLE public.owned_drawing_tables_before EXCEPT SELECT oid,relowner,relacl::text,relrowsecurity,relforcerowsecurity FROM pg_class WHERE oid IN(SELECT oid FROM public.owned_drawing_tables_before))
 OR EXISTS(SELECT oid,relowner,relacl::text,relrowsecurity,relforcerowsecurity FROM pg_class WHERE oid IN(SELECT oid FROM public.owned_drawing_tables_before) EXCEPT TABLE public.owned_drawing_tables_before)
 OR EXISTS(TABLE public.owned_drawing_columns_before EXCEPT SELECT attrelid,attnum,attname,attacl::text FROM pg_attribute WHERE attrelid IN(SELECT oid FROM public.owned_drawing_tables_before))
 OR EXISTS(SELECT attrelid,attnum,attname,attacl::text FROM pg_attribute WHERE attrelid IN(SELECT oid FROM public.owned_drawing_tables_before) AND attname NOT IN ('oekaki','oekaki_replays','oekaki_width','oekaki_height') EXCEPT TABLE public.owned_drawing_columns_before)
 OR EXISTS(TABLE public.owned_drawing_triggers_before EXCEPT SELECT t.oid,t.tgrelid,t.tgname,t.tgfoid,t.tgtype,t.tgenabled,t.tgisinternal,t.tgqual::text,t.tgargs FROM pg_trigger t WHERE t.tgrelid IN(SELECT oid FROM public.owned_drawing_tables_before))
 OR EXISTS(SELECT t.oid,t.tgrelid,t.tgname,t.tgfoid,t.tgtype,t.tgenabled,t.tgisinternal,t.tgqual::text,t.tgargs FROM pg_trigger t WHERE t.tgrelid IN(SELECT oid FROM public.owned_drawing_tables_before) EXCEPT TABLE public.owned_drawing_triggers_before)
 THEN RAISE EXCEPTION 'Drawing upgrade changed history, unrelated policy, privacy or authority'; END IF;
END $$;
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('future','Owned future drawing board','Synthetic',1000,100,100,100,10);
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM content.boards WHERE slug='future' AND NOT oekaki AND NOT oekaki_replays AND oekaki_width=400 AND oekaki_height=400)
 THEN RAISE EXCEPTION 'Future boards did not inherit global drawing defaults'; END IF;
 BEGIN UPDATE content.boards SET oekaki=NULL WHERE slug='future';
   RAISE EXCEPTION 'Nullable drawing policy' USING ERRCODE='ZX001';
 EXCEPTION WHEN not_null_violation THEN NULL; END;
 BEGIN UPDATE content.boards SET oekaki_replays=NULL WHERE slug='future';
   RAISE EXCEPTION 'Nullable replay policy' USING ERRCODE='ZX001';
 EXCEPTION WHEN not_null_violation THEN NULL; END;
 BEGIN UPDATE content.boards SET oekaki_width=0 WHERE slug='future';
   RAISE EXCEPTION 'Nonpositive default width' USING ERRCODE='ZX001';
 EXCEPTION WHEN check_violation THEN NULL; END;
 BEGIN UPDATE content.boards SET oekaki_height=-1 WHERE slug='future';
   RAISE EXCEPTION 'Nonpositive default height' USING ERRCODE='ZX001';
 EXCEPTION WHEN check_violation THEN NULL; END;
END $$;
-- Source OEKAKI_MIN/MAX are unused constants, not board admission constraints.
UPDATE content.boards SET oekaki=true,oekaki_width=1,oekaki_height=9999 WHERE slug='future';
UPDATE content.boards SET oekaki_replays=true WHERE slug='future';
UPDATE content.boards SET oekaki=false,oekaki_replays=false,oekaki_width=400,oekaki_height=400 WHERE slug='future';
SQL
"${psql[@]}" -U board_public -d drawing_policy_upgrade <<'SQL'
SELECT oekaki,oekaki_replays,oekaki_width,oekaki_height FROM content.boards LIMIT 0;
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM content.boards WHERE slug='j')
 OR EXISTS(SELECT 1 FROM content.posts WHERE board='j')
 OR EXISTS(SELECT 1 FROM content.visible_threads WHERE board='j')
 OR EXISTS(SELECT 1 FROM content.threads WHERE board='j')
 OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=1140001 AND comment='Literal [math]x[/math] body')
 OR NOT EXISTS(SELECT 1 FROM content.boards WHERE slug='i' AND oekaki AND oekaki_replays)
 OR NOT EXISTS(SELECT 1 FROM content.boards WHERE slug='qst' AND oekaki AND NOT oekaki_replays AND image_limit=0 AND title='Operator title' AND math_tags)
 THEN RAISE EXCEPTION 'Public reads lost privacy, source policy or existing content'; END IF;
END $$;
SQL
"${psql[@]}" -U board_staff -d drawing_policy_upgrade <<'SQL'
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM content.boards WHERE slug='j')
 OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=1140002)
 THEN RAISE EXCEPTION 'Staff lost private-board visibility'; END IF;
END $$;
SQL
for role in board_public board_staff board_auth board_media board_media_read board_media_intake board_monitor; do
    "${psql[@]}" -U "$role" -d drawing_policy_upgrade <<'SQL'
DO $$ BEGIN
 BEGIN UPDATE content.boards SET oekaki=true,oekaki_replays=false,oekaki_width=400,oekaki_height=400 WHERE false;
   RAISE EXCEPTION 'Runtime wrote drawing policy' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN INSERT INTO content.boards(slug,title,description,oekaki,oekaki_replays,oekaki_width,oekaki_height) SELECT 'denied','Owned','Synthetic',true,false,400,400 WHERE false;
   RAISE EXCEPTION 'Runtime inserted drawing policy' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN ALTER TABLE content.boards DROP COLUMN oekaki;
   RAISE EXCEPTION 'Runtime changed drawing schema' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SQL
done
cleanup
trap - EXIT
printf 'Drawing populated upgrade passed: all 82 source policies, future defaults, pre-upgrade readiness failure, historical content and authority, private-board visibility, operator changes and seven runtime write denials verified. Private cluster removed.\n'
