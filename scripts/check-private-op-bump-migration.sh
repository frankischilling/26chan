#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-private-op-bump.XXXXXXXX)
started=0
cleanup() {
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null
        started=0
    fi
    [[ $cluster =~ ^/tmp/board-private-op-bump\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
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
ALTER ROLE board_staff LOGIN;
ALTER ROLE board_auth LOGIN;
ALTER ROLE board_media_read LOGIN;
ALTER ROLE board_media_intake LOGIN;
ALTER ROLE board_monitor LOGIN;
SQL
for mode in fresh upgrade; do
 database="private_op_bump_$mode"
 runuser -u postgres -- "${psql[@]}" -d postgres -v database="$database" <<'SQL'
CREATE DATABASE :"database" OWNER board_migrator;
REVOKE ALL ON DATABASE :"database" FROM PUBLIC;
GRANT CONNECT ON DATABASE :"database" TO board_migrator,board_public,board_staff,board_auth,
    board_media,board_media_read,board_media_intake,board_monitor;
SQL
 migrator=("${psql[@]}" -U board_migrator -d "$database")
 for migration in migrations/*.sql; do
    [[ $migration != migrations/0090_private_op_bump_context.sql ]] || break
    "${migrator[@]}" --single-transaction -f "$migration"
 done
 if [[ $mode = fresh ]]; then
    "${migrator[@]}" --single-transaction -f migrations/0090_private_op_bump_context.sql
 fi
 "${migrator[@]}" <<'SQL'
-- All identities are synthetic. Legacy rows deliberately have no actor history.
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,staff_only)
VALUES('bumpcheck','Bump migration fixture','Synthetic',1000,100,100,100,10,false),
      ('bumppriv','Private bump fixture','Synthetic',1000,100,100,100,10,true);
UPDATE content.boards SET posting_reply_seconds=17,op_bump_initial_seconds=901,
 op_bump_repeat_seconds=301 WHERE slug='bumpcheck';
INSERT INTO content.threads(id,board,created_at,modified_at)
VALUES(9005000,'bumpcheck',to_timestamp(500),to_timestamp(700)),
      (9006000,'bumpcheck',to_timestamp(600),to_timestamp(600));
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(9005000,'bumpcheck',9005000,'Legacy','Legacy OP','Preserve legacy identity',to_timestamp(500)),
      (9005001,'bumpcheck',9005000,'Legacy','','Lower ID later clock',to_timestamp(700)),
      (9005002,'bumpcheck',9005000,'Legacy','','Highest legacy ID',to_timestamp(600)),
      (9006000,'bumpcheck',9006000,'Legacy','No replies','Preserve no-reply OP',to_timestamp(600));
INSERT INTO post_secrets.op_peers(thread_id,peer) VALUES(9005000,'192.0.2.90'),(9006000,'192.0.2.90');
INSERT INTO post_secrets.op_replies(post_id,thread_id) VALUES(9005001,9005000),(9005002,9005000);
INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES(9005000,'synthetic-legacy-deletion');
INSERT INTO content.reports(board,post_id,reason) VALUES('bumpcheck',9005000,'Synthetic legacy report');
BEGIN;
SELECT set_config('board.posting_actor',repeat('11',32),true);
INSERT INTO content.threads(id,board) VALUES(9001000,'bumpcheck'),(9002000,'bumpcheck'),
 (9003000,'bumppriv'),(9004000,'j');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(9001000,'bumpcheck',9001000,'Synthetic','Tracked OP','Tracked OP',to_timestamp(900)),
      (9001001,'bumpcheck',9001000,'Synthetic','','Lower ID later clock',to_timestamp(1100)),
      (9001002,'bumpcheck',9001000,'Synthetic','','Highest own surviving ID',to_timestamp(1000)),
      (9001003,'bumpcheck',9001000,'Synthetic','','Deleted own reply',to_timestamp(1200)),
      (9002000,'bumpcheck',9002000,'Synthetic','No replies','Tracked no-reply OP',to_timestamp(1300)),
      (9003000,'bumppriv',9003000,'Synthetic','Private OP','Private OP',to_timestamp(1400)),
      (9003001,'bumppriv',9003000,'Synthetic','','Private reply',to_timestamp(1500));
-- /j/ enables wordfilters. Ordinary boards clear these GUCs, so provide the
-- same synthetic WF01 envelope used by test-authorized-post-migration only
-- after their inserts, with the matching unchanged comment/search text.
SELECT set_config('board.wordfilter_payload','5746303100000001ffff0000000000104f776e656420686973746f726963616c',true);
SELECT set_config('board.wordfilter_search','Owned historical',true);
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(9004000,'j',9004000,'Synthetic','Private j OP','Owned historical',to_timestamp(1600)),
      (9004001,'j',9004000,'Synthetic','','Owned historical',to_timestamp(1700));
COMMIT;
-- Deliberately diverge immutable request time from content time, and delete a
-- newer own reply. The result must use surviving content by ID.
UPDATE content.posts SET created_at=to_timestamp(1001) WHERE id=9001002;
UPDATE content.posts SET deleted=true WHERE id=9001003;
BEGIN;
SELECT set_config('board.posting_actor',repeat('22',32),true);
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(9001004,'bumpcheck',9001000,'Other','','Other actor newest ID',to_timestamp(1800));
COMMIT;
CREATE TABLE public.bump_rows_before(relation text,value jsonb);
DO $$ DECLARE r record; BEGIN
 FOR r IN SELECT n.nspname,c.relname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname IN ('content','post_secrets','staff_identity','media','deployment','admission') AND c.relkind='r' LOOP
  EXECUTE format('INSERT INTO public.bump_rows_before SELECT %L,to_jsonb(r) FROM %I.%I r',
   r.nspname||'.'||r.relname,r.nspname,r.relname);
 END LOOP;
 IF (SELECT count(*) FROM post_secrets.posting_history)<>9
 OR EXISTS(SELECT 1 FROM post_secrets.posting_history WHERE post_id BETWEEN 9005000 AND 9006000)
 THEN RAISE EXCEPTION 'Synthetic populated history/legacy fixture missing'; END IF;
END $$;
CREATE TABLE public.bump_policies_before AS SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy;
CREATE TABLE public.bump_relations_before AS SELECT c.oid,c.relowner,c.relacl::text AS acl,c.relrowsecurity,c.relforcerowsecurity
 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname IN ('content','post_secrets','staff_identity','media','deployment','admission');
CREATE TABLE public.bump_functions_before AS SELECT p.oid,p.proowner,p.proacl::text AS acl,p.prosecdef,p.proconfig,p.prosrc,
 p.prorettype,p.proargtypes,p.provolatile,p.proallargtypes,p.proargmodes,p.proargnames
 FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname NOT IN ('pg_catalog','information_schema');
-- Snapshot effective per-column grants as well as table ACLs. Only the five
-- explicitly required owner SELECT columns may change in 0090.
CREATE TABLE public.bump_columns_before AS
 SELECT a.attrelid,a.attnum,acl.grantor,acl.grantee,acl.privilege_type,acl.is_grantable
 FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace,
 LATERAL aclexplode(a.attacl) acl WHERE a.attnum>0 AND NOT a.attisdropped
 AND n.nspname IN ('content','post_secrets','staff_identity','media','deployment','admission');
SQL
 if [[ $mode = upgrade ]]; then
    "${migrator[@]}" <<'SQL'
DO $$ BEGIN
 IF to_regprocedure('content.posting_op_bump_context(bytea,text,bigint)') IS NOT NULL
 OR to_regprocedure('content.staff_op_bump_context(text,bigint,text)') IS NOT NULL
 THEN RAISE EXCEPTION 'New bump API unexpectedly present before 0090'; END IF;
END $$;
SQL
    "${migrator[@]}" --single-transaction -f migrations/0090_private_op_bump_context.sql
 fi
 "${migrator[@]}" <<'SQL'
CREATE TABLE public.bump_rows_after(LIKE public.bump_rows_before);
DO $$ DECLARE r record; runtime text; rel text; fn regprocedure; expected_owner text; BEGIN
 FOR r IN SELECT n.nspname,c.relname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname IN ('content','post_secrets','staff_identity','media','deployment','admission') AND c.relkind='r' LOOP
  EXECUTE format('INSERT INTO public.bump_rows_after SELECT %L,to_jsonb(r) FROM %I.%I r',
   r.nspname||'.'||r.relname,r.nspname,r.relname);
 END LOOP;
 IF EXISTS(TABLE public.bump_rows_before EXCEPT ALL TABLE public.bump_rows_after)
 OR EXISTS(TABLE public.bump_rows_after EXCEPT ALL TABLE public.bump_rows_before)
 THEN RAISE EXCEPTION '0090 changed existing rows, clocks, identities, policies or history'; END IF;
 IF EXISTS(TABLE public.bump_policies_before EXCEPT SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy)
 OR EXISTS(SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy EXCEPT TABLE public.bump_policies_before)
 OR EXISTS(SELECT 1 FROM public.bump_relations_before old LEFT JOIN pg_class c USING(oid)
 WHERE c.oid IS NULL OR (old.relowner,old.acl,old.relrowsecurity,old.relforcerowsecurity)
 IS DISTINCT FROM (c.relowner,c.relacl::text,c.relrowsecurity,c.relforcerowsecurity))
 OR EXISTS(TABLE public.bump_functions_before EXCEPT SELECT p.oid,p.proowner,p.proacl::text,p.prosecdef,p.proconfig,p.prosrc,
 p.prorettype,p.proargtypes,p.provolatile,p.proallargtypes,p.proargmodes,p.proargnames FROM pg_proc p)
 THEN RAISE EXCEPTION '0090 changed existing functions, RLS or table authority'; END IF;
 IF (SELECT count(*) FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
 WHERE n.nspname='content' AND p.proname IN ('posting_op_bump_context','staff_op_bump_context'))<>2
 THEN RAISE EXCEPTION 'Unexpected bump API overloads'; END IF;
 FOREACH fn IN ARRAY ARRAY['content.posting_op_bump_context(bytea,text,bigint)'::regprocedure,
 'content.staff_op_bump_context(text,bigint,text)'::regprocedure] LOOP
  expected_owner:=CASE WHEN fn='content.posting_op_bump_context(bytea,text,bigint)'::regprocedure
   THEN 'board_posting_cooldown_owner' ELSE 'board_staff_post_owner' END;
  IF NOT EXISTS(SELECT 1 FROM pg_proc WHERE oid=fn AND proowner=expected_owner::regrole
   AND prosecdef AND provolatile='s' AND proretset AND prorettype='record'::regtype
   AND proconfig=ARRAY['search_path=pg_catalog, pg_temp']
   AND pg_get_function_result(oid)='TABLE(own_reply boolean, latest_post_id bigint, latest_created_at timestamp with time zone)')
  OR EXISTS(SELECT 1 FROM pg_proc p,LATERAL aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a
   WHERE p.oid=fn AND a.grantee=0 AND a.privilege_type='EXECUTE')
  THEN RAISE EXCEPTION 'Unsafe owner/search path/STABLE/definer/return contract/PUBLIC grant for %',fn; END IF;
  IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname=expected_owner AND NOT rolcanlogin
   AND NOT rolsuper AND NOT rolcreatedb AND NOT rolcreaterole AND NOT rolreplication AND NOT rolbypassrls)
  OR has_schema_privilege(expected_owner,'content','CREATE')
  OR has_schema_privilege(expected_owner,'post_secrets','CREATE')
  THEN RAISE EXCEPTION 'Bump owner gained unsafe authority'; END IF;
  FOREACH runtime IN ARRAY ARRAY['board_public','board_staff','board_auth','board_media','board_media_read','board_media_intake','board_monitor'] LOOP
   IF has_function_privilege(runtime,fn,'EXECUTE') IS DISTINCT FROM
    (runtime='board_staff' OR (runtime='board_public' AND expected_owner='board_posting_cooldown_owner'))
   OR pg_has_role(runtime,expected_owner,'MEMBER')
   THEN RAISE EXCEPTION 'Unexpected % authority for %',runtime,fn; END IF;
   FOREACH rel IN ARRAY ARRAY['posting_history','posting_thread_actions','posting_actor_gates','posting_action_capacity'] LOOP
    IF has_table_privilege(runtime,'post_secrets.'||rel,'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
    OR EXISTS(SELECT 1 FROM pg_attribute a WHERE a.attrelid=('post_secrets.'||rel)::regclass
     AND a.attnum>0 AND NOT a.attisdropped AND has_column_privilege(runtime,a.attrelid,a.attnum,'SELECT,INSERT,UPDATE,REFERENCES'))
    THEN RAISE EXCEPTION 'Runtime % gained private history authority on %',runtime,rel; END IF;
   END LOOP;
  END LOOP;
 END LOOP;
END $$;
CREATE TABLE public.bump_columns_after AS
 SELECT a.attrelid,a.attnum,acl.grantor,acl.grantee,acl.privilege_type,acl.is_grantable
 FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace,
 LATERAL aclexplode(a.attacl) acl WHERE a.attnum>0 AND NOT a.attisdropped
 AND n.nspname IN ('content','post_secrets','staff_identity','media','deployment','admission');
DO $$ BEGIN
 IF EXISTS(TABLE public.bump_columns_before EXCEPT TABLE public.bump_columns_after)
 OR EXISTS(SELECT 1 FROM (TABLE public.bump_columns_after EXCEPT TABLE public.bump_columns_before) delta
 JOIN pg_attribute a ON a.attrelid=delta.attrelid AND a.attnum=delta.attnum
 WHERE NOT (delta.attrelid='content.posts'::regclass AND a.attname IN ('id','board','thread_id','deleted','created_at')
 AND delta.grantee='board_posting_cooldown_owner'::regrole AND delta.privilege_type='SELECT' AND NOT delta.is_grantable))
 THEN RAISE EXCEPTION '0090 changed unrelated column grants'; END IF;
END $$;
SQL
 # Real runtime connections verify session_user handling, not only SET ROLE.
 for role in board_public board_staff board_auth board_media board_media_read board_media_intake board_monitor; do
 "${psql[@]}" -U "$role" -d "$database" <<'SQL'
DO $$ DECLARE rel text; BEGIN
 FOREACH rel IN ARRAY ARRAY['posting_history','posting_thread_actions','posting_actor_gates','posting_action_capacity'] LOOP
  BEGIN
   EXECUTE format('SELECT 1 FROM post_secrets.%I LIMIT 1',rel);
   RAISE EXCEPTION 'Runtime read private history' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 END LOOP;
 IF session_user<>'board_staff' THEN
  BEGIN
   PERFORM * FROM content.staff_op_bump_context('bumpcheck',9005000,'192.0.2.90');
   RAISE EXCEPTION 'Nonstaff executed legacy companion' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 END IF;
 IF session_user NOT IN ('board_public','board_staff') THEN
  BEGIN
   PERFORM * FROM content.posting_op_bump_context(decode(repeat('11',32),'hex'),'bumpcheck',9001000);
   RAISE EXCEPTION 'Unrelated runtime executed actor bump context' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 END IF;
END $$;
SQL
 done
 for role in board_public board_staff; do
 "${psql[@]}" -U "$role" -d "$database" <<'SQL'
DO $$ DECLARE actor bytea:=decode(repeat('11',32),'hex'); bad_actor bytea; bad_thread bigint; BEGIN
 IF (SELECT count(*) FROM content.posting_op_bump_context(actor,'bumpcheck',9001000)
 WHERE own_reply AND latest_post_id=9001002 AND latest_created_at=to_timestamp(1001))<>1
 OR (SELECT count(*) FROM content.posting_op_bump_context(actor,'bumpcheck',9002000)
 WHERE own_reply AND latest_post_id IS NULL AND latest_created_at IS NULL)<>1
 OR (SELECT count(*) FROM content.posting_op_bump_context(decode(repeat('33',32),'hex'),'bumpcheck',9001000)
 WHERE NOT own_reply AND latest_post_id IS NULL AND latest_created_at IS NULL)<>1
 OR (SELECT count(*) FROM content.posting_op_bump_context(decode(repeat('22',32),'hex'),'bumpcheck',9001000)
 WHERE NOT own_reply AND latest_post_id=9001004 AND latest_created_at=to_timestamp(1800))<>1
 OR (SELECT count(*) FROM content.posting_op_bump_context(actor,'bumpcheck',9005000)
 WHERE NOT own_reply AND latest_post_id IS NULL AND latest_created_at IS NULL)<>1
 OR EXISTS(SELECT 1 FROM content.posting_op_bump_context(actor,'bumpcheck',9003000))
 OR EXISTS(SELECT 1 FROM content.posting_op_bump_context(actor,'missing',9001000))
 THEN RAISE EXCEPTION 'Actor context ownership, highest-ID content time, OP-only or scope failed'; END IF;
 FOREACH bad_actor IN ARRAY ARRAY[NULL::bytea,''::bytea,decode(repeat('00',31),'hex'),decode(repeat('00',33),'hex')] LOOP
  BEGIN
   PERFORM * FROM content.posting_op_bump_context(bad_actor,'bumpcheck',9001000);
   RAISE EXCEPTION 'Malformed actor accepted' USING ERRCODE='ZX001';
  EXCEPTION WHEN check_violation THEN NULL; END;
 END LOOP;
 FOREACH bad_thread IN ARRAY ARRAY[NULL::bigint,0::bigint,-1::bigint] LOOP
  BEGIN
   PERFORM * FROM content.posting_op_bump_context(actor,'bumpcheck',bad_thread);
   RAISE EXCEPTION 'Malformed actor-context thread accepted' USING ERRCODE='ZX001';
  EXCEPTION WHEN check_violation THEN NULL; END;
 END LOOP;
 IF session_user='board_public' THEN
  IF EXISTS(SELECT 1 FROM content.posting_op_bump_context(actor,'bumppriv',9003000))
  OR EXISTS(SELECT 1 FROM content.posting_op_bump_context(actor,'j',9004000))
  OR EXISTS(SELECT 1 FROM content.posts WHERE board IN ('j','bumppriv'))
  OR EXISTS(SELECT 1 FROM content.threads WHERE board IN ('j','bumppriv'))
  THEN RAISE EXCEPTION 'Private /j/ or staff-only content leaked to public runtime'; END IF;
 ELSE
  IF (SELECT count(*) FROM content.posting_op_bump_context(actor,'bumppriv',9003000)
   WHERE own_reply AND latest_post_id=9003001 AND latest_created_at=to_timestamp(1500))<>1
  OR (SELECT count(*) FROM content.posting_op_bump_context(actor,'j',9004000)
   WHERE own_reply AND latest_post_id=9004001 AND latest_created_at=to_timestamp(1700))<>1
  THEN RAISE EXCEPTION 'Staff private-board or /j/ actor context missing'; END IF;
 END IF;
END $$;
SQL
 done
 "${psql[@]}" -U board_staff -d "$database" <<'SQL'
DO $$ DECLARE bad_thread bigint; BEGIN
 IF (SELECT count(*) FROM content.staff_op_bump_context('bumpcheck',9005000,'192.0.2.90')
 WHERE own_reply AND latest_post_id=9005002 AND latest_created_at=to_timestamp(600))<>1
 OR (SELECT count(*) FROM content.staff_op_bump_context('bumpcheck',9006000,'192.0.2.90')
 WHERE own_reply AND latest_post_id IS NULL AND latest_created_at IS NULL)<>1
 OR (SELECT count(*) FROM content.staff_op_bump_context('bumpcheck',9005000,'192.0.2.91')
 WHERE NOT own_reply AND latest_post_id IS NULL AND latest_created_at IS NULL)<>1
 OR EXISTS(SELECT 1 FROM content.staff_op_bump_context('bumppriv',9003000,'192.0.2.90'))
 OR EXISTS(SELECT 1 FROM content.staff_op_bump_context('j',9004000,'192.0.2.90'))
 OR EXISTS(SELECT 1 FROM content.staff_op_bump_context('bumpcheck',9003000,'192.0.2.90'))
 THEN RAISE EXCEPTION 'Legacy highest-ID/no-reply ownership or scope failed'; END IF;
 FOREACH bad_thread IN ARRAY ARRAY[NULL::bigint,0::bigint,-1::bigint] LOOP
  BEGIN
   PERFORM * FROM content.staff_op_bump_context('bumpcheck',bad_thread,'192.0.2.90');
   RAISE EXCEPTION 'Malformed legacy-context thread accepted' USING ERRCODE='ZX001';
  EXCEPTION WHEN check_violation THEN NULL; END;
 END LOOP;
END $$;
SQL
 # Preservation was verified above; now test normal lifecycle invalidation.
 "${migrator[@]}" <<'SQL'
UPDATE content.threads SET archived_at=statement_timestamp(),
 archive_expires_at=statement_timestamp()+interval '1 hour' WHERE id IN (9001000,9005000);
UPDATE content.threads SET deleted=true WHERE id=9002000;
UPDATE content.posts SET deleted=true WHERE id=9006000;
SQL
 "${psql[@]}" -U board_staff -d "$database" <<'SQL'
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM content.posting_op_bump_context(decode(repeat('11',32),'hex'),'bumpcheck',9001000))
 OR EXISTS(SELECT 1 FROM content.posting_op_bump_context(decode(repeat('11',32),'hex'),'bumpcheck',9002000))
 OR EXISTS(SELECT 1 FROM content.staff_op_bump_context('bumpcheck',9005000,'192.0.2.90'))
 OR EXISTS(SELECT 1 FROM content.staff_op_bump_context('bumpcheck',9006000,'192.0.2.90'))
 THEN RAISE EXCEPTION 'Archived/deleted thread or deleted OP retained bump context'; END IF;
END $$;
SQL
 printf 'Private OP bump %s qualification passed.\n' "$mode"
done
cleanup
trap - EXIT
printf 'Private OP bump migration passed fresh and populated 0089 upgrade, preservation, exact API authority, runtime privacy and content timestamp selection. Private cluster removed.\n'
