#!/usr/bin/env bash
# Owned synthetic clusters only. Bound qualification to 0102 -> 0103.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-grouped-options.XXXXXXXX)
started=0
cleanup() {
    status=$?
    trap - EXIT
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null || status=1
    fi
    [[ $cluster =~ ^/tmp/board-grouped-options\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
    [[ $(readlink -f "$cluster") = "$cluster" ]] || exit 1
    rm -rf -- "$cluster"
    exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
chown postgres:postgres "$cluster"
runuser -u postgres -- "$pg_bin/initdb" -D "$cluster/data" --auth=trust --encoding=UTF8 --no-locale > /dev/null
runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" \
    -o "-c listen_addresses='' -c unix_socket_directories='$cluster' -c statement_timeout=30000 -c lock_timeout=5000" -w start > /dev/null
started=1
psql=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h "$cluster")
exec 3>&1
exec > "$cluster/qualification.log" 2>&1
trap 'printf "Grouped-options qualification failed at line %s (private diagnostics removed during cleanup).\n" "$LINENO" >&3' ERR
# The shell opens private root-owned fixture files before dropping privileges.
runuser -u postgres -- "${psql[@]}" -d postgres -f - < deploy/roles.sql
runuser -u postgres -- "${psql[@]}" -d postgres -c 'ALTER ROLE board_staff LOGIN; ALTER ROLE board_auth LOGIN'
create_database() {
    runuser -u postgres -- "${psql[@]}" -d postgres -v database="$1" <<'SQL'
CREATE DATABASE :"database" OWNER board_migrator;
REVOKE ALL ON DATABASE :"database" FROM PUBLIC;
GRANT CONNECT ON DATABASE :"database" TO board_migrator,board_public,board_staff,board_auth;
SQL
}
cat > "$cluster/fixture.sql" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,staff_only,archive_retention_seconds)
VALUES ('groupopt','Owned grouped fixture','Synthetic',1000,100,100,100,10,false,2592000),
       ('grouppriv','Owned private fixture','Synthetic',1000,100,100,100,10,true,2592000);
INSERT INTO content.threads(id,board,created_at,bumped_at,modified_at,sticky,closed,permasage,permaage,undead)
SELECT 10300000+i,CASE WHEN i=8 THEN 'grouppriv' ELSE 'groupopt' END,
       '2020-01-01 UTC'::timestamptz,'2020-01-01 UTC'::timestamptz+i*interval '1 minute',
       '2020-02-01 UTC'::timestamptz,i IN(1,2,3),i=2,i=1,i=3,i=2
FROM generate_series(1,8) i;
BEGIN;
SELECT set_config('board.posting_actor',repeat('11',32),true);
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
SELECT id,board,id,'Synthetic','','Retained body','2020-01-01 UTC'::timestamptz FROM content.threads WHERE id BETWEEN 10300001 AND 10300008;
COMMIT;
-- Register posts while the threads are live, then establish historical states
-- through the normal owner update path. Production history-removal triggers
-- must run; never disable them or manufacture identity for archived content.
UPDATE content.threads SET archived_at='2020-03-01 UTC'::timestamptz,
 archive_expires_at=CASE id WHEN 10300005 THEN '2099-01-01 UTC'::timestamptz ELSE '2020-04-01 UTC'::timestamptz END
WHERE id IN(10300005,10300006);
UPDATE content.threads SET deleted=true WHERE id=10300007;
INSERT INTO content.moderation_audit(account_id,board,target_id,action,created_at)
SELECT 1,'groupopt',10300001,action,'2020-02-02 UTC'::timestamptz
FROM unnest(ARRAY['close','reopen','sticky','unsticky','permasage','unpermasage','permaage','unpermaage',
 'remove-post','remove-file','remove-thread','resolve','dismiss','staff-post','spoiler','unspoiler','undead','unundead']) action;
SQL
cat > "$cluster/role-checks.sql" <<'SQL'
BEGIN;
DO $$ BEGIN
 IF (SELECT array_agg(id ORDER BY id) FROM content.visible_threads WHERE board IN('groupopt','grouppriv'))
    IS DISTINCT FROM ARRAY[10300001,10300002,10300003,10300004,10300005]::bigint[]
 THEN RAISE EXCEPTION 'Public visibility changed'; END IF;
 IF EXISTS(SELECT 1 FROM content.threads WHERE board='grouppriv')
 THEN RAISE EXCEPTION 'Public direct read exposed private board'; END IF;
 BEGIN
  UPDATE content.threads SET sticky_rank=60 WHERE id=10300001;
  RAISE EXCEPTION 'Public rank update succeeded' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN
  INSERT INTO content.threads(id,board,sticky_rank) VALUES(10300999,'groupopt',60);
  RAISE EXCEPTION 'Public rank insertion succeeded' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
ROLLBACK;
SQL
cat > "$cluster/staff-checks.sql" <<'SQL'
BEGIN;
DO $$ DECLARE bad integer; item record; BEGIN
 IF NOT EXISTS(SELECT 1 FROM content.threads WHERE id=10300008 AND board='grouppriv')
 OR NOT EXISTS(SELECT 1 FROM content.visible_threads WHERE id=10300008 AND board='grouppriv')
 THEN RAISE EXCEPTION 'Staff private visibility changed'; END IF;
 -- Smallint 60 is deliberately explicit local storage. No source timestamp
 -- encoding or legacy persistence claim is made for this boundary value.
 UPDATE content.threads SET sticky_rank=60 WHERE id=10300001;
 UPDATE content.threads SET sticky_rank=1 WHERE id=10300002;
 UPDATE content.threads SET sticky_rank=0 WHERE id=10300003;
 UPDATE content.threads SET sticky_rank=60 WHERE id=10300004;
 IF (SELECT array_agg(id ORDER BY sticky DESC,CASE WHEN sticky THEN sticky_rank ELSE 0 END DESC,bumped_at DESC,id DESC)
     FROM content.threads WHERE board='groupopt' AND NOT deleted AND archived_at IS NULL)
    IS DISTINCT FROM ARRAY[10300001,10300002,10300003,10300004]::bigint[]
 THEN RAISE EXCEPTION 'Sticky rank ordering failed'; END IF;
 UPDATE content.threads SET sticky=false WHERE id IN(10300001,10300002,10300003);
 IF (SELECT array_agg(id ORDER BY sticky DESC,CASE WHEN sticky THEN sticky_rank ELSE 0 END DESC,bumped_at DESC,id DESC)
     FROM content.threads WHERE board='groupopt' AND NOT deleted AND archived_at IS NULL)
    IS DISTINCT FROM ARRAY[10300004,10300003,10300002,10300001]::bigint[]
 THEN RAISE EXCEPTION 'Inactive sticky rank affected order'; END IF;
 FOREACH bad IN ARRAY ARRAY[-1,61] LOOP
  BEGIN
   UPDATE content.threads SET sticky_rank=bad WHERE id=10300001;
   RAISE EXCEPTION 'Out-of-range rank accepted' USING ERRCODE='ZX001';
  EXCEPTION WHEN check_violation THEN NULL; END;
 END LOOP;
 BEGIN
  UPDATE content.threads SET sticky_rank=NULL WHERE id=10300001;
  RAISE EXCEPTION 'NULL rank accepted' USING ERRCODE='ZX001';
 EXCEPTION WHEN not_null_violation THEN NULL; END;
 -- Explicit identity keeps rejected inserts from advancing a sequence, so
 -- the same checks can run again after restore without changing the snapshot.
 INSERT INTO content.moderation_audit(id,account_id,board,target_id,action,before_mask,after_mask)
 OVERRIDING SYSTEM VALUE VALUES(10300990,1,'groupopt',10300001,'thread-options',0,31),
 (10300991,1,'groupopt',10300001,'thread-options',31,0);
 FOR item IN SELECT * FROM (VALUES
  ('thread-options',NULL::smallint,NULL::smallint),('thread-options',NULL,1),('thread-options',1,NULL),
  ('thread-options',0,0),('thread-options',31,31),('thread-options',-1,0),('thread-options',0,-1),
  ('thread-options',32,0),('thread-options',0,32),('close',0,1),('close',NULL,1),('close',1,NULL)
 ) invalid(action,before_mask,after_mask) LOOP
  BEGIN
   INSERT INTO content.moderation_audit(id,account_id,board,target_id,action,before_mask,after_mask)
   OVERRIDING SYSTEM VALUE VALUES(10300992,1,'groupopt',10300001,item.action,item.before_mask,item.after_mask);
   RAISE EXCEPTION 'Invalid audit masks accepted' USING ERRCODE='ZX001';
  EXCEPTION WHEN check_violation THEN NULL; END;
 END LOOP;
END $$;
ROLLBACK;
SQL
for mode in upgrade fresh; do
 database="grouped_options_$mode"
 create_database "$database"
 migrator=("${psql[@]}" -U board_migrator -d "$database")
 admin=(runuser -u postgres -- "${psql[@]}" -d "$database")
 for migration in migrations/*.sql; do
  [[ $migration < migrations/0103_grouped_thread_options.sql ]] || break
  "${migrator[@]}" --single-transaction -f - < "$migration"
 done
 # Historical readiness is introspection only: never extract current Rust
 # readiness queries, which are allowed to require schema newer than 0102.
 "${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF to_regclass('post_secrets.report_weight_evidence') IS NULL
 OR EXISTS(SELECT 1 FROM pg_attribute WHERE attrelid='content.threads'::regclass AND attname='sticky_rank' AND NOT attisdropped)
 OR EXISTS(SELECT 1 FROM pg_attribute WHERE attrelid='content.moderation_audit'::regclass AND attname IN('before_mask','after_mask') AND NOT attisdropped)
 THEN RAISE EXCEPTION 'Expected historical 0102 boundary'; END IF;
END $$;
CREATE FUNCTION public.capture_rows() RETURNS TABLE(relation text,value jsonb)
LANGUAGE plpgsql AS $$ DECLARE r record; BEGIN
 FOR r IN SELECT n.nspname,c.relname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission') AND c.relkind='r' LOOP
  RETURN QUERY EXECUTE format('SELECT %L,to_jsonb(r) FROM %I.%I r',r.nspname||'.'||r.relname,r.nspname,r.relname);
 END LOOP;
END $$;
REVOKE ALL ON FUNCTION public.capture_rows() FROM PUBLIC;
CREATE VIEW public.old_shape AS SELECT relation,CASE relation
 WHEN 'content.threads' THEN value-'sticky_rank'
 WHEN 'content.moderation_audit' THEN value-'before_mask'-'after_mask'
 ELSE value END value FROM public.capture_rows();
CREATE VIEW public.narrow_grants AS
SELECT table_schema,table_name,column_name,grantor,grantee,privilege_type,is_grantable
FROM information_schema.column_privileges WHERE grantee='board_staff_post_owner';
CREATE VIEW public.functions AS
SELECT n.nspname,p.oid::regprocedure::text signature,pg_get_userbyid(p.proowner) owner,
 (SELECT jsonb_agg(jsonb_build_array(pg_get_userbyid(a.grantor),
   CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
   ORDER BY pg_get_userbyid(a.grantor),CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
  FROM aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a) acl,
 p.proconfig::text config,md5(pg_get_functiondef(p.oid)) definition
FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
WHERE n.nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission');
SQL
 if [[ $mode = upgrade ]]; then "${migrator[@]}" -f - < "$cluster/fixture.sql"; fi
 "${admin[@]}" -c 'CREATE TABLE public.before_rows AS TABLE public.old_shape; CREATE TABLE public.before_grants AS TABLE public.narrow_grants; CREATE TABLE public.before_functions AS TABLE public.functions'
 "${migrator[@]}" --single-transaction -f - < migrations/0103_grouped_thread_options.sql
 "${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(TABLE public.before_rows EXCEPT ALL TABLE public.old_shape)
 OR EXISTS(TABLE public.old_shape EXCEPT ALL TABLE public.before_rows)
 OR EXISTS(SELECT 1 FROM content.threads WHERE sticky_rank<>0)
 OR EXISTS(SELECT 1 FROM content.moderation_audit WHERE before_mask IS NOT NULL OR after_mask IS NOT NULL)
 THEN RAISE EXCEPTION 'Migration changed retained rows, clocks, flags or audit history'; END IF;
 IF EXISTS(TABLE public.before_grants EXCEPT ALL TABLE public.narrow_grants)
 OR EXISTS(TABLE public.narrow_grants EXCEPT ALL TABLE public.before_grants)
 OR EXISTS(TABLE public.before_functions EXCEPT ALL TABLE public.functions)
 OR EXISTS(TABLE public.functions EXCEPT ALL TABLE public.before_functions)
 THEN RAISE EXCEPTION 'Migration changed helper definitions or narrow grants'; END IF;
END $$;
DROP TABLE public.before_rows,public.before_grants,public.before_functions;
SQL
 if [[ $mode = fresh ]]; then "${migrator[@]}" -f - < "$cluster/fixture.sql"; fi
 "${psql[@]}" -U board_staff -d "$database" <<'SQL'
UPDATE content.threads SET sticky_rank=60 WHERE id=10300001;
UPDATE content.threads SET sticky_rank=12 WHERE id=10300004;
INSERT INTO content.moderation_audit(account_id,board,target_id,action,before_mask,after_mask)
VALUES(1,'groupopt',10300001,'thread-options',1,31);
SQL
 for phase in live restored; do
  if [[ $phase = restored ]]; then
   runuser -u postgres -- "$pg_bin/pg_dump" -h "$cluster" -U postgres -d "$database" --format=custom > "$cluster/current.dump"
   create_database "${database}_restore"
   runuser -u postgres -- "$pg_bin/pg_restore" -h "$cluster" --dbname="${database}_restore" --single-transaction --exit-on-error < "$cluster/current.dump"
   database="${database}_restore"
   admin=(runuser -u postgres -- "${psql[@]}" -d "$database")
  fi
  "${psql[@]}" -U board_public -d "$database" -f - < "$cluster/role-checks.sql"
  "${psql[@]}" -U board_staff -d "$database" -f - < "$cluster/staff-checks.sql"
  "${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM pg_attribute WHERE attrelid='content.threads'::regclass AND attname='sticky_rank' AND atttypid='smallint'::regtype AND attnotnull)
 OR (SELECT pg_get_expr(d.adbin,d.adrelid) FROM pg_attrdef d JOIN pg_attribute a ON a.attrelid=d.adrelid AND a.attnum=d.adnum WHERE a.attrelid='content.threads'::regclass AND a.attname='sticky_rank') IS DISTINCT FROM '0'
 OR NOT has_column_privilege('board_staff','content.threads','sticky_rank','UPDATE')
 OR has_column_privilege('board_public','content.threads','sticky_rank','UPDATE')
 OR has_column_privilege('board_public','content.threads','sticky_rank','INSERT')
 OR has_column_privilege('board_staff_post_owner','content.threads','sticky_rank','UPDATE')
 OR has_column_privilege('board_staff_post_owner','content.moderation_audit','before_mask','INSERT')
 OR has_column_privilege('board_staff_post_owner','content.moderation_audit','after_mask','INSERT')
 OR NOT coalesce((SELECT reloptions @> ARRAY['security_barrier=true'] FROM pg_class WHERE oid='content.visible_threads'::regclass),false)
 THEN RAISE EXCEPTION 'Rank schema or runtime privilege contract changed'; END IF;
 IF (SELECT count(*) FROM content.moderation_audit WHERE action<>'thread-options' AND before_mask IS NULL AND after_mask IS NULL)<>18
 OR NOT EXISTS(SELECT 1 FROM content.moderation_audit WHERE action='thread-options' AND before_mask=1 AND after_mask=31)
 OR (SELECT sticky_rank FROM content.threads WHERE id=10300001)<>60
 OR (SELECT sticky_rank FROM content.threads WHERE id=10300004)<>12
 THEN RAISE EXCEPTION 'Retained fixture or grouped evidence missing'; END IF;
END $$;
-- The owner role cannot inherit the staff runtime's new column authority.
SET ROLE board_staff_post_owner;
DO $$ BEGIN
 BEGIN
  UPDATE content.threads SET sticky_rank=1 WHERE id=10300001;
  RAISE EXCEPTION 'Narrow helper owner changed rank' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
RESET ROLE;
SQL
  "${admin[@]}" -At > "$cluster/$mode-$phase.fingerprint" <<'SQL'
SELECT relation,md5(value::text) FROM public.capture_rows() ORDER BY relation,value::text;
SELECT * FROM public.functions ORDER BY nspname,signature;
SELECT * FROM public.narrow_grants ORDER BY table_schema,table_name,column_name,grantor,grantee,privilege_type,is_grantable;
-- pg_dump can reorder ACL array entries. Compare the complete privilege set
-- semantically, including grantor, grantee, grant option and implicit defaults.
SELECT n.nspname,c.relname,pg_get_userbyid(c.relowner),
 (SELECT jsonb_agg(jsonb_build_array(pg_get_userbyid(a.grantor),
   CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
   ORDER BY pg_get_userbyid(a.grantor),CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
  FROM aclexplode(coalesce(c.relacl,acldefault(CASE WHEN c.relkind='S' THEN 's'::"char" ELSE 'r'::"char" END,c.relowner))) a),
 c.reloptions,c.relrowsecurity,c.relforcerowsecurity
FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission') ORDER BY 1,2;
SELECT table_schema,table_name,column_name,grantor,grantee,privilege_type,is_grantable FROM information_schema.column_privileges WHERE table_schema IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission') ORDER BY 1,2,3,4,5,6,7;
SELECT c.conrelid::regclass,c.conname,pg_get_constraintdef(c.oid) FROM pg_constraint c WHERE c.conrelid IN('content.threads'::regclass,'content.moderation_audit'::regclass) ORDER BY c.conrelid::regclass::text,c.conname;
SELECT indexname,indexdef FROM pg_indexes WHERE schemaname='content' AND tablename='threads' ORDER BY indexname;
SELECT pg_get_viewdef('content.visible_threads'::regclass,true);
SELECT schemaname,tablename,policyname,roles,cmd,qual,with_check FROM pg_policies ORDER BY 1,2,3;
SELECT last_value,is_called FROM content.moderation_audit_id_seq;
SELECT rolname,rolsuper,rolinherit,rolcreaterole,rolcreatedb,rolcanlogin,rolreplication,rolbypassrls,rolconfig FROM pg_roles WHERE rolname LIKE 'board_%' ORDER BY rolname;
SQL
 done
 cmp -s "$cluster/$mode-live.fingerprint" "$cluster/$mode-restored.fingerprint" || {
  printf 'Grouped-options dump/restore fingerprint mismatch (%s); bounded synthetic schema/row-hash diff follows.\n' "$mode" >&3
  # No source content, secrets, credentials or fixture bodies enter this diff:
  # fingerprints contain only schema metadata, privilege names and row hashes.
  # Write the diff first so pipefail cannot interrupt bounded diagnostic output.
  diff -u --label live --label restored "$cluster/$mode-live.fingerprint" "$cluster/$mode-restored.fingerprint" > "$cluster/fingerprint.diff" || true
  sed -n '1,100p' "$cluster/fingerprint.diff" >&3
  exit 1
 }
 printf '%s grouped-options migration, role separation and administrator dump/restore passed.\n' "$mode" >&3
done
printf '0103 grouped masks and explicit bounded sticky rank qualified without historical timestamp rewriting.\n' >&3
