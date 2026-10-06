#!/usr/bin/env bash
# Owned synthetic clusters only. Historical identities stay unknown; no backfill.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-user-thread-quota.XXXXXXXX)
started=0
cleanup() {
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null
    fi
    [[ $cluster =~ ^/tmp/board-user-thread-quota\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
    [[ $(readlink -f "$cluster") = "$cluster" ]] || exit 1
    rm -rf -- "$cluster"
}
trap cleanup EXIT
chown postgres:postgres "$cluster"
runuser -u postgres -- "$pg_bin/initdb" -D "$cluster/data" --auth=trust --encoding=UTF8 --no-locale > /dev/null
runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" \
    -o "-c listen_addresses='' -c unix_socket_directories='$cluster' -c statement_timeout=30000 -c lock_timeout=5000" -w start > /dev/null
started=1
psql=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h "$cluster")
# SQL diagnostics and synthetic fixture values stay in the private directory.
exec 3>&1
exec > "$cluster/qualification.log" 2>&1
trap 'printf "Thread-quota qualification failed at line %s (private diagnostics removed during cleanup).\n" "$LINENO" >&3' ERR
runuser -u postgres -- "${psql[@]}" -d postgres -f deploy/roles.sql
runuser -u postgres -- "${psql[@]}" -d postgres <<'SQL'
ALTER ROLE board_staff LOGIN;
ALTER ROLE board_media LOGIN;
ALTER ROLE board_auth LOGIN;
ALTER ROLE board_media_read LOGIN;
ALTER ROLE board_media_intake LOGIN;
ALTER ROLE board_monitor LOGIN;
SQL
create_database() {
 runuser -u postgres -- "${psql[@]}" -d postgres -v database="$1" <<'SQL'
CREATE DATABASE :"database" OWNER board_migrator;
REVOKE ALL ON DATABASE :"database" FROM PUBLIC;
GRANT CONNECT ON DATABASE :"database" TO board_migrator,board_public,board_staff,board_auth,
 board_media,board_media_read,board_media_intake,board_monitor;
SQL
}
for mode in fresh upgrade; do
 database="user_thread_quota_$mode"
 create_database "$database"
 migrator=("${psql[@]}" -U board_migrator -d "$database")
 for migration in migrations/*.sql; do
    [[ $migration != migrations/0092* ]] || break
    "${migrator[@]}" --single-transaction -f "$migration"
 done
 if [[ $mode = fresh ]]; then
    "${migrator[@]}" --single-transaction -f migrations/0092_user_thread_quota.sql
 fi
 "${migrator[@]}" <<'SQL'
-- Include operator overrides and unrelated content/secret/proof/media state.
UPDATE content.boards SET comment_spoiler_cleanup=false,thread_limit=37 WHERE slug='a';
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,staff_only)
VALUES('quota','Quota fixture','Synthetic',1000,100,100,100,10,false),
 ('quotapriv','Private quota fixture','Synthetic',1000,100,100,100,10,true);
-- Historical imports deliberately omit board.posting_actor. Migration must not
-- infer their identity from unrelated deletion passwords or anonymous proofs.
INSERT INTO content.threads(id,board) VALUES(9200000,'quota'),(9200100,'quotapriv');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(9200000,'quota',9200000,'Synthetic','Historical OP','Retained body',to_timestamp(1000000)),
 (9200001,'quota',9200000,'Synthetic','','Retained reply',to_timestamp(1000001)),
 (9200100,'quotapriv',9200100,'Synthetic','Private historical OP','Private body',to_timestamp(1000000));
INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES(9200000,'synthetic-hash');
INSERT INTO content.reports(board,post_id,reason) VALUES('quota',9200000,'Synthetic report');
INSERT INTO content.moderation_audit(account_id,board,target_id,action) VALUES(42,'quota',9200001,'remove-post');
INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at)
VALUES(repeat('c',32),repeat('c',32),repeat('c',32),repeat('c',64),100,500,300,'approved',clock_timestamp());
INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler,file_deleted)
VALUES(9200000,repeat('c',32),repeat('c',32),'synthetic.png',100,500,300,true,false);
INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,
 created_at,network_at,address_at,environment_at,expires_at,posts,threads)
VALUES(decode(repeat('01',32),'hex'),decode(repeat('02',32),'hex'),decode(repeat('03',32),'hex'),
 decode(repeat('04',32),'hex'),1,1,1,1,4102444800,1,1);
INSERT INTO post_secrets.anonymous_posts(post_id,token_hash,password_proof)
VALUES(9200000,decode(repeat('01',32),'hex'),decode(repeat('05',32),'hex'));
INSERT INTO post_secrets.anonymous_reports(report_id,token_hash)
SELECT id,decode(repeat('01',32),'hex') FROM content.reports WHERE board='quota';
-- Existing, legitimately registered known identities must also survive.
BEGIN;
SELECT set_config('board.posting_actor',repeat('11',32),true);
INSERT INTO content.threads(id,board) SELECT 9201000+i,'quota' FROM generate_series(1,12) i;
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
SELECT 9201000+i,'quota',9201000+i,'Synthetic','Known OP','Known body',to_timestamp(1000000+i)
FROM generate_series(1,12) i;
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(9202000,'quota',9201001,'Synthetic','','Known reply',to_timestamp(1000013));
COMMIT;
SQL
 "${migrator[@]}" <<'SQL'
CREATE TABLE public.quota_rows_before(relation text,value jsonb);
CREATE FUNCTION public.capture_quota_rows() RETURNS TABLE(relation text,value jsonb)
LANGUAGE plpgsql AS $$ DECLARE r record; BEGIN
 FOR r IN SELECT n.nspname,c.relname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname IN ('content','post_secrets','staff_identity','media','media_intake','deployment','admission') AND c.relkind='r' LOOP
  RETURN QUERY EXECUTE format('SELECT %L,to_jsonb(r) FROM %I.%I r',r.nspname||'.'||r.relname,r.nspname,r.relname);
 END LOOP;
END $$;
INSERT INTO public.quota_rows_before SELECT * FROM public.capture_quota_rows();
-- Canonical names, rather than database OIDs, also support dump/restore comparison.
-- acldefault uses lowercase s for sequences (uppercase S means foreign server),
-- unlike pg_class.relkind. Expand NULL ACLs to the correct object-type defaults.
-- Dump/restore can replace
-- explicit owner-only ACLs with NULL and reorder entries without changing rights.
-- Exploded table/column tuples are compared as sets and sorted in fingerprints;
-- nested schema/function tuples are sorted explicitly. Owners remain separate.
CREATE VIEW public.quota_authority AS
SELECT 'relation' kind,n.nspname||'.'||c.relname object,
 jsonb_build_array(pg_get_userbyid(c.relowner),c.relrowsecurity,c.relforcerowsecurity) value
 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname NOT LIKE 'pg_%' AND n.nspname NOT IN ('information_schema','public')
UNION ALL SELECT 'table grant',n.nspname||'.'||c.relname,
 jsonb_build_array(pg_get_userbyid(a.grantor),CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace,
 LATERAL aclexplode(coalesce(c.relacl,acldefault(CASE WHEN c.relkind='S' THEN 's'::"char" ELSE 'r'::"char" END,c.relowner))) a
 WHERE c.relkind IN ('r','p','v','m','f','S')
 AND n.nspname NOT LIKE 'pg_%' AND n.nspname NOT IN ('information_schema','public')
UNION ALL SELECT 'column grant',n.nspname||'.'||c.relname||'.'||att.attname,
 jsonb_build_array(pg_get_userbyid(a.grantor),CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
 FROM pg_attribute att JOIN pg_class c ON c.oid=att.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace,LATERAL aclexplode(att.attacl) a
 WHERE n.nspname NOT LIKE 'pg_%' AND n.nspname NOT IN ('information_schema','public')
UNION ALL SELECT 'function',p.oid::regprocedure::text,
 jsonb_build_array(pg_get_userbyid(p.proowner),coalesce((
  SELECT jsonb_agg(jsonb_build_array(pg_get_userbyid(a.grantor),
   CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
   ORDER BY pg_get_userbyid(a.grantor),CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
  FROM aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a),'[]'::jsonb),pg_get_functiondef(p.oid))
 FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
 WHERE n.nspname NOT LIKE 'pg_%' AND n.nspname NOT IN ('information_schema','public')
UNION ALL SELECT 'policy',p.polrelid::regclass::text||'.'||p.polname,
 jsonb_build_array(p.polcmd,p.polpermissive,ARRAY(SELECT pg_get_userbyid(x) FROM unnest(p.polroles) x ORDER BY x),pg_get_expr(p.polqual,p.polrelid),pg_get_expr(p.polwithcheck,p.polrelid)) FROM pg_policy p
UNION ALL SELECT 'trigger',t.tgrelid::regclass::text||'.'||t.tgname,
 jsonb_build_array(t.tgenabled,pg_get_triggerdef(t.oid)) FROM pg_trigger t WHERE NOT t.tgisinternal
UNION ALL SELECT 'schema',n.nspname,jsonb_build_array(pg_get_userbyid(n.nspowner),coalesce((
 SELECT jsonb_agg(jsonb_build_array(pg_get_userbyid(a.grantor),
  CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
  ORDER BY pg_get_userbyid(a.grantor),CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
 FROM aclexplode(coalesce(n.nspacl,acldefault('n',n.nspowner))) a),'[]'::jsonb))
 FROM pg_namespace n WHERE n.nspname NOT LIKE 'pg_%' AND n.nspname NOT IN ('information_schema','public');
CREATE TABLE public.quota_authority_before AS TABLE public.quota_authority;
SQL
 if [[ $mode = upgrade ]]; then
    "${migrator[@]}" --single-transaction -f migrations/0092_user_thread_quota.sql
 fi
 "${migrator[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(SELECT relation,CASE WHEN relation='content.boards' THEN value-ARRAY['user_thread_limit','user_thread_period_hours'] ELSE value END
  FROM public.quota_rows_before EXCEPT ALL
  SELECT relation,CASE WHEN relation='content.boards' THEN value-ARRAY['user_thread_limit','user_thread_period_hours'] ELSE value END
  FROM public.capture_quota_rows())
 OR EXISTS(SELECT relation,CASE WHEN relation='content.boards' THEN value-ARRAY['user_thread_limit','user_thread_period_hours'] ELSE value END
  FROM public.capture_quota_rows() EXCEPT ALL
  SELECT relation,CASE WHEN relation='content.boards' THEN value-ARRAY['user_thread_limit','user_thread_period_hours'] ELSE value END
  FROM public.quota_rows_before)
 THEN RAISE EXCEPTION 'Migration changed preexisting rows'; END IF;
 IF EXISTS(TABLE public.quota_authority_before EXCEPT TABLE public.quota_authority)
 THEN RAISE EXCEPTION 'Migration removed or rewrote preexisting authority'; END IF;
 IF EXISTS(SELECT 1 FROM (TABLE public.quota_authority EXCEPT TABLE public.quota_authority_before) d
 WHERE NOT ((kind='column grant' AND object IN ('content.boards.user_thread_limit','content.boards.user_thread_period_hours')
  AND value=jsonb_build_array('board_migrator','board_posting_cooldown_owner','SELECT',false)) OR
  (kind='function' AND object='content.check_user_thread_quota(bytea,text,bigint)')))
 THEN RAISE EXCEPTION 'Migration added unrelated authority'; END IF;
 IF EXISTS(SELECT 1 FROM post_secrets.posting_history WHERE post_id IN(9200000,9200001,9200100))
 THEN RAISE EXCEPTION 'Historical identity was backfilled'; END IF;
 IF EXISTS(SELECT 1 FROM content.boards WHERE user_thread_limit<>CASE
   WHEN slug IN('a','bant','i','pol','qa','v','vm','vmg','vrpg','vst') THEN 3
   WHEN slug='test' THEN 50 ELSE 5 END
  OR user_thread_period_hours<>CASE WHEN slug='i' THEN 168 WHEN slug='pol' THEN 6
   WHEN slug='qa' THEN 48 WHEN slug='qst' THEN 72 WHEN slug='news' THEN 120 ELSE 24 END)
 THEN RAISE EXCEPTION 'Incorrect seeded quota policy'; END IF;
 IF (SELECT count(*) FROM pg_attribute a JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
  WHERE a.attrelid='content.boards'::regclass AND a.attnotnull AND a.atttypid='integer'::regtype
  AND ((a.attname='user_thread_limit' AND pg_get_expr(d.adbin,d.adrelid)='5')
   OR (a.attname='user_thread_period_hours' AND pg_get_expr(d.adbin,d.adrelid)='24')))<>2
 THEN RAISE EXCEPTION 'Wrong quota column type/default/nullability'; END IF;
END $$;
-- Actual constraint enforcement: the internal sentinel is never caught.
DO $$ DECLARE col text; invalid integer; BEGIN
 FOREACH col IN ARRAY ARRAY['user_thread_limit','user_thread_period_hours'] LOOP
  FOREACH invalid IN ARRAY ARRAY[-1,CASE WHEN col='user_thread_limit' THEN 100001 ELSE 876001 END] LOOP
   BEGIN
    EXECUTE format('UPDATE content.boards SET %I=$1 WHERE slug=''quota''',col) USING invalid;
    RAISE EXCEPTION 'Invalid quota policy accepted' USING ERRCODE='ZX001';
   EXCEPTION WHEN check_violation THEN NULL; END;
  END LOOP;
  BEGIN
   EXECUTE format('UPDATE content.boards SET %I=NULL WHERE slug=''quota''',col);
   RAISE EXCEPTION 'NULL quota policy accepted' USING ERRCODE='ZX001';
  EXCEPTION WHEN not_null_violation THEN NULL; END;
 END LOOP;
END $$;
-- Exercise upper endpoints without changing the policy saved for runtime tests.
BEGIN;
UPDATE content.boards SET user_thread_limit=100000,user_thread_period_hours=876000 WHERE slug='quota';
ROLLBACK;
UPDATE content.boards SET user_thread_limit=3,user_thread_period_hours=24 WHERE slug='quota';
DROP TABLE public.quota_rows_before,public.quota_authority_before;
SQL
 for phase in live restored; do
 if [[ $phase = restored ]]; then
  runuser -u postgres -- "$pg_bin/pg_dump" -h "$cluster" -U board_migrator -d "$database" \
    --format=custom --file="$cluster/current.dump"
  create_database "${database}_restore"
  runuser -u postgres -- "$pg_bin/pg_restore" -h "$cluster" --dbname="${database}_restore" \
    --single-transaction --exit-on-error < "$cluster/current.dump"
  database="${database}_restore"
 fi
 "${psql[@]}" -U board_migrator -d "$database" <<'SQL'
DO $$ DECLARE runtime text; fn regprocedure:='content.check_user_thread_quota(bytea,text,bigint)'::regprocedure; BEGIN
 IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='board_posting_cooldown_owner' AND NOT rolcanlogin
  AND NOT rolsuper AND NOT rolcreatedb AND NOT rolcreaterole AND NOT rolreplication AND NOT rolbypassrls)
 OR has_schema_privilege('board_posting_cooldown_owner','content','CREATE')
 OR has_schema_privilege('board_posting_cooldown_owner','post_secrets','CREATE')
 OR NOT has_column_privilege('board_posting_cooldown_owner','content.boards','user_thread_limit','SELECT')
 OR NOT has_column_privilege('board_posting_cooldown_owner','content.boards','user_thread_period_hours','SELECT')
 OR has_column_privilege('board_posting_cooldown_owner','content.boards','user_thread_limit','UPDATE')
 OR has_column_privilege('board_posting_cooldown_owner','content.boards','user_thread_period_hours','UPDATE')
 THEN RAISE EXCEPTION 'Unsafe quota owner authority'; END IF;
 IF NOT EXISTS(SELECT 1 FROM pg_proc WHERE oid=fn AND proowner='board_posting_cooldown_owner'::regrole
  AND prosecdef AND proconfig=ARRAY['search_path=pg_catalog, pg_temp'])
 OR EXISTS(SELECT 1 FROM pg_proc p,LATERAL aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a
  WHERE p.oid=fn AND (a.privilege_type<>'EXECUTE' OR a.is_grantable OR
   a.grantee NOT IN(p.proowner,'board_public'::regrole,'board_staff'::regrole)))
 THEN RAISE EXCEPTION 'Unsafe quota function owner/path/ACL'; END IF;
 FOREACH runtime IN ARRAY ARRAY['board_public','board_staff','board_auth','board_media','board_media_read','board_media_intake','board_monitor'] LOOP
  IF has_function_privilege(runtime,fn,'EXECUTE')<>(runtime IN('board_public','board_staff'))
   OR pg_has_role(runtime,'board_posting_cooldown_owner','MEMBER')
   OR has_table_privilege(runtime,'post_secrets.posting_history','SELECT,INSERT,UPDATE,DELETE')
   OR has_column_privilege(runtime,'content.boards','user_thread_limit','UPDATE')
   OR has_column_privilege(runtime,'content.boards','user_thread_period_hours','UPDATE')
  THEN RAISE EXCEPTION 'Runtime gained quota authority'; END IF;
 END LOOP;
 IF EXISTS(SELECT 1 FROM pg_class WHERE oid IN ('content.posts'::regclass,'content.threads'::regclass,'content.boards'::regclass) AND NOT relrowsecurity)
 THEN RAISE EXCEPTION 'Content RLS disabled'; END IF;
END $$;
SQL
 # Bootstrap administrator switches between fixture mutation and the actual
 # public API caller; no EXECUTE or membership grants are broadened for tests.
 runuser -u postgres -- "${psql[@]}" -d "$database" <<'SQL'
-- Roll back all behavior probes so the live/restored current rows compare exactly.
BEGIN;
DO $$ DECLARE answer record; BEGIN
 EXECUTE 'SET LOCAL ROLE board_public';
 SELECT * INTO STRICT answer FROM content.check_user_thread_quota(decode(repeat('11',32),'hex'),'quota',1000013);
 IF NOT answer.rejected OR answer.user_thread_limit<>3 OR answer.user_thread_period_hours<>24
 THEN RAISE EXCEPTION 'Bounded known actor quota mismatch'; END IF;
 EXECUTE 'SET LOCAL ROLE board_migrator';
 UPDATE content.boards SET user_thread_limit=13 WHERE slug='quota';
 EXECUTE 'SET LOCAL ROLE board_public';
 SELECT * INTO STRICT answer FROM content.check_user_thread_quota(decode(repeat('11',32),'hex'),'quota',1000013);
 IF answer.rejected THEN RAISE EXCEPTION 'Historical OP or reply counted as known OP'; END IF;
 EXECUTE 'SET LOCAL ROLE board_migrator';
 UPDATE content.boards SET user_thread_limit=3 WHERE slug='quota';
 EXECUTE 'SET LOCAL ROLE board_public';
 SELECT * INTO STRICT answer FROM content.check_user_thread_quota(decode(repeat('22',32),'hex'),'quota',1000013);
 IF answer.rejected THEN RAISE EXCEPTION 'Unrelated actor charged'; END IF;
 -- With period zero, strictly future OPs still count: source has no upper edge.
 EXECUTE 'SET LOCAL ROLE board_migrator';
 UPDATE content.boards SET user_thread_period_hours=0 WHERE slug='quota';
 EXECUTE 'SET LOCAL ROLE board_public';
 SELECT * INTO STRICT answer FROM content.check_user_thread_quota(decode(repeat('11',32),'hex'),'quota',1000009);
 IF NOT answer.rejected THEN RAISE EXCEPTION 'Future OPs excluded'; END IF;
 SELECT * INTO STRICT answer FROM content.check_user_thread_quota(decode(repeat('11',32),'hex'),'quota',1000010);
 IF answer.rejected THEN RAISE EXCEPTION 'Strict lower edge or reply exclusion wrong'; END IF;
 EXECUTE 'SET LOCAL ROLE board_migrator';
 UPDATE content.boards SET user_thread_limit=0 WHERE slug='quota';
 EXECUTE 'SET LOCAL ROLE board_public';
 SELECT * INTO STRICT answer FROM content.check_user_thread_quota(decode(repeat('22',32),'hex'),'quota',1000013);
 IF NOT answer.rejected OR answer.user_thread_limit<>0 OR answer.user_thread_period_hours<>0
 THEN RAISE EXCEPTION 'Zero must reject even an unknown actor'; END IF;
 EXECUTE 'SET LOCAL ROLE board_migrator';
 UPDATE content.boards SET user_thread_limit=12,user_thread_period_hours=24 WHERE slug='quota';
 UPDATE content.threads SET sticky=true,undead=true WHERE id=9201001;
 EXECUTE 'SET LOCAL ROLE board_public';
 SELECT * INTO STRICT answer FROM content.check_user_thread_quota(decode(repeat('11',32),'hex'),'quota',1000013);
 IF NOT answer.rejected THEN RAISE EXCEPTION 'Protected OP incorrectly exempted'; END IF;
 EXECUTE 'SET LOCAL ROLE board_migrator';
 UPDATE content.posts SET deleted=true WHERE id=9201001;
 EXECUTE 'SET LOCAL ROLE board_public';
 SELECT * INTO STRICT answer FROM content.check_user_thread_quota(decode(repeat('11',32),'hex'),'quota',1000013);
 IF answer.rejected THEN RAISE EXCEPTION 'Deleted OP still charged'; END IF;
 EXECUTE 'SET LOCAL ROLE board_migrator';
 UPDATE content.boards SET user_thread_limit=11 WHERE slug='quota';
 UPDATE content.threads SET archived_at=statement_timestamp(),archive_expires_at=statement_timestamp()+interval '1 day' WHERE id=9201002;
 EXECUTE 'SET LOCAL ROLE board_public';
 SELECT * INTO STRICT answer FROM content.check_user_thread_quota(decode(repeat('11',32),'hex'),'quota',1000013);
 IF answer.rejected THEN RAISE EXCEPTION 'Archived OP still charged'; END IF;
 EXECUTE 'SET LOCAL ROLE board_migrator';
 UPDATE content.boards SET user_thread_limit=10 WHERE slug='quota';
 UPDATE content.threads SET deleted=true WHERE id=9201003;
 EXECUTE 'SET LOCAL ROLE board_public';
 SELECT * INTO STRICT answer FROM content.check_user_thread_quota(decode(repeat('11',32),'hex'),'quota',1000013);
 IF answer.rejected THEN RAISE EXCEPTION 'Deleted thread still charged'; END IF;
 EXECUTE 'SET LOCAL ROLE board_migrator';
 UPDATE content.posts SET deleted=false WHERE id=9201001;
 UPDATE content.threads SET deleted=false WHERE id=9201003;
 EXECUTE 'SET LOCAL ROLE board_public';
 SELECT * INTO STRICT answer FROM content.check_user_thread_quota(decode(repeat('11',32),'hex'),'quota',1000013);
 IF answer.rejected THEN RAISE EXCEPTION 'Undelete reconstructed unknown identity'; END IF;
END $$;
ROLLBACK;
SQL
 for role in board_public board_staff board_auth board_media board_media_read board_media_intake board_monitor; do
 "${psql[@]}" -U "$role" -d "$database" <<'SQL'
DO $$ DECLARE actor bytea; invalid_request bigint; answer record; BEGIN
 IF session_user IN('board_public','board_staff') THEN
  SELECT * INTO STRICT answer FROM content.check_user_thread_quota(decode(repeat('11',32),'hex'),'quota',1000013);
  IF NOT answer.rejected THEN RAISE EXCEPTION 'Runtime quota did not reject'; END IF;
  FOREACH actor IN ARRAY ARRAY[NULL::bytea,''::bytea,decode(repeat('11',31),'hex'),decode(repeat('11',33),'hex')] LOOP
   BEGIN
    PERFORM * FROM content.check_user_thread_quota(actor,'quota',1000013);
    RAISE EXCEPTION 'Malformed actor accepted' USING ERRCODE='ZX001';
   EXCEPTION WHEN check_violation THEN NULL; END;
  END LOOP;
  FOREACH invalid_request IN ARRAY ARRAY[NULL::bigint,-1::bigint,9223372036854689408::bigint] LOOP
   BEGIN
    PERFORM * FROM content.check_user_thread_quota(decode(repeat('11',32),'hex'),'quota',invalid_request);
    RAISE EXCEPTION 'Malformed request time accepted' USING ERRCODE='ZX001';
   EXCEPTION WHEN check_violation THEN NULL; END;
  END LOOP;
  BEGIN
   PERFORM * FROM content.check_user_thread_quota(decode(repeat('11',32),'hex'),'missing',1000013);
   RAISE EXCEPTION 'Unknown board accepted' USING ERRCODE='ZX001';
  EXCEPTION WHEN no_data_found THEN NULL; END;
  IF session_user='board_public' THEN
   BEGIN
    PERFORM * FROM content.check_user_thread_quota(decode(repeat('11',32),'hex'),'quotapriv',1000013);
    RAISE EXCEPTION 'Private board exposed' USING ERRCODE='ZX001';
   EXCEPTION WHEN no_data_found THEN NULL; END;
   IF EXISTS(SELECT 1 FROM content.posts WHERE board='quotapriv') THEN RAISE EXCEPTION 'Private content exposed'; END IF;
  ELSE
   SELECT * INTO STRICT answer FROM content.check_user_thread_quota(decode(repeat('11',32),'hex'),'quotapriv',1000013);
   IF answer.rejected THEN RAISE EXCEPTION 'Staff private board quota mismatch'; END IF;
  END IF;
 ELSE
  BEGIN
   PERFORM * FROM content.check_user_thread_quota(decode(repeat('11',32),'hex'),'quota',1000013);
   RAISE EXCEPTION 'Unrelated runtime called quota' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 END IF;
 BEGIN
  PERFORM actor_hash FROM post_secrets.posting_history;
  RAISE EXCEPTION 'Runtime read raw history' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SQL
 done
 "${psql[@]}" -U board_public -d "$database" <<'SQL'
BEGIN ISOLATION LEVEL REPEATABLE READ;
DO $$ BEGIN
 BEGIN
  PERFORM * FROM content.check_user_thread_quota(decode(repeat('11',32),'hex'),'quota',1000013);
  RAISE EXCEPTION 'Non Read Committed quota accepted' USING ERRCODE='ZX001';
 EXCEPTION WHEN invalid_parameter_value THEN NULL; END;
END $$;
ROLLBACK;
SQL
 # Canonical names and effective ACL tuples normalize NULL vs explicit owner
 # defaults and ACL ordering. Sequences use acldefault('s'), never uppercase S.
 "${psql[@]}" -U board_migrator -d "$database" -At > "$cluster/$mode-$phase.fingerprint" <<'SQL'
SELECT relation,md5(value::text) FROM public.capture_quota_rows() ORDER BY relation,value::text;
SELECT kind,object,md5(value::text) FROM public.quota_authority ORDER BY kind,object,value::text;
SELECT n.nspname||'.'||c.relname||'.'||a.attname,a.attnum,a.atttypid::regtype,a.attnotnull,
 coalesce(pg_get_expr(d.adbin,d.adrelid),'')
 FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace
 LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
 WHERE n.nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission')
 AND a.attnum>0 AND NOT a.attisdropped ORDER BY 1;
-- PostgreSQL's precedence-aware pretty deparser suppresses redundant grouping
-- of nested ANDs introduced by BETWEEN expansion. Dump/reparse may flatten
-- those nodes. Keep the entire definition, including necessary mixed AND/OR
-- grouping, plus validation/deferral/inheritance flags; never strip parentheses
-- textually or whitelist away existing constraints.
SELECT conrelid::regclass::text,conname,contype,convalidated,condeferrable,condeferred,
 connoinherit,pg_get_constraintdef(oid,true) FROM pg_constraint
 WHERE connamespace IN(SELECT oid FROM pg_namespace WHERE nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission')) ORDER BY 1,2;
SELECT schemaname,indexname,indexdef FROM pg_indexes WHERE schemaname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission') ORDER BY 1,2;
SQL
 done
 cmp -s "$cluster/$mode-live.fingerprint" "$cluster/$mode-restored.fingerprint" || {
  printf 'Thread-quota current dump/restore fingerprint mismatch (%s).\n' "$mode" >&3; exit 1;
 }
 printf '%s IP-only thread-quota migration and current dump/restore passed.\n' "$mode" >&3
done
printf 'Historical identity remains unknown; no identity backfill or password/Pass equivalence claimed.\n' >&3
