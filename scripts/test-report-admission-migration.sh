#!/usr/bin/env bash
# Owned synthetic clusters only. Qualify 0093 -> 0094 and current dump/restore.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-report-admission.XXXXXXXX)
started=0
cleanup() {
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null
    fi
    [[ $cluster =~ ^/tmp/board-report-admission\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
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
trap 'printf "Report-admission qualification failed at line %s (private diagnostics removed during cleanup).\n" "$LINENO" >&3' ERR
# Model an existing cluster explicitly: deploy/roles.sql is CREATE ROLE based,
# so do not rerun it against an existing deployment. Bootstrap only the new
# NOLOGIN owner/membership immediately before the upgrade below.
# Root owns these mode-0600 files. Open them before dropping privileges;
# PostgreSQL receives only stdin, without broadening file permissions.
sed '/board_report_admission_owner/d' deploy/roles.sql > "$cluster/legacy-roles.sql"
runuser -u postgres -- "${psql[@]}" -d postgres -f - < "$cluster/legacy-roles.sql"
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
# Exercise the actual shared readiness query inside rolled-back catalog changes.
# Only this owned-cluster administrator can switch to both runtime roles.
python3 - "$cluster/readiness.sql" <<'PYREADINESS'
import pathlib, re, sys
source = pathlib.Path("crates/store/src/report_admission.rs").read_text()
queries = re.findall(r'pub const READINESS_SQL: &str = r#"(.*?)"#;', source, re.S)
assert len(queries) == 1
query = queries[0]
changes = ['ALTER TABLE content.posts DISABLE TRIGGER retire_deleted_post_report_membership', 'ALTER TABLE content.threads DISABLE TRIGGER retire_deleted_thread_report_membership', 'DROP TRIGGER retire_deleted_post_report_membership ON content.posts', 'DROP TRIGGER retire_deleted_thread_report_membership ON content.threads', 'SET LOCAL ROLE board_report_admission_owner; GRANT EXECUTE ON FUNCTION post_secrets.check_report_limits(text,bigint,bytea,timestamptz) TO board_public; RESET ROLE', 'SET LOCAL ROLE board_report_admission_owner; REVOKE EXECUTE ON FUNCTION post_secrets.retire_staff_file_report_membership(text,bigint) FROM board_attachment_owner; RESET ROLE', 'SET LOCAL ROLE board_report_admission_owner; ALTER FUNCTION post_secrets.report_target(text,bigint) SECURITY INVOKER; RESET ROLE', 'SET LOCAL ROLE board_report_admission_owner; ALTER FUNCTION post_secrets.check_report_limits(text,bigint,bytea,timestamptz) SET search_path=public; RESET ROLE', 'SET LOCAL ROLE board_report_admission_owner; GRANT SELECT(actor_hash) ON post_secrets.report_membership TO board_public; RESET ROLE', 'GRANT INSERT(reason) ON content.reports TO board_public', 'GRANT USAGE ON SEQUENCE content.reports_id_seq TO board_public', 'SET LOCAL ROLE board_report_admission_owner; ALTER TABLE post_secrets.report_admission_gate ALTER COLUMN membership_limit DROP NOT NULL; RESET ROLE', 'SET LOCAL ROLE board_report_admission_owner; ALTER TABLE post_secrets.report_admission_gate ALTER COLUMN membership_limit SET DEFAULT 1000001; RESET ROLE', 'SET LOCAL ROLE board_report_admission_owner; ALTER TABLE post_secrets.report_admission_gate DROP CONSTRAINT report_admission_gate_membership_limit_check; RESET ROLE']
with open(sys.argv[1], "w") as output:
    for change, expected in [("", True), *[(item, False) for item in changes],
        ("SET LOCAL ROLE board_report_admission_owner; UPDATE post_secrets.report_admission_gate SET membership_limit=50000; RESET ROLE", True)]:
        output.write("BEGIN;\n")
        if change:
            output.write(change + ";\n")
        for role in ("board_public", "board_staff"):
            output.write(f"SET LOCAL ROLE {role};\n")
            output.write("DO $readiness$ BEGIN IF (" + query + ") IS DISTINCT FROM " + str(expected).lower()
                + " THEN RAISE EXCEPTION 'Report readiness drift contract failed'; END IF; END $readiness$;\n")
            output.write("RESET ROLE;\n")
        output.write("ROLLBACK;\n")
PYREADINESS
for mode in upgrade fresh; do
 database="report_admission_$mode"
 create_database "$database"
 migrator=("${psql[@]}" -U board_migrator -d "$database")
 admin=(runuser -u postgres -- "${psql[@]}" -d "$database")
 for migration in migrations/*.sql; do
    [[ $migration != migrations/0094* ]] || break
    "${migrator[@]}" --single-transaction -f "$migration"
 done
 if [[ $mode = upgrade ]]; then
    "${migrator[@]}" -f "$cluster/fixtures.sql"
    # Apply this additive role bootstrap as the existing-cluster administrator.
    # INHERIT FALSE is essential: migration access must require SET ROLE.
    runuser -u postgres -- "${psql[@]}" -d postgres -f deploy/report-admission-role.sql
 fi
 "${admin[@]}" <<'SQL'
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
-- Index attributes belong to the index contract, not table/view columns.
AND c.relkind IN('r','p','v','m','f') AND a.attnum>0 AND NOT a.attisdropped;
CREATE TABLE public.report_columns_before AS TABLE public.report_columns;
CREATE VIEW public.report_constraints AS
SELECT conrelid::regclass::text relation,conname,contype,convalidated,condeferrable,condeferred,
 connoinherit,pg_get_constraintdef(oid,true) definition FROM pg_constraint
WHERE connamespace IN(SELECT oid FROM pg_namespace WHERE nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission'));
CREATE TABLE public.report_constraints_before AS TABLE public.report_constraints;
CREATE VIEW public.report_indexes AS
SELECT schemaname,tablename,indexname,indexdef FROM pg_indexes
WHERE schemaname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission');
CREATE TABLE public.report_indexes_before AS TABLE public.report_indexes;
SQL
 "${migrator[@]}" --single-transaction -f migrations/0094_report_admission.sql
 "${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(TABLE public.report_rows_before EXCEPT ALL
  SELECT * FROM public.capture_report_rows() WHERE relation NOT IN
   ('post_secrets.report_membership','post_secrets.report_admission_gate'))
 OR EXISTS(SELECT * FROM public.capture_report_rows() WHERE relation NOT IN
   ('post_secrets.report_membership','post_secrets.report_admission_gate')
  EXCEPT ALL TABLE public.report_rows_before)
 THEN RAISE EXCEPTION 'Migration rewrote existing policy, reports, content, audit, secrets or activity'; END IF;
 IF EXISTS(SELECT 1 FROM post_secrets.report_membership)
 THEN RAISE EXCEPTION 'Migration invented historical report identities'; END IF;
 IF (SELECT count(*) FROM post_secrets.report_admission_gate WHERE singleton)<>1
 THEN RAISE EXCEPTION 'Missing admission serialization gate'; END IF;
 IF EXISTS(TABLE public.report_columns_before EXCEPT TABLE public.report_columns)
 OR EXISTS(SELECT * FROM public.report_columns WHERE relation NOT IN
   ('post_secrets.report_membership','post_secrets.report_admission_gate')
  EXCEPT TABLE public.report_columns_before)
 THEN RAISE EXCEPTION 'Migration changed existing columns'; END IF;
 IF EXISTS(TABLE public.report_constraints_before EXCEPT TABLE public.report_constraints)
 OR EXISTS(SELECT * FROM public.report_constraints WHERE relation NOT IN
   ('post_secrets.report_membership','post_secrets.report_admission_gate')
  EXCEPT TABLE public.report_constraints_before)
 THEN RAISE EXCEPTION 'Migration changed existing constraints'; END IF;
 IF EXISTS(TABLE public.report_indexes_before EXCEPT TABLE public.report_indexes)
 OR EXISTS(SELECT * FROM public.report_indexes WHERE schemaname||'.'||tablename NOT IN
   ('post_secrets.report_membership','post_secrets.report_admission_gate')
  EXCEPT TABLE public.report_indexes_before)
 THEN RAISE EXCEPTION 'Migration changed existing indexes or added unrelated indexes'; END IF;
 -- Existing functions/triggers/RLS are not replaced by the additive admission.
 IF EXISTS(SELECT * FROM public.report_authority_before WHERE kind IN('function','trigger','policy')
  EXCEPT TABLE public.report_authority)
 THEN RAISE EXCEPTION 'Migration changed existing function, trigger or RLS contracts'; END IF;
END $$;
DROP TABLE public.report_rows_before,public.report_authority_before,public.report_columns_before,public.report_constraints_before,public.report_indexes_before;
SQL
 if [[ $mode = fresh ]]; then
    "${migrator[@]}" -f "$cluster/fixtures.sql"
 fi
 # Persist a genuinely new report through the restricted public connection.
 # Historical reports above still have no inferred report-membership hashes.
 "${psql[@]}" -U board_public -d "$database" <<'SQL'
SELECT content.check_report_admission('report',9300000,decode(repeat('31',32),'hex'));
SELECT content.admit_report('report',9300000,'New admission qualification',decode(repeat('31',32),'hex'));
SQL
 # Explicit SET ROLE is required for trusted private-fixture aging. It must not
 # silently work as board_migrator through inherited table ownership.
 "${migrator[@]}" <<'SQL'
BEGIN;
SET LOCAL ROLE board_report_admission_owner;
UPDATE post_secrets.report_membership SET reported_at=to_timestamp(1);
COMMIT;
SQL
 for phase in live restored; do
  if [[ $phase = restored ]]; then
   # The migrator intentionally cannot read the new owner's private tables.
   # An owned-cluster administrative dump is needed for a complete backup.
   runuser -u postgres -- "$pg_bin/pg_dump" -h "$cluster" -d "$database" \
    --format=custom --file="$cluster/current.dump"
   create_database "${database}_restore"
   runuser -u postgres -- "$pg_bin/pg_restore" -h "$cluster" --dbname="${database}_restore" \
    --single-transaction --exit-on-error < "$cluster/current.dump"
   database="${database}_restore"
   admin=(runuser -u postgres -- "${psql[@]}" -d "$database")
  fi
  "${admin[@]}" <<'SQL'
DO $$ DECLARE r text; f text; BEGIN
 IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='board_report_admission_owner'
  AND NOT rolcanlogin AND NOT rolsuper AND NOT rolbypassrls AND NOT rolcreaterole
  AND NOT rolcreatedb AND NOT rolreplication)
 THEN RAISE EXCEPTION 'Unsafe report admission owner attributes'; END IF;
 IF NOT EXISTS(SELECT 1 FROM pg_auth_members WHERE roleid='board_report_admission_owner'::regrole
  AND member='board_migrator'::regrole AND NOT inherit_option AND set_option AND NOT admin_option)
 OR EXISTS(SELECT 1 FROM pg_auth_members WHERE roleid='board_report_admission_owner'::regrole
  AND member<>'board_migrator'::regrole)
 THEN RAISE EXCEPTION 'Unsafe admission owner membership'; END IF;
 IF EXISTS(SELECT 1 FROM pg_class WHERE oid IN('post_secrets.report_membership'::regclass,
  'post_secrets.report_admission_gate'::regclass) AND relowner<>'board_report_admission_owner'::regrole)
 THEN RAISE EXCEPTION 'Private admission relations have wrong owner'; END IF;
 FOREACH f IN ARRAY ARRAY['post_secrets.report_target(text,bigint)',
  'post_secrets.check_report_limits(text,bigint,bytea,timestamp with time zone)',
  'post_secrets.retire_deleted_report_membership()',
  'post_secrets.retire_staff_file_report_membership(text,bigint)',
  'content.check_report_admission(text,bigint,bytea)','content.admit_report(text,bigint,text,bytea)'] LOOP
  IF NOT EXISTS(SELECT 1 FROM pg_proc WHERE oid=f::regprocedure
   AND proowner='board_report_admission_owner'::regrole AND prosecdef
   AND proconfig @> ARRAY['search_path=pg_catalog, pg_temp'])
  THEN RAISE EXCEPTION 'Unsafe admission function owner or search path'; END IF;
 END LOOP;
 FOREACH r IN ARRAY ARRAY['board_public','board_staff','board_auth','board_media',
  'board_media_read','board_media_intake','board_monitor','board_migrator'] LOOP
  IF has_table_privilege(r,'post_secrets.report_membership','SELECT,INSERT,UPDATE,DELETE')
   OR has_any_column_privilege(r,'post_secrets.report_membership','SELECT,INSERT,UPDATE')
   OR has_table_privilege(r,'post_secrets.report_admission_gate','SELECT,INSERT,UPDATE,DELETE')
  THEN RAISE EXCEPTION 'Runtime or migrator inherited private admission authority'; END IF;
 END LOOP;
 IF has_any_column_privilege('board_public','content.reports','INSERT')
  OR has_sequence_privilege('board_public','content.reports_id_seq','USAGE,UPDATE')
  OR NOT has_function_privilege('board_public','content.admit_report(text,bigint,text,bytea)','EXECUTE')
  OR NOT has_function_privilege('board_staff','content.admit_report(text,bigint,text,bytea)','EXECUTE')
  OR has_function_privilege('board_auth','content.admit_report(text,bigint,text,bytea)','EXECUTE')
 THEN RAISE EXCEPTION 'Wrong public admission ACL'; END IF;
 IF (SELECT count(*) FROM post_secrets.report_membership)<>1
  OR EXISTS(SELECT 1 FROM post_secrets.report_membership m JOIN content.reports r ON r.id=m.report_id
    WHERE r.reason<>'New admission qualification' OR m.reported_at<>to_timestamp(1))
 THEN RAISE EXCEPTION 'Historical identities fabricated or durable membership changed'; END IF;
END $$;
SQL
  for role in board_public board_staff board_migrator; do
   "${psql[@]}" -U "$role" -d "$database" <<'SQL'
BEGIN;
DO $$ BEGIN
 BEGIN
  PERFORM * FROM post_secrets.report_membership;
  RAISE EXCEPTION 'Direct private membership read allowed' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN
  INSERT INTO post_secrets.report_membership(report_id,actor_hash,board,post_id,thread_id,reported_at)
  VALUES(-1,decode(repeat('33',32),'hex'),'report',9300000,9300000,clock_timestamp());
  RAISE EXCEPTION 'Direct private membership write allowed' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
ROLLBACK;
SQL
  done
  "${psql[@]}" -U board_public -d "$database" <<'SQL'
BEGIN;
DO $$ BEGIN
 BEGIN
  INSERT INTO content.reports(board,post_id,reason) VALUES('report',9300001,'Bypass');
  RAISE EXCEPTION 'Direct public report insertion allowed' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN
  PERFORM content.admit_report('report',9300000,'Duplicate',decode(repeat('31',32),'hex'));
  RAISE EXCEPTION 'Old duplicate membership expired' USING ERRCODE='ZX001';
 EXCEPTION WHEN SQLSTATE 'P0001' THEN
  IF SQLERRM<>'You have already reported this post.' THEN RAISE; END IF;
 END;
 BEGIN
  PERFORM content.admit_report('reportpriv',9300100,'Private',decode(repeat('32',32),'hex'));
  RAISE EXCEPTION 'Public report on private board allowed' USING ERRCODE='ZX001';
 EXCEPTION WHEN no_data_found THEN NULL; END;
 PERFORM content.admit_report('report',9300001,'Actual public admission',decode(repeat('32',32),'hex'));
 BEGIN
  PERFORM content.admit_report('report',9301001,'Too soon',decode(repeat('32',32),'hex'));
  RAISE EXCEPTION 'Public cooldown bypassed' USING ERRCODE='ZX001';
 EXCEPTION WHEN SQLSTATE 'P0001' THEN
  IF SQLERRM<>'You have to wait a while before reporting another post.' THEN RAISE; END IF;
 END;
END $$;
ROLLBACK;
SQL
  # Exercise transitions within one rollback-only administrative fixture. Real
  # invokers use SET ROLE so security-definer invoker checks remain meaningful.
  "${admin[@]}" <<'SQL'
BEGIN;
SET LOCAL ROLE board_migrator;
UPDATE content.reports SET state='resolved' WHERE reason='New admission qualification';
DELETE FROM post_secrets.anonymous_sessions;
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN
 IF (SELECT count(*) FROM post_secrets.report_membership)<>1
 THEN RAISE EXCEPTION 'Session GC or report resolution retired admission history'; END IF;
END $$;
SET LOCAL ROLE board_migrator;
UPDATE content.reports SET state='dismissed' WHERE reason='New admission qualification';
UPDATE content.boards SET archive_retention_seconds=3600 WHERE slug='report';
UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour'
 WHERE id=9300000;
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN
 IF (SELECT count(*) FROM post_secrets.report_membership)<>1
 THEN RAISE EXCEPTION 'Archive or dismissal retired admission history'; END IF;
END $$;
SET LOCAL ROLE board_public;
DO $$ BEGIN
 BEGIN
  PERFORM content.admit_report('report',9300000,'Archived duplicate',decode(repeat('31',32),'hex'));
  RAISE EXCEPTION 'Retained archived duplicate admitted' USING ERRCODE='ZX001';
 EXCEPTION WHEN SQLSTATE 'P0001' THEN
  IF SQLERRM<>'You have already reported this post.' THEN RAISE; END IF;
 END;
END $$;
SET LOCAL ROLE board_migrator;
UPDATE content.posts SET deleted=true WHERE id=9300000;
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM post_secrets.report_membership)
 THEN RAISE EXCEPTION 'Post deletion failed to retire membership'; END IF;
END $$;
SET LOCAL ROLE board_migrator;
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM content.reports WHERE reason='New admission qualification')
 THEN RAISE EXCEPTION 'Membership retirement removed report history'; END IF;
END $$;
ROLLBACK;
BEGIN;
SET LOCAL ROLE board_migrator;
UPDATE content.threads SET deleted=true WHERE id=9300000;
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM post_secrets.report_membership)
 THEN RAISE EXCEPTION 'Thread deletion failed to retire membership'; END IF;
END $$;
ROLLBACK;
SQL
  "${admin[@]}" <<'SQL'
BEGIN;
SET LOCAL ROLE board_staff;
SELECT content.admit_report('reportpriv',9300100,'Staff private admission',decode(repeat('34',32),'hex'));
ROLLBACK;
BEGIN;
SET LOCAL ROLE board_public;
SELECT content.delete_post_attachment('report',9300000);
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN
 IF (SELECT count(*) FROM post_secrets.report_membership)<>1
 THEN RAISE EXCEPTION 'Public file-only removal retired membership'; END IF;
END $$;
ROLLBACK;
BEGIN;
SET LOCAL ROLE board_staff;
SELECT content.staff_delete_post_attachment('report',9300000);
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM post_secrets.report_membership)
 THEN RAISE EXCEPTION 'Staff file-only removal failed to retire membership'; END IF;
END $$;
ROLLBACK;
BEGIN;
SET LOCAL ROLE board_report_admission_owner;
UPDATE post_secrets.report_admission_gate SET membership_limit=1;
SET LOCAL ROLE board_public;
DO $$ BEGIN
 BEGIN
  PERFORM content.check_report_admission('report',9300001,decode(repeat('35',32),'hex'));
  RAISE EXCEPTION 'Advisory capacity exhaustion admitted' USING ERRCODE='ZX001';
 EXCEPTION WHEN SQLSTATE 'P0094' THEN NULL; END;
 BEGIN
  PERFORM content.admit_report('report',9300001,'Capacity exhausted',decode(repeat('35',32),'hex'));
  RAISE EXCEPTION 'Atomic capacity exhaustion admitted' USING ERRCODE='ZX001';
 EXCEPTION WHEN SQLSTATE 'P0094' THEN NULL; END;
END $$;
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN
 IF (SELECT count(*) FROM post_secrets.report_membership)<>1
  OR NOT EXISTS(SELECT 1 FROM post_secrets.report_membership WHERE reported_at=to_timestamp(1))
 THEN RAISE EXCEPTION 'Capacity check evicted durable report membership'; END IF;
END $$;
ROLLBACK;
SQL
  "${admin[@]}" -f - < "$cluster/readiness.sql"
  "${admin[@]}" -At > "$cluster/$mode-$phase.fingerprint" <<'SQL'
SELECT relation,md5(value::text) FROM public.capture_report_rows() ORDER BY relation,value::text;
SELECT kind,object,md5(value::text) FROM public.report_authority ORDER BY kind,object,value::text;
SELECT * FROM public.report_columns ORDER BY relation,attnum;
SELECT * FROM public.report_constraints ORDER BY relation,conname;
SELECT * FROM public.report_indexes ORDER BY schemaname,tablename,indexname;
SQL
 done
 cmp -s "$cluster/$mode-live.fingerprint" "$cluster/$mode-restored.fingerprint" || {
  printf 'Report-admission current dump/restore fingerprint mismatch (%s).\n' "$mode" >&3; exit 1;
 }
 printf '%s report-admission migration and current dump/restore passed.\n' "$mode" >&3
done
printf 'Historical rows preserved; restricted admission, durable membership and lifecycle qualified.\n' >&3
