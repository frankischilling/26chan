#!/usr/bin/env bash
# Owned synthetic clusters only. Historical archives are intentionally grandfathered:
# no backfill or physical-erasure claim is made by this qualification.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-archive-secrets.XXXXXXXX)
started=0
cleanup() {
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null
    fi
    [[ $cluster =~ ^/tmp/board-archive-secrets\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
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
trap 'printf "Archive-secret qualification failed at line %s (private diagnostics removed during cleanup).\n" "$LINENO" >&3' ERR
runuser -u postgres -- "${psql[@]}" -d postgres -f deploy/roles.sql
runuser -u postgres -- "${psql[@]}" -d postgres <<'SQL'
ALTER ROLE board_staff LOGIN;
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
 database="archive_secrets_$mode"
 create_database "$database"
 migrator=("${psql[@]}" -U board_migrator -d "$database")
 for migration in migrations/*.sql; do
    [[ $migration != migrations/0091* ]] || break
    "${migrator[@]}" --single-transaction -f "$migration"
 done
 # Fresh installs get 0091 before content; upgrades get all fixture rows at 0090.
 if [[ $mode = fresh ]]; then
    "${migrator[@]}" --single-transaction -f migrations/0091*.sql
 fi
 "${migrator[@]}" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,staff_only)
VALUES('seccheck','Archive retirement fixture','Synthetic',1000,100,100,100,10,false),
 ('secpriv','Private archive fixture','Synthetic',1000,100,100,100,10,true);
UPDATE content.boards SET archive_retention_seconds=86400 WHERE slug IN ('seccheck','secpriv');
BEGIN;
SELECT set_config('board.posting_actor',repeat('11',32),true);
INSERT INTO content.threads(id,board,sticky,undead)
VALUES(9101000,'seccheck',false,false),(9102000,'seccheck',false,false),
 (9103000,'seccheck',true,true),(9104000,'secpriv',false,false),
 (9105000,'seccheck',false,false),(9106000,'secpriv',false,false);
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,deleted)
VALUES(9101000,'seccheck',9101000,'Synthetic','Retiring OP','Retain body',false),
 (9101001,'seccheck',9101000,'Synthetic','','Retain reply',false),
 (9101002,'seccheck',9101000,'Synthetic','','Retain deleted reply',false),
 (9102000,'seccheck',9102000,'Synthetic','Active OP','Keep active authority',false),
 (9102001,'seccheck',9102000,'Synthetic','','Active reply without initial hash',false),
 (9103000,'seccheck',9103000,'Synthetic','Protected OP','Keep protected authority',false),
 (9104000,'secpriv',9104000,'Synthetic','Private OP','Keep private body',false),
 (9104001,'secpriv',9104000,'Synthetic','','Private reply',false),
 (9105000,'seccheck',9105000,'Synthetic','Legacy OP','Keep legacy body',false),
 (9105001,'seccheck',9105000,'Synthetic','','Legacy reply',false),
 (9106000,'secpriv',9106000,'Synthetic','Private active OP','Keep private authority',false),
 (9106001,'secpriv',9106000,'Synthetic','','Private active reply without hash',false);
COMMIT;
UPDATE content.posts SET deleted=true WHERE id=9101002;
INSERT INTO post_secrets.deletion(post_id,password_hash)
SELECT id,'synthetic-deletion-'||id FROM content.posts WHERE id BETWEEN 9101000 AND 9106001 AND id NOT IN(9102001,9106001);
INSERT INTO post_secrets.op_peers(thread_id,peer) VALUES(9101000,'192.0.2.91');
INSERT INTO post_secrets.op_replies(post_id,thread_id) VALUES(9101001,9101000);
INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at)
VALUES(repeat('c',32),repeat('c',32),repeat('c',32),repeat('c',64),100,500,300,'approved',clock_timestamp());
INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler,file_deleted)
VALUES(9101000,repeat('c',32),repeat('c',32),'synthetic-retained.png',100,500,300,true,false);
INSERT INTO content.moderation_audit(account_id,board,target_id,action) VALUES(42,'seccheck',9101002,'remove-post');
INSERT INTO content.reports(board,post_id,reason) VALUES('seccheck',9101000,'Synthetic historical report');
INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,
 created_at,network_at,address_at,environment_at,expires_at,posts,threads)
VALUES(decode(repeat('01',32),'hex'),decode(repeat('02',32),'hex'),decode(repeat('03',32),'hex'),
 decode(repeat('04',32),'hex'),1,1,1,1,4102444800,1,1);
INSERT INTO post_secrets.anonymous_posts(post_id,token_hash,password_proof)
VALUES(9101000,decode(repeat('01',32),'hex'),decode(repeat('05',32),'hex'));
INSERT INTO post_secrets.anonymous_reports(report_id,token_hash)
SELECT id,decode(repeat('01',32),'hex') FROM content.reports WHERE board='seccheck';
INSERT INTO post_secrets.posting_thread_actions(actor_hash,board,request_at)
VALUES(decode(repeat('11',32),'hex'),'seccheck',1000)
ON CONFLICT(actor_hash,board) DO UPDATE SET request_at=EXCLUDED.request_at;
SQL
 if [[ $mode = upgrade ]]; then
 "${migrator[@]}" <<'SQL'
-- The old archive hashes must exist before installing 0091. No trigger bypass.
UPDATE content.threads SET archived_at=statement_timestamp(),
 archive_expires_at=statement_timestamp()+interval '1 day' WHERE id=9105000;
SQL
 fi
 "${migrator[@]}" <<'SQL'
CREATE TABLE public.secret_rows_before(relation text,value jsonb);
CREATE FUNCTION public.capture_secret_rows() RETURNS TABLE(relation text,value jsonb)
LANGUAGE plpgsql AS $$ DECLARE r record; BEGIN
 FOR r IN SELECT n.nspname,c.relname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname IN ('content','post_secrets','staff_identity','media','media_intake','deployment','admission') AND c.relkind='r' LOOP
  RETURN QUERY EXECUTE format('SELECT %L,to_jsonb(r) FROM %I.%I r',r.nspname||'.'||r.relname,r.nspname,r.relname);
 END LOOP;
END $$;
INSERT INTO public.secret_rows_before SELECT * FROM public.capture_secret_rows();
-- Canonical names, rather than database OIDs, also support dump/restore comparison.
-- acldefault uses lowercase s for sequences (uppercase S means foreign server),
-- unlike pg_class.relkind. Expand NULL ACLs to the correct object-type defaults.
-- Dump/restore can replace
-- explicit owner-only ACLs with NULL and reorder entries without changing rights.
-- Exploded table/column tuples are compared as sets and sorted in fingerprints;
-- nested schema/function tuples are sorted explicitly. Owners remain separate.
CREATE VIEW public.secret_authority AS
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
CREATE TABLE public.secret_authority_before AS TABLE public.secret_authority;
SQL
 if [[ $mode = upgrade ]]; then
    "${migrator[@]}" --single-transaction -f migrations/0091*.sql
 fi
 "${migrator[@]}" -v upgrade="$mode" <<'SQL'
SELECT set_config('qualification.mode',:'upgrade',false);
DO $$ BEGIN
 IF EXISTS(TABLE public.secret_rows_before EXCEPT ALL SELECT * FROM public.capture_secret_rows())
 OR EXISTS(SELECT * FROM public.capture_secret_rows() EXCEPT ALL TABLE public.secret_rows_before)
 THEN RAISE EXCEPTION 'Migration changed preexisting rows, including legacy hashes'; END IF;
 IF EXISTS(TABLE public.secret_authority_before EXCEPT TABLE public.secret_authority)
 THEN RAISE EXCEPTION 'Migration removed or rewrote preexisting authority'; END IF;
 IF EXISTS(SELECT 1 FROM (TABLE public.secret_authority EXCEPT TABLE public.secret_authority_before) d
 WHERE NOT (
  (kind='table grant' AND object='post_secrets.deletion' AND value=jsonb_build_array('board_migrator','board_posting_cooldown_owner','DELETE',false)) OR
  (kind='column grant' AND object='post_secrets.deletion.post_id' AND value=jsonb_build_array('board_migrator','board_posting_cooldown_owner','SELECT',false)) OR
  (kind='column grant' AND object='content.threads.id' AND value=jsonb_build_array('board_migrator','board_posting_cooldown_owner','UPDATE',false)) OR
  (kind='function' AND object IN ('post_secrets.guard_archived_deletion_secret()','post_secrets.retire_archived_deletion_secrets()')) OR
  (kind='trigger' AND object IN ('post_secrets.deletion.deletion_archive_guard','content.threads.retire_archived_deletion_secrets'))))
 THEN RAISE EXCEPTION 'Migration added unrelated authority'; END IF;
 IF current_setting('qualification.mode')='upgrade' THEN
  IF (SELECT count(*) FROM post_secrets.deletion WHERE post_id IN (9105000,9105001))<>2
  THEN RAISE EXCEPTION 'Prospective migration removed legacy hashes'; END IF;
  BEGIN
   UPDATE post_secrets.deletion SET password_hash='synthetic-rotation' WHERE post_id=9105000;
   RAISE EXCEPTION 'Legacy archived hash rotation succeeded' USING ERRCODE='ZX001';
  EXCEPTION WHEN check_violation THEN NULL; END;
  BEGIN
   UPDATE post_secrets.deletion SET post_id=9102001 WHERE post_id=9105001;
   RAISE EXCEPTION 'OLD archived hash moved into active content' USING ERRCODE='ZX001';
  EXCEPTION WHEN check_violation THEN NULL; END;
 END IF;
END $$;
-- Existing 0024/0087 archive cleanup also removes private peer/reply and posting
-- history, while anonymous proof derivatives and action rows must survive.
DELETE FROM public.secret_rows_before WHERE relation IN
 ('post_secrets.op_peers','post_secrets.op_replies','post_secrets.posting_history')
 AND (value->>'thread_id')::bigint IN(9101000,9104000);
-- Snapshot expected exceptions only: target thread archive metadata and hashes.
UPDATE content.threads SET archived_at=statement_timestamp(),
 archive_expires_at=statement_timestamp()+interval '1 day' WHERE id IN (9101000,9104000);
-- Only the two updated threads may change archive fields and the existing
-- 0025 HTTP validator clock. Unrelated thread clocks/archives remain exact.
CREATE TEMP TABLE expected_archive_rows AS
 SELECT relation,CASE WHEN relation='content.threads' AND value->>'id' IN('9101000','9104000')
  THEN value-ARRAY['archived_at','archive_expires_at','http_modified_at'] ELSE value END value
 FROM public.secret_rows_before
 WHERE NOT(relation='post_secrets.deletion' AND (value->>'post_id')::bigint IN(9101000,9101001,9101002,9104000,9104001));
CREATE TEMP TABLE actual_archive_rows AS
 SELECT relation,CASE WHEN relation='content.threads' AND value->>'id' IN('9101000','9104000')
  THEN value-ARRAY['archived_at','archive_expires_at','http_modified_at'] ELSE value END value
 FROM public.capture_secret_rows();
DO $$ DECLARE mismatch record; BEGIN
 BEGIN
  UPDATE post_secrets.deletion SET post_id=9101001 WHERE post_id=9102000;
  RAISE EXCEPTION 'Active hash moved into NEW archived parent' USING ERRCODE='ZX001';
 EXCEPTION WHEN check_violation THEN NULL; END;
 IF EXISTS(SELECT 1 FROM post_secrets.deletion WHERE post_id IN(9101000,9101001,9101002,9104000,9104001))
 THEN RAISE EXCEPTION 'New archive retained an OP, reply or deleted-reply hash'; END IF;
 IF (SELECT count(*) FROM content.threads t JOIN public.secret_rows_before old
  ON old.relation='content.threads' AND old.value->>'id'=t.id::text
  WHERE t.id IN(9101000,9104000) AND t.archived_at IS NOT NULL
   AND t.archive_expires_at=t.archived_at+interval '1 day'
   AND t.http_modified_at >= (old.value->>'http_modified_at')::timestamptz
   AND t.http_modified_at >= t.archived_at AND t.http_modified_at <= clock_timestamp())<>2
 THEN RAISE EXCEPTION 'Archive metadata or existing HTTP validator clock invalid'; END IF;
 IF EXISTS(TABLE expected_archive_rows EXCEPT ALL TABLE actual_archive_rows)
 OR EXISTS(TABLE actual_archive_rows EXCEPT ALL TABLE expected_archive_rows) THEN
  -- Bounded diagnostics expose relation, numeric content ID and column names
  -- only. No row values, hashes, proof derivatives or private keys are logged.
  FOR mismatch IN
   WITH missing AS (TABLE expected_archive_rows EXCEPT ALL TABLE actual_archive_rows),
   extra AS (TABLE actual_archive_rows EXCEPT ALL TABLE expected_archive_rows),
   paired AS (
    SELECT coalesce(b.relation,a.relation) relation,b.value old_value,a.value new_value
    FROM missing b FULL JOIN extra a ON a.relation=b.relation AND
     coalesce(b.value->'id',b.value->'post_id',b.value->'thread_id',b.value->'slug',b.value->'key',b.value->'token_hash',b.value->'actor_hash',b.value->'stripe',b.value->'singleton',b.value)
     =coalesce(a.value->'id',a.value->'post_id',a.value->'thread_id',a.value->'slug',a.value->'key',a.value->'token_hash',a.value->'actor_hash',a.value->'stripe',a.value->'singleton',a.value))
   SELECT relation,
    CASE WHEN relation IN('content.threads','content.posts','content.reports','content.moderation_audit')
     THEN coalesce(old_value->>'id',new_value->>'id') ELSE '[private key omitted]' END row_id,
    ARRAY(SELECT k FROM jsonb_object_keys(coalesce(old_value,'{}'::jsonb)||coalesce(new_value,'{}'::jsonb)) k
     WHERE old_value->k IS DISTINCT FROM new_value->k ORDER BY k) columns
   FROM paired ORDER BY relation LIMIT 20
  LOOP
   RAISE WARNING 'Archive preservation mismatch: relation=%, row=%, columns=%',mismatch.relation,mismatch.row_id,mismatch.columns;
  END LOOP;
  RAISE EXCEPTION 'Archive changed active/protected/legacy authority, content, media, audit or private proof state';
 END IF;
END $$;
-- Drop old secret snapshots before dump: the current backup must not contain
-- retired hashes in a qualification helper table either.
DROP TABLE public.secret_rows_before,public.secret_authority_before;
SQL
 # Same role/trigger contract is exercised before and after an ownership-preserving restore.
 for phase in live restored; do
 if [[ $phase = restored ]]; then
  # Keep the private dump owned by the same OS identity that restores it;
  # retain DB-level board_migrator access and umask 077 throughout.
  runuser -u postgres -- "$pg_bin/pg_dump" -h "$cluster" -U board_migrator -d "$database" \
    --format=custom --file="$cluster/current.dump"
  create_database "${database}_restore"
  # Existing bootstrap administrator restores NOLOGIN ownership and ACLs. No new role.
  runuser -u postgres -- "$pg_bin/pg_restore" -h "$cluster" --dbname="${database}_restore" \
    --single-transaction --exit-on-error < "$cluster/current.dump"
  database="${database}_restore"
 fi
 "${psql[@]}" -U board_migrator -d "$database" <<'SQL'
DO $$ DECLARE fn regprocedure; runtime text; BEGIN
 IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='board_posting_cooldown_owner' AND NOT rolcanlogin
  AND NOT rolsuper AND NOT rolcreatedb AND NOT rolcreaterole AND NOT rolreplication AND NOT rolbypassrls)
 OR has_schema_privilege('board_posting_cooldown_owner','post_secrets','CREATE')
 OR has_schema_privilege('board_posting_cooldown_owner','content','CREATE')
 OR has_column_privilege('board_posting_cooldown_owner','post_secrets.deletion','password_hash','SELECT')
 OR NOT has_column_privilege('board_posting_cooldown_owner','post_secrets.deletion','post_id','SELECT')
 OR NOT has_table_privilege('board_posting_cooldown_owner','post_secrets.deletion','DELETE')
 OR NOT has_column_privilege('board_posting_cooldown_owner','content.threads','id','UPDATE')
 THEN RAISE EXCEPTION 'Unsafe retirement owner authority'; END IF;
 FOREACH fn IN ARRAY ARRAY['post_secrets.guard_archived_deletion_secret()'::regprocedure,'post_secrets.retire_archived_deletion_secrets()'::regprocedure] LOOP
  IF NOT EXISTS(SELECT 1 FROM pg_proc WHERE oid=fn AND proowner='board_posting_cooldown_owner'::regrole
   AND prosecdef AND prorettype='trigger'::regtype AND proconfig=ARRAY['search_path=pg_catalog, pg_temp'])
  OR EXISTS(SELECT 1 FROM pg_proc p,LATERAL aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a
   WHERE p.oid=fn AND a.privilege_type='EXECUTE' AND a.grantee<>p.proowner)
  THEN RAISE EXCEPTION 'Unsafe function owner, path or EXECUTE ACL'; END IF;
  FOREACH runtime IN ARRAY ARRAY['board_public','board_staff','board_auth','board_media','board_media_read','board_media_intake','board_monitor'] LOOP
   IF has_function_privilege(runtime,fn,'EXECUTE') OR pg_has_role(runtime,'board_posting_cooldown_owner','MEMBER')
    OR has_table_privilege(runtime,'post_secrets.deletion','DELETE')
   THEN RAISE EXCEPTION 'Runtime gained retirement authority'; END IF;
  END LOOP;
 END LOOP;
 IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='post_secrets.deletion'::regclass
  AND tgname='deletion_archive_guard' AND tgfoid='post_secrets.guard_archived_deletion_secret()'::regprocedure
  AND tgtype=31 AND tgenabled='O' AND tgqual IS NULL AND tgattr::text='')
 OR NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='content.threads'::regclass
  AND tgname='retire_archived_deletion_secrets' AND tgfoid='post_secrets.retire_archived_deletion_secrets()'::regprocedure
  AND tgtype=17 AND tgenabled='O' AND tgattr::text=(SELECT attnum::text FROM pg_attribute WHERE attrelid='content.threads'::regclass AND attname='archived_at')
  AND pg_get_triggerdef(oid) LIKE '%WHEN (((old.archived_at IS NULL) AND (new.archived_at IS NOT NULL)))%')
 THEN RAISE EXCEPTION 'Wrong retirement trigger shape'; END IF;
 IF EXISTS(SELECT 1 FROM post_secrets.deletion WHERE post_id IN(9101000,9101001,9101002,9104000,9104001))
 OR (SELECT count(*) FROM post_secrets.deletion WHERE post_id IN(9102000,9103000,9105000,9105001,9106000))<>5
 THEN RAISE EXCEPTION 'Current state revived retired hashes or lost surviving authority'; END IF;
END $$;
SQL
 for role in board_public board_staff board_auth board_media board_media_read board_media_intake board_monitor; do
 "${psql[@]}" -U "$role" -d "$database" <<'SQL'
DO $$ BEGIN
 IF session_user='board_public' THEN
  IF EXISTS(SELECT 1 FROM post_secrets.deletion WHERE post_id IN(9104000,9104001,9106000,9106001))
  THEN RAISE EXCEPTION 'Public runtime read private hashes'; END IF;
 ELSE
  BEGIN
   PERFORM password_hash FROM post_secrets.deletion;
   RAISE EXCEPTION 'Unrelated runtime read raw password hashes' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 END IF;
 -- Extending the safety trigger to DELETE must not grant runtime revocation.
 BEGIN
  DELETE FROM post_secrets.deletion WHERE post_id=9102000;
  RAISE EXCEPTION 'Runtime gained raw deletion authority' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN
  INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES(9101001,'synthetic-revival');
  RAISE EXCEPTION 'Runtime revived archived authority' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege OR check_violation THEN NULL; END;
 BEGIN
  INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES(9106001,'synthetic-private-probe');
  RAISE EXCEPTION 'Runtime wrote private authority' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege OR check_violation THEN NULL; END;
 -- Keep permission-sensitive SQL in the role's own PL/pgSQL branch:
 -- boolean AND does not prevent planning/schema privilege checks.
 IF session_user='board_public' THEN
  IF EXISTS(SELECT 1 FROM content.posts WHERE board='secpriv')
   OR EXISTS(SELECT 1 FROM content.threads WHERE board='secpriv')
  THEN RAISE EXCEPTION 'Public runtime saw private content'; END IF;
 END IF;
END $$;
SQL
 done
 # Exact current rows and canonical ownership/grants must survive restore.
 "${psql[@]}" -U board_migrator -d "$database" -At > "$cluster/$mode-$phase.fingerprint" <<'SQL'
SELECT relation,md5(value::text) FROM public.capture_secret_rows() ORDER BY relation,value::text;
SELECT kind,object,md5(value::text) FROM public.secret_authority ORDER BY kind,object,value::text;
SQL
 # Diagnostic components distinguish permission semantics from ACL display order.
 # Values remain private hashes; failure output below prints names only.
 "${psql[@]}" -U board_migrator -d "$database" -At > "$cluster/$mode-$phase.components" <<'SQL'
SELECT 'function owner',p.oid::regprocedure::text,md5(pg_get_userbyid(p.proowner))
 FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
 WHERE n.nspname NOT LIKE 'pg_%' AND n.nspname NOT IN ('information_schema','public')
UNION ALL
SELECT 'function ACL representation',p.oid::regprocedure::text,md5(coalesce(p.proacl::text,'NULL'))
 FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
 WHERE n.nspname NOT LIKE 'pg_%' AND n.nspname NOT IN ('information_schema','public')
UNION ALL
SELECT 'function ACL semantics',p.oid::regprocedure::text,md5(coalesce((
 SELECT jsonb_agg(jsonb_build_array(pg_get_userbyid(a.grantor),
  CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
  ORDER BY pg_get_userbyid(a.grantor),CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
 FROM aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a),'[]'::jsonb)::text)
 FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
 WHERE n.nspname NOT LIKE 'pg_%' AND n.nspname NOT IN ('information_schema','public')
UNION ALL
SELECT 'function definition',p.oid::regprocedure::text,md5(pg_get_functiondef(p.oid))
 FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
 WHERE n.nspname NOT LIKE 'pg_%' AND n.nspname NOT IN ('information_schema','public')
UNION ALL
SELECT 'schema owner',n.nspname,md5(pg_get_userbyid(n.nspowner))
 FROM pg_namespace n WHERE n.nspname NOT LIKE 'pg_%' AND n.nspname NOT IN ('information_schema','public')
UNION ALL
SELECT 'schema ACL semantics',n.nspname,md5(coalesce((
 SELECT jsonb_agg(jsonb_build_array(pg_get_userbyid(a.grantor),
  CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
  ORDER BY pg_get_userbyid(a.grantor),CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
 FROM aclexplode(coalesce(n.nspacl,acldefault('n',n.nspowner))) a),'[]'::jsonb)::text)
 FROM pg_namespace n WHERE n.nspname NOT LIKE 'pg_%' AND n.nspname NOT IN ('information_schema','public')
ORDER BY 1,2,3;
SQL
 # Permission metadata only, never row data. Keep raw NULL/explicit ACL evidence
 # beside effective default-expanded tuples for the two observed mismatches.
 "${psql[@]}" -U board_migrator -d "$database" -At > "$cluster/$mode-$phase.acl-tuples" <<'SQL'
SELECT 'schema owner',n.nspname,pg_get_userbyid(n.nspowner)::text FROM pg_namespace n WHERE n.nspname='deployment'
UNION ALL SELECT 'schema stored ACL',n.nspname,coalesce(n.nspacl::text,'NULL') FROM pg_namespace n WHERE n.nspname='deployment'
UNION ALL SELECT 'schema effective ACL',n.nspname,jsonb_build_array(pg_get_userbyid(a.grantor),
 CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)::text
 FROM pg_namespace n,LATERAL aclexplode(coalesce(n.nspacl,acldefault('n',n.nspowner))) a WHERE n.nspname='deployment'
UNION ALL SELECT 'sequence owner',c.oid::regclass::text,pg_get_userbyid(c.relowner)::text
 FROM pg_class c WHERE c.oid='staff_identity.accounts_id_seq'::regclass
UNION ALL SELECT 'sequence stored ACL',c.oid::regclass::text,coalesce(c.relacl::text,'NULL')
 FROM pg_class c WHERE c.oid='staff_identity.accounts_id_seq'::regclass
UNION ALL SELECT 'sequence effective ACL',c.oid::regclass::text,jsonb_build_array(pg_get_userbyid(a.grantor),
 CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)::text
 FROM pg_class c,LATERAL aclexplode(coalesce(c.relacl,acldefault('s',c.relowner))) a
 WHERE c.oid='staff_identity.accounts_id_seq'::regclass
ORDER BY 1,2,3;
SQL
 done
 # Raw ACL representation is intentionally diagnostic only; canonical effective
 # schema/function ACL components must match, including on successful restores.
 grep -v '^function ACL representation|' "$cluster/$mode-live.components" > "$cluster/$mode-live.semantic-components"
 grep -v '^function ACL representation|' "$cluster/$mode-restored.components" > "$cluster/$mode-restored.semantic-components"
 if ! cmp -s "$cluster/$mode-live.fingerprint" "$cluster/$mode-restored.fingerprint" ||
    ! cmp -s "$cluster/$mode-live.semantic-components" "$cluster/$mode-restored.semantic-components"; then
  python3 - "$cluster/$mode-live.fingerprint" "$cluster/$mode-restored.fingerprint" \
   "$cluster/$mode-live.components" "$cluster/$mode-restored.components" \
   "$cluster/$mode-live.acl-tuples" "$cluster/$mode-restored.acl-tuples" >&3 <<'PY_DIAGNOSTICS'
import collections
import sys
for before_path, after_path in zip(sys.argv[1::2], sys.argv[2::2]):
    with open(before_path, encoding='utf-8') as f:
        before = collections.Counter(line.rstrip('\n') for line in f)
    with open(after_path, encoding='utf-8') as f:
        after = collections.Counter(line.rstrip('\n') for line in f)
    if before_path.endswith('.acl-tuples'):
        # These files contain only permission metadata: role names and grants.
        # Include matching effective tuples as well as the changed raw encoding.
        for label, values in [('before', before), ('after', after)]:
            for line in sorted(values)[:20]:
                print('Restore ACL evidence ' + label + ': ' + line)
        continue
    # Drop the final hash: expose only catalog names or row relation, bounded.
    changed = sorted({line.rsplit('|', 1)[0] for line in (before - after) | (after - before)})
    for name in changed[:20]:
        print('Restore fingerprint mismatch: ' + name)
    if len(changed) > 20:
        print('Additional mismatch names omitted: ' + str(len(changed) - 20))
raise SystemExit(1)
PY_DIAGNOSTICS
 fi
 printf '%s archive-secret migration and current dump/restore passed.\n' "$mode" >&3
done
printf 'Prospective retirement qualified; historical archived hashes intentionally survive. Content, media and proof derivatives remain. No physical erasure claim.\n' >&3
