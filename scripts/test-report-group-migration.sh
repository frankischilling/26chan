#!/usr/bin/env bash
# Owned synthetic clusters only. Qualify 0099 -> 0100 and frozen 0100 dump/restore.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-report-group.XXXXXXXX)
started=0
cleanup() {
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null
    fi
    [[ $cluster =~ ^/tmp/board-report-group\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
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
trap 'printf "Report-group qualification failed at line %s (private diagnostics removed during cleanup).\n" "$LINENO" >&3' ERR
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
# Frozen 0100 contract: this historical qualification deliberately stops before
# 0101. Never grant test-only private access to runtime roles.
python3 - "$cluster" <<'PYREADINESS'
import pathlib, sys
root=pathlib.Path(sys.argv[1])
query=pathlib.Path('scripts/fixtures/report-group-0100-readiness.sql').read_text().strip().rstrip(';')
(root/'readiness.sql').write_text("DO $check$ BEGIN IF ("+query+") IS DISTINCT FROM true THEN RAISE EXCEPTION '0100 readiness failed'; END IF; END $check$;")
changes=[
 'ALTER TABLE post_secrets.report_group RENAME TO missing_report_group',
 'ALTER TABLE post_secrets.report_group OWNER TO board_migrator',
 'ALTER TABLE post_secrets.report_group ALTER COLUMN illegal_count DROP NOT NULL',
 'ALTER TABLE post_secrets.report_group ALTER COLUMN illegal_count SET DEFAULT 0',
 'ALTER TABLE post_secrets.report_group ADD COLUMN unexpected integer',
 'ALTER TABLE post_secrets.report_group DROP CONSTRAINT report_group_pkey',
 'ALTER TABLE post_secrets.report_group DROP CONSTRAINT report_group_illegal_count_check',
 'ALTER TABLE post_secrets.report_group DROP CONSTRAINT report_group_illegal_count_check; ALTER TABLE post_secrets.report_group ADD CONSTRAINT report_group_illegal_count_check CHECK(illegal_count>=-1)',
 'ALTER TABLE post_secrets.report_group ENABLE ROW LEVEL SECURITY',
 'GRANT SELECT ON post_secrets.report_group TO board_public',
 'GRANT SELECT(illegal_count) ON post_secrets.report_group TO board_staff',
 'GRANT SELECT ON post_secrets.report_group TO board_auth',
 'GRANT SELECT ON post_secrets.report_group TO board_migrator',
 'REVOKE SELECT(category_kind) ON content.reports FROM board_report_admission_owner',
]
for name in ('increment_report_group','retire_empty_report_group','retire_archived_report_membership'):
 changes += [f'ALTER FUNCTION post_secrets.{name}() SECURITY INVOKER',
  f'ALTER FUNCTION post_secrets.{name}() OWNER TO board_migrator',
  f'ALTER FUNCTION post_secrets.{name}() SET search_path=public',
  f'GRANT EXECUTE ON FUNCTION post_secrets.{name}() TO board_public',
  f'GRANT EXECUTE ON FUNCTION post_secrets.{name}() TO board_staff']
for name in ('report_membership_group_insert','report_membership_group_delete'):
 changes += [f'ALTER TABLE post_secrets.report_membership DISABLE TRIGGER {name}',
             f'DROP TRIGGER {name} ON post_secrets.report_membership']
changes += [
 'DROP TRIGGER report_membership_group_insert ON post_secrets.report_membership; CREATE TRIGGER report_membership_group_insert AFTER INSERT ON post_secrets.report_membership REFERENCING NEW TABLE AS wrong_alias FOR EACH STATEMENT EXECUTE FUNCTION post_secrets.increment_report_group()',
 'DROP TRIGGER report_membership_group_delete ON post_secrets.report_membership; CREATE TRIGGER report_membership_group_delete AFTER DELETE ON post_secrets.report_membership REFERENCING OLD TABLE AS wrong_alias FOR EACH STATEMENT EXECUTE FUNCTION post_secrets.retire_empty_report_group()',
 'ALTER TABLE content.threads DISABLE TRIGGER retire_archived_report_membership',
 'DROP TRIGGER retire_archived_report_membership ON content.threads',
]
for event,predicate in [('UPDATE OF archived_at',''),('UPDATE OF archived_at','WHEN (NEW.archived_at IS NOT NULL)'),('UPDATE','WHEN (OLD.archived_at IS NULL AND NEW.archived_at IS NOT NULL)')]:
 changes.append('DROP TRIGGER retire_archived_report_membership ON content.threads; CREATE TRIGGER retire_archived_report_membership AFTER '+event+' ON content.threads FOR EACH ROW '+predicate+' EXECUTE FUNCTION post_secrets.retire_archived_report_membership()')
with (root/'drift.sql').open('w') as out:
 for i,change in enumerate(changes):
  out.write('BEGIN; '+change+';\n')
  for role in ('board_public','board_staff'):
   out.write(f"SET LOCAL ROLE {role}; DO $check$ BEGIN IF ({query}) IS DISTINCT FROM false THEN RAISE EXCEPTION 'Readiness accepted drift {i}'; END IF; END $check$; RESET ROLE;\n")
  out.write('ROLLBACK;\n')
PYREADINESS
for mode in upgrade fresh; do
 database="report_group_$mode"
 create_database "$database"
 migrator=("${psql[@]}" -U board_migrator -d "$database")
 admin=(runuser -u postgres -- "${psql[@]}" -d "$database")
 for migration in migrations/*.sql; do
  [[ $migration < migrations/0100_report_group_lifetimes.sql ]] || break
  "${migrator[@]}" --single-transaction -f - < "$migration"
 done
 "${admin[@]}" <<'SQL'
CREATE FUNCTION public.capture_report_rows() RETURNS TABLE(relation text,value jsonb)
LANGUAGE plpgsql AS $$ DECLARE r record; BEGIN
 FOR r IN SELECT n.nspname,c.relname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname IN ('content','post_secrets','staff_identity','media','media_intake','deployment','admission') AND c.relkind='r' LOOP
  RETURN QUERY EXECUTE format('SELECT %L,to_jsonb(r) FROM %I.%I r',r.nspname||'.'||r.relname,r.nspname,r.relname);
 END LOOP;
END $$;
-- Owner-maintenance fixtures only, never a new runtime entry point.
CREATE FUNCTION public.add_fixture(p_post bigint,p_kind smallint) RETURNS bigint
LANGUAGE plpgsql AS $$ DECLARE v_id bigint; BEGIN
 INSERT INTO content.reports(board,post_id,reason,category_revision,category_id,category_kind,category_base_weight)
 VALUES('grouplife',p_post,'Synthetic retained report',CASE WHEN p_kind IS NOT NULL THEN 1 END,
 CASE WHEN p_kind=2 THEN 31 WHEN p_kind=1 THEN 1 END,p_kind,CASE WHEN p_kind IS NOT NULL THEN 1.0 END) RETURNING id INTO v_id;
 INSERT INTO post_secrets.report_membership(report_id,actor_hash,board,post_id,thread_id,reported_at)
 VALUES(v_id,decode(repeat('aa',32),'hex'),'grouplife',p_post,10000001,clock_timestamp());
 RETURN v_id;
END $$;
REVOKE ALL ON FUNCTION public.add_fixture(bigint,smallint) FROM PUBLIC;
SQL
 if [[ $mode = fresh ]]; then
  "${migrator[@]}" --single-transaction -f - < migrations/0100_report_group_lifetimes.sql
 fi
 "${migrator[@]}" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,archive_retention_seconds)
VALUES('grouplife','Owned lifetime fixture','Synthetic',1000,100,100,100,10,3600);
BEGIN;
SELECT set_config('board.posting_actor',repeat('11',32),true);
INSERT INTO content.threads(id,board) VALUES(10000001,'grouplife');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
SELECT 10000000+i,'grouplife',10000001,'Synthetic','','Retained body' FROM generate_series(1,12) i;
COMMIT;
SELECT content.import_report_catalog('{"version":1,"categories":[
 {"id":1,"board":"","op_only":false,"reply_only":false,"image_only":false,"exclude_boards":null,"title":"Synthetic rule","weight":1,"filtered":0},
 {"id":31,"board":"","op_only":false,"reply_only":false,"image_only":false,"exclude_boards":null,"title":"Synthetic illegal","weight":1,"filtered":0}]}');
SELECT content.set_report_catalog_active(1);
SQL
 # Actual public admission retains opaque UUID/session linkage through upgrade.
 "${psql[@]}" -U board_public -d "$database" <<'SQL'
SELECT content.admit_categorical_report('grouplife',10000010,31,1,decode(repeat('31',32),'hex'),
 decode(repeat('11',32),'hex'),decode(repeat('12',32),'hex'),decode(repeat('13',32),'hex'),
 decode(repeat('14',32),'hex'),true,extract(epoch FROM clock_timestamp())::bigint);
SQL
 "${admin[@]}" <<'SQL'
SELECT public.add_fixture(10000011,1::smallint);
INSERT INTO content.moderation_audit(account_id,board,target_id,action)
VALUES(1,'grouplife',10000011,'resolve');
CREATE TABLE public.before_install AS SELECT * FROM public.capture_report_rows();
SQL
 if [[ $mode = upgrade ]]; then
  "${migrator[@]}" --single-transaction -f - < migrations/0100_report_group_lifetimes.sql
  "${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM post_secrets.report_group) THEN RAISE EXCEPTION '0100 backfilled counters'; END IF;
 IF EXISTS(TABLE public.before_install EXCEPT ALL SELECT * FROM public.capture_report_rows())
 OR EXISTS(SELECT * FROM public.capture_report_rows() EXCEPT ALL TABLE public.before_install)
 THEN RAISE EXCEPTION '0100 rewrote retained rows or UUIDs'; END IF;
END $$;
-- Already-categorized 0099 membership is unknown, not historical evidence.
SELECT public.add_fixture(10000010,2::smallint);
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM post_secrets.report_group WHERE post_id=10000010 AND illegal_count=1 AND incomplete)
 OR EXISTS(SELECT 1 FROM post_secrets.report_group WHERE post_id=10000011)
 THEN RAISE EXCEPTION 'Preexisting lifetime was reconstructed'; END IF;
END $$;
SQL
 fi
 "${admin[@]}" <<'SQL'
DROP TABLE public.before_install;
-- Per-post thresholds: zero, two, three; all kinds retire for complete <3.
SELECT public.add_fixture(10000001,1::smallint);
SELECT public.add_fixture(10000002,k::smallint) FROM unnest(ARRAY[1,2,2]) k;
SELECT public.add_fixture(10000003,k::smallint) FROM unnest(ARRAY[1,2,2,2]) k;
-- Partial purge keeps both lifetime count and sticky unknown history.
SELECT public.add_fixture(10000004,k::smallint) FROM unnest(ARRAY[1,2,2,2]) k;
DELETE FROM post_secrets.report_membership m USING content.reports r WHERE m.report_id=r.id AND m.post_id=10000004 AND r.category_kind=2;
SELECT public.add_fixture(10000005,k::smallint) FROM unnest(ARRAY[NULL,1]) k;
DELETE FROM post_secrets.report_membership m USING content.reports r WHERE m.report_id=r.id AND m.post_id=10000005 AND r.category_kind IS NULL;
-- Actual emptiness resets despite retained categorized and unclassified history.
SELECT public.add_fixture(10000006,k::smallint) FROM unnest(ARRAY[NULL,2,2,2]) k;
DELETE FROM post_secrets.report_membership WHERE post_id=10000006;
SELECT public.add_fixture(10000006,1::smallint);
-- A conflict/zero-row statement has no new contribution.
INSERT INTO post_secrets.report_membership SELECT * FROM post_secrets.report_membership WHERE post_id=10000003 ON CONFLICT DO NOTHING;
INSERT INTO post_secrets.report_membership SELECT * FROM post_secrets.report_membership WHERE false;
DELETE FROM post_secrets.report_membership WHERE false;
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM post_secrets.report_group WHERE post_id=10000004 AND illegal_count=3 AND NOT incomplete)
 OR NOT EXISTS(SELECT 1 FROM post_secrets.report_group WHERE post_id=10000005 AND illegal_count=0 AND incomplete)
 OR NOT EXISTS(SELECT 1 FROM post_secrets.report_group WHERE post_id=10000006 AND illegal_count=0 AND NOT incomplete)
 OR NOT EXISTS(SELECT 1 FROM post_secrets.report_group WHERE post_id=10000003 AND illegal_count=3 AND NOT incomplete)
 THEN RAISE EXCEPTION 'Lifetime count, taint, empty reset or ignored insert failed'; END IF;
END $$;
CREATE TABLE public.before_rollback AS SELECT * FROM public.capture_report_rows();
BEGIN;
SELECT public.add_fixture(10000003,2::smallint);
UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=10000001;
ROLLBACK;
DO $$ BEGIN
 IF EXISTS(TABLE public.before_rollback EXCEPT ALL SELECT * FROM public.capture_report_rows())
 OR EXISTS(SELECT * FROM public.capture_report_rows() EXCEPT ALL TABLE public.before_rollback)
 THEN RAISE EXCEPTION 'Rollback changed lifetime/history state'; END IF;
END $$;
DROP TABLE public.before_rollback;
CREATE TABLE public.retained_history AS SELECT * FROM public.capture_report_rows()
WHERE relation IN('content.reports','content.moderation_audit','post_secrets.anonymous_sessions','post_secrets.anonymous_reports');
UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=10000001;
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM post_secrets.report_membership WHERE post_id IN(10000001,10000002,10000006))
 OR EXISTS(SELECT 1 FROM post_secrets.report_group WHERE post_id IN(10000001,10000002,10000006))
 OR (SELECT count(*) FROM post_secrets.report_membership WHERE post_id=10000003)<>4
 OR (SELECT count(*) FROM post_secrets.report_membership WHERE post_id IN(10000004,10000005))<>2
 THEN RAISE EXCEPTION 'Per-post archive threshold or all-kinds retirement failed'; END IF;
 IF EXISTS(TABLE public.retained_history EXCEPT ALL SELECT * FROM public.capture_report_rows())
 OR EXISTS(SELECT * FROM public.capture_report_rows() WHERE relation IN
 ('content.reports','content.moderation_audit','post_secrets.anonymous_sessions','post_secrets.anonymous_reports')
 EXCEPT ALL TABLE public.retained_history)
 THEN RAISE EXCEPTION 'Archive rewrote retained report/audit/session history'; END IF;
END $$;
-- New reports after archive are not reprocessed by repeated archive writes.
SELECT public.add_fixture(10000001,1::smallint);
UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '2 hours' WHERE id=10000001;
DO $$ BEGIN
 IF (SELECT count(*) FROM post_secrets.report_membership WHERE post_id=10000001)<>1
 THEN RAISE EXCEPTION 'Repeated archive retired new group'; END IF;
END $$;
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
 if [[ $mode = upgrade ]]; then
  "${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF (SELECT count(*) FROM post_secrets.report_membership WHERE post_id IN(10000010,10000011))<>3
 THEN RAISE EXCEPTION 'Archive retired historical unknown groups'; END IF;
END $$;
SQL
 fi
 for phase in live restored; do
  if [[ $phase = restored ]]; then
   runuser -u postgres -- "$pg_bin/pg_dump" -h "$cluster" -U postgres -d "$database" --format=custom > "$cluster/current.dump"
   create_database "${database}_restore"
   runuser -u postgres -- "$pg_bin/pg_restore" -h "$cluster" --dbname="${database}_restore" --single-transaction --exit-on-error < "$cluster/current.dump"
   database="${database}_restore"
   admin=(runuser -u postgres -- "${psql[@]}" -d "$database")
  fi
  for role in board_public board_staff; do
   "${psql[@]}" -U "$role" -d "$database" -f - < "$cluster/readiness.sql"
  done
  "${admin[@]}" -f - < "$cluster/drift.sql"
  for role in board_public board_staff board_auth; do
   "${psql[@]}" -U "$role" -d "$database" <<'SQL'
DO $$ BEGIN
 BEGIN
  PERFORM * FROM post_secrets.report_group;
  RAISE EXCEPTION 'Runtime read private group' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN
  PERFORM post_secrets.increment_report_group();
  RAISE EXCEPTION 'Runtime executed private helper' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SQL
  done
  "${admin[@]}" -At > "$cluster/$mode-$phase.fingerprint" <<'SQL'
SELECT relation,md5(value::text) FROM public.capture_report_rows() ORDER BY relation,value::text;
SELECT kind,object,md5(value::text) FROM public.report_authority ORDER BY kind,object,value::text;
SELECT * FROM public.report_columns ORDER BY relation,attnum;
SELECT * FROM public.report_constraints ORDER BY relation,conname;
SELECT * FROM public.report_indexes ORDER BY schemaname,tablename,indexname;
SQL
 done
 cmp -s "$cluster/$mode-live.fingerprint" "$cluster/$mode-restored.fingerprint" || {
  printf 'Report-group dump/restore fingerprint mismatch (%s).\n' "$mode" >&3; exit 1;
 }
 printf '%s report-group migration and current administrator dump/restore passed.\n' "$mode" >&3
done
printf '0099 history preserved; 0100 forward lifetimes, archive retirement, rollback, runtime denial and readiness drift qualified.\n' >&3
