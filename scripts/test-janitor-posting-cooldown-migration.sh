#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-janitor-posting-cooldown.XXXXXXXX)
started=0
cleanup() {
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null
        started=0
    fi
    [[ $cluster =~ ^/tmp/board-janitor-posting-cooldown\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
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
CREATE DATABASE janitor_posting_cooldown_upgrade OWNER board_migrator;
REVOKE ALL ON DATABASE janitor_posting_cooldown_upgrade FROM PUBLIC;
GRANT CONNECT ON DATABASE janitor_posting_cooldown_upgrade TO board_migrator,board_public,board_staff,board_auth,
    board_media,board_media_read,board_media_intake,board_monitor;
ALTER ROLE board_staff LOGIN;
ALTER ROLE board_auth LOGIN;
ALTER ROLE board_media LOGIN;
ALTER ROLE board_media_read LOGIN;
ALTER ROLE board_media_intake LOGIN;
ALTER ROLE board_monitor LOGIN;
SQL
migrator=("${psql[@]}" -U board_migrator -d janitor_posting_cooldown_upgrade)
for migration in migrations/*.sql; do
    [[ $migration != migrations/0089_janitor_posting_cooldowns.sql ]] || break
    "${migrator[@]}" --single-transaction -f "$migration"
done
"${migrator[@]}" <<'SQL'
-- Synthetic populated 0088 state: owned OP, unowned deleted reply, attachment,
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
-- A scoped janitor, separate from the admin above, drives authenticated branch tests.
DO $$ DECLARE actor bigint; BEGIN
 INSERT INTO staff_identity.accounts(role,allow_boards) VALUES('janitor',ARRAY['ownroll','coolpriv']) RETURNING id INTO actor;
 INSERT INTO staff_identity.credentials(id,account_id,credential)
 VALUES(convert_to('janitor-timer-fixture','UTF8'),actor,'{}');
 INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id)
 VALUES(decode(repeat('aa',32),'hex'),decode(repeat('bb',32),'hex'),actor,convert_to('janitor-timer-fixture','UTF8'));
END $$;
UPDATE staff_identity.accounts SET allow_boards=ARRAY['all'] WHERE role='admin';
UPDATE content.boards SET user_ids=false,country_flags=false,op_markup=false,forced_anon=false,
 strip_tripcode=false,meta_board=false WHERE slug='ownroll';
UPDATE content.boards SET max_comment_chars=1000 WHERE slug='j';
CREATE TABLE public.janitor_rows_before(relation text,value jsonb);
DO $$ DECLARE r record; BEGIN
 FOR r IN SELECT n.nspname,c.relname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
  WHERE n.nspname IN ('content','post_secrets','staff_identity','media','deployment','admission') AND c.relkind='r' LOOP
  EXECUTE format('INSERT INTO public.janitor_rows_before SELECT %L,to_jsonb(r) FROM %I.%I r',
   r.nspname||'.'||r.relname,r.nspname,r.relname);
 END LOOP;
 IF (SELECT count(*) FROM post_secrets.posting_history)<>5 OR (SELECT count(*) FROM post_secrets.staff_post_intents)<>2
 THEN RAISE EXCEPTION 'Populated 0088 history/proof fixture missing'; END IF;
END $$;
CREATE TABLE public.janitor_policies_before AS SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy;
CREATE TABLE public.janitor_relations_before AS SELECT c.oid,c.relowner,c.relacl::text AS acl,c.relrowsecurity,c.relforcerowsecurity
 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname IN ('content','post_secrets','staff_identity','media','deployment','admission');
CREATE TABLE public.janitor_functions_before AS
 SELECT p.oid,p.proowner,p.proacl::text AS acl,p.prosecdef,p.proconfig,p.prosrc,p.prorettype,p.proargtypes
 FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
 WHERE n.nspname NOT IN ('pg_catalog','information_schema') AND p.proname NOT IN
 ('issue_post_authority','issue_wordfiltered_post_authority','issue_limited_post_authority',
  'issue_source_post_authority','issue_ordinary_post_authority','consume_staff_post_authority',
  'apply_staff_capcode','check_posting_cooldown');
SQL
"${migrator[@]}" --single-transaction -f migrations/0089_janitor_posting_cooldowns.sql
"${migrator[@]}" <<'SQL'
CREATE TABLE public.janitor_rows_after(LIKE public.janitor_rows_before);
DO $$ DECLARE r record; BEGIN
 FOR r IN SELECT n.nspname,c.relname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
  WHERE n.nspname IN ('content','post_secrets','staff_identity','media','deployment','admission') AND c.relkind='r' LOOP
  EXECUTE format('INSERT INTO public.janitor_rows_after SELECT %L,to_jsonb(r)%s FROM %I.%I r',
   r.nspname||'.'||r.relname,CASE WHEN r.nspname='post_secrets' AND r.relname='staff_post_intents'
   THEN '-ARRAY[''raw_name_nonempty'',''is_janitor'',''meta_board'']' ELSE '' END,r.nspname,r.relname);
 END LOOP;
 IF EXISTS(TABLE public.janitor_rows_before EXCEPT ALL TABLE public.janitor_rows_after)
 OR EXISTS(TABLE public.janitor_rows_after EXCEPT ALL TABLE public.janitor_rows_before)
 OR EXISTS(SELECT 1 FROM post_secrets.staff_post_intents
  WHERE raw_name_nonempty IS NOT NULL OR is_janitor IS NOT NULL OR meta_board IS NOT NULL)
 THEN RAISE EXCEPTION '0089 changed populated state or guessed historical timer context'; END IF;
 IF EXISTS(TABLE public.janitor_policies_before EXCEPT SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy)
 OR EXISTS(SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy EXCEPT TABLE public.janitor_policies_before)
 OR EXISTS(SELECT 1 FROM public.janitor_relations_before old LEFT JOIN pg_class c USING(oid)
  WHERE c.oid IS NULL OR (old.relowner,old.acl,old.relrowsecurity,old.relforcerowsecurity)
   IS DISTINCT FROM (c.relowner,c.relacl::text,c.relrowsecurity,c.relforcerowsecurity))
 OR EXISTS(TABLE public.janitor_functions_before EXCEPT
  SELECT p.oid,p.proowner,p.proacl::text,p.prosecdef,p.proconfig,p.prosrc,p.prorettype,p.proargtypes FROM pg_proc p)
 THEN RAISE EXCEPTION '0089 changed unrelated RLS, functions or relation authority'; END IF;
END $$;
-- Every entrypoint is classified by actual signature, never a cosmetic capcode.
DO $$ DECLARE p record; runtime text; allowed text; expected_owner text; BEGIN
 IF (SELECT count(*) FROM pg_proc proc JOIN pg_namespace n ON n.oid=proc.pronamespace
  WHERE n.nspname='staff_identity' AND proc.proname IN
  ('issue_limited_post_authority','issue_source_post_authority','issue_ordinary_post_authority')
  AND proc.prorettype='boolean'::regtype)<>3 THEN RAISE EXCEPTION 'Current authenticated issuers missing'; END IF;
 FOR p IN SELECT proc.* FROM pg_proc proc JOIN pg_namespace n ON n.oid=proc.pronamespace
  WHERE (n.nspname='staff_identity' AND proc.proname IN ('issue_post_authority','issue_wordfiltered_post_authority',
   'issue_limited_post_authority','issue_source_post_authority','issue_ordinary_post_authority','bind_post_timer_context'))
  OR (n.nspname='content' AND proc.proname IN ('consume_staff_post_authority','consume_staff_post_authority_core',
   'check_posting_cooldown','check_posting_cooldown_core','check_janitor_posting_cooldown')) LOOP
  allowed:=CASE WHEN p.proname LIKE 'issue_%' AND p.prorettype='boolean'::regtype THEN 'board_auth'
   WHEN p.proname IN ('consume_staff_post_authority','check_janitor_posting_cooldown') THEN 'board_staff' ELSE NULL END;
  expected_owner:=CASE WHEN p.proname LIKE 'check_%' THEN 'board_posting_cooldown_owner' ELSE 'board_staff_post_owner' END;
  IF p.proowner<>expected_owner::regrole OR p.proconfig IS DISTINCT FROM ARRAY['search_path=pg_catalog, pg_temp']
   OR (p.proname<>'bind_post_timer_context' AND NOT p.prosecdef)
   OR EXISTS(SELECT 1 FROM aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a
    WHERE a.grantee=0 AND a.privilege_type='EXECUTE')
  THEN RAISE EXCEPTION 'Unsafe owner/search path/PUBLIC grant for %',p.oid::regprocedure; END IF;
  FOREACH runtime IN ARRAY ARRAY['board_public','board_staff','board_auth','board_media','board_media_read','board_media_intake','board_monitor'] LOOP
   IF has_function_privilege(runtime,p.oid,'EXECUTE') IS DISTINCT FROM
    (coalesce(runtime=allowed,false) OR (p.proname='check_posting_cooldown' AND runtime IN ('board_public','board_staff')))
   THEN RAISE EXCEPTION 'Unexpected % execution of %',runtime,p.oid::regprocedure; END IF;
  END LOOP;
 END LOOP;
END $$;
SQL
# Real runtime logins exercise invoker and ACL behavior, not migrator SET ROLE.
for role in board_public board_staff board_auth board_media board_media_read board_media_intake board_monitor; do
 "${psql[@]}" -U "$role" -d janitor_posting_cooldown_upgrade <<'SQL'
DO $$ DECLARE p record; args text; rel text; BEGIN
 FOREACH rel IN ARRAY ARRAY['staff_post_intents','posting_history','posting_thread_actions'] LOOP
  BEGIN
   EXECUTE format('SELECT 1 FROM post_secrets.%I LIMIT 1',rel);
   RAISE EXCEPTION 'Runtime read private timer/proof state' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 END LOOP;
 FOR p IN SELECT proc.*,n.nspname FROM pg_proc proc JOIN pg_namespace n ON n.oid=proc.pronamespace
  WHERE (n.nspname='staff_identity' AND proc.proname IN ('issue_post_authority','issue_wordfiltered_post_authority',
   'issue_limited_post_authority','issue_source_post_authority','issue_ordinary_post_authority') AND proc.prorettype='void'::regtype)
  OR (n.nspname='content' AND proc.proname IN ('consume_staff_post_authority_core','check_posting_cooldown_core'))
  OR (n.nspname='staff_identity' AND proc.proname='bind_post_timer_context') LOOP
  SELECT string_agg('NULL::'||t::regtype::text,',' ORDER BY ord) INTO args FROM unnest(p.proargtypes::oid[]) WITH ORDINALITY a(t,ord);
  BEGIN
   EXECUTE format('SELECT %I.%I(%s)',p.nspname,p.proname,args);
   RAISE EXCEPTION 'Runtime executed retired/private API' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 END LOOP;
 IF session_user<>'board_staff' THEN
  BEGIN
   PERFORM * FROM content.check_janitor_posting_cooldown(decode(repeat('11',32),'hex'),'ownroll',8811100,false,1009);
   RAISE EXCEPTION 'Nonstaff requested janitor discount' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 END IF;
END $$;
SQL
done
"${migrator[@]}" <<'SQL'
-- After preservation checks, refresh only the owned old proof's expiry so
-- rejection cannot be explained by its lifetime elapsing during setup.
UPDATE post_secrets.staff_post_intents SET expires_at=clock_timestamp()+interval '15 seconds'
 WHERE token_hash=decode(repeat('77',32),'hex');
SQL
"${psql[@]}" -U board_staff -d janitor_posting_cooldown_upgrade <<'SQL'
BEGIN;
SELECT set_config('board.staff_raw_name_nonempty','false',true);
DO $$ BEGIN
 BEGIN
  PERFORM content.consume_staff_post_authority(decode(repeat('77',32),'hex'),8811400,'ownroll',8811100,
   'Synthetic','','Rejected badge proof',to_timestamp(4000));
  RAISE EXCEPTION 'Unbound pre-0089 proof consumed' USING ERRCODE='ZX001';
 EXCEPTION WHEN invalid_authorization_specification THEN NULL; END;
END $$;
ROLLBACK;
BEGIN;
DO $$ DECLARE actor bytea:=decode(repeat('11',32),'hex'); BEGIN
 PERFORM content.lock_posting_actor(actor,true);
 -- Odd intervals round upward: ceil(17/2)=9 and ceil(23/2)=12.
 IF (SELECT count(*) FROM content.check_janitor_posting_cooldown(actor,'ownroll',8811100,false,1008)
  WHERE kind='reply' AND remaining_seconds=1)<>1
 OR EXISTS(SELECT 1 FROM content.check_janitor_posting_cooldown(actor,'ownroll',8811100,false,1009))
 OR (SELECT count(*) FROM content.check_janitor_posting_cooldown(actor,'ownroll',8811100,true,1011)
  WHERE kind='image' AND remaining_seconds=1)<>1
 OR EXISTS(SELECT 1 FROM content.check_janitor_posting_cooldown(actor,'ownroll',8811100,true,1012))
 THEN RAISE EXCEPTION 'Half reply/image strict edge or latest post-number history failed'; END IF;
 IF (SELECT count(*) FROM content.check_janitor_posting_cooldown(actor,'ownroll',0,false,1036)
  WHERE kind='thread' AND remaining_seconds=1)<>1
 OR EXISTS(SELECT 1 FROM content.check_janitor_posting_cooldown(actor,'ownroll',0,false,1037))
 THEN RAISE EXCEPTION 'Janitor incorrectly discounted OP interval'; END IF;
 IF (SELECT count(*) FROM content.check_staff_posting_cooldown(actor,'ownroll',1004)
  WHERE kind='reply' AND remaining_seconds=1)<>1
 OR EXISTS(SELECT 1 FROM content.check_staff_posting_cooldown(actor,'ownroll',1005))
 THEN RAISE EXCEPTION 'Independent five-second gate changed'; END IF;
END $$;
ROLLBACK;
SQL
"${psql[@]}" -U board_public -d janitor_posting_cooldown_upgrade <<'SQL'
BEGIN;
DO $$ DECLARE actor bytea:=decode(repeat('11',32),'hex'); BEGIN
 PERFORM content.lock_posting_actor(actor,false);
 IF (SELECT count(*) FROM content.check_posting_cooldown(actor,'ownroll',8811100,false,1009)
  WHERE kind='reply' AND remaining_seconds=8)<>1
 THEN RAISE EXCEPTION 'Public path inherited janitor discount'; END IF;
END $$;
ROLLBACK;
SQL
# Cross-board edge uses the database clock. Retry only if a second tick makes
# the measurement ambiguous; never weaken the inclusive 300-second assertion.
runuser -u postgres -- "${psql[@]}" -d janitor_posting_cooldown_upgrade <<'SQL'
BEGIN;
DO $$ DECLARE actor bytea:=decode(repeat('44',32),'hex'); now_s bigint; attempt integer; blocked boolean; BEGIN
 PERFORM set_config('role','board_staff',true);
 PERFORM content.lock_posting_actor(actor,true);
 PERFORM set_config('role','none',true);
 FOR attempt IN 1..10 LOOP
  now_s:=floor(extract(epoch FROM clock_timestamp()))::bigint;
  INSERT INTO post_secrets.posting_thread_actions(actor_hash,board,request_at) VALUES(actor,'coolpriv',now_s-300)
   ON CONFLICT(actor_hash,board) DO UPDATE SET request_at=excluded.request_at;
  PERFORM set_config('role','board_staff',true);
  SELECT EXISTS(SELECT 1 FROM content.check_janitor_posting_cooldown(actor,'ownroll',0,false,now_s)
   WHERE kind='cross_board_thread' AND remaining_seconds=1) INTO blocked;
  PERFORM set_config('role','none',true);
  IF floor(extract(epoch FROM clock_timestamp()))::bigint<>now_s THEN CONTINUE; END IF;
  IF NOT blocked THEN RAISE EXCEPTION 'Janitor discounted inclusive cross-board edge'; END IF;
  UPDATE post_secrets.posting_thread_actions SET request_at=now_s-301 WHERE actor_hash=actor AND board='coolpriv';
  PERFORM set_config('role','board_staff',true);
  IF EXISTS(SELECT 1 FROM content.check_janitor_posting_cooldown(actor,'ownroll',0,false,now_s))
  THEN RAISE EXCEPTION 'Expired cross-board edge blocked'; END IF;
  RETURN;
 END LOOP;
 RAISE EXCEPTION 'Could not obtain an unambiguous whole-second edge measurement';
END $$;
ROLLBACK;
SQL
# Keep issuance/consumption adjacent: proofs retain their 15-second lifetime.
# A deployment must drain old writers and retry in-flight old proofs with a
# matching new binary. It must never infer raw-name context for old tickets.
"${psql[@]}" -U board_auth -d janitor_posting_cooldown_upgrade <<'SQL'
DO $$ DECLARE ctx jsonb; raw boolean; ticket bytea; branch boolean; BEGIN
 ctx:=jsonb_build_object('poster_id','','poster_fingerprint',repeat('11',32),'poster_epoch',repeat('22',32),
  'post_sage','false','country','','country_name','','flag','','source_op_reply','false',
  'dice_result','','fortune_text','','fortune_color','','peer','192.0.2.89','deletion_hash','owned-hash','op_password_proof','');
 FOREACH raw IN ARRAY ARRAY[false,true] LOOP
  ticket:=decode(repeat(CASE WHEN raw THEN 'c1' ELSE 'c0' END,32),'hex');
  branch:=staff_identity.issue_ordinary_post_authority(ticket,decode(repeat('aa',32),'hex'),decode(repeat('bb',32),'hex'),
   900,8811500,'ownroll',8811100,'Anonymous','','Owned ordinary proof',to_timestamp(4000),false,1000,
   NULL,NULL,'',NULL,true,ctx,raw);
  IF branch IS DISTINCT FROM raw THEN RAISE EXCEPTION 'Ordinary issuer ignored raw-name janitor branch'; END IF;
 END LOOP;
 IF staff_identity.issue_limited_post_authority(decode(repeat('c2',32),'hex'),decode(repeat('55',32),'hex'),
  decode(repeat('66',32),'hex'),900,false,8811501,'ownroll',8811100,'Synthetic','','Bound admin',to_timestamp(4000),
  false,1000,NULL,NULL,true) IS DISTINCT FROM false
 THEN RAISE EXCEPTION 'Admin incorrectly entered janitor branch'; END IF;
 IF staff_identity.issue_source_post_authority(decode(repeat('c3',32),'hex'),decode(repeat('55',32),'hex'),
  decode(repeat('66',32),'hex'),900,false,8811502,'ownroll',8811100,'Synthetic','','Bound source admin',to_timestamp(4000),
  true,10000,NULL,NULL,'capcode_admin',NULL,true,true) IS DISTINCT FROM false
 THEN RAISE EXCEPTION 'Source badge incorrectly selected janitor branch'; END IF;
 IF staff_identity.issue_limited_post_authority(decode(repeat('c4',32),'hex'),decode(repeat('aa',32),'hex'),
  decode(repeat('bb',32),'hex'),900,false,8811503,'j',8811503,'Anonymous','','Private janitor',to_timestamp(4000),
  false,1000,NULL,NULL,true) IS DISTINCT FROM true
 THEN RAISE EXCEPTION 'Private issuer did not return authenticated janitor branch'; END IF;
END $$;
SQL
"${psql[@]}" -U board_staff -d janitor_posting_cooldown_upgrade <<'SQL'
BEGIN;
SELECT set_config('board.staff_raw_name_nonempty','false',true);
DO $$ BEGIN
 BEGIN
  PERFORM content.consume_staff_post_authority(decode(repeat('c2',32),'hex'),8811501,'ownroll',8811100,
   'Synthetic','','Bound admin',to_timestamp(4000));
  RAISE EXCEPTION 'Raw-name mismatch consumed proof' USING ERRCODE='ZX001';
 EXCEPTION WHEN invalid_authorization_specification THEN NULL; END;
 PERFORM set_config('board.staff_raw_name_nonempty','true',true);
 IF content.consume_staff_post_authority(decode(repeat('c2',32),'hex'),8811501,'ownroll',8811100,
  'Synthetic','','Bound admin',to_timestamp(4000)) IS DISTINCT FROM 'admin'
 THEN RAISE EXCEPTION 'Matching bound proof failed consumption'; END IF;
END $$;
ROLLBACK;
SQL
"${migrator[@]}" <<'SQL'
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM post_secrets.staff_post_intents WHERE token_hash=decode(repeat('c1',32),'hex')
  AND raw_name_nonempty AND is_janitor AND NOT meta_board AND name='Anonymous')
 THEN RAISE EXCEPTION 'Raw-name fact was guessed from normalized display name'; END IF;
END $$;
UPDATE content.boards SET meta_board=true WHERE slug='ownroll';
SQL
"${psql[@]}" -U board_staff -d janitor_posting_cooldown_upgrade <<'SQL'
BEGIN;
SELECT set_config('board.staff_raw_name_nonempty','true',true);
DO $$ BEGIN
 BEGIN
  PERFORM content.consume_staff_post_authority(decode(repeat('c2',32),'hex'),8811501,'ownroll',8811100,
   'Synthetic','','Bound admin',to_timestamp(4000));
  RAISE EXCEPTION 'Changed meta-board policy consumed proof' USING ERRCODE='ZX001';
 EXCEPTION WHEN invalid_authorization_specification THEN NULL; END;
END $$;
ROLLBACK;
SQL
"${psql[@]}" -U board_auth -d janitor_posting_cooldown_upgrade <<'SQL'
DO $$ DECLARE ctx jsonb; BEGIN
 ctx:=jsonb_build_object('poster_id','','poster_fingerprint',repeat('11',32),'poster_epoch',repeat('22',32),
  'post_sage','false','country','','country_name','','flag','','source_op_reply','false',
  'dice_result','','fortune_text','','fortune_color','','peer','192.0.2.89','deletion_hash','owned-hash','op_password_proof','');
 IF staff_identity.issue_ordinary_post_authority(decode(repeat('c5',32),'hex'),decode(repeat('aa',32),'hex'),
  decode(repeat('bb',32),'hex'),900,8811504,'ownroll',8811100,'Anonymous','','Meta janitor',to_timestamp(4000),
  false,1000,NULL,NULL,'',NULL,true,ctx,false) IS DISTINCT FROM true
 THEN RAISE EXCEPTION 'Meta janitor with empty raw name did not enter ordinary branch'; END IF;
END $$;
SQL
cleanup
trap - EXIT
printf 'Janitor posting timer populated upgrade passed: preserved 0088 state, nullable legacy context, authenticated issuer branches, bound raw-name/meta policy, role boundaries and timer edges. Private cluster removed.\n'
