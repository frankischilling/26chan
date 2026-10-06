#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-staff-posting-cooldown.XXXXXXXX)
started=0
cleanup() {
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null
        started=0
    fi
    [[ $cluster =~ ^/tmp/board-staff-posting-cooldown\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
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
CREATE DATABASE staff_posting_cooldown_upgrade OWNER board_migrator;
REVOKE ALL ON DATABASE staff_posting_cooldown_upgrade FROM PUBLIC;
GRANT CONNECT ON DATABASE staff_posting_cooldown_upgrade TO board_migrator,board_public,board_staff,board_auth,
    board_media,board_media_read,board_media_intake,board_monitor;
ALTER ROLE board_staff LOGIN;
ALTER ROLE board_auth LOGIN;
ALTER ROLE board_media LOGIN;
ALTER ROLE board_media_read LOGIN;
ALTER ROLE board_media_intake LOGIN;
ALTER ROLE board_monitor LOGIN;
SQL
migrator=("${psql[@]}" -U board_migrator -d staff_posting_cooldown_upgrade)
for migration in migrations/*.sql; do
    [[ $migration != migrations/0088_staff_posting_cooldowns.sql ]] || break
    "${migrator[@]}" --single-transaction -f "$migration"
done
"${migrator[@]}" <<'SQL'
-- Synthetic populated 0087 state: owned OP, unowned deleted reply, attachment,
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
-- Keep ordinary operator overrides, legacy content and populated actor history.
UPDATE content.boards SET posting_reply_seconds=17,posting_image_seconds=23,
    posting_thread_seconds=37 WHERE slug='ownroll';
BEGIN;
SELECT set_config('board.posting_actor',repeat('11',32),true);
INSERT INTO content.threads(id,board) VALUES(8811100,'ownroll');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(8811100,'ownroll',8811100,'Synthetic','Tracked OP','Tracked OP',to_timestamp(1000)),
      (8811101,'ownroll',8811100,'Synthetic','','Older ID, later clock',to_timestamp(1100)),
      (8811102,'ownroll',8811100,'Synthetic','','Newest ID, earlier clock',to_timestamp(1000));
COMMIT;
BEGIN;
SELECT set_config('board.posting_actor',repeat('22',32),true);
INSERT INTO content.threads(id,board) VALUES(8811200,'ownroll');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(8811200,'ownroll',8811200,'Synthetic','OP only','OP must enter staff history',to_timestamp(2000));
COMMIT;
BEGIN;
SELECT set_config('board.posting_actor',repeat('33',32),true);
INSERT INTO content.threads(id,board) VALUES(8811300,'coolpriv');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(8811300,'coolpriv',8811300,'Synthetic','Private OP','Private tracked OP',to_timestamp(3000));
COMMIT;
-- Owned synthetic proof rows exercise scoped cleanup without issuing real authority.
DO $$ DECLARE actor bigint; BEGIN
 INSERT INTO staff_identity.accounts(role) VALUES('admin') RETURNING id INTO actor;
 INSERT INTO staff_identity.credentials(id,account_id,credential)
 VALUES(convert_to('staff-timer-fixture','UTF8'),actor,'{}');
 INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id)
 VALUES(decode(repeat('55',32),'hex'),decode(repeat('66',32),'hex'),actor,convert_to('staff-timer-fixture','UTF8'));
 INSERT INTO post_secrets.staff_post_intents(token_hash,session_hash,account_id,capcode,post_id,board,
   thread_id,name,subject,comment,posted_at,idle_seconds)
 VALUES(decode(repeat('77',32),'hex'),decode(repeat('55',32),'hex'),actor,'admin',8811400,'ownroll',8811100,
   'Synthetic','','Rejected badge proof',to_timestamp(4000),900);
 INSERT INTO post_secrets.staff_post_intents(token_hash,session_hash,account_id,capcode,post_id,board,
   thread_id,name,subject,comment,posted_at,idle_seconds,ordinary,source_options,source_name_allowed,
   ordinary_context,ordinary_policy)
 VALUES(decode(repeat('88',32),'hex'),decode(repeat('55',32),'hex'),actor,'none',8811401,'ownroll',8811100,
   'Anonymous','','Ordinary proof retained',to_timestamp(4000),900,true,'',false,'{}','{}');
END $$;
-- Snapshot every application table, including private history and staff state.
CREATE TABLE public.staff_timer_rows_before(relation text,value jsonb);
DO $$ DECLARE r record; BEGIN
 FOR r IN SELECT n.nspname,c.relname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
  WHERE n.nspname IN ('content','post_secrets','staff_identity','media','deployment') AND c.relkind='r' LOOP
  EXECUTE format('INSERT INTO public.staff_timer_rows_before SELECT %L,to_jsonb(r) FROM %I.%I r',
    r.nspname||'.'||r.relname,r.nspname,r.relname);
 END LOOP;
 IF (SELECT count(*) FROM post_secrets.posting_history)<>5
 OR NOT EXISTS(SELECT 1 FROM post_secrets.posting_history WHERE post_id=thread_id)
 OR NOT EXISTS(SELECT 1 FROM post_secrets.posting_history WHERE post_id<>thread_id)
 THEN RAISE EXCEPTION 'Populated 0087 OP/reply fixture missing'; END IF;
END $$;
CREATE TABLE public.staff_timer_policies_before AS SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy;
CREATE TABLE public.staff_timer_functions_before AS
 SELECT p.oid,p.proowner,p.proacl::text AS acl,p.prosecdef,p.proconfig,p.prosrc,p.prorettype,p.proargtypes
 FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
 WHERE n.nspname NOT IN ('pg_catalog','information_schema');
CREATE TABLE public.staff_timer_relations_before AS
 SELECT c.oid,c.relowner,c.relacl::text AS acl,c.relrowsecurity,c.relforcerowsecurity
 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname IN ('content','post_secrets','staff_identity','media','deployment');
DO $$ BEGIN
 IF to_regprocedure('content.check_staff_posting_cooldown(bytea,text,bigint)') IS NOT NULL
 THEN RAISE EXCEPTION 'Pre-upgrade schema unexpectedly has staff timer'; END IF;
END $$;
SQL
# The new staff readiness dependency is absent before the actual migration.
"${psql[@]}" -U board_staff -d staff_posting_cooldown_upgrade <<'SQL'
DO $$ BEGIN
 BEGIN
  PERFORM * FROM content.check_staff_posting_cooldown(decode(repeat('11',32),'hex'),'ownroll',1004);
  RAISE EXCEPTION 'Pre-0088 staff schema accepted new timer API' USING ERRCODE='ZX001';
 EXCEPTION WHEN undefined_function THEN NULL; END;
END $$;
SQL
"${migrator[@]}" --single-transaction -f migrations/0088_staff_posting_cooldowns.sql
"${migrator[@]}" <<'SQL'
CREATE TABLE public.staff_timer_rows_after(LIKE public.staff_timer_rows_before);
DO $$ DECLARE r record; runtime text; rel text; fn regprocedure;
BEGIN
 FOR r IN SELECT n.nspname,c.relname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
  WHERE n.nspname IN ('content','post_secrets','staff_identity','media','deployment') AND c.relkind='r' LOOP
  EXECUTE format('INSERT INTO public.staff_timer_rows_after SELECT %L,to_jsonb(r) FROM %I.%I r',
    r.nspname||'.'||r.relname,r.nspname,r.relname);
 END LOOP;
 IF EXISTS(TABLE public.staff_timer_rows_before EXCEPT ALL TABLE public.staff_timer_rows_after)
 OR EXISTS(TABLE public.staff_timer_rows_after EXCEPT ALL TABLE public.staff_timer_rows_before)
 THEN RAISE EXCEPTION '0088 changed populated content, policy, secrets or clocks'; END IF;
 IF EXISTS(TABLE public.staff_timer_policies_before EXCEPT
   SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy)
 OR EXISTS(SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy
   EXCEPT TABLE public.staff_timer_policies_before)
 OR EXISTS(TABLE public.staff_timer_functions_before EXCEPT
   SELECT p.oid,p.proowner,p.proacl::text,p.prosecdef,p.proconfig,p.prosrc,p.prorettype,p.proargtypes
   FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
   WHERE n.nspname NOT IN ('pg_catalog','information_schema'))
 OR EXISTS(SELECT 1 FROM public.staff_timer_relations_before old LEFT JOIN pg_class c USING(oid)
   WHERE c.oid IS NULL OR (old.relowner,old.acl,old.relrowsecurity,old.relforcerowsecurity)
     IS DISTINCT FROM (c.relowner,c.relacl::text,c.relrowsecurity,c.relforcerowsecurity))
 THEN RAISE EXCEPTION '0088 changed existing RLS, functions or relation authority'; END IF;
 fn:='content.check_staff_posting_cooldown(bytea,text,bigint)'::regprocedure;
 IF NOT EXISTS(SELECT 1 FROM pg_proc WHERE oid=fn
   AND proowner='board_posting_cooldown_owner'::regrole AND prosecdef
   AND proconfig=ARRAY['search_path=pg_catalog, pg_temp'])
 OR EXISTS(SELECT 1 FROM pg_proc p, LATERAL aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a
   WHERE p.oid=fn AND a.grantee=0 AND a.privilege_type='EXECUTE')
 THEN RAISE EXCEPTION 'Unsafe staff timer definer or PUBLIC execute grant'; END IF;
 IF NOT EXISTS(SELECT 1 FROM pg_index WHERE indexrelid='post_secrets.posting_history_staff'::regclass
   AND indrelid='post_secrets.posting_history'::regclass AND indisvalid AND indisready
   AND indpred IS NULL AND pg_get_indexdef(indexrelid) LIKE '%(board, actor_hash, post_id DESC)%')
 THEN RAISE EXCEPTION 'Staff newest-ID index missing or excludes OPs'; END IF;
 IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='board_posting_cooldown_owner'
   AND NOT rolcanlogin AND NOT rolsuper AND NOT rolcreatedb AND NOT rolcreaterole
   AND NOT rolreplication AND NOT rolbypassrls)
 OR has_schema_privilege('board_posting_cooldown_owner','content','CREATE')
 OR has_schema_privilege('board_posting_cooldown_owner','post_secrets','CREATE')
 THEN RAISE EXCEPTION 'Staff timer owner gained unsafe authority'; END IF;
 IF NOT EXISTS(SELECT 1 FROM pg_proc WHERE oid='staff_identity.discard_badged_post_authority(bytea,bytea)'::regprocedure
   AND proowner='board_staff_post_owner'::regrole AND prosecdef
   AND proconfig=ARRAY['search_path=pg_catalog, pg_temp'])
 OR EXISTS(SELECT 1 FROM pg_proc p, LATERAL aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a
   WHERE p.oid='staff_identity.discard_badged_post_authority(bytea,bytea)'::regprocedure
   AND a.grantee=0 AND a.privilege_type='EXECUTE')
 OR has_schema_privilege('board_staff_post_owner','staff_identity','CREATE')
 THEN RAISE EXCEPTION 'Unsafe badge proof cleanup authority'; END IF;
 FOREACH runtime IN ARRAY ARRAY['board_public','board_staff','board_auth','board_media','board_media_read','board_media_intake','board_monitor'] LOOP
  IF has_function_privilege(runtime,'staff_identity.discard_badged_post_authority(bytea,bytea)','EXECUTE')
    IS DISTINCT FROM (runtime='board_auth')
  THEN RAISE EXCEPTION 'Unexpected badge cleanup privilege for %',runtime; END IF;
  IF has_function_privilege(runtime,fn,'EXECUTE') IS DISTINCT FROM (runtime='board_staff')
  OR pg_has_role(runtime,'board_posting_cooldown_owner','MEMBER')
  THEN RAISE EXCEPTION 'Unexpected staff timer privilege for %',runtime; END IF;
  FOREACH rel IN ARRAY ARRAY['posting_history','posting_thread_actions','posting_actor_gates','posting_action_capacity'] LOOP
   IF has_table_privilege(runtime,'post_secrets.'||rel,'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
   OR EXISTS(SELECT 1 FROM pg_attribute a WHERE a.attrelid=('post_secrets.'||rel)::regclass AND a.attnum>0 AND NOT a.attisdropped
     AND has_column_privilege(runtime,a.attrelid,a.attnum,'SELECT,INSERT,UPDATE,REFERENCES'))
   THEN RAISE EXCEPTION 'Runtime % has direct access to %',runtime,rel; END IF;
  END LOOP;
 END LOOP;
END $$;
SQL
# Actual runtime logins preserve invoker behavior; do not emulate with SET ROLE.
for role in board_public board_staff board_auth board_media board_media_read board_media_intake board_monitor; do
 "${psql[@]}" -U "$role" -d staff_posting_cooldown_upgrade <<'SQL'
DO $$ DECLARE rel text; BEGIN
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
 IF session_user<>'board_auth' THEN
  BEGIN
   PERFORM staff_identity.discard_badged_post_authority(decode(repeat('77',32),'hex'),decode(repeat('55',32),'hex'));
   RAISE EXCEPTION 'Non-auth runtime discarded badge proof' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 END IF;
 IF session_user<>'board_staff' THEN
  BEGIN
   PERFORM * FROM content.check_staff_posting_cooldown(decode(repeat('11',32),'hex'),'ownroll',1004);
   RAISE EXCEPTION 'Non-staff executed staff timer' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 END IF;
END $$;
SQL
done
"${psql[@]}" -U board_public -d staff_posting_cooldown_upgrade <<'SQL'
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM content.boards WHERE slug='coolpriv')
 OR EXISTS(SELECT 1 FROM content.posts WHERE board='coolpriv')
 THEN RAISE EXCEPTION 'Private staff history content leaked through public RLS'; END IF;
END $$;
SQL
"${psql[@]}" -U board_staff -d staff_posting_cooldown_upgrade <<'SQL'
BEGIN;
DO $$ DECLARE actor bytea; bad_actor bytea; bad_time bigint; BEGIN
 actor:=decode(repeat('11',32),'hex');
 PERFORM content.lock_posting_actor(actor,false);
 IF (SELECT count(*) FROM content.check_staff_posting_cooldown(actor,'ownroll',1004)
     WHERE kind='reply' AND remaining_seconds=1)<>1
 THEN RAISE EXCEPTION 'Four-second staff boundary did not reject for one second'; END IF;
 IF EXISTS(SELECT 1 FROM content.check_staff_posting_cooldown(actor,'ownroll',1005))
 THEN RAISE EXCEPTION 'Five-second equality rejected or maximum clock replaced newest post ID'; END IF;
 IF EXISTS(SELECT 1 FROM content.check_staff_posting_cooldown(decode(repeat('44',32),'hex'),'ownroll',1004))
 THEN RAISE EXCEPTION 'Unrelated actor inherited history'; END IF;
 IF EXISTS(SELECT 1 FROM content.check_staff_posting_cooldown(actor,'coolpriv',1004))
 THEN RAISE EXCEPTION 'Other board inherited history'; END IF;
 actor:=decode(repeat('22',32),'hex');
 PERFORM content.lock_posting_actor(actor,true);
 IF (SELECT count(*) FROM content.check_staff_posting_cooldown(actor,'ownroll',2004)
     WHERE kind='reply' AND remaining_seconds=1)<>1
 OR EXISTS(SELECT 1 FROM content.check_staff_posting_cooldown(actor,'ownroll',2005))
 THEN RAISE EXCEPTION 'OP-only staff history or strict boundary failed'; END IF;
 actor:=decode(repeat('33',32),'hex');
 PERFORM content.lock_posting_actor(actor,true);
 IF NOT EXISTS(SELECT 1 FROM content.boards WHERE slug='coolpriv')
 OR (SELECT count(*) FROM content.check_staff_posting_cooldown(actor,'coolpriv',3004)
     WHERE kind='reply' AND remaining_seconds=1)<>1
 OR EXISTS(SELECT 1 FROM content.check_staff_posting_cooldown(actor,'coolpriv',3005))
 THEN RAISE EXCEPTION 'Staff private-board timer failed'; END IF;
 FOREACH bad_actor IN ARRAY ARRAY[NULL::bytea,decode(repeat('00',31),'hex'),decode(repeat('00',33),'hex')] LOOP
  BEGIN
   PERFORM * FROM content.check_staff_posting_cooldown(bad_actor,'ownroll',1005);
   RAISE EXCEPTION 'Malformed staff actor accepted' USING ERRCODE='ZX001';
  EXCEPTION WHEN check_violation THEN NULL; END;
 END LOOP;
 FOREACH bad_time IN ARRAY ARRAY[NULL::bigint,-1::bigint,9223372036854689408::bigint] LOOP
  BEGIN
   PERFORM * FROM content.check_staff_posting_cooldown(actor,'ownroll',bad_time);
   RAISE EXCEPTION 'Malformed staff request time accepted' USING ERRCODE='ZX001';
  EXCEPTION WHEN check_violation THEN NULL; END;
 END LOOP;
 BEGIN
  PERFORM * FROM content.check_staff_posting_cooldown(actor,'missing',1005);
  RAISE EXCEPTION 'Missing board accepted' USING ERRCODE='ZX001';
 EXCEPTION WHEN no_data_found THEN NULL; END;
END $$;
ROLLBACK;
SQL
"${psql[@]}" -U board_auth -d staff_posting_cooldown_upgrade <<'SQL'
SELECT staff_identity.discard_badged_post_authority(decode(repeat('99',32),'hex'),decode(repeat('55',32),'hex'));
SELECT staff_identity.discard_badged_post_authority(decode(repeat('77',32),'hex'),decode(repeat('99',32),'hex'));
SELECT staff_identity.discard_badged_post_authority(decode(repeat('88',32),'hex'),decode(repeat('55',32),'hex'));
DO $$ DECLARE bad bytea; BEGIN
 FOREACH bad IN ARRAY ARRAY[NULL::bytea,decode(repeat('00',31),'hex'),decode(repeat('00',33),'hex')] LOOP
  BEGIN
   PERFORM staff_identity.discard_badged_post_authority(bad,decode(repeat('55',32),'hex'));
   RAISE EXCEPTION 'Malformed cleanup ticket accepted' USING ERRCODE='ZX001';
  EXCEPTION WHEN check_violation THEN NULL; END;
  BEGIN
   PERFORM staff_identity.discard_badged_post_authority(decode(repeat('77',32),'hex'),bad);
   RAISE EXCEPTION 'Malformed cleanup session accepted' USING ERRCODE='ZX001';
  EXCEPTION WHEN check_violation THEN NULL; END;
 END LOOP;
END $$;
SQL
"${migrator[@]}" <<'SQL'
DO $$ BEGIN
 IF (SELECT count(*) FROM post_secrets.staff_post_intents)<>2
 THEN RAISE EXCEPTION 'Wrong ticket/session or badge cleanup removed ordinary proof'; END IF;
END $$;
SQL
"${psql[@]}" -U board_auth -d staff_posting_cooldown_upgrade <<'SQL'
SELECT staff_identity.discard_badged_post_authority(decode(repeat('77',32),'hex'),decode(repeat('55',32),'hex'));
-- A retry is harmless and does not widen the cleanup scope.
SELECT staff_identity.discard_badged_post_authority(decode(repeat('77',32),'hex'),decode(repeat('55',32),'hex'));
SQL
"${migrator[@]}" <<'SQL'
DO $$ BEGIN
 IF (SELECT count(*) FROM post_secrets.staff_post_intents)<>1
 OR NOT EXISTS(SELECT 1 FROM post_secrets.staff_post_intents
   WHERE token_hash=decode(repeat('88',32),'hex') AND ordinary)
 THEN RAISE EXCEPTION 'Exact badge cleanup failed or changed ordinary proof'; END IF;
END $$;
SQL
cleanup
trap - EXIT
printf 'Staff posting timer populated upgrade passed: preserved 0087 rows and authority, newest-ID OP/reply strict boundaries, runtime privacy, malformed inputs and private-board behavior. Private cluster removed.\n'
