#!/usr/bin/env bash
# Owned synthetic clusters only. Additive board policy must not rewrite reports or activity.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-report-target-policy.XXXXXXXX)
started=0
cleanup() {
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null
    fi
    [[ $cluster =~ ^/tmp/board-report-target-policy\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
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
trap 'printf "Report-target-policy qualification failed at line %s (private diagnostics removed during cleanup).\n" "$LINENO" >&3' ERR
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
cat > "$cluster/fixtures.sql" <<'SQL'
-- Include operator overrides and unrelated content/secret/proof/media state.
UPDATE content.boards SET comment_spoiler_cleanup=false,thread_limit=37,posting_reply_seconds=17,user_thread_limit=9 WHERE slug='a';
UPDATE content.boards SET thread_limit=41,posting_thread_seconds=73,user_thread_period_hours=33 WHERE slug='j';
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,staff_only)
VALUES('report','Report fixture','Synthetic',1000,100,100,100,10,false),
 ('reportpriv','Private report fixture','Synthetic',1000,100,100,100,10,true);
-- Historical imports deliberately omit board.posting_actor. Migration must not
-- infer their identity from unrelated deletion passwords or anonymous proofs.
INSERT INTO content.threads(id,board) VALUES(9300000,'report'),(9300100,'reportpriv');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(9300000,'report',9300000,'Synthetic','Historical OP','Retained body',to_timestamp(1000000)),
 (9300001,'report',9300000,'Synthetic','','Retained reply',to_timestamp(1000001)),
 (9300100,'reportpriv',9300100,'Synthetic','Private historical OP','Private body',to_timestamp(1000000));
INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES(9300000,'synthetic-hash');
INSERT INTO content.reports(board,post_id,reason) VALUES('report',9300000,'Synthetic report');
INSERT INTO content.moderation_audit(account_id,board,target_id,action) VALUES(42,'report',9300001,'remove-post');
INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at)
VALUES(repeat('c',32),repeat('c',32),repeat('c',32),repeat('c',64),100,500,300,'approved',clock_timestamp());
INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler,file_deleted)
VALUES(9300000,repeat('c',32),repeat('c',32),'synthetic.png',100,500,300,true,false);
INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,
 created_at,network_at,address_at,environment_at,expires_at,posts,threads)
VALUES(decode(repeat('01',32),'hex'),decode(repeat('02',32),'hex'),decode(repeat('03',32),'hex'),
 decode(repeat('04',32),'hex'),1,1,1,1,4102444800,1,1);
INSERT INTO post_secrets.anonymous_posts(post_id,token_hash,password_proof)
VALUES(9300000,decode(repeat('01',32),'hex'),decode(repeat('05',32),'hex'));
INSERT INTO post_secrets.anonymous_reports(report_id,token_hash)
SELECT id,decode(repeat('01',32),'hex') FROM content.reports WHERE board='report';
-- Existing, legitimately registered known identities must also survive.
BEGIN;
SELECT set_config('board.posting_actor',repeat('11',32),true);
INSERT INTO content.threads(id,board) SELECT 9301000+i,'report' FROM generate_series(1,12) i;
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
SELECT 9301000+i,'report',9301000+i,'Synthetic','Known OP','Known body',to_timestamp(1000000+i)
FROM generate_series(1,12) i;
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(9302000,'report',9301001,'Synthetic','','Known reply',to_timestamp(1000013));
COMMIT;
-- Existing j reports remain valid historical records despite its new target veto.
-- These are owned historical import fixtures, not wordfilter runtime requests.
-- Use an explicit operator policy override while importing, then restore the
-- exact source policy before the migration baseline. Leave the trigger enabled
-- and let it honestly store NULL filter metadata for these unfiltered imports.
BEGIN;
CREATE TEMP TABLE report_import_wordfilter_policy ON COMMIT DROP AS
SELECT slug,word_filter_enabled FROM content.boards WHERE slug='j';
UPDATE content.boards SET word_filter_enabled=false WHERE slug='j';
INSERT INTO content.threads(id,board) VALUES(9303000,'j');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(9303000,'j',9303000,'Synthetic','Historical j OP','Retained j body'),
 (9303001,'j',9303000,'Synthetic','','Retained j reply');
UPDATE content.boards b SET word_filter_enabled=p.word_filter_enabled
FROM report_import_wordfilter_policy p WHERE b.slug=p.slug;
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM report_import_wordfilter_policy p JOIN content.boards b USING(slug)
  WHERE b.word_filter_enabled IS DISTINCT FROM p.word_filter_enabled)
 OR EXISTS(SELECT 1 FROM content.posts WHERE id IN(9303000,9303001)
  AND (wordfilter_payload IS NOT NULL OR wordfilter_search IS NOT NULL))
 THEN RAISE EXCEPTION 'Historical import changed source policy or fabricated filter metadata'; END IF;
END $$;
COMMIT;
INSERT INTO content.reports(board,post_id,reason,state)
VALUES('j',9303000,'Historical j open report','open'),
 ('j',9303001,'Historical j resolved report','resolved'),
 ('report',9300001,'Historical dismissed report','dismissed');
INSERT INTO post_secrets.anonymous_reports(report_id,token_hash)
SELECT id,decode(repeat('01',32),'hex') FROM content.reports WHERE board='j';
UPDATE post_secrets.anonymous_sessions SET activity_at=100,action_at=99,
 verified_level=2,posts=17,images=7,threads=5,reports=11,pending=3,change_score=4;
INSERT INTO staff_identity.accounts(role,username) VALUES('moderator','synthetic_report_upgrade');
INSERT INTO staff_identity.credentials(id,account_id,credential)
SELECT decode('01','hex'),id,'{}'::jsonb FROM staff_identity.accounts WHERE username='synthetic_report_upgrade';
INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id)
SELECT decode(repeat('21',32),'hex'),decode(repeat('22',32),'hex'),id,decode('01','hex')
FROM staff_identity.accounts WHERE username='synthetic_report_upgrade';
SQL
for mode in fresh upgrade; do
 database="report_target_policy_$mode"
 create_database "$database"
 migrator=("${psql[@]}" -U board_migrator -d "$database")
 for migration in migrations/*.sql; do
    [[ $migration != migrations/0093* ]] || break
    "${migrator[@]}" --single-transaction -f "$migration"
 done
 if [[ $mode = upgrade ]]; then
    "${migrator[@]}" -f "$cluster/fixtures.sql"
 fi
 "${migrator[@]}" <<'SQL'
CREATE TABLE public.report_rows_before(relation text,value jsonb);
CREATE FUNCTION public.capture_report_rows() RETURNS TABLE(relation text,value jsonb)
LANGUAGE plpgsql AS $$ DECLARE r record; BEGIN
 FOR r IN SELECT n.nspname,c.relname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname IN ('content','post_secrets','staff_identity','media','media_intake','deployment','admission') AND c.relkind='r' LOOP
  RETURN QUERY EXECUTE format('SELECT %L,to_jsonb(r) FROM %I.%I r',r.nspname||'.'||r.relname,r.nspname,r.relname);
 END LOOP;
END $$;
INSERT INTO public.report_rows_before SELECT * FROM public.capture_report_rows();
-- Canonical names, rather than database OIDs, also support dump/restore comparison.
-- acldefault uses lowercase s for sequences (uppercase S means foreign server),
-- unlike pg_class.relkind. Expand NULL ACLs to the correct object-type defaults.
-- Dump/restore can replace
-- explicit owner-only ACLs with NULL and reorder entries without changing rights.
-- Exploded table/column tuples are compared as sets and sorted in fingerprints;
-- nested schema/function tuples are sorted explicitly. Owners remain separate.
CREATE VIEW public.report_authority AS
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
 FROM pg_namespace n WHERE n.nspname NOT LIKE 'pg_%' AND n.nspname NOT IN ('information_schema','public')
UNION ALL SELECT 'role',rolname,
 jsonb_build_array(rolsuper,rolinherit,rolcreaterole,rolcreatedb,rolcanlogin,rolreplication,
  rolbypassrls,rolconnlimit,rolvaliduntil,rolconfig)
 FROM pg_roles WHERE rolname LIKE 'board_%'
UNION ALL SELECT 'membership',pg_get_userbyid(roleid)||'.'||pg_get_userbyid(member),
 jsonb_build_array(pg_get_userbyid(grantor),admin_option,inherit_option,set_option)
 FROM pg_auth_members WHERE pg_get_userbyid(roleid) LIKE 'board_%' OR pg_get_userbyid(member) LIKE 'board_%'
UNION ALL SELECT 'default grant',pg_get_userbyid(d.defaclrole)||'.'||coalesce(n.nspname,'*')||'.'||d.defaclobjtype::text,
 jsonb_build_array(pg_get_userbyid(a.grantor),CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
 FROM pg_default_acl d LEFT JOIN pg_namespace n ON n.oid=d.defaclnamespace,
 LATERAL aclexplode(d.defaclacl) a;
CREATE TABLE public.report_authority_before AS TABLE public.report_authority;
CREATE VIEW public.report_columns AS
SELECT n.nspname||'.'||c.relname relation,a.attname,a.attnum,a.atttypid::regtype::text type,
 a.attnotnull,coalesce(pg_get_expr(d.adbin,d.adrelid),'') default_value
FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace
LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
WHERE n.nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission')
AND a.attnum>0 AND NOT a.attisdropped;
CREATE TABLE public.report_columns_before AS TABLE public.report_columns;
CREATE VIEW public.report_constraints AS
SELECT conrelid::regclass::text relation,conname,contype,convalidated,condeferrable,condeferred,
 connoinherit,pg_get_constraintdef(oid,true) definition FROM pg_constraint
WHERE connamespace IN(SELECT oid FROM pg_namespace WHERE nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission'));
CREATE TABLE public.report_constraints_before AS TABLE public.report_constraints;
SQL
 "${migrator[@]}" --single-transaction -f migrations/0093_report_target_policy.sql
 "${migrator[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(SELECT relation,CASE WHEN relation='content.boards' THEN value-'can_report_posts' ELSE value END
  FROM public.report_rows_before EXCEPT ALL
  SELECT relation,CASE WHEN relation='content.boards' THEN value-'can_report_posts' ELSE value END
  FROM public.capture_report_rows())
 OR EXISTS(SELECT relation,CASE WHEN relation='content.boards' THEN value-'can_report_posts' ELSE value END
  FROM public.capture_report_rows() EXCEPT ALL
  SELECT relation,CASE WHEN relation='content.boards' THEN value-'can_report_posts' ELSE value END
  FROM public.report_rows_before)
 THEN RAISE EXCEPTION 'Migration changed existing board policy, content, reports, secrets or activity'; END IF;
 IF EXISTS(TABLE public.report_authority_before EXCEPT TABLE public.report_authority)
 OR EXISTS(TABLE public.report_authority EXCEPT TABLE public.report_authority_before)
 THEN RAISE EXCEPTION 'Migration changed authority, functions, triggers, grants or RLS'; END IF;
 IF EXISTS(TABLE public.report_columns_before EXCEPT TABLE public.report_columns)
 OR EXISTS(SELECT * FROM public.report_columns WHERE NOT(relation='content.boards' AND attname='can_report_posts')
  EXCEPT TABLE public.report_columns_before)
 THEN RAISE EXCEPTION 'Migration changed existing column contracts or added unrelated columns'; END IF;
 IF EXISTS(TABLE public.report_constraints_before EXCEPT TABLE public.report_constraints)
 OR EXISTS(TABLE public.report_constraints EXCEPT TABLE public.report_constraints_before)
 THEN RAISE EXCEPTION 'Migration changed existing constraints'; END IF;
 IF NOT EXISTS(SELECT 1 FROM content.boards WHERE slug='j' AND NOT can_report_posts)
 OR EXISTS(SELECT 1 FROM content.boards WHERE can_report_posts IS DISTINCT FROM (slug<>'j'))
 THEN RAISE EXCEPTION 'Report target policy must default true with only seeded j false'; END IF;
 IF NOT EXISTS(SELECT 1 FROM public.report_columns WHERE relation='content.boards'
  AND attname='can_report_posts' AND type='boolean' AND attnotnull AND default_value='true')
 THEN RAISE EXCEPTION 'Wrong report policy type/default/nullability'; END IF;
END $$;
DROP TABLE public.report_rows_before,public.report_authority_before,public.report_columns_before,public.report_constraints_before;
SQL
 # Fresh creation also exercises new board defaults and populated current restore.
 if [[ $mode = fresh ]]; then
    "${migrator[@]}" -f "$cluster/fixtures.sql"
 fi
 "${migrator[@]}" <<'SQL'
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM content.boards WHERE slug='a' AND NOT comment_spoiler_cleanup
  AND thread_limit=37 AND posting_reply_seconds=17 AND user_thread_limit=9)
 OR NOT EXISTS(SELECT 1 FROM content.boards WHERE slug='j' AND thread_limit=41
  AND posting_thread_seconds=73 AND user_thread_period_hours=33 AND NOT can_report_posts)
 OR NOT EXISTS(SELECT 1 FROM content.boards WHERE slug='report' AND can_report_posts)
 OR NOT EXISTS(SELECT 1 FROM content.boards WHERE slug='reportpriv' AND can_report_posts)
 THEN RAISE EXCEPTION 'Operator overrides or new board defaults changed'; END IF;
 BEGIN
  UPDATE content.boards SET can_report_posts=NULL WHERE slug='report';
  RAISE EXCEPTION 'NULL report policy accepted' USING ERRCODE='ZX001';
 EXCEPTION WHEN not_null_violation THEN NULL; END;
END $$;
-- Both booleans are valid operator policy, without changing the retained fixture.
BEGIN;
UPDATE content.boards SET can_report_posts=false WHERE slug='report';
UPDATE content.boards SET can_report_posts=true WHERE slug='j';
ROLLBACK;
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
  for role in board_public board_staff; do
   "${psql[@]}" -U "$role" -d "$database" <<'SQL'
BEGIN;
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM content.boards WHERE slug='report' AND can_report_posts)
 THEN RAISE EXCEPTION 'Runtime cannot read report target policy'; END IF;
 BEGIN
  UPDATE content.boards SET can_report_posts=false WHERE slug='report';
  RAISE EXCEPTION 'Runtime can rewrite report target policy' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
ROLLBACK;
SQL
  done
  "${psql[@]}" -U board_migrator -d "$database" -At > "$cluster/$mode-$phase.fingerprint" <<'SQL'
SELECT relation,md5(value::text) FROM public.capture_report_rows() ORDER BY relation,value::text;
SELECT kind,object,md5(value::text) FROM public.report_authority ORDER BY kind,object,value::text;
SELECT * FROM public.report_columns ORDER BY relation,attnum;
SELECT * FROM public.report_constraints ORDER BY relation,conname;
SELECT schemaname,indexname,indexdef FROM pg_indexes WHERE schemaname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission') ORDER BY 1,2;
SQL
 done
 cmp -s "$cluster/$mode-live.fingerprint" "$cluster/$mode-restored.fingerprint" || {
  printf 'Report-target-policy current dump/restore fingerprint mismatch (%s).\n' "$mode" >&3; exit 1;
 }
 printf '%s report-target-policy migration and current dump/restore passed.\n' "$mode" >&3
done
printf 'Existing board overrides, reports, private activity, content and authority preserved; default true, j false.\n' >&3
