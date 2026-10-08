#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-public-deletion.XXXXXXXX)
started=0
cleanup() {
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null
        started=0
    fi
    [[ $cluster =~ ^/tmp/board-public-deletion\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
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
CREATE DATABASE public_deletion_upgrade OWNER board_migrator;
REVOKE ALL ON DATABASE public_deletion_upgrade FROM PUBLIC;
GRANT CONNECT ON DATABASE public_deletion_upgrade TO board_migrator,board_public,board_staff,board_auth,
    board_media,board_media_read,board_media_intake,board_monitor;
ALTER ROLE board_staff LOGIN;
ALTER ROLE board_auth LOGIN;
ALTER ROLE board_media LOGIN;
ALTER ROLE board_media_read LOGIN;
ALTER ROLE board_media_intake LOGIN;
ALTER ROLE board_monitor LOGIN;
SQL
migrator=("${psql[@]}" -U board_migrator -d public_deletion_upgrade)
for migration in migrations/*.sql; do
    [[ $migration != migrations/0084_public_deletion_policy.sql ]] || break
    "${migrator[@]}" --single-transaction -f "$migration"
done
"${migrator[@]}" <<'SQL'
-- Synthetic populated 0083 state: owned OP, unowned deleted reply, attachment,
-- report, private activity and unrelated operator overrides must survive intact.
UPDATE content.boards SET comment_spoiler_cleanup=false WHERE slug='a';
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('owndel','Owned deletion upgrade','Synthetic',1000,100,100,100,10);
INSERT INTO content.threads(id,board,created_at,modified_at,undead)
VALUES(8811001,'owndel','2026-01-01Z','2026-01-02Z',true);
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at,deleted)
VALUES(8811001,'owndel',8811001,'Owned','Historical OP','Owned body','2026-01-01Z',false),
      (8811002,'owndel',8811001,'Owned','','Deleted reply','2026-01-02Z',true);
INSERT INTO post_secrets.deletion(post_id,password_hash)
VALUES(8811001,'owned-op-upgrade-hash'),(8811002,'owned-reply-upgrade-hash');
INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at)
VALUES(repeat('c',32),repeat('c',32),repeat('c',32),repeat('c',64),100,500,300,'approved',clock_timestamp());
INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler,file_deleted)
VALUES(8811001,repeat('c',32),repeat('c',32),'owned-deletion.png',100,500,300,true,true);
INSERT INTO content.moderation_audit(account_id,board,target_id,action) VALUES(42,'owndel',8811002,'remove-post');
INSERT INTO content.reports(board,post_id,reason) VALUES('owndel',8811001,'Synthetic historical report');
INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,
    created_at,network_at,address_at,environment_at,expires_at,posts,threads)
VALUES(decode(repeat('01',32),'hex'),decode(repeat('02',32),'hex'),decode(repeat('03',32),'hex'),
    decode(repeat('04',32),'hex'),1,1,1,1,4102444800,1,1);
INSERT INTO post_secrets.anonymous_posts(post_id,token_hash,password_proof)
VALUES(8811001,decode(repeat('01',32),'hex'),decode(repeat('05',32),'hex'));
INSERT INTO post_secrets.anonymous_reports(report_id,token_hash)
SELECT id,decode(repeat('01',32),'hex') FROM content.reports WHERE board='owndel';
CREATE VIEW public.owned_deletion_rows AS
SELECT 'boards' AS relation,to_jsonb(r)-'deletion_no_op'-'deletion_no_reply'-'deletion_known_min_seconds'-'deletion_unknown_min_seconds'-'deletion_max_seconds' AS value FROM content.boards r
 UNION ALL SELECT 'posts' AS relation,to_jsonb(r) AS value FROM content.posts r
 UNION ALL SELECT 'threads' AS relation,to_jsonb(r) AS value FROM content.threads r
 UNION ALL SELECT 'attachments' AS relation,to_jsonb(r) AS value FROM content.post_media r
 UNION ALL SELECT 'assets' AS relation,to_jsonb(r) AS value FROM media.assets r
 UNION ALL SELECT 'deletion' AS relation,to_jsonb(r) AS value FROM post_secrets.deletion r
 UNION ALL SELECT 'audit' AS relation,to_jsonb(r) AS value FROM content.moderation_audit r
 UNION ALL SELECT 'reports' AS relation,to_jsonb(r) AS value FROM content.reports r
 UNION ALL SELECT 'sessions' AS relation,to_jsonb(r) AS value FROM post_secrets.anonymous_sessions r
 UNION ALL SELECT 'ownership' AS relation,to_jsonb(r) AS value FROM post_secrets.anonymous_posts r
 UNION ALL SELECT 'report_ownership' AS relation,to_jsonb(r) AS value FROM post_secrets.anonymous_reports r
 UNION ALL SELECT 'anonymous_policy' AS relation,to_jsonb(r) AS value FROM post_secrets.anonymous_policy r;
CREATE TABLE public.owned_deletion_rows_before AS TABLE public.owned_deletion_rows;
CREATE TABLE public.owned_deletion_policies_before AS SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy;
CREATE TABLE public.owned_deletion_functions_before AS SELECT p.oid,p.proowner,p.proacl::text AS acl,p.prosecdef,p.proconfig,p.prosrc FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname NOT IN ('pg_catalog','information_schema');
CREATE TABLE public.owned_deletion_relations_before AS SELECT c.oid,c.relowner,c.relacl::text AS acl,c.relrowsecurity,c.relforcerowsecurity FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname IN ('content','media','post_secrets','staff_identity','deployment');
CREATE TABLE public.owned_deletion_columns_before AS SELECT a.attrelid,a.attnum,a.attname,a.attacl::text AS acl FROM pg_attribute a WHERE a.attrelid IN(SELECT oid FROM public.owned_deletion_relations_before);
SQL
"${migrator[@]}" --single-transaction -f migrations/0084_public_deletion_policy.sql
"${migrator[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(TABLE public.owned_deletion_rows_before EXCEPT ALL TABLE public.owned_deletion_rows)
 OR EXISTS(TABLE public.owned_deletion_rows EXCEPT ALL TABLE public.owned_deletion_rows_before)
 OR EXISTS(TABLE public.owned_deletion_policies_before EXCEPT SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy)
 OR EXISTS(SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy EXCEPT TABLE public.owned_deletion_policies_before)
 OR EXISTS(TABLE public.owned_deletion_functions_before EXCEPT SELECT p.oid,p.proowner,p.proacl::text AS acl,p.prosecdef,p.proconfig,p.prosrc FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname NOT IN ('pg_catalog','information_schema'))
 OR EXISTS(SELECT p.oid,p.proowner,p.proacl::text AS acl,p.prosecdef,p.proconfig,p.prosrc FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname NOT IN ('pg_catalog','information_schema') EXCEPT TABLE public.owned_deletion_functions_before)
 OR EXISTS(TABLE public.owned_deletion_relations_before EXCEPT SELECT c.oid,c.relowner,c.relacl::text AS acl,c.relrowsecurity,c.relforcerowsecurity FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname IN ('content','media','post_secrets','staff_identity','deployment'))
 OR EXISTS(SELECT c.oid,c.relowner,c.relacl::text AS acl,c.relrowsecurity,c.relforcerowsecurity FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname IN ('content','media','post_secrets','staff_identity','deployment') EXCEPT TABLE public.owned_deletion_relations_before)
 OR EXISTS(TABLE public.owned_deletion_columns_before EXCEPT SELECT a.attrelid,a.attnum,a.attname,a.attacl::text AS acl FROM pg_attribute a WHERE a.attrelid IN(SELECT oid FROM public.owned_deletion_relations_before) AND a.attname NOT IN('deletion_no_op','deletion_no_reply','deletion_known_min_seconds','deletion_unknown_min_seconds','deletion_max_seconds'))
 OR EXISTS(SELECT a.attrelid,a.attnum,a.attname,a.attacl::text AS acl FROM pg_attribute a WHERE a.attrelid IN(SELECT oid FROM public.owned_deletion_relations_before) AND a.attname NOT IN('deletion_no_op','deletion_no_reply','deletion_known_min_seconds','deletion_unknown_min_seconds','deletion_max_seconds') EXCEPT TABLE public.owned_deletion_columns_before)
 THEN RAISE EXCEPTION 'Deletion upgrade changed historical content, ownership, privacy or authority'; END IF;
 IF (SELECT count(*) FROM content.boards WHERE deletion_no_op)<>18
 OR EXISTS(SELECT 1 FROM content.boards WHERE
     deletion_no_op IS DISTINCT FROM (slug IN ('a','bant','his','int','jp','pol','pw','qa','qst','sp','tv','v','vip','vm','vmg','vrpg','vst','vt'))
     OR deletion_no_reply OR deletion_known_min_seconds<>60
     OR deletion_unknown_min_seconds<>600 OR deletion_max_seconds<>1800)
 THEN RAISE EXCEPTION 'Imported deletion policy differs, including qa reply and vg OP defaults'; END IF;
END $$;
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('newdel','Owned new board','Synthetic',1000,100,100,100,10);
DO $$ DECLARE assignment text; policy_column text; BEGIN
 IF NOT EXISTS(SELECT 1 FROM content.boards WHERE slug='newdel'
     AND NOT deletion_no_op AND NOT deletion_no_reply AND deletion_known_min_seconds=60
     AND deletion_unknown_min_seconds=600 AND deletion_max_seconds=1800)
 THEN RAISE EXCEPTION 'New board did not receive safe default deletion policy'; END IF;
 -- Exercise both accepted endpoints and the relationship constraints.
 UPDATE content.boards SET deletion_no_op=true,deletion_no_reply=true,
     deletion_known_min_seconds=0,deletion_unknown_min_seconds=0,deletion_max_seconds=1 WHERE slug='newdel';
 UPDATE content.boards SET deletion_known_min_seconds=86399,deletion_unknown_min_seconds=86399,
     deletion_max_seconds=86400 WHERE slug='newdel';
 UPDATE content.boards SET deletion_known_min_seconds=60,deletion_unknown_min_seconds=600,
     deletion_max_seconds=1800 WHERE slug='newdel';
 FOREACH assignment IN ARRAY ARRAY[
     'deletion_known_min_seconds=-1','deletion_known_min_seconds=86401',
     'deletion_known_min_seconds=601','deletion_unknown_min_seconds=-1',
     'deletion_unknown_min_seconds=59','deletion_unknown_min_seconds=86401',
     'deletion_unknown_min_seconds=1800','deletion_unknown_min_seconds=1801',
     'deletion_max_seconds=0','deletion_max_seconds=-1','deletion_max_seconds=86401',
     'deletion_max_seconds=600','deletion_max_seconds=599'] LOOP
   BEGIN
     EXECUTE 'UPDATE content.boards SET '||assignment||' WHERE slug=''newdel''';
     RAISE EXCEPTION 'Invalid deletion bounds accepted: %',assignment USING ERRCODE='ZX001';
   EXCEPTION WHEN check_violation THEN NULL; END;
 END LOOP;
 FOREACH policy_column IN ARRAY ARRAY['deletion_no_op','deletion_no_reply',
     'deletion_known_min_seconds','deletion_unknown_min_seconds','deletion_max_seconds'] LOOP
   BEGIN
     EXECUTE format('UPDATE content.boards SET %I=NULL WHERE slug=''newdel''',policy_column);
     RAISE EXCEPTION 'NULL deletion policy accepted: %',policy_column USING ERRCODE='ZX001';
   EXCEPTION WHEN not_null_violation THEN NULL; END;
 END LOOP;
END $$;
-- A denied INSERT on required base columns must not conceal a column-only
-- policy grant. Inspect all role grants as migrator, including roles without
-- content schema USAGE, then prove denials below with the actual logins.
DO $$ DECLARE runtime text; policy_column text; BEGIN
 FOREACH runtime IN ARRAY ARRAY['board_public','board_staff','board_auth','board_media',
     'board_media_read','board_media_intake','board_monitor'] LOOP
   FOREACH policy_column IN ARRAY ARRAY['deletion_no_op','deletion_no_reply',
       'deletion_known_min_seconds','deletion_unknown_min_seconds','deletion_max_seconds'] LOOP
     IF has_column_privilege(runtime,'content.boards',policy_column,'INSERT')
         OR has_column_privilege(runtime,'content.boards',policy_column,'UPDATE') THEN
       RAISE EXCEPTION 'Runtime % holds deletion policy write grant: %',runtime,policy_column;
     END IF;
   END LOOP;
 END LOOP;
END $$;
SQL
# Use actual restricted logins. Probe each column separately so one denied
# column cannot hide an accidental grant on another policy column.
for role in board_public board_staff board_auth board_media board_media_read board_media_intake board_monitor; do
    "${psql[@]}" -U "$role" -d public_deletion_upgrade <<'SQL'
DO $$ DECLARE policy_column text; policy_value text; BEGIN
 FOREACH policy_column IN ARRAY ARRAY['deletion_no_op','deletion_no_reply',
     'deletion_known_min_seconds','deletion_unknown_min_seconds','deletion_max_seconds'] LOOP
   policy_value:=CASE policy_column WHEN 'deletion_no_op' THEN 'true' WHEN 'deletion_no_reply' THEN 'true'
       WHEN 'deletion_known_min_seconds' THEN '60' WHEN 'deletion_unknown_min_seconds' THEN '600' ELSE '1800' END;
   BEGIN
     EXECUTE format('UPDATE content.boards SET %I=%s WHERE slug=''owndel''',policy_column,policy_value);
     RAISE EXCEPTION 'Runtime wrote deletion policy: %',policy_column USING ERRCODE='ZX001';
   EXCEPTION WHEN insufficient_privilege THEN NULL; END;
   BEGIN
     EXECUTE format('INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,%I) VALUES(''denied'',''Owned'',''Synthetic'',1000,100,100,100,10,%s)',policy_column,policy_value);
     RAISE EXCEPTION 'Runtime inserted deletion policy: %',policy_column USING ERRCODE='ZX001';
   EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 END LOOP;
END $$;
SQL
done
# Public and staff readers must still be able to load the new policy.
for role in board_public board_staff; do
    "${psql[@]}" -U "$role" -d public_deletion_upgrade <<'SQL'
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM content.boards WHERE slug='qa' AND deletion_no_op
     AND NOT deletion_no_reply AND deletion_known_min_seconds=60
     AND deletion_unknown_min_seconds=600 AND deletion_max_seconds=1800)
 THEN RAISE EXCEPTION 'Runtime could not read deletion policy'; END IF;
END $$;
SQL
done
cleanup
trap - EXIT
printf 'Public deletion populated upgrade passed: history, ownership and authority preserved; imported/new defaults, bounds and seven runtime write denials verified. Private cluster removed.\n'
