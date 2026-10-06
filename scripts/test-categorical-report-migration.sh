#!/usr/bin/env bash
# Owned synthetic clusters only. Qualify 0098 -> 0099 and current dump/restore.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-categorical-report.XXXXXXXX)
started=0
cleanup() {
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null
    fi
    [[ $cluster =~ ^/tmp/board-categorical-report\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
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
trap 'printf "Categorical-report qualification failed at line %s (private diagnostics removed during cleanup).\n" "$LINENO" >&3' ERR
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
# Pull the current application contract, never a separately maintained copy.
python3 - "$cluster" <<'PYREADINESS'
import pathlib, re, sys
root = pathlib.Path(sys.argv[1])
source = pathlib.Path('crates/store/src/report_admission.rs').read_text()
match = re.search(r'pub const READINESS_SQL: &str = r#"(.*?)"#;', source, re.S)
if not match:
 raise SystemExit('Cannot extract actual report_admission READINESS_SQL')
query = match.group(1)
with (root/'readiness.sql').open('w') as out:
 for role in ('board_public','board_staff'):
  out.write(f"BEGIN; SET LOCAL ROLE {role}; DO $check$ BEGIN IF ({query}) IS DISTINCT FROM true THEN RAISE EXCEPTION 'Current report readiness failed'; END IF; END $check$; ROLLBACK;\n")
# Every mutation is rolled back separately, and both serving roles must refuse.
changes = [
 'ALTER TABLE content.reports DROP CONSTRAINT '+name
 for name in ('reports_category_complete','reports_category_kind_check','reports_category_weight_check','reports_reason_check','reports_category_catalog_fk')
] + [
 'DROP FUNCTION content.report_category_form(text,bigint)',
 'DROP FUNCTION content.set_report_catalog_active(bigint)',
 'DROP FUNCTION content.admit_categorical_report(text,bigint,bigint,bigint,bytea,bytea,bytea,bytea,bytea,boolean,bigint)',
 'DROP FUNCTION post_secrets.eligible_report_categories(text,bigint,bigint)',
 'REVOKE EXECUTE ON FUNCTION content.report_category_form(text,bigint) FROM board_public',
 'REVOKE EXECUTE ON FUNCTION content.report_category_form(text,bigint) FROM board_staff',
 'REVOKE EXECUTE ON FUNCTION content.admit_categorical_report(text,bigint,bigint,bigint,bytea,bytea,bytea,bytea,bytea,boolean,bigint) FROM board_public',
 'GRANT EXECUTE ON FUNCTION content.set_report_catalog_active(bigint) TO board_public',
 'GRANT EXECUTE ON FUNCTION post_secrets.eligible_report_categories(text,bigint,bigint) TO board_staff',
 'GRANT SELECT ON post_secrets.report_catalog_rows TO board_public',
 'GRANT INSERT(category_id) ON content.reports TO board_staff',
 'REVOKE INSERT(category_id) ON content.reports FROM board_report_admission_owner',
 'ALTER TABLE content.reports ALTER COLUMN category_id SET DEFAULT 1',
 'ALTER TABLE post_secrets.report_admission_gate DROP COLUMN active_catalog_revision',
]
with (root/'drift.sql').open('w') as out:
 for index, change in enumerate(changes):
  out.write(f'BEGIN; {change};\n')
  for role in ('board_public','board_staff'):
   out.write(f"SET LOCAL ROLE {role}; DO $check$ BEGIN IF ({query}) IS DISTINCT FROM false THEN RAISE EXCEPTION 'Readiness accepted drift {index}'; END IF; END $check$; RESET ROLE;\n")
  out.write('ROLLBACK;\n')
PYREADINESS
cat > "$cluster/fixtures.sql" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('catreport','Categorical report fixture','Synthetic',1000,100,100,100,10);
BEGIN;
SELECT set_config('board.posting_actor',repeat('11',32),true);
INSERT INTO content.threads(id,board) VALUES(9900001,'catreport');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
SELECT 9900000+i,'catreport',9900001,'Synthetic','','Retained body' FROM generate_series(1,20) i;
INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES(9900001,'owned-deletion-hash');
COMMIT;
SQL
seed_reports() {
 "${migrator[@]}" -f - < "$cluster/fixtures.sql"
 "${psql[@]}" -U board_public -d "$database" <<'SQL'
SELECT content.admit_report('catreport',9900001,'Retained session report',decode(repeat('21',32),'hex'),
 decode(repeat('01',32),'hex'),decode(repeat('02',32),'hex'),decode(repeat('03',32),'hex'),
 decode(repeat('04',32),'hex'),true,extract(epoch FROM clock_timestamp())::bigint);
SQL
 "${psql[@]}" -U board_staff -d "$database" <<'SQL'
SELECT content.admit_report('catreport',9900002,'Retained staff report',decode(repeat('22',32),'hex'));
SQL
}
for mode in upgrade fresh; do
 database="categorical_report_$mode"
 create_database "$database"
 migrator=("${psql[@]}" -U board_migrator -d "$database")
 admin=(runuser -u postgres -- "${psql[@]}" -d "$database")
 for migration in migrations/*.sql; do
  [[ $migration < migrations/0099_categorical_report_admission.sql ]] || break
  "${migrator[@]}" --single-transaction -f - < "$migration"
 done
 if [[ $mode = upgrade ]]; then seed_reports; fi
 "${admin[@]}" <<'SQL'
CREATE FUNCTION public.capture_report_rows() RETURNS TABLE(relation text,value jsonb)
LANGUAGE plpgsql AS $$ DECLARE r record; BEGIN
 FOR r IN SELECT n.nspname,c.relname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname IN ('content','post_secrets','staff_identity','media','media_intake','deployment','admission') AND c.relkind='r' LOOP
  RETURN QUERY EXECUTE format('SELECT %L,to_jsonb(r) FROM %I.%I r',r.nspname||'.'||r.relname,r.nspname,r.relname);
 END LOOP;
END $$;
CREATE VIEW public.historical_rows AS
SELECT relation,CASE WHEN relation='content.reports' THEN value-ARRAY['category_revision','category_id','category_kind','category_base_weight']
 WHEN relation='post_secrets.report_admission_gate' THEN value-'active_catalog_revision' ELSE value END value
FROM public.capture_report_rows();
CREATE TABLE public.rows_before AS TABLE public.historical_rows;
SQL
 "${migrator[@]}" --single-transaction -f - < migrations/0099_categorical_report_admission.sql
 "${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(TABLE public.rows_before EXCEPT ALL TABLE public.historical_rows)
 OR EXISTS(TABLE public.historical_rows EXCEPT ALL TABLE public.rows_before)
 THEN RAISE EXCEPTION '0099 rewrote retained report/session/history/UUID rows'; END IF;
 IF EXISTS(SELECT 1 FROM content.reports WHERE category_revision IS NOT NULL OR category_id IS NOT NULL
 OR category_kind IS NOT NULL OR category_base_weight IS NOT NULL)
 OR EXISTS(SELECT 1 FROM post_secrets.report_admission_gate WHERE active_catalog_revision IS NOT NULL)
 OR EXISTS(SELECT 1 FROM post_secrets.report_catalog_versions)
 THEN RAISE EXCEPTION '0099 fabricated metadata, activated a catalog or installed labels'; END IF;
END $$;
DROP TABLE public.rows_before;
SQL
 if [[ $mode = fresh ]]; then seed_reports; fi
 # Retain exact old records separately, including opaque session UUIDs.
 "${admin[@]}" <<'SQL'
CREATE TABLE public.retained_rows AS SELECT * FROM public.capture_report_rows()
WHERE relation IN ('content.reports','post_secrets.report_membership','post_secrets.anonymous_reports','post_secrets.anonymous_sessions');
SQL
 "${migrator[@]}" <<'SQL'
SELECT content.import_report_catalog('{"version":1,"categories":[
 {"id":17,"board":"","op_only":false,"reply_only":false,"image_only":false,"exclude_boards":null,"title":"Synthetic β category","weight":-2.25,"filtered":-7},
 {"id":31,"board":null,"op_only":true,"reply_only":true,"image_only":true,"exclude_boards":"catreport","title":"Synthetic special","weight":1.5,"filtered":9}]}');
DO $$ BEGIN
 IF content.report_category_form('catreport',9900003)<>'{"revision":null,"categories":[]}'::jsonb
 THEN RAISE EXCEPTION 'Import activated reporting'; END IF;
END $$;
SELECT content.set_report_catalog_active(1);
SQL
 "${psql[@]}" -U board_public -d "$database" <<'SQL'
SELECT content.admit_categorical_report('catreport',9900003,17,1,decode(repeat('31',32),'hex'),
 decode(repeat('11',32),'hex'),decode(repeat('12',32),'hex'),decode(repeat('13',32),'hex'),
 decode(repeat('14',32),'hex'),true,extract(epoch FROM clock_timestamp())::bigint);
SQL
 "${admin[@]}" <<'SQL'
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
CREATE VIEW public.report_columns AS
SELECT n.nspname||'.'||c.relname relation,a.attname,a.attnum,a.atttypid::regtype::text type,
 a.attnotnull,coalesce(pg_get_expr(d.adbin,d.adrelid),'') default_value
FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace
LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
WHERE n.nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission')
-- Index attributes belong to the index contract, not table/view columns.
AND c.relkind IN('r','p','v','m','f') AND a.attnum>0 AND NOT a.attisdropped;
CREATE VIEW public.report_constraints AS
SELECT conrelid::regclass::text relation,conname,contype,convalidated,condeferrable,condeferred,
 connoinherit,pg_get_constraintdef(oid,true) definition FROM pg_constraint
WHERE connamespace IN(SELECT oid FROM pg_namespace WHERE nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission'));
CREATE VIEW public.report_indexes AS
SELECT schemaname,tablename,indexname,indexdef FROM pg_indexes
WHERE schemaname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission');
SQL
 for phase in live restored; do
  if [[ $phase = restored ]]; then
   runuser -u postgres -- "$pg_bin/pg_dump" -h "$cluster" -U postgres -d "$database" --format=custom > "$cluster/current.dump"
   create_database "${database}_restore"
   runuser -u postgres -- "$pg_bin/pg_restore" -h "$cluster" --dbname="${database}_restore" --single-transaction --exit-on-error < "$cluster/current.dump"
   database="${database}_restore"
   migrator=("${psql[@]}" -U board_migrator -d "$database")
   admin=(runuser -u postgres -- "${psql[@]}" -d "$database")
  fi
  "${admin[@]}" -f - < "$cluster/readiness.sql"
  "${admin[@]}" -f - < "$cluster/drift.sql"
  "${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(TABLE public.retained_rows EXCEPT ALL SELECT * FROM public.capture_report_rows())
 THEN RAISE EXCEPTION 'Retained report/session/UUID changed'; END IF;
 IF (SELECT active_catalog_revision FROM post_secrets.report_admission_gate) IS DISTINCT FROM 1
 OR (SELECT count(*) FROM post_secrets.report_catalog_versions)<>1
 OR (SELECT count(*) FROM post_secrets.report_catalog_rows)<>2
 OR NOT EXISTS(SELECT 1 FROM content.reports r JOIN post_secrets.report_membership m ON m.report_id=r.id
 JOIN post_secrets.anonymous_reports a ON a.report_id=r.id
 JOIN post_secrets.anonymous_sessions s ON s.token_hash=a.token_hash
 WHERE r.post_id=9900003 AND r.reason='Synthetic β category' AND r.category_revision=1 AND r.category_id=17
 AND r.category_kind=1 AND r.category_base_weight=-2.25 AND m.automatic_identity=s.automatic_identity
 AND m.automatic_identity IS NOT NULL)
 THEN RAISE EXCEPTION 'Active catalog, captured category or session UUID failed'; END IF;
END $$;
CREATE TABLE public.rejection_before AS SELECT * FROM public.capture_report_rows();
SQL
  # True runtime connections, not just grants inspected as an administrator.
  for role in board_public board_staff board_auth; do
   "${psql[@]}" -U "$role" -d "$database" <<'SQL'
DO $$ BEGIN
 BEGIN
  PERFORM content.set_report_catalog_active(NULL);
  RAISE EXCEPTION 'Runtime activated/deactivated catalog' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN
  PERFORM * FROM post_secrets.eligible_report_categories('catreport',9900003,1);
  RAISE EXCEPTION 'Runtime called private selector' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SQL
  done
  for role in board_public board_staff; do
   "${psql[@]}" -U "$role" -d "$database" <<'SQL'
DO $$ BEGIN
 IF content.report_category_form('catreport',9900004) IS DISTINCT FROM
 '{"revision":1,"categories":[{"id":17,"title":"Synthetic β category","kind":"rule"},{"id":31,"title":"Synthetic special","kind":"illegal"}]}'::jsonb
 THEN RAISE EXCEPTION 'Public form order/labels/kinds or private-field minimization failed'; END IF;
END $$;
SQL
  done
  # Rejected reports and advisory reads leave every durable row untouched.
  "${psql[@]}" -U board_public -d "$database" <<'SQL'
DO $$ DECLARE category bigint; revision bigint; BEGIN
 BEGIN
  PERFORM content.admit_report('catreport',9900004,'Must be denied',decode(repeat('41',32),'hex'),
   decode(repeat('21',32),'hex'),decode(repeat('22',32),'hex'),decode(repeat('23',32),'hex'),decode(repeat('24',32),'hex'),true,extract(epoch FROM clock_timestamp())::bigint);
  RAISE EXCEPTION 'Active mode accepted old free text' USING ERRCODE='ZX001';
 EXCEPTION WHEN SQLSTATE 'P0001' THEN
  IF SQLERRM<>'Free-text reporting is not active.' THEN RAISE; END IF;
 END;
 FOR category,revision IN VALUES(17::bigint,2::bigint),(999::bigint,1::bigint) LOOP
  BEGIN
   PERFORM content.admit_categorical_report('catreport',9900004,category,revision,decode(repeat('41',32),'hex'),
    decode(repeat('21',32),'hex'),decode(repeat('22',32),'hex'),decode(repeat('23',32),'hex'),decode(repeat('24',32),'hex'),true,extract(epoch FROM clock_timestamp())::bigint);
   RAISE EXCEPTION 'Invalid/stale category accepted' USING ERRCODE='ZX001';
  EXCEPTION WHEN SQLSTATE 'P0001' THEN
   IF SQLERRM<>(CASE WHEN revision=2 THEN 'Report categories changed. Please reload the report form.' ELSE 'Invalid category selected.' END) THEN RAISE; END IF;
  END;
 END LOOP;
END $$;
SQL
  "${psql[@]}" -U board_staff -d "$database" <<'SQL'
DO $$ BEGIN
 BEGIN
  PERFORM content.admit_report('catreport',9900004,'Must be denied',decode(repeat('42',32),'hex'));
  RAISE EXCEPTION 'Active mode accepted staff free text' USING ERRCODE='ZX001';
 EXCEPTION WHEN SQLSTATE 'P0001' THEN
  IF SQLERRM<>'Free-text reporting is not active.' THEN RAISE; END IF;
 END;
END $$;
SQL
  "${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(TABLE public.rejection_before EXCEPT ALL SELECT * FROM public.capture_report_rows())
 OR EXISTS(SELECT * FROM public.capture_report_rows() EXCEPT ALL TABLE public.rejection_before)
 THEN RAISE EXCEPTION 'Rejected admission or form read mutated durable state'; END IF;
END $$;
DROP TABLE public.rejection_before;
SQL
  # Actual current API works after restore, and special kind is captured.
  # Rolled back so live/restored durable fingerprints can be compared exactly.
  "${psql[@]}" -U board_public -d "$database" <<'SQL'
BEGIN;
SELECT content.admit_categorical_report('catreport',9900004,31,1,decode(repeat('51',32),'hex'),
 decode(repeat('31',32),'hex'),decode(repeat('32',32),'hex'),decode(repeat('33',32),'hex'),decode(repeat('34',32),'hex'),true,extract(epoch FROM clock_timestamp())::bigint);
ROLLBACK;
SQL
  "${migrator[@]}" <<'SQL'
DO $$ BEGIN
 BEGIN
  PERFORM content.set_report_catalog_active(64);
  RAISE EXCEPTION 'Activated absent revision' USING ERRCODE='ZX001';
 EXCEPTION WHEN invalid_parameter_value THEN NULL; END;
END $$;
SELECT content.set_report_catalog_active(NULL);
SQL
  "${psql[@]}" -U board_public -d "$database" <<'SQL'
BEGIN;
DO $$ BEGIN
 IF content.report_category_form('catreport',9900004)<>'{"revision":null,"categories":[]}'::jsonb
 THEN RAISE EXCEPTION 'Deactivation retained categorical form'; END IF;
 BEGIN
  PERFORM content.admit_categorical_report('catreport',9900004,17,1,decode(repeat('61',32),'hex'),
   decode(repeat('41',32),'hex'),decode(repeat('42',32),'hex'),decode(repeat('43',32),'hex'),decode(repeat('44',32),'hex'),true,extract(epoch FROM clock_timestamp())::bigint);
  RAISE EXCEPTION 'Inactive mode accepted category' USING ERRCODE='ZX001';
 EXCEPTION WHEN SQLSTATE 'P0001' THEN
  IF SQLERRM<>'Categorical reporting is not active.' THEN RAISE; END IF;
 END;
END $$;
SELECT content.admit_report('catreport',9900004,'Restored free text',decode(repeat('61',32),'hex'),
 decode(repeat('41',32),'hex'),decode(repeat('42',32),'hex'),decode(repeat('43',32),'hex'),decode(repeat('44',32),'hex'),true,extract(epoch FROM clock_timestamp())::bigint);
ROLLBACK;
SQL
  "${psql[@]}" -U board_staff -d "$database" <<'SQL'
BEGIN;
SELECT content.admit_report('catreport',9900004,'Restored staff free text',decode(repeat('62',32),'hex'));
ROLLBACK;
SQL
  "${migrator[@]}" <<'SQL'
SELECT content.set_report_catalog_active(1);
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
  printf 'Categorical-report dump/restore fingerprint mismatch (%s).\n' "$mode" >&3; exit 1;
 }
 printf '%s categorical-report migration and current administrator dump/restore passed.\n' "$mode" >&3
done
printf '0098 history/UUIDs preserved; 0099 explicit activation, public category admission, rollback, role denial and current readiness qualified.\n' >&3
