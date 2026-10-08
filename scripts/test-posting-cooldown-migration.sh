#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-posting-cooldown.XXXXXXXX)
started=0
cleanup() {
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null
        started=0
    fi
    [[ $cluster =~ ^/tmp/board-posting-cooldown\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
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
CREATE DATABASE posting_cooldown_upgrade OWNER board_migrator;
REVOKE ALL ON DATABASE posting_cooldown_upgrade FROM PUBLIC;
GRANT CONNECT ON DATABASE posting_cooldown_upgrade TO board_migrator,board_public,board_staff,board_auth,
    board_media,board_media_read,board_media_intake,board_monitor;
ALTER ROLE board_staff LOGIN;
ALTER ROLE board_auth LOGIN;
ALTER ROLE board_media LOGIN;
ALTER ROLE board_media_read LOGIN;
ALTER ROLE board_media_intake LOGIN;
ALTER ROLE board_monitor LOGIN;
SQL
migrator=("${psql[@]}" -U board_migrator -d posting_cooldown_upgrade)
for migration in migrations/*.sql; do
    [[ $migration != migrations/0087_ordinary_posting_cooldowns.sql ]] || break
    "${migrator[@]}" --single-transaction -f "$migration"
done
"${migrator[@]}" <<'SQL'
-- Synthetic populated 0086 state: owned OP, unowned deleted reply, attachment,
-- report, private activity and unrelated operator overrides must survive intact.
UPDATE content.boards SET comment_spoiler_cleanup=false WHERE slug='a';
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('ownroll','Owned cooldown upgrade','Synthetic',1000,100,100,100,10);
UPDATE content.boards SET expire_neglected=false WHERE slug='ownroll';
INSERT INTO content.threads(id,board,created_at,modified_at,undead)
VALUES(8811001,'ownroll','2026-01-01Z','2026-01-02Z',true);
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at,deleted)
VALUES(8811001,'ownroll',8811001,'Owned','Historical OP','Owned body','2026-01-01Z',false),
      (8811002,'ownroll',8811001,'Owned','','Deleted reply','2026-01-02Z',true);
INSERT INTO post_secrets.deletion(post_id,password_hash)
VALUES(8811001,'owned-op-upgrade-hash'),(8811002,'owned-reply-upgrade-hash');
INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at)
VALUES(repeat('c',32),repeat('c',32),repeat('c',32),repeat('c',64),100,500,300,'approved',clock_timestamp());
INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler,file_deleted)
VALUES(8811001,repeat('c',32),repeat('c',32),'owned-rollover.png',100,500,300,true,true);
INSERT INTO content.moderation_audit(account_id,board,target_id,action) VALUES(42,'ownroll',8811002,'remove-post');
INSERT INTO content.reports(board,post_id,reason) VALUES('ownroll',8811001,'Synthetic historical report');
INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,
    created_at,network_at,address_at,environment_at,expires_at,posts,threads)
VALUES(decode(repeat('01',32),'hex'),decode(repeat('02',32),'hex'),decode(repeat('03',32),'hex'),
    decode(repeat('04',32),'hex'),1,1,1,1,4102444800,1,1);
INSERT INTO post_secrets.anonymous_posts(post_id,token_hash,password_proof)
VALUES(8811001,decode(repeat('01',32),'hex'),decode(repeat('05',32),'hex'));
INSERT INTO post_secrets.anonymous_reports(report_id,token_hash)
SELECT id,decode(repeat('01',32),'hex') FROM content.reports WHERE board='ownroll';
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,staff_only)
VALUES('coolpriv','Private upgrade','Synthetic',1000,100,100,100,10,true);
INSERT INTO content.threads(id,board) VALUES(8811003,'coolpriv');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(8811003,'coolpriv',8811003,'Private','Private subject','Private body');
INSERT INTO post_secrets.public_deletion_actors(actor_hash,events,expires_at)
VALUES(decode(repeat('09',32),'hex'),ARRAY[1,2]::bigint[],86402);
CREATE VIEW public.owned_rollover_rows AS
SELECT 'boards' AS relation,to_jsonb(r)-ARRAY['posting_reply_seconds','posting_image_seconds','posting_thread_seconds'] AS value FROM content.boards r
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
 UNION ALL SELECT 'deletion_quota' AS relation,to_jsonb(r) AS value FROM post_secrets.public_deletion_actors r
 UNION ALL SELECT 'deletion_capacity' AS relation,to_jsonb(r) AS value FROM post_secrets.public_deletion_capacity r
 UNION ALL SELECT 'anonymous_policy' AS relation,to_jsonb(r) AS value FROM post_secrets.anonymous_policy r;
CREATE TABLE public.owned_rollover_rows_before AS TABLE public.owned_rollover_rows;
CREATE TABLE public.owned_rollover_policies_before AS SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy;
CREATE TABLE public.owned_rollover_functions_before AS SELECT p.oid,p.proowner,p.proacl::text AS acl,p.prosecdef,p.proconfig,p.prosrc FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname NOT IN ('pg_catalog','information_schema');
CREATE TABLE public.owned_rollover_relations_before AS SELECT c.oid,c.relowner,c.relacl::text AS acl,c.relrowsecurity,c.relforcerowsecurity FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname IN ('content','media','post_secrets','staff_identity','deployment');
SQL
# Pre-0087 readiness fails closed with the application's real public role.
"${psql[@]}" -U board_public -d posting_cooldown_upgrade <<'SQL'
DO $$ BEGIN
 BEGIN
  PERFORM posting_reply_seconds,posting_image_seconds,posting_thread_seconds FROM content.boards LIMIT 1;
  RAISE EXCEPTION 'Pre-upgrade schema accepted cooldown readiness' USING ERRCODE='ZX001';
 EXCEPTION WHEN undefined_column THEN NULL; END;
END $$;
SQL
# A fresh bootstrap must create the same constrained owner as an existing install.
runuser -u postgres -- "${psql[@]}" -d postgres <<'SQL'
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='board_posting_cooldown_owner'
   AND NOT rolcanlogin AND NOT rolsuper AND NOT rolcreatedb AND NOT rolcreaterole
   AND NOT rolreplication AND NOT rolbypassrls)
 OR NOT EXISTS(SELECT 1 FROM pg_auth_members WHERE roleid='board_posting_cooldown_owner'::regrole
   AND member='board_migrator'::regrole AND NOT inherit_option AND set_option AND NOT admin_option)
 THEN RAISE EXCEPTION 'Fresh posting role bootstrap is unsafe'; END IF;
END $$;
DROP ROLE board_posting_cooldown_owner;
SQL
# Real migration, transactional failure: never silently proceed without its owner.
if "${migrator[@]}" --single-transaction -f migrations/0087_ordinary_posting_cooldowns.sql >"$cluster/missing-role.log" 2>&1; then
    echo 'Migration accepted missing posting owner' >&2; exit 1
fi
grep -q 'role "board_posting_cooldown_owner" does not exist' "$cluster/missing-role.log"
"${migrator[@]}" <<'SQL'
DO $$ BEGIN
 IF to_regclass('post_secrets.posting_history') IS NOT NULL
 OR EXISTS(SELECT 1 FROM information_schema.columns WHERE table_schema='content'
   AND table_name='boards' AND column_name='posting_reply_seconds')
 THEN RAISE EXCEPTION 'Missing-owner migration left partial schema'; END IF;
END $$;
SQL
runuser -u postgres -- "${psql[@]}" -d postgres -f deploy/posting-cooldown-role.sql
"${migrator[@]}" --single-transaction -f migrations/0087_ordinary_posting_cooldowns.sql
"${migrator[@]}" <<'SQL'
DO $$ DECLARE col text; bad bigint; runtime text; rel text; fn text; BEGIN
 IF EXISTS(TABLE public.owned_rollover_rows_before EXCEPT ALL TABLE public.owned_rollover_rows)
 OR EXISTS(TABLE public.owned_rollover_rows EXCEPT ALL TABLE public.owned_rollover_rows_before)
 THEN RAISE EXCEPTION '0087 changed historical content, ownership, clocks, 0085 quota or 0086 policy'; END IF;
 IF EXISTS(TABLE public.owned_rollover_functions_before EXCEPT
   SELECT p.oid,p.proowner,p.proacl::text,p.prosecdef,p.proconfig,p.prosrc FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
   WHERE n.nspname NOT IN ('pg_catalog','information_schema'))
 OR EXISTS(TABLE public.owned_rollover_policies_before EXCEPT
   SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy)
 OR EXISTS(SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy
   WHERE polname NOT IN ('posting_cooldown_board_read','posting_cooldown_board_lock')
   EXCEPT TABLE public.owned_rollover_policies_before)
 OR EXISTS(SELECT 1 FROM public.owned_rollover_relations_before old JOIN pg_class c USING(oid)
   WHERE (old.relowner,old.acl,old.relrowsecurity,old.relforcerowsecurity)
     IS DISTINCT FROM (c.relowner,c.relacl::text,c.relrowsecurity,c.relforcerowsecurity))
 THEN RAISE EXCEPTION '0087 changed existing functions, RLS or ownership'; END IF;
 IF to_regprocedure('content.record_posting_history(bytea,bigint)') IS NOT NULL
 OR NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='content.posts'::regclass
   AND tgname='record_inserted_posting_history' AND tgenabled='O' AND tgtype=5
   AND tgfoid='content.record_inserted_posting_history()'::regprocedure AND NOT tgisinternal)
 THEN RAISE EXCEPTION 'Posting provenance trigger missing or obsolete public registration API remains'; END IF;
 IF EXISTS(SELECT 1 FROM post_secrets.posting_history)
 OR EXISTS(SELECT 1 FROM post_secrets.posting_thread_actions)
 THEN RAISE EXCEPTION 'Migration fabricated actor identity for old posts'; END IF;
 IF (SELECT count(*) FROM post_secrets.posting_actor_gates)<>4096
 OR (SELECT min(stripe) FROM post_secrets.posting_actor_gates)<>0
 OR (SELECT max(stripe) FROM post_secrets.posting_actor_gates)<>4095
 OR (SELECT count(*) FROM post_secrets.posting_action_capacity WHERE singleton)<>1
 THEN RAISE EXCEPTION 'Posting gates are not fixed and bounded'; END IF;
 -- Read-only reference: global_config.ini:201-209, categories/{ws,nws},
 -- and b/pol/bant/vg/jp/vt/s4s/test board config RENZOKU{,2,3}.
 IF EXISTS(SELECT 1 FROM content.boards WHERE
   posting_reply_seconds<>CASE WHEN slug IN('b','pol') THEN 30 WHEN slug='bant' THEN 15 WHEN slug='vg' THEN 90 ELSE 60 END
   OR posting_image_seconds<>CASE WHEN slug IN('b','pol') THEN 30 WHEN slug='bant' THEN 15 WHEN slug='vg' THEN 120 WHEN NOT worksafe THEN 30 ELSE 60 END
   OR posting_thread_seconds<>CASE WHEN slug IN('b','pol') THEN 90 WHEN slug='bant' THEN 60 WHEN slug IN('jp','vt') THEN 3600 WHEN slug='s4s' THEN 300 WHEN slug='test' THEN 30 ELSE 600 END)
 THEN RAISE EXCEPTION 'Imported posting defaults or overrides differ from pinned source'; END IF;
 FOREACH col IN ARRAY ARRAY['posting_reply_seconds','posting_image_seconds','posting_thread_seconds'] LOOP
  FOREACH bad IN ARRAY ARRAY[-1::bigint,86401::bigint] LOOP
   BEGIN
    EXECUTE format('UPDATE content.boards SET %I=$1 WHERE slug=''ownroll''',col) USING bad;
    RAISE EXCEPTION 'Out-of-range timer accepted' USING ERRCODE='ZX001';
   EXCEPTION WHEN check_violation THEN NULL; END;
  END LOOP;
  BEGIN
   EXECUTE format('UPDATE content.boards SET %I=NULL WHERE slug=''ownroll''',col);
   RAISE EXCEPTION 'NULL timer accepted' USING ERRCODE='ZX001';
  EXCEPTION WHEN not_null_violation THEN NULL; END;
  FOREACH runtime IN ARRAY ARRAY['board_public','board_staff','board_auth','board_media','board_media_read','board_media_intake','board_monitor'] LOOP
   IF has_column_privilege(runtime,'content.boards',col,'INSERT,UPDATE')
   THEN RAISE EXCEPTION 'Runtime % can change %',runtime,col; END IF;
  END LOOP;
 END LOOP;
 IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='board_posting_cooldown_owner'
   AND NOT rolcanlogin AND NOT rolsuper AND NOT rolcreatedb AND NOT rolcreaterole
   AND NOT rolreplication AND NOT rolbypassrls)
 OR NOT EXISTS(SELECT 1 FROM pg_auth_members WHERE roleid='board_posting_cooldown_owner'::regrole
   AND member='board_migrator'::regrole AND NOT inherit_option AND set_option AND NOT admin_option)
 OR EXISTS(SELECT 1 FROM pg_auth_members WHERE roleid='board_posting_cooldown_owner'::regrole AND member<>'board_migrator'::regrole)
 OR has_table_privilege('board_posting_cooldown_owner','post_secrets.posting_history','UPDATE,TRUNCATE')
 OR has_table_privilege('board_posting_cooldown_owner','post_secrets.posting_actor_gates','INSERT,DELETE,TRUNCATE')
 OR has_table_privilege('board_posting_cooldown_owner','post_secrets.posting_action_capacity','INSERT,DELETE,TRUNCATE')
 OR has_schema_privilege('board_posting_cooldown_owner','content','CREATE')
 OR has_schema_privilege('board_posting_cooldown_owner','post_secrets','CREATE')
 OR has_column_privilege('board_posting_cooldown_owner','content.posts','comment','SELECT')
 OR has_column_privilege('board_posting_cooldown_owner','content.posts','deleted','UPDATE')
 OR has_column_privilege('board_posting_cooldown_owner','content.boards','posting_reply_seconds','UPDATE')
 THEN RAISE EXCEPTION 'Dedicated owner has excessive authority'; END IF;
 FOREACH fn IN ARRAY ARRAY['content.lock_posting_actor(bytea,boolean)',
   'content.check_posting_cooldown(bytea,text,bigint,boolean,bigint)',
   'content.record_inserted_posting_history()', 'content.remove_posting_history()',
   'content.remove_thread_posting_history()'] LOOP
  IF NOT EXISTS(SELECT 1 FROM pg_proc WHERE oid=fn::regprocedure
    AND proowner='board_posting_cooldown_owner'::regrole AND prosecdef
    AND proconfig=ARRAY['search_path=pg_catalog, pg_temp'])
  THEN RAISE EXCEPTION 'Unsafe function %',fn; END IF;
 END LOOP;
 FOREACH runtime IN ARRAY ARRAY['board_public','board_staff','board_auth','board_media','board_media_read','board_media_intake','board_monitor'] LOOP
  IF pg_has_role(runtime,'board_posting_cooldown_owner','MEMBER')
  THEN RAISE EXCEPTION 'Runtime % can assume dedicated owner',runtime; END IF;
  FOREACH rel IN ARRAY ARRAY['posting_history','posting_thread_actions','posting_actor_gates','posting_action_capacity'] LOOP
   IF has_table_privilege(runtime,'post_secrets.'||rel,'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
     OR EXISTS(SELECT 1 FROM pg_attribute a WHERE a.attrelid=('post_secrets.'||rel)::regclass AND a.attnum>0 AND NOT a.attisdropped
       AND has_column_privilege(runtime,a.attrelid,a.attnum,'SELECT,INSERT,UPDATE,REFERENCES'))
   THEN RAISE EXCEPTION 'Runtime % has direct access to %',runtime,rel; END IF;
  END LOOP;
  FOREACH fn IN ARRAY ARRAY['content.lock_posting_actor(bytea,boolean)',
   'content.check_posting_cooldown(bytea,text,bigint,boolean,bigint)'] LOOP
   IF has_function_privilege(runtime,fn,'EXECUTE') IS DISTINCT FROM (runtime IN('board_public','board_staff'))
   THEN RAISE EXCEPTION 'Unexpected runtime % API grant on %',runtime,fn; END IF;
  END LOOP;
  IF has_function_privilege(runtime,'content.record_inserted_posting_history()','EXECUTE')
   OR has_function_privilege(runtime,'content.remove_posting_history()','EXECUTE')
   OR has_function_privilege(runtime,'content.remove_thread_posting_history()','EXECUTE')
  THEN RAISE EXCEPTION 'Runtime can call private lifecycle triggers'; END IF;
 END LOOP;
END $$;
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,worksafe)
VALUES('newcool','New board','Synthetic',1000,100,100,100,10,false);
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM content.boards WHERE slug='newcool'
   AND posting_reply_seconds=60 AND posting_image_seconds=60 AND posting_thread_seconds=600 AND expire_neglected)
 THEN RAISE EXCEPTION 'New board defaults differ from schema contract'; END IF;
END $$;
-- Both inclusive numeric bounds and operator overrides remain writable.
UPDATE content.boards SET posting_reply_seconds=0,posting_image_seconds=86400,posting_thread_seconds=17 WHERE slug='newcool';
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM content.boards WHERE slug='newcool'
   AND posting_reply_seconds=0 AND posting_image_seconds=86400 AND posting_thread_seconds=17)
 THEN RAISE EXCEPTION 'Operator posting override failed'; END IF;
 BEGIN
  INSERT INTO post_secrets.posting_actor_gates VALUES(4096);
  RAISE EXCEPTION 'Unbounded actor gate accepted' USING ERRCODE='ZX001';
 EXCEPTION WHEN check_violation THEN NULL; END;
 BEGIN
  INSERT INTO post_secrets.posting_action_capacity VALUES(false);
  RAISE EXCEPTION 'Additional capacity accepted' USING ERRCODE='ZX001';
 EXCEPTION WHEN check_violation THEN NULL; END;
END $$;
SQL
# Connect as each actual runtime, rather than a superuser pretending via SET ROLE.
for role in board_public board_staff board_auth board_media board_media_read board_media_intake board_monitor; do
 "${psql[@]}" -U "$role" -d posting_cooldown_upgrade <<'SQL'
DO $$ DECLARE col text; rel text; BEGIN
 FOREACH col IN ARRAY ARRAY['posting_reply_seconds','posting_image_seconds','posting_thread_seconds'] LOOP
  BEGIN
   EXECUTE format('UPDATE content.boards SET %I=0 WHERE slug=''ownroll''',col);
   RAISE EXCEPTION 'Runtime wrote timer policy' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 END LOOP;
 FOREACH rel IN ARRAY ARRAY['posting_history','posting_thread_actions','posting_actor_gates','posting_action_capacity'] LOOP
  BEGIN
   EXECUTE format('SELECT 1 FROM post_secrets.%I LIMIT 1',rel);
   RAISE EXCEPTION 'Runtime read private posting state' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 END LOOP;
 BEGIN
  EXECUTE 'SET ROLE board_posting_cooldown_owner';
  RAISE EXCEPTION 'Runtime assumed posting owner' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SQL
done
for role in board_public board_staff; do
 "${psql[@]}" -U "$role" -d posting_cooldown_upgrade <<'SQL'
BEGIN;
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM content.boards WHERE slug='f' AND NOT expire_neglected)
 OR NOT EXISTS(SELECT 1 FROM content.boards WHERE slug='test' AND expire_neglected AND posting_thread_seconds=30)
 THEN RAISE EXCEPTION 'Runtime cannot read 0086/0087 board policy'; END IF;
 PERFORM content.lock_posting_actor(decode(repeat('07',32),'hex'),false);
 IF EXISTS(SELECT 1 FROM content.check_posting_cooldown(decode(repeat('07',32),'hex'),'ownroll',8811001,false,1000))
 THEN RAISE EXCEPTION 'Historical content invented cooldown'; END IF;
END $$;
ROLLBACK;
SQL
done
"${psql[@]}" -U board_public -d posting_cooldown_upgrade <<'SQL'
BEGIN;
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM content.boards WHERE slug='coolpriv')
 OR EXISTS(SELECT 1 FROM content.threads WHERE id=8811003)
 OR EXISTS(SELECT 1 FROM content.posts WHERE id=8811003)
 THEN RAISE EXCEPTION 'Private board leaked through public RLS'; END IF;
 BEGIN
  PERFORM * FROM content.check_posting_cooldown(decode(repeat('07',32),'hex'),'coolpriv',0,false,1000);
  RAISE EXCEPTION 'Private board leaked through cooldown API' USING ERRCODE='ZX001';
 EXCEPTION WHEN no_data_found THEN NULL; END;
 BEGIN
  PERFORM content.record_inserted_posting_history();
  RAISE EXCEPTION 'Public executed private history trigger' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 -- Restoring historical content must not manufacture actor provenance.
 PERFORM set_config('board.posting_actor',repeat('07',32),true);
 UPDATE content.posts SET deleted=false WHERE id=8811002;
 IF EXISTS(SELECT 1 FROM content.check_posting_cooldown(decode(repeat('07',32),'hex'),'ownroll',8811001,false,1))
 THEN RAISE EXCEPTION 'Undelete fabricated historical posting identity'; END IF;
 -- 0085 still works through its original public-only entry points.
 PERFORM content.check_public_deletion_quota(decode(repeat('08',32),'hex'));
 PERFORM content.reserve_public_deletion(decode(repeat('08',32),'hex'));
 PERFORM content.reserve_public_deletion(decode(repeat('08',32),'hex'));
 PERFORM content.reserve_public_deletion(decode(repeat('08',32),'hex'));
 BEGIN
  PERFORM content.reserve_public_deletion(decode(repeat('08',32),'hex'));
  RAISE EXCEPTION '0085 hourly deletion quota no longer enforced' USING ERRCODE='ZX001';
 EXCEPTION WHEN SQLSTATE 'P0081' THEN NULL; END;
END $$;
ROLLBACK;
SQL
"${psql[@]}" -U board_staff -d posting_cooldown_upgrade <<'SQL'
BEGIN;
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM content.boards WHERE slug='coolpriv')
 THEN RAISE EXCEPTION 'Staff lost private-board visibility'; END IF;
 PERFORM content.lock_posting_actor(decode(repeat('07',32),'hex'),true);
 PERFORM * FROM content.check_posting_cooldown(decode(repeat('07',32),'hex'),'coolpriv',0,false,1000);
END $$;
ROLLBACK;
SQL
# Exercise gate failure with real runtime credentials and restore exact state.
"${migrator[@]}" -c 'DELETE FROM post_secrets.posting_actor_gates WHERE stripe=0'
"${psql[@]}" -U board_public -d posting_cooldown_upgrade <<'SQL'
DO $$ BEGIN
 BEGIN
  PERFORM content.lock_posting_actor(decode(repeat('00',32),'hex'),false);
  RAISE EXCEPTION 'Missing actor gate failed open' USING ERRCODE='ZX001';
 EXCEPTION WHEN SQLSTATE 'P0087' THEN NULL; END;
END $$;
SQL
"${migrator[@]}" -c 'INSERT INTO post_secrets.posting_actor_gates VALUES(0); DELETE FROM post_secrets.posting_action_capacity'
"${psql[@]}" -U board_public -d posting_cooldown_upgrade <<'SQL'
DO $$ BEGIN
 BEGIN
  PERFORM content.lock_posting_actor(decode(repeat('00',32),'hex'),true);
  RAISE EXCEPTION 'Missing OP capacity gate failed open' USING ERRCODE='ZX001';
 EXCEPTION WHEN SQLSTATE 'P0087' THEN NULL; END;
END $$;
SQL
"${migrator[@]}" <<'SQL'
INSERT INTO post_secrets.posting_action_capacity VALUES(true);
-- Post-upgrade historical imports still cannot invent recoverable identities.
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(8811004,'ownroll',8811001,'Historical','','Imported without actor','2026-01-03Z');
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM post_secrets.posting_history)
 OR EXISTS(SELECT 1 FROM post_secrets.posting_thread_actions)
 OR (SELECT count(*) FROM post_secrets.posting_actor_gates)<>4096
 OR (SELECT count(*) FROM post_secrets.posting_action_capacity)<>1
 THEN RAISE EXCEPTION 'Gate checks grew persistent state or fabricated history'; END IF;
END $$;
SQL
cleanup
trap - EXIT
printf 'Posting cooldown populated upgrade passed: preserved 0086 history and 0085 quota, source/new defaults, numeric bounds, dedicated authority, runtime privacy and fixed fail-closed gates. Private cluster removed.\n'
