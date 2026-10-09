#!/usr/bin/env bash
# Owned synthetic clusters only. Populated pre-0122 upgrade and current-schema administrator dump/restore.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-deleted-report-cleanup.XXXXXXXX)
exec 3>&1
started=0
cleanup() {
    status=$?
    trap - EXIT
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null || status=1
    fi
    [[ $cluster =~ ^/tmp/board-deleted-report-cleanup\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
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
exec > "$cluster/qualification.log" 2>&1
trap 'printf "Deleted-report-cleanup qualification failed at line %s (private diagnostics removed during cleanup).\n" "$LINENO" >&3' ERR
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
python3 - "$cluster" <<'PYREADINESS'
import pathlib,re,sys
root=pathlib.Path(sys.argv[1])
source=pathlib.Path('crates/store/src/report_admission.rs').read_text()
match=re.search(r'pub const READINESS_SQL: &str = r#"(.*?)"#;',source,re.S)
if not match: raise SystemExit('Cannot extract current report readiness')
query=match.group(1)
if 'delete_reports_for_deleted_target' not in query: raise SystemExit('Missing deletion cleanup readiness')
(root/'readiness.sql').write_text("DO $check$ BEGIN IF ("+query+") IS DISTINCT FROM true THEN RAISE EXCEPTION 'Current readiness failed'; END IF; END $check$;")
changes=[
 'ALTER FUNCTION post_secrets.delete_reports_for_deleted_target() SECURITY INVOKER',
 'ALTER FUNCTION post_secrets.delete_reports_for_deleted_target() SET search_path=public',
 'ALTER FUNCTION post_secrets.delete_reports_for_deleted_target() OWNER TO board_migrator',
 'GRANT EXECUTE ON FUNCTION post_secrets.delete_reports_for_deleted_target() TO PUBLIC',
 'GRANT EXECUTE ON FUNCTION post_secrets.delete_reports_for_deleted_target() TO board_migrator',
 'REVOKE DELETE ON content.reports FROM board_report_admission_owner',
 'GRANT DELETE ON content.reports TO board_public',
 'GRANT DELETE ON content.reports TO board_staff',
 'GRANT TRUNCATE ON content.reports TO board_report_admission_owner',
 'ALTER TABLE content.posts DISABLE TRIGGER retire_deleted_post_report_membership',
 'ALTER TABLE content.threads DISABLE TRIGGER retire_deleted_thread_report_membership',
]
for table in ('report_membership','anonymous_reports','report_weight_evidence'):
 changes.append("DO $drift$ DECLARE c text; BEGIN SELECT conname INTO STRICT c FROM pg_constraint WHERE conrelid='post_secrets."+table+"'::regclass AND confrelid='content.reports'::regclass; EXECUTE format('ALTER TABLE post_secrets."+table+" DROP CONSTRAINT %I',c); END $drift$")
for table in ('report_membership','anonymous_reports','report_weight_evidence'):
 changes.append("DO $drift$ DECLARE trigger_name text; BEGIN SELECT t.tgname INTO STRICT trigger_name FROM pg_trigger t JOIN pg_constraint k ON k.oid=t.tgconstraint JOIN pg_proc p ON p.oid=t.tgfoid JOIN pg_namespace n ON n.oid=p.pronamespace WHERE k.conrelid='post_secrets."+table+"'::regclass AND k.confrelid='content.reports'::regclass AND t.tgrelid=k.confrelid AND t.tgisinternal AND n.nspname='pg_catalog' AND p.proname='RI_FKey_cascade_del'; EXECUTE format('ALTER TABLE content.reports DISABLE TRIGGER %I',trigger_name); END $drift$")
with (root/'drift.sql').open('w') as out:
 for i,change in enumerate(changes):
  out.write('BEGIN; '+change+';\n')
  for role in ('board_public','board_staff'):
   out.write(f"SET LOCAL ROLE {role}; DO $check$ BEGIN IF ({query}) IS DISTINCT FROM false THEN RAISE EXCEPTION 'Readiness accepted cleanup drift {i}'; END IF; END $check$; RESET ROLE;\n")
  out.write('ROLLBACK;\n')
staff=pathlib.Path('apps/staff/src/store.rs').read_text()
m=re.search(r'let mut reports: Vec<Report> = sqlx::query_as\("(.*?)"\)',staff,re.S)
if not m: raise SystemExit('Cannot extract current staff queue query')
queue=m.group(1).replace('$1',"ARRAY['all']::text[]").replace('$2','ARRAY[]::text[]')
(root/'queue.sql').write_text("DO $check$ BEGIN IF EXISTS(SELECT 1 FROM ("+queue+") q WHERE id IN (12201005,12201006)) OR NOT EXISTS(SELECT 1 FROM ("+queue+") q WHERE id=12201007) THEN RAISE EXCEPTION 'Historical deleted report queue filtering failed'; END IF; END $check$;")
PYREADINESS
create_database deleted_report_upgrade
migrator=("${psql[@]}" -U board_migrator -d deleted_report_upgrade)
admin=(runuser -u postgres -- "${psql[@]}" -d deleted_report_upgrade)
for migration in migrations/*.sql; do
 [[ $(basename "$migration") < 0122_ ]] || break
 "${migrator[@]}" --single-transaction -f - < "$migration"
done
"${admin[@]}" <<'SQL'
CREATE FUNCTION public.capture_rows() RETURNS TABLE(relation text,value jsonb)
LANGUAGE plpgsql AS $$ DECLARE r record; BEGIN
 FOR r IN SELECT n.nspname,c.relname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission') AND c.relkind='r' LOOP
  RETURN QUERY EXECUTE format('SELECT %L,to_jsonb(r) FROM %I.%I r',r.nspname||'.'||r.relname,r.nspname,r.relname);
 END LOOP;
END $$;
REVOKE ALL ON FUNCTION public.capture_rows() FROM PUBLIC;
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('delreport','Owned deletion fixture','Synthetic',1000,100,100,100,10),('delother','Other fixture','Synthetic',1000,100,100,100,10);
BEGIN;
SET LOCAL ROLE board_migrator;
SELECT set_config('board.posting_actor',repeat('11',32),true);
INSERT INTO content.threads(id,board) VALUES(12200001,'delreport'),(12200010,'delreport'),(12200020,'delother');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
SELECT i,'delreport',12200001,'Saved name','Saved subject','Saved body' FROM unnest(ARRAY[12200001,12200002,12200003]) i;
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(12200010,'delreport',12200010,'Historical OP','','Saved body'),(12200011,'delreport',12200010,'Historical child','','Saved body'),(12200020,'delother',12200020,'Other','','Saved body');
COMMIT;
INSERT INTO content.reports(id,board,post_id,reason,state,created_at) OVERRIDING SYSTEM VALUE
SELECT 12201000+i,CASE WHEN i=7 THEN 'delother' ELSE 'delreport' END,
CASE WHEN i<=3 THEN 12200002 WHEN i=4 THEN 12200001 WHEN i=5 THEN 12200003 WHEN i=6 THEN 12200011 ELSE 12200020 END,
'Historical report',CASE i WHEN 2 THEN 'resolved' WHEN 3 THEN 'dismissed' ELSE 'open' END,'2020-01-01 UTC'
FROM generate_series(1,7) i;
UPDATE content.reports SET reporter_cleared_at='2020-01-02 UTC',group_cleared_at='2020-01-01 UTC',group_cleared_by=1,group_clear_inherited=false WHERE id=12201003;
BEGIN;
SELECT slug FROM content.boards ORDER BY slug FOR UPDATE;
INSERT INTO post_secrets.report_membership(report_id,actor_hash,board,post_id,thread_id,reported_at)
SELECT id,decode(repeat('22',32),'hex'),board,post_id,12200001,'2020-01-01 UTC' FROM content.reports WHERE id IN(12201002,12201004);
COMMIT;
INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,created_at,network_at,address_at,environment_at,expires_at)
VALUES(decode(repeat('33',32),'hex'),decode(repeat('44',32),'hex'),decode(repeat('55',32),'hex'),decode(repeat('66',32),'hex'),1,1,1,1,2000000000);
INSERT INTO post_secrets.anonymous_reports(report_id,token_hash) VALUES(12201002,decode(repeat('33',32),'hex'));
INSERT INTO post_secrets.report_weight_evidence(report_id,evaluator_version,evaluated_at) VALUES(12201002,1,'2020-01-01 UTC');
INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler)
VALUES(12200002,repeat('a',32),repeat('b',32),'owned.png',1,1,1,false);
INSERT INTO content.moderation_audit(account_id,board,target_id,action,created_at) VALUES(1,'delreport',12200002,'resolve','2020-01-01 UTC');
BEGIN;
SELECT slug FROM content.boards ORDER BY slug FOR UPDATE;
UPDATE content.posts SET deleted=true WHERE id=12200003;
UPDATE content.threads SET deleted=true WHERE id=12200010;
COMMIT;
CREATE TABLE public.before_rows AS SELECT * FROM public.capture_rows();
SQL
"${migrator[@]}" --single-transaction -f - < migrations/0122_deleted_report_cleanup.sql
"${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(TABLE public.before_rows EXCEPT ALL SELECT * FROM public.capture_rows())
 OR EXISTS(SELECT * FROM public.capture_rows() EXCEPT ALL TABLE public.before_rows)
 THEN RAISE EXCEPTION 'Migration changed retained rows'; END IF;
 IF (SELECT count(*) FROM content.reports WHERE id IN(12201005,12201006))<>2
 THEN RAISE EXCEPTION 'Migration swept historical deleted reports'; END IF;
END $$;
DROP TABLE public.before_rows;
SQL
for migration in migrations/*.sql; do
 [[ $(basename "$migration") > 0122_deleted_report_cleanup.sql ]] || continue
 "${migrator[@]}" --single-transaction -f - < "$migration"
done
for phase in live restored; do
 database=deleted_report_upgrade
 if [[ $phase = restored ]]; then
  runuser -u postgres -- "$pg_bin/pg_dump" -h "$cluster" -U postgres -d "$database" --format=custom > "$cluster/current.dump"
  create_database deleted_report_restore
  runuser -u postgres -- "$pg_bin/pg_restore" -h "$cluster" --dbname=deleted_report_restore --single-transaction --exit-on-error < "$cluster/current.dump"
  database=deleted_report_restore
 fi
 admin=(runuser -u postgres -- "${psql[@]}" -d "$database")
 for role in board_public board_staff; do
  "${psql[@]}" -U "$role" -d "$database" -f - < "$cluster/readiness.sql"
 done
 "${psql[@]}" -U board_staff -d "$database" -f - < "$cluster/queue.sql"
 "${admin[@]}" -f - < "$cluster/drift.sql"
 "${admin[@]}" <<'SQL'
BEGIN;
SELECT slug FROM content.boards ORDER BY slug FOR UPDATE;
SET LOCAL ROLE board_public;
UPDATE content.posts SET deleted=true WHERE board='delreport' AND id=12200002;
RESET ROLE;
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM content.reports WHERE id BETWEEN 12201001 AND 12201003)
 OR EXISTS(SELECT 1 FROM post_secrets.report_membership WHERE report_id=12201002)
 OR EXISTS(SELECT 1 FROM post_secrets.anonymous_reports WHERE report_id=12201002)
 OR EXISTS(SELECT 1 FROM post_secrets.report_weight_evidence WHERE report_id=12201002)
 OR EXISTS(SELECT 1 FROM post_secrets.report_group WHERE board='delreport' AND post_id=12200002)
 THEN RAISE EXCEPTION 'Reply cleanup or cascade failed'; END IF;
 IF (SELECT count(*) FROM content.reports)<>4
 OR NOT EXISTS(SELECT 1 FROM content.moderation_audit WHERE target_id=12200002)
 OR NOT EXISTS(SELECT 1 FROM content.post_media WHERE post_id=12200002 AND NOT file_deleted)
 THEN RAISE EXCEPTION 'Reply cleanup changed unrelated evidence'; END IF;
END $$;
ROLLBACK;
DO $$ BEGIN IF (SELECT count(*) FROM content.reports)<>7 THEN RAISE EXCEPTION 'Rollback lost reports'; END IF; END $$;
BEGIN;
SELECT slug FROM content.boards ORDER BY slug FOR UPDATE;
SET LOCAL ROLE board_public;
UPDATE content.threads SET deleted=true WHERE board='delreport' AND id=12200001;
UPDATE content.posts SET deleted=true WHERE board='delreport' AND thread_id=12200001;
RESET ROLE;
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM content.reports WHERE id BETWEEN 12201001 AND 12201005)
 OR (SELECT count(*) FROM content.reports)<>2
 THEN RAISE EXCEPTION 'Thread cleanup changed the wrong targets'; END IF;
END $$;
ROLLBACK;
-- File-only and archive transitions keep report rows under their existing rules.
BEGIN;
SET LOCAL ROLE board_public;
SELECT content.delete_post_attachment('delreport',12200002);
RESET ROLE;
DO $$ BEGIN IF (SELECT count(*) FROM content.reports)<>7 THEN RAISE EXCEPTION 'Public file-only erased reports'; END IF; END $$;
ROLLBACK;
BEGIN;
SELECT slug FROM content.boards ORDER BY slug FOR UPDATE;
SET LOCAL ROLE board_staff;
SELECT content.staff_delete_post_attachment('delreport',12200002);
RESET ROLE;
DO $$ BEGIN IF (SELECT count(*) FROM content.reports)<>7 THEN RAISE EXCEPTION 'Staff file-only erased reports'; END IF; END $$;
ROLLBACK;
BEGIN;
SELECT slug FROM content.boards ORDER BY slug FOR UPDATE;
UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=12200001;
DO $$ BEGIN IF (SELECT count(*) FROM content.reports)<>7 THEN RAISE EXCEPTION 'Archive erased reports'; END IF; END $$;
ROLLBACK;
SQL
 "${admin[@]}" -At > "$cluster/$phase.fingerprint" <<'SQL'
SELECT relation,md5(value::text) FROM public.capture_rows() ORDER BY relation,value::text;
SELECT n.nspname,p.proname,pg_get_function_identity_arguments(p.oid),pg_get_userbyid(p.proowner),p.proconfig::text,md5(pg_get_functiondef(p.oid)) FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname IN('content','post_secrets') ORDER BY 1,2,3;
SELECT t.tgrelid::regclass::text,t.tgname,t.tgenabled,pg_get_triggerdef(t.oid) FROM pg_trigger t WHERE NOT t.tgisinternal ORDER BY 1,2;
SELECT c.conrelid::regclass::text,c.conname,pg_get_constraintdef(c.oid,true) FROM pg_constraint c JOIN pg_namespace n ON n.oid=c.connamespace WHERE n.nspname IN('content','post_secrets') ORDER BY 1,2;
SELECT table_schema,table_name,column_name,grantor,grantee,privilege_type,is_grantable FROM information_schema.column_privileges WHERE table_schema IN('content','post_secrets') ORDER BY 1,2,3,4,5,6,7;
SQL
done
cmp -s "$cluster/live.fingerprint" "$cluster/restored.fingerprint" || { printf 'Deleted-report cleanup dump/restore fingerprint mismatch.\n' >&3; exit 1; }
printf '0122 populated upgrade, current readiness, queue filtering, cascades, drift rejection and administrator restore passed.\n' >&3
