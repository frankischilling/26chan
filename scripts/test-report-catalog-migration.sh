#!/usr/bin/env bash
# Owned synthetic clusters only. Qualify 0097 -> 0098 and current dump/restore.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-report-catalog.XXXXXXXX)
started=0
cleanup() {
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null
    fi
    [[ $cluster =~ ^/tmp/board-report-catalog\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
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
trap 'printf "Report-catalog qualification failed at line %s (private diagnostics removed during cleanup).\n" "$LINENO" >&3' ERR
# All fixture SQL is root-owned mode 0600. The parent shell opens stdin before
# runuser drops privileges; never pass these private paths to postgres psql.
runuser -u postgres -- "${psql[@]}" -d postgres -f - < deploy/roles.sql
runuser -u postgres -- "${psql[@]}" -d postgres -c 'ALTER ROLE board_staff LOGIN; ALTER ROLE board_auth LOGIN'
create_database() {
 runuser -u postgres -- "${psql[@]}" -d postgres -v database="$1" <<'SQL'
CREATE DATABASE :"database" OWNER board_migrator;
REVOKE ALL ON DATABASE :"database" FROM PUBLIC;
GRANT CONNECT ON DATABASE :"database" TO board_migrator,board_public,board_staff,board_auth;
SQL
}
cat > "$cluster/fixtures.sql" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('sessreport','Session report fixture','Synthetic',1000,100,100,100,10);
BEGIN;
SELECT set_config('board.posting_actor',repeat('11',32),true);
INSERT INTO content.threads(id,board) VALUES(9700001,'sessreport');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
SELECT 9700000+i,'sessreport',9700001,'Synthetic','','Retained body' FROM generate_series(1,100) i;
INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES(9700001,'owned-deletion-hash');
COMMIT;
SQL
# Use the frozen 0097/0098 report contract for this historical target.
python3 - "$cluster/readiness.sql" <<'PYREADINESS'
import pathlib, re, sys
with open(sys.argv[1], 'w') as out:
 for profile in ('report_admission', 'automatic_admission'):
  source = pathlib.Path(f'crates/store/src/{profile}.rs').read_text()
  match = re.search(r'pub const READINESS_SQL: &str = r#"(.*?)"#;', source, re.S)
  if not match:
   raise SystemExit(f'Cannot extract actual {profile} READINESS_SQL')
  query = match.group(1)
  if profile == 'report_admission':
   query = pathlib.Path('scripts/fixtures/session-report-0097-readiness.sql').read_text()
  for role in ('board_public', 'board_staff'):
   out.write(f"BEGIN; SET LOCAL ROLE {role}; DO $readiness$ BEGIN IF ({query}) IS DISTINCT FROM true THEN RAISE EXCEPTION '{profile} readiness failed'; END IF; END $readiness$; ROLLBACK;\n")
PYREADINESS
seed_reports() {
 "${migrator[@]}" -f - < "$cluster/fixtures.sql"
 "${psql[@]}" -U board_public -d "$database" <<'SQL'
SELECT content.admit_report('sessreport',9700001,'Retained session report',decode(repeat('21',32),'hex'),
 decode(repeat('01',32),'hex'),decode(repeat('02',32),'hex'),decode(repeat('03',32),'hex'),
 decode(repeat('04',32),'hex'),true,extract(epoch FROM clock_timestamp())::bigint);
SQL
 "${psql[@]}" -U board_staff -d "$database" <<'SQL'
SELECT content.admit_report('sessreport',9700002,'Retained staff report',decode(repeat('22',32),'hex'));
SQL
}
for mode in upgrade fresh; do
 database="report_catalog_$mode"
 create_database "$database"
 migrator=("${psql[@]}" -U board_migrator -d "$database")
 admin=(runuser -u postgres -- "${psql[@]}" -d "$database")
 # Pin the predecessor; a later migration must not silently change this exercise.
 for migration in migrations/*.sql; do
  [[ $migration < migrations/0098_report_category_catalog.sql ]] || break
  "${migrator[@]}" --single-transaction -f - < "$migration"
 done
 if [[ $mode = upgrade ]]; then seed_reports; fi
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
 "${migrator[@]}" --single-transaction -f - < migrations/0098_report_category_catalog.sql
 "${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(TABLE public.report_rows_before EXCEPT ALL SELECT * FROM public.capture_report_rows())
 OR EXISTS(SELECT * FROM public.capture_report_rows() WHERE relation NOT LIKE 'post_secrets.report_catalog_%'
 EXCEPT ALL TABLE public.report_rows_before)
 THEN RAISE EXCEPTION '0098 rewrote existing report/session/history/UUID rows'; END IF;
 IF EXISTS(TABLE public.report_columns_before EXCEPT TABLE public.report_columns)
 OR EXISTS(SELECT * FROM public.report_columns WHERE relation NOT LIKE 'post_secrets.report_catalog_%' EXCEPT TABLE public.report_columns_before)
 OR EXISTS(TABLE public.report_constraints_before EXCEPT TABLE public.report_constraints)
 OR EXISTS(SELECT * FROM public.report_constraints WHERE relation NOT LIKE 'post_secrets.report_catalog_%' EXCEPT TABLE public.report_constraints_before)
 OR EXISTS(TABLE public.report_indexes_before EXCEPT TABLE public.report_indexes)
 OR EXISTS(SELECT * FROM public.report_indexes WHERE tablename NOT LIKE 'report_catalog_%' EXCEPT TABLE public.report_indexes_before)
 THEN RAISE EXCEPTION '0098 changed existing column, constraint or index contracts'; END IF;
 IF EXISTS(TABLE public.report_authority_before EXCEPT TABLE public.report_authority)
 OR EXISTS(SELECT * FROM public.report_authority
 WHERE object NOT LIKE 'post_secrets.report_catalog_%'
 AND object NOT IN ('content.import_report_catalog(jsonb)','content.read_report_catalog(bigint)')
 EXCEPT TABLE public.report_authority_before)
 THEN RAISE EXCEPTION '0098 changed existing authority or runtime function'; END IF;
 IF EXISTS(SELECT 1 FROM post_secrets.report_catalog_versions)
 OR EXISTS(SELECT 1 FROM post_secrets.report_catalog_rows)
 THEN RAISE EXCEPTION '0098 installed catalog data'; END IF;
END $$;
DROP TABLE public.report_rows_before,public.report_authority_before,public.report_columns_before,
 public.report_constraints_before,public.report_indexes_before;
SQL
 if [[ $mode = fresh ]]; then seed_reports; fi
 "${admin[@]}" <<'SQL'
CREATE TABLE public.retained_report_rows AS
SELECT * FROM public.capture_report_rows() WHERE relation NOT LIKE 'post_secrets.report_catalog_%';
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM content.reports r JOIN post_secrets.report_membership m ON m.report_id=r.id
 JOIN post_secrets.anonymous_reports a ON a.report_id=r.id
 JOIN post_secrets.anonymous_sessions s ON s.token_hash=a.token_hash
 WHERE r.reason='Retained session report' AND m.automatic_identity=s.automatic_identity
 AND m.automatic_identity IS NOT NULL)
 OR NOT EXISTS(SELECT 1 FROM content.reports r JOIN post_secrets.report_membership m ON m.report_id=r.id
 WHERE r.reason='Retained staff report' AND m.automatic_identity IS NULL)
 THEN RAISE EXCEPTION 'Representative UUID and staff memberships are missing'; END IF;
END $$;
SQL
 # Deliberately non-ID order, ties, contradictory flags, NULL/empty distinction,
 # opaque exclusion text, signed filtered values and non-ASCII strings survive.
 "${migrator[@]}" <<'SQL'
DO $$ DECLARE fixture jsonb := '{"version":1,"categories":[
 {"id":31,"board":null,"op_only":true,"reply_only":true,"image_only":false,"exclude_boards":null,"title":"Synthetic β","weight":1.5,"filtered":-7},
 {"id":2,"board":"","op_only":false,"reply_only":true,"image_only":true,"exclude_boards":"","title":"Synthetic β","weight":1.5,"filtered":0},
 {"id":17,"board":"never-live","op_only":false,"reply_only":false,"image_only":false,"exclude_boards":" a,b / unknown ","title":"Third","weight":-2.25,"filtered":9223372036854775807}]}';
 revision bigint;
BEGIN
 revision:=content.import_report_catalog(fixture);
 IF revision<>1 OR content.read_report_catalog(revision) IS DISTINCT FROM fixture
 THEN RAISE EXCEPTION 'Exact nine-field order/NULL readback failed'; END IF;
 IF content.import_report_catalog(fixture)<>2 OR content.read_report_catalog(2) IS DISTINCT FROM fixture
 THEN RAISE EXCEPTION 'Repeated imports must retain independent immutable revisions'; END IF;
END $$;
SQL
 for phase in live restored; do
  if [[ $phase = restored ]]; then
   # Full administrator dump includes private data/ACLs; root opens descriptors.
   runuser -u postgres -- "$pg_bin/pg_dump" -h "$cluster" -U postgres -d "$database" --format=custom > "$cluster/current.dump"
   create_database "${database}_restore"
   runuser -u postgres -- "$pg_bin/pg_restore" -h "$cluster" --dbname="${database}_restore" --single-transaction --exit-on-error < "$cluster/current.dump"
   database="${database}_restore"
   migrator=("${psql[@]}" -U board_migrator -d "$database")
   admin=(runuser -u postgres -- "${psql[@]}" -d "$database")
  fi
  "${migrator[@]}" <<'SQL'
BEGIN;
DO $$ DECLARE fixture jsonb:=content.read_report_catalog(1); bad jsonb; i integer; BEGIN
 IF content.read_report_catalog(2) IS DISTINCT FROM fixture
 OR (fixture#>>'{categories,0,id}')::bigint<>31
 OR fixture#>'{categories,0,board}'<>'null'::jsonb
 OR fixture#>'{categories,1,board}'<>'""'::jsonb
 THEN RAISE EXCEPTION 'Retained catalog ordering/NULLs changed'; END IF;
 -- Rejected imports must roll back even when a valid row precedes a bad row.
 FOREACH bad IN ARRAY ARRAY[
  'null'::jsonb, '{}'::jsonb,
  jsonb_set(fixture,'{version}','2'),
  jsonb_set(fixture,'{categories,2,id}','31'),
  jsonb_set(fixture,'{categories,2,board}',to_jsonb(repeat('x',257))),
  jsonb_set(fixture,'{categories,2,title}',to_jsonb(repeat('β',2049))),
  jsonb_set(fixture,'{categories,2,exclude_boards}',to_jsonb(repeat('x',65537))),
  jsonb_set(fixture,'{categories,2,id}','1.5'),
  jsonb_set(fixture,'{categories,2,op_only}','null'),
  jsonb_set(fixture,'{categories,2,weight}','1e1000'),
  jsonb_set(fixture,'{categories,2}',(fixture#>'{categories,2}')-'filtered'),
  jsonb_set(fixture,'{categories,2,extra}','true'),
  jsonb_build_object('version',1,'categories',(SELECT jsonb_agg(fixture#>'{categories,0}') FROM generate_series(1,4097))),
  jsonb_build_object('version',1,'categories',jsonb_build_array(jsonb_build_object('title',repeat('x',8388608))))
 ] LOOP
  BEGIN
   PERFORM content.import_report_catalog(bad);
   RAISE EXCEPTION 'Invalid catalog accepted' USING ERRCODE='ZX001';
  EXCEPTION WHEN invalid_parameter_value THEN NULL; END;
 END LOOP;
 IF content.import_report_catalog('{"version":1,"categories":[]}')<>3
 OR content.read_report_catalog(3)<>'{"version":1,"categories":[]}'::jsonb
 THEN RAISE EXCEPTION 'Failed import consumed revision or empty readback failed'; END IF;
 FOR i IN 4..64 LOOP
  IF content.import_report_catalog('{"version":1,"categories":[]}')<>i
  THEN RAISE EXCEPTION 'Revision progression failed'; END IF;
 END LOOP;
 BEGIN
  PERFORM content.import_report_catalog('{"version":1,"categories":[]}');
  RAISE EXCEPTION 'Revision cap bypass' USING ERRCODE='ZX001';
 EXCEPTION WHEN SQLSTATE 'P0098' THEN NULL; END;
 IF content.read_report_catalog(1) IS DISTINCT FROM fixture
 THEN RAISE EXCEPTION 'Previous immutable revision changed'; END IF;
END $$;
ROLLBACK;
SQL
  # Exact positive row and UTF-8 byte boundaries; keep this synthetic revision
  # rolled back so live/restored fingerprints describe the same durable state.
  "${migrator[@]}" <<'SQL'
BEGIN;
DO $$ DECLARE boundary jsonb; revision bigint; BEGIN
 SELECT jsonb_build_object('version',1,'categories',jsonb_agg(jsonb_build_object(
  'id',i,'board',CASE WHEN i=1 THEN repeat('β',128) ELSE NULL END,
  'op_only',false,'reply_only',false,'image_only',false,
  'exclude_boards',CASE WHEN i=1 THEN repeat('x',65536) ELSE NULL END,
  'title',CASE WHEN i=1 THEN repeat('β',2048) ELSE '' END,
  'weight',0,'filtered',-9223372036854775808) ORDER BY i DESC)) INTO boundary
 FROM generate_series(1,4096) i;
 revision:=content.import_report_catalog(boundary);
 IF revision<>3 OR content.read_report_catalog(revision) IS DISTINCT FROM boundary
 THEN RAISE EXCEPTION 'Exact catalog capacity boundaries failed'; END IF;
 BEGIN
  PERFORM content.read_report_catalog(64);
  RAISE EXCEPTION 'Missing catalog revision was fabricated' USING ERRCODE='ZX001';
 EXCEPTION WHEN SQLSTATE 'P0002' THEN NULL; END;
END $$;
ROLLBACK;
SQL
  # Actual restricted sessions, not administrator SET ROLE, prove denial.
  for role in board_public board_staff board_auth; do
   "${psql[@]}" -U "$role" -d "$database" <<'SQL'
DO $$ DECLARE object text; command text; BEGIN
 FOREACH command IN ARRAY ARRAY[
  'SELECT content.import_report_catalog(''{"version":1,"categories":[]}''::jsonb)',
  'SELECT content.read_report_catalog(1)'
 ] LOOP
  BEGIN EXECUTE command; RAISE EXCEPTION 'Catalog API exposed' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 END LOOP;
 FOREACH object IN ARRAY ARRAY['report_catalog_gate','report_catalog_versions','report_catalog_rows'] LOOP
  FOREACH command IN ARRAY ARRAY['SELECT * FROM ','INSERT INTO ','DELETE FROM ','UPDATE ','TRUNCATE '] LOOP
   BEGIN
    EXECUTE command||'post_secrets.'||object||CASE WHEN command='INSERT INTO ' THEN ' DEFAULT VALUES' WHEN command='UPDATE ' THEN CASE WHEN object='report_catalog_gate' THEN ' SET singleton=singleton' ELSE ' SET revision=revision' END ELSE '' END;
    RAISE EXCEPTION 'Private catalog table exposed' USING ERRCODE='ZX001';
   EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  END LOOP;
 END LOOP;
END $$;
SQL
  done
  "${admin[@]}" -f - < "$cluster/readiness.sql"
  "${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(TABLE public.retained_report_rows EXCEPT ALL SELECT * FROM public.capture_report_rows())
 OR EXISTS(SELECT * FROM public.capture_report_rows() WHERE relation NOT LIKE 'post_secrets.report_catalog_%'
 EXCEPT ALL TABLE public.retained_report_rows)
 THEN RAISE EXCEPTION 'Catalog operations changed existing runtime data'; END IF;
 IF EXISTS(SELECT 1 FROM pg_roles r CROSS JOIN (VALUES
  ('post_secrets.report_catalog_gate'),('post_secrets.report_catalog_versions'),('post_secrets.report_catalog_rows')) AS t(name)
  WHERE r.rolname IN ('board_migrator','board_public','board_staff','board_auth')
  AND (has_table_privilege(r.rolname,t.name,'SELECT') OR has_table_privilege(r.rolname,t.name,'INSERT')
   OR has_table_privilege(r.rolname,t.name,'UPDATE') OR has_table_privilege(r.rolname,t.name,'DELETE')
   OR has_table_privilege(r.rolname,t.name,'TRUNCATE')))
 THEN RAISE EXCEPTION 'Unnecessary direct catalog table authority'; END IF;
 IF (SELECT count(*) FROM post_secrets.report_catalog_versions)<>2
 OR (SELECT count(*) FROM post_secrets.report_catalog_rows)<>6
 THEN RAISE EXCEPTION 'Rejected or rolled-back imports left private state'; END IF;
END $$;
SQL
  "${admin[@]}" -At > "$cluster/$mode-$phase.fingerprint" <<'SQL'
SELECT relation,md5(value::text) FROM public.capture_report_rows() ORDER BY relation,value::text;
SELECT kind,object,md5(value::text) FROM public.report_authority ORDER BY kind,object,value::text;
SELECT * FROM public.report_columns ORDER BY relation,attnum;
SELECT * FROM public.report_constraints ORDER BY relation,conname;
SELECT * FROM public.report_indexes ORDER BY schemaname,tablename,indexname;
SQL
 done
 cmp -s "$cluster/$mode-live.fingerprint" "$cluster/$mode-restored.fingerprint" || {
  printf 'Report-catalog dump/restore fingerprint mismatch (%s).\n' "$mode" >&3; exit 1;
 }
 printf '%s report-catalog migration and current dump/restore passed.\n' "$mode" >&3
done
printf '0097 runtime rows, UUIDs and authority preserved; 0098 private imports, bounds, rollback, role denial and readiness qualified.\n' >&3
