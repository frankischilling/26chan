#!/usr/bin/env bash
# Owned synthetic PostgreSQL 16 cluster. Populate 0123, upgrade through 0125,
# then exercise prospective sticky reply retirement and an administrator restore.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-sticky-retention.XXXXXXXX)
exec 3>&1
started=0
cleanup() {
    status=$?
    trap - EXIT
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null || status=1
    fi
    [[ $cluster =~ ^/tmp/board-sticky-retention\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
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
# Fixture rows, anonymous proofs, and SQL diagnostics stay inside this private directory.
exec > "$cluster/qualification.log" 2>&1
diagnose_error() {
    local at_line=$1
    printf 'Sticky-retention migration qualification failed at line %s.\n' "$at_line" >&3
    # Only the last PostgreSQL ERROR/FATAL line escapes the disposable cluster.
    # Mask quoted names/values, long IDs and digests; never output statement
    # lines, SQL context, DETAIL, HINT, or any captured private row.
    python3 - "$cluster/qualification.log" >&3 <<'PYDIAGNOSTIC' || true
import pathlib
import re
import sys

path = pathlib.Path(sys.argv[1])
if path.is_file():
    matches = list(re.finditer(r'\b(ERROR|FATAL):\s*([^\r\n]*)',
                               path.read_text(encoding='utf-8', errors='replace')))
    if matches:
        match = matches[-1]
        message = re.sub(r'''(['"]).*?\1''', '[quoted value]', match.group(2))
        message = re.sub(r'(?:\\x)?[0-9a-fA-F]{16,}|\b\d{5,}\b', '[redacted]', message)
        message = re.sub(r'[^A-Za-z0-9 _.,:;()/=+<>-]', '?', message)
        print('PostgreSQL ' + match.group(1) + ': ' + message[:180])
PYDIAGNOSTIC
}
trap 'diagnose_error "$LINENO"' ERR
runuser -u postgres -- "${psql[@]}" -d postgres -f - < deploy/roles.sql
# These are login roles only in the owned disposable cluster, for direct denial tests.
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
GRANT CONNECT ON DATABASE :"database" TO board_migrator,board_public,board_staff,
    board_auth,board_media,board_media_read,board_media_intake,board_monitor;
SQL
}
python3 - "$cluster" <<'PYREADINESS'
import pathlib
import re
import sys

source = pathlib.Path('crates/store/src/sticky_retention.rs').read_text(encoding='utf-8')
match = re.search(r'pub const READINESS_SQL: &str = r#"(.*?)"#;', source, re.S)
if not match or 'retire_pruned_reply_credentials' not in match.group(1):
    raise SystemExit('Cannot extract current sticky-retention readiness SQL')
pathlib.Path(sys.argv[1], 'readiness.sql').write_text(
    'DO $check$ BEGIN IF (' + match.group(1) +
    ") IS DISTINCT FROM true THEN RAISE EXCEPTION 'Sticky retention readiness failed'; "
    'END IF; END $check$;\n', encoding='utf-8')
pathlib.Path(sys.argv[1], 'membership-fault.sql').write_text('''BEGIN;
-- The role can SET the private owner, but cannot inherit its DELETE grants.
GRANT board_posting_cooldown_owner TO board_media_read WITH INHERIT FALSE, SET TRUE;
DO $member$ BEGIN
 IF NOT pg_has_role('board_media_read','board_posting_cooldown_owner','MEMBER')
 OR has_table_privilege('board_media_read','post_secrets.anonymous_posts','DELETE')
 OR has_table_privilege('board_media_read','post_secrets.deletion','DELETE')
 THEN RAISE EXCEPTION 'Membership fixture unexpectedly inherits retirement rights'; END IF;
END $member$;
SET LOCAL ROLE board_media_read;
DO $denied$ BEGIN
 BEGIN
  DELETE FROM post_secrets.anonymous_posts WHERE post_id=12501002;
  RAISE EXCEPTION 'NOINHERIT member deleted private proof' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $denied$;
RESET ROLE;
DO $readiness$ BEGIN IF (''' + match.group(1) + ''') IS DISTINCT FROM false
 THEN RAISE EXCEPTION 'Readiness accepted runtime membership with SET authority';
 END IF; END $readiness$;
ROLLBACK;
''', encoding='utf-8')
PYREADINESS
create_database sticky_upgrade
migrator=("${psql[@]}" -U board_migrator -d sticky_upgrade)
admin=(runuser -u postgres -- "${psql[@]}" -d sticky_upgrade)
for migration in migrations/*.sql; do
    [[ $(basename "$migration") < 0125_ ]] || break
    "${migrator[@]}" --single-transaction -f - < "$migration"
done
# The synthetic fixture needs raw private report/anonymous tables that the
# migrator intentionally cannot access. Keep those imports and every private
# snapshot under this disposable cluster's bootstrap administrator. Only the
# inserts whose existing trigger checks require board_migrator SET that role.
"${admin[@]}" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,staff_only)
VALUES ('stickyup','Public sticky fixture','Synthetic',1000,3,3,100,10,false),
       ('stickypr','Private sticky fixture','Synthetic',1000,3,3,100,10,true),
       ('stickyno','Ordinary thread fixture','Synthetic',1000,3,3,100,10,false);
BEGIN;
SET LOCAL ROLE board_migrator;
SELECT set_config('board.posting_actor',repeat('11',32),true);
INSERT INTO content.threads(id,board,sticky,undead)
VALUES(12501000,'stickyup',true,true),(12502000,'stickypr',true,true),
      (12503000,'stickyno',false,false);
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
SELECT id,board,thread_id,'Fixture',CASE WHEN id=thread_id THEN 'Historical OP' ELSE '' END,
       'Retained body '||id FROM (VALUES
    (12501000,'stickyup',12501000),(12501001,'stickyup',12501000),
    (12501002,'stickyup',12501000),(12501003,'stickyup',12501000),
    (12501004,'stickyup',12501000),(12501005,'stickyup',12501000),
    (12502000,'stickypr',12502000),(12502001,'stickypr',12502000),
    (12502002,'stickypr',12502000),(12502003,'stickypr',12502000),
    (12503000,'stickyno',12503000),(12503001,'stickyno',12503000),
    (12503002,'stickyno',12503000),(12503003,'stickyno',12503000)
) AS fixture(id,board,thread_id);
COMMIT;
BEGIN;
SET LOCAL ROLE board_migrator;
INSERT INTO post_secrets.deletion(post_id,password_hash)
SELECT p.id,'synthetic-password-'||p.id FROM content.posts p WHERE p.board IN('stickyup','stickypr','stickyno');
COMMIT;
INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,
    created_at,network_at,address_at,environment_at,expires_at)
VALUES(decode(repeat('11',32),'hex'),decode(repeat('22',32),'hex'),
       decode(repeat('33',32),'hex'),decode(repeat('44',32),'hex'),1,1,1,1,4102444800);
INSERT INTO post_secrets.anonymous_posts(post_id,token_hash,password_proof)
SELECT d.post_id,decode(repeat('11',32),'hex'),sha256(convert_to(d.password_hash,'UTF8'))
FROM post_secrets.deletion d JOIN content.posts p ON p.id=d.post_id
WHERE p.board IN('stickyup','stickypr','stickyno');
-- This pre-0125 deletion must keep its old credentials; installation cannot sweep it.
BEGIN;
SELECT slug FROM content.boards WHERE slug='stickyup' FOR UPDATE;
UPDATE content.posts SET deleted=true WHERE id=12501001;
COMMIT;
INSERT INTO content.reports(board,post_id,reason,created_at)
SELECT board,id,'Retain historical report','2020-01-01 UTC'
FROM content.posts WHERE id IN(12501002,12501003,12501005,12502001,12503001);
BEGIN;
SELECT slug FROM content.boards WHERE slug IN('stickypr','stickyup') ORDER BY slug FOR UPDATE;
INSERT INTO post_secrets.report_membership(report_id,actor_hash,board,post_id,thread_id,reported_at)
SELECT r.id,decode(repeat('55',32),'hex'),r.board,r.post_id,
       CASE WHEN r.board='stickypr' THEN 12502000 ELSE 12501000 END,'2020-01-01 UTC'
FROM content.reports r WHERE r.post_id IN(12501002,12502001);
COMMIT;
INSERT INTO post_secrets.anonymous_reports(report_id,token_hash)
SELECT r.id,decode(repeat('11',32),'hex') FROM content.reports r WHERE r.post_id=12501003;
INSERT INTO post_secrets.report_weight_evidence(report_id,evaluator_version,evaluated_at)
SELECT r.id,1,'2020-01-01 UTC' FROM content.reports r WHERE r.post_id=12501002;
INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler)
VALUES(12501002,repeat('a',32),repeat('b',32),'retained.png',1,1,1,false);
INSERT INTO content.moderation_audit(account_id,board,target_id,action,created_at)
VALUES(1,'stickyup',12501002,'resolve','2020-01-01 UTC');
CREATE FUNCTION public.capture_rows() RETURNS TABLE(relation text,value jsonb)
LANGUAGE plpgsql SECURITY INVOKER SET search_path=pg_catalog,pg_temp AS $$
DECLARE r record;
BEGIN
 FOR r IN SELECT n.nspname,c.relname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
  WHERE n.nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission')
   AND c.relkind='r' LOOP
  RETURN QUERY EXECUTE format('SELECT %L,to_jsonb(x) FROM %I.%I x',r.nspname||'.'||r.relname,r.nspname,r.relname);
 END LOOP;
END $$;
-- The snapshot helper stays admin-owned and invoker-privileged. Every caller
-- below is the administrator; do not make it a runtime private-data bypass.
REVOKE ALL ON FUNCTION public.capture_rows() FROM PUBLIC;
CREATE TABLE public.before_rows AS SELECT * FROM public.capture_rows();
CREATE TABLE public.before_policies AS
SELECT n.nspname,c.relname,p.polname,p.polcmd,p.polpermissive,
  ARRAY(SELECT CASE WHEN x.role_id=0 THEN 'PUBLIC' ELSE pg_get_userbyid(x.role_id) END
        FROM unnest(p.polroles) AS x(role_id) ORDER BY 1) roles,
  pg_get_expr(p.polqual,p.polrelid) qualifier,
  pg_get_expr(p.polwithcheck,p.polrelid) check_expr
FROM pg_policy p JOIN pg_class c ON c.oid=p.polrelid JOIN pg_namespace n ON n.oid=c.relnamespace
WHERE n.nspname IN('content','post_secrets');
CREATE TABLE public.before_rls AS
SELECT n.nspname,c.relname,c.relrowsecurity,c.relforcerowsecurity
FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
WHERE n.nspname IN('content','post_secrets') AND c.relkind IN('r','p');
SQL
"${migrator[@]}" --single-transaction -f - < migrations/0125_sticky_reply_retirement.sql
# Both databases start with the identical populated state, before prospective pruning.
runuser -u postgres -- "$pg_bin/pg_dump" -h "$cluster" -d sticky_upgrade --format=custom > "$cluster/populated.dump"
create_database sticky_restore
runuser -u postgres -- "$pg_bin/pg_restore" -h "$cluster" --dbname=sticky_restore \
    --single-transaction --exit-on-error < "$cluster/populated.dump"
for database in sticky_upgrade sticky_restore; do
    migrator=("${psql[@]}" -U board_migrator -d "$database")
    admin=(runuser -u postgres -- "${psql[@]}" -d "$database")
    # Before/after snapshots include relations deliberately unavailable to
    # board_migrator. The restored helper remains SECURITY INVOKER.
    "${admin[@]}" <<'SQL'
DO $check$ BEGIN
 IF EXISTS(TABLE public.before_rows EXCEPT ALL SELECT * FROM public.capture_rows())
 OR EXISTS(SELECT * FROM public.capture_rows() EXCEPT ALL TABLE public.before_rows)
 THEN RAISE EXCEPTION '0125 changed historical data, hashes, proof rows, reports or board policy'; END IF;
 IF EXISTS(TABLE public.before_policies EXCEPT ALL
   SELECT n.nspname,c.relname,p.polname,p.polcmd,p.polpermissive,
     ARRAY(SELECT CASE WHEN x.role_id=0 THEN 'PUBLIC' ELSE pg_get_userbyid(x.role_id) END
           FROM unnest(p.polroles) AS x(role_id) ORDER BY 1),
     pg_get_expr(p.polqual,p.polrelid),pg_get_expr(p.polwithcheck,p.polrelid)
   FROM pg_policy p JOIN pg_class c ON c.oid=p.polrelid JOIN pg_namespace n ON n.oid=c.relnamespace
   WHERE n.nspname IN('content','post_secrets'))
 OR EXISTS(
   SELECT n.nspname,c.relname,p.polname,p.polcmd,p.polpermissive,
     ARRAY(SELECT CASE WHEN x.role_id=0 THEN 'PUBLIC' ELSE pg_get_userbyid(x.role_id) END
           FROM unnest(p.polroles) AS x(role_id) ORDER BY 1),
     pg_get_expr(p.polqual,p.polrelid),pg_get_expr(p.polwithcheck,p.polrelid)
   FROM pg_policy p JOIN pg_class c ON c.oid=p.polrelid JOIN pg_namespace n ON n.oid=c.relnamespace
   WHERE n.nspname IN('content','post_secrets') EXCEPT ALL TABLE public.before_policies)
 THEN RAISE EXCEPTION '0125 changed existing RLS policies'; END IF;
 IF EXISTS(TABLE public.before_rls EXCEPT ALL
   SELECT n.nspname,c.relname,c.relrowsecurity,c.relforcerowsecurity
   FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
   WHERE n.nspname IN('content','post_secrets') AND c.relkind IN('r','p'))
 OR EXISTS(
   SELECT n.nspname,c.relname,c.relrowsecurity,c.relforcerowsecurity
   FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
   WHERE n.nspname IN('content','post_secrets') AND c.relkind IN('r','p')
   EXCEPT ALL TABLE public.before_rls)
 THEN RAISE EXCEPTION '0125 changed RLS enablement'; END IF;
 IF NOT EXISTS(SELECT 1 FROM content.posts WHERE id=12501001 AND deleted)
 OR NOT EXISTS(SELECT 1 FROM post_secrets.deletion WHERE post_id=12501001)
 OR NOT EXISTS(SELECT 1 FROM post_secrets.anonymous_posts WHERE post_id=12501001)
 OR (SELECT count(*) FROM content.reports WHERE post_id IN(12501002,12501003,12501005,12502001,12503001))<>5
 THEN RAISE EXCEPTION 'Historical deleted credentials or reports were swept'; END IF;
END $check$;
SQL
    # Catalog-only ownership/permission assertions remain under the real
    # restricted migrator. This role never reads private fixture relations.
    "${migrator[@]}" <<'SQL'
DO $authority$ DECLARE fn regprocedure:='post_secrets.retire_pruned_reply_credentials()'; runtime text; BEGIN
 IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='board_posting_cooldown_owner'
   AND NOT (rolcanlogin OR rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls))
 OR has_schema_privilege('board_posting_cooldown_owner','post_secrets','CREATE')
 OR has_schema_privilege('board_posting_cooldown_owner','content','CREATE')
 OR NOT has_column_privilege('board_posting_cooldown_owner','post_secrets.anonymous_posts','post_id','SELECT')
 OR has_column_privilege('board_posting_cooldown_owner','post_secrets.anonymous_posts','token_hash','SELECT')
 OR has_column_privilege('board_posting_cooldown_owner','post_secrets.anonymous_posts','password_proof','SELECT')
 OR NOT has_table_privilege('board_posting_cooldown_owner','post_secrets.anonymous_posts','DELETE')
 OR has_table_privilege('board_posting_cooldown_owner','post_secrets.anonymous_posts','INSERT,UPDATE,TRUNCATE,REFERENCES,TRIGGER')
 OR NOT has_table_privilege('board_posting_cooldown_owner','post_secrets.deletion','DELETE')
 OR has_column_privilege('board_posting_cooldown_owner','post_secrets.deletion','password_hash','SELECT')
 OR NOT has_column_privilege('board_posting_cooldown_owner','content.threads','sticky','SELECT')
 OR NOT has_column_privilege('board_posting_cooldown_owner','content.threads','undead','SELECT')
 OR NOT has_column_privilege('board_posting_cooldown_owner','content.boards','reply_limit','SELECT')
 THEN RAISE EXCEPTION 'Unsafe sticky retirement owner grants'; END IF;
 IF NOT EXISTS(SELECT 1 FROM pg_proc p WHERE p.oid=fn
   AND p.proowner='board_posting_cooldown_owner'::regrole AND p.prosecdef
   AND p.prorettype='pg_catalog.trigger'::regtype
   AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp'])
 OR (SELECT count(*) FROM pg_proc p,
   LATERAL aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a
   WHERE p.oid=fn AND a.privilege_type='EXECUTE' AND a.grantee=p.proowner)<>1
 OR EXISTS(SELECT 1 FROM pg_proc p,
   LATERAL aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a
   WHERE p.oid=fn AND a.privilege_type='EXECUTE' AND a.grantee<>p.proowner)
 OR has_function_privilege('board_migrator',fn,'EXECUTE')
 THEN RAISE EXCEPTION 'Unsafe sticky retirement function ownership or EXECUTE ACL'; END IF;
 IF NOT EXISTS(SELECT 1 FROM pg_trigger t WHERE t.tgrelid='content.posts'::regclass
   AND t.tgname='retire_pruned_reply_credentials' AND t.tgfoid=fn
   AND NOT t.tgisinternal AND t.tgtype=17 AND t.tgenabled='O'
   AND t.tgattr::text=(SELECT attnum::text FROM pg_attribute
       WHERE attrelid='content.posts'::regclass AND attname='deleted')
   AND t.tgqual IS NOT NULL AND t.tgnargs=0 AND t.tgconstraint=0)
 THEN RAISE EXCEPTION 'Wrong sticky retirement trigger'; END IF;
 FOREACH runtime IN ARRAY ARRAY['board_public','board_staff','board_auth','board_media',
   'board_media_read','board_media_intake','board_monitor'] LOOP
  IF has_function_privilege(runtime,fn,'EXECUTE')
   OR pg_has_role(runtime,'board_posting_cooldown_owner','MEMBER')
   OR has_table_privilege(runtime,'post_secrets.anonymous_posts','DELETE')
   OR has_table_privilege(runtime,'post_secrets.deletion','DELETE')
  THEN RAISE EXCEPTION 'Runtime gained sticky private authority: %',runtime; END IF;
 END LOOP;
END $authority$;
SQL
    # Membership alone grants a future SET ROLE escape despite NOINHERIT.
    # Only this transaction's bootstrap administrator can add the membership;
    # rollback removes it before real runtime probes and readiness assertions.
    "${admin[@]}" -f - < "$cluster/membership-fault.sql"
    for role in board_public board_staff board_auth board_media board_media_read board_media_intake board_monitor; do
        "${psql[@]}" -U "$role" -d "$database" -f - < "$cluster/readiness.sql"
        "${psql[@]}" -U "$role" -d "$database" <<'SQL'
DO $denied$ DECLARE fn oid; anonymous_relation oid; deletion_relation oid; BEGIN
 -- Some runtimes cannot resolve qualified names in the private schema. Read
 -- catalog identities without granting USAGE before checking effective ACLs.
 SELECT p.oid INTO STRICT fn FROM pg_catalog.pg_proc p
 JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace
 WHERE n.nspname='post_secrets' AND p.proname='retire_pruned_reply_credentials' AND p.pronargs=0;
 SELECT c.oid INTO STRICT anonymous_relation FROM pg_catalog.pg_class c
 JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname='post_secrets' AND c.relname='anonymous_posts';
 SELECT c.oid INTO STRICT deletion_relation FROM pg_catalog.pg_class c
 JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname='post_secrets' AND c.relname='deletion';
 PERFORM set_config('board.sticky_prune_thread','12501000',true);
 IF has_function_privilege(session_user,fn,'EXECUTE')
 OR has_table_privilege(session_user,anonymous_relation,'DELETE')
 OR has_table_privilege(session_user,deletion_relation,'DELETE')
 THEN RAISE EXCEPTION 'Runtime acquired private retirement privileges'; END IF;
 BEGIN
  PERFORM post_secrets.retire_pruned_reply_credentials();
  RAISE EXCEPTION 'Runtime directly invoked sticky retirement' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN
  DELETE FROM post_secrets.anonymous_posts WHERE post_id=12501002;
  RAISE EXCEPTION 'Runtime deleted raw anonymous proof' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN
  DELETE FROM post_secrets.deletion WHERE post_id=12501002;
  RAISE EXCEPTION 'Runtime deleted raw password hash' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 IF session_user='board_public' THEN
  IF EXISTS(SELECT 1 FROM content.posts WHERE board='stickypr')
  OR EXISTS(SELECT 1 FROM content.threads WHERE board='stickypr')
  THEN RAISE EXCEPTION 'Public role saw private fixture'; END IF;
 END IF;
END $denied$;
SQL
    done
    # The administrator can SET LOCAL ROLE for an actual public write; the
    # migrator intentionally has no membership in the public runtime role.
    "${admin[@]}" <<'SQL'
-- A forged marker cannot retire newest replies or non-sticky proofs.
BEGIN;
SELECT slug FROM content.boards WHERE slug IN('stickyno','stickyup') ORDER BY slug FOR UPDATE;
SET LOCAL ROLE board_public;
SELECT set_config('board.sticky_prune_thread','12501000',true);
DO $reject$ BEGIN
 UPDATE content.posts SET deleted=true WHERE id=12501005;
 RAISE EXCEPTION 'Retained newest reply was pruned' USING ERRCODE='ZX001';
EXCEPTION WHEN check_violation THEN NULL; END $reject$;
SELECT set_config('board.sticky_prune_thread','12503000',true);
DO $reject$ BEGIN
 UPDATE content.posts SET deleted=true WHERE id=12503001;
 RAISE EXCEPTION 'Nonsticky reply was credential-pruned' USING ERRCODE='ZX001';
EXCEPTION WHEN check_violation THEN NULL; END $reject$;
RESET ROLE;
ROLLBACK;
-- Ordinary soft deletion without the marker retains private posting authority.
BEGIN;
SELECT slug FROM content.boards WHERE slug='stickyno' FOR UPDATE;
SET LOCAL ROLE board_public;
UPDATE content.posts SET deleted=true WHERE id=12503001;
RESET ROLE;
DO $check$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM post_secrets.deletion WHERE post_id=12503001)
 OR NOT EXISTS(SELECT 1 FROM post_secrets.anonymous_posts WHERE post_id=12503001)
 THEN RAISE EXCEPTION 'Ordinary deletion erased private proofs'; END IF;
END $check$;
ROLLBACK;
-- Public cannot reach the staff-only board even when it supplies the marker.
BEGIN;
SELECT slug FROM content.boards WHERE slug='stickypr' FOR UPDATE;
SET LOCAL ROLE board_public;
SELECT set_config('board.sticky_prune_thread','12502000',true);
DO $reject$ DECLARE changed integer; BEGIN
 UPDATE content.posts SET deleted=true WHERE id=12502001;
 GET DIAGNOSTICS changed=ROW_COUNT;
 IF changed<>0 THEN RAISE EXCEPTION 'Public marked private reply deleted'; END IF;
EXCEPTION WHEN insufficient_privilege THEN NULL; END $reject$;
RESET ROLE;
ROLLBACK;
-- Prove both proofs and report membership roll back with the post transition.
BEGIN;
SELECT slug FROM content.boards WHERE slug='stickyup' FOR UPDATE;
SET LOCAL ROLE board_public;
SELECT set_config('board.sticky_prune_thread','12501000',true);
UPDATE content.posts SET deleted=true WHERE board='stickyup' AND id IN(12501002,12501003);
RESET ROLE;
DO $check$ BEGIN
 IF (SELECT count(*) FROM content.posts WHERE id IN(12501002,12501003) AND deleted)<>2
 OR EXISTS(SELECT 1 FROM post_secrets.anonymous_posts WHERE post_id IN(12501002,12501003))
 OR EXISTS(SELECT 1 FROM post_secrets.deletion WHERE post_id IN(12501002,12501003))
 OR EXISTS(SELECT 1 FROM content.reports WHERE post_id IN(12501002,12501003))
 OR EXISTS(SELECT 1 FROM post_secrets.report_membership WHERE post_id=12501002)
 OR EXISTS(SELECT 1 FROM post_secrets.anonymous_reports WHERE report_id IN
   (SELECT (value->>'id')::bigint FROM public.before_rows
    WHERE relation='content.reports' AND value->>'post_id'='12501003'))
 OR EXISTS(SELECT 1 FROM post_secrets.report_weight_evidence WHERE report_id IN
   (SELECT (value->>'id')::bigint FROM public.before_rows
    WHERE relation='content.reports' AND value->>'post_id'='12501002'))
 THEN RAISE EXCEPTION 'Sticky prune did not retire proofs and reports in its transaction'; END IF;
 IF NOT EXISTS(SELECT 1 FROM post_secrets.deletion WHERE post_id=12501001)
 OR NOT EXISTS(SELECT 1 FROM post_secrets.anonymous_posts WHERE post_id=12501005)
 THEN RAISE EXCEPTION 'Sticky prune changed historical or retained authority'; END IF;
END $check$;
ROLLBACK;
DO $check$ BEGIN
 IF (SELECT count(*) FROM content.posts WHERE id IN(12501002,12501003) AND NOT deleted)<>2
 OR (SELECT count(*) FROM post_secrets.anonymous_posts WHERE post_id IN(12501002,12501003))<>2
 OR (SELECT count(*) FROM post_secrets.deletion WHERE post_id IN(12501002,12501003))<>2
 OR (SELECT count(*) FROM content.reports WHERE post_id IN(12501002,12501003))<>2
 OR NOT EXISTS(SELECT 1 FROM post_secrets.report_membership WHERE post_id=12501002)
 OR (SELECT count(*) FROM post_secrets.anonymous_reports WHERE report_id IN
   (SELECT (value->>'id')::bigint FROM public.before_rows
    WHERE relation='content.reports' AND value->>'post_id'='12501003'))<>1
 OR (SELECT count(*) FROM post_secrets.report_weight_evidence WHERE report_id IN
   (SELECT (value->>'id')::bigint FROM public.before_rows
    WHERE relation='content.reports' AND value->>'post_id'='12501002'))<>1
 THEN RAISE EXCEPTION 'Sticky prune rollback lost posts, proofs or reports'; END IF;
END $check$;
-- Commit the identical source-window pruning to demonstrate durable retirement.
BEGIN;
SELECT slug FROM content.boards WHERE slug='stickyup' FOR UPDATE;
SET LOCAL ROLE board_public;
SELECT set_config('board.sticky_prune_thread','12501000',true);
UPDATE content.posts SET deleted=true WHERE board='stickyup' AND id IN(12501002,12501003);
RESET ROLE;
COMMIT;
DO $check$ BEGIN
 IF (SELECT count(*) FROM content.posts WHERE id IN(12501002,12501003) AND deleted)<>2
 OR EXISTS(SELECT 1 FROM post_secrets.anonymous_posts WHERE post_id IN(12501002,12501003))
 OR EXISTS(SELECT 1 FROM post_secrets.deletion WHERE post_id IN(12501002,12501003))
 OR EXISTS(SELECT 1 FROM content.reports WHERE post_id IN(12501002,12501003))
 OR EXISTS(SELECT 1 FROM post_secrets.report_membership WHERE post_id=12501002)
 OR EXISTS(SELECT 1 FROM post_secrets.anonymous_reports WHERE report_id IN
   (SELECT (value->>'id')::bigint FROM public.before_rows
    WHERE relation='content.reports' AND value->>'post_id'='12501003'))
 OR EXISTS(SELECT 1 FROM post_secrets.report_weight_evidence WHERE report_id IN
   (SELECT (value->>'id')::bigint FROM public.before_rows
    WHERE relation='content.reports' AND value->>'post_id'='12501002'))
 OR (SELECT count(*) FROM post_secrets.deletion WHERE post_id BETWEEN 12501000 AND 12503003)<>12
 OR (SELECT count(*) FROM post_secrets.anonymous_posts WHERE post_id BETWEEN 12501000 AND 12503003)<>12
 OR (SELECT count(*) FROM post_secrets.deletion WHERE post_id IN(12501000,12501001,12501004,12501005,12502001))<>5
 OR (SELECT count(*) FROM post_secrets.anonymous_posts WHERE post_id IN(12501000,12501001,12501004,12501005,12502001))<>5
 OR NOT EXISTS(SELECT 1 FROM content.post_media WHERE post_id=12501002 AND NOT file_deleted)
 OR NOT EXISTS(SELECT 1 FROM content.moderation_audit WHERE target_id=12501002)
 OR (SELECT count(*) FROM content.reports WHERE post_id IN(12501005,12502001,12503001))<>3
 OR NOT EXISTS(SELECT 1 FROM content.boards WHERE slug='stickypr' AND staff_only AND reply_limit=3)
 OR NOT EXISTS(SELECT 1 FROM content.threads WHERE id=12501000 AND sticky AND undead AND NOT deleted)
 THEN RAISE EXCEPTION 'Committed prune changed retained content, proof or board policy'; END IF;
END $check$;
DROP TABLE public.before_rows,public.before_policies,public.before_rls;
SQL
    # This includes the complete private-row snapshot, so use only the admin.
    "${admin[@]}" -At > "$cluster/$database.fingerprint" <<'SQL'
SELECT relation,md5(value::text) FROM public.capture_rows() ORDER BY relation,value::text;
SELECT n.nspname,p.proname,pg_get_function_identity_arguments(p.oid),pg_get_userbyid(p.proowner),
 p.proconfig::text,md5(pg_get_functiondef(p.oid)),
 coalesce((SELECT jsonb_agg(jsonb_build_array(pg_get_userbyid(a.grantor),
  CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,
  a.privilege_type,a.is_grantable) ORDER BY pg_get_userbyid(a.grantor),a.grantee,a.privilege_type)
  FROM aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a),'[]'::jsonb)
 FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
 WHERE n.nspname IN('content','post_secrets') ORDER BY 1,2,3;
SELECT t.tgrelid::regclass::text,t.tgname,t.tgenabled,pg_get_triggerdef(t.oid)
 FROM pg_trigger t WHERE NOT t.tgisinternal ORDER BY 1,2;
SELECT n.nspname,c.relname,c.relrowsecurity,c.relforcerowsecurity
 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname IN('content','post_secrets') AND c.relkind IN('r','p') ORDER BY 1,2;
SELECT n.nspname,c.relname,p.polname,p.polcmd,p.polpermissive,
 pg_get_expr(p.polqual,p.polrelid),pg_get_expr(p.polwithcheck,p.polrelid)
 FROM pg_policy p JOIN pg_class c ON c.oid=p.polrelid JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname IN('content','post_secrets') ORDER BY 1,2,3;
SELECT table_schema,table_name,column_name,grantor,grantee,privilege_type,is_grantable
 FROM information_schema.column_privileges WHERE table_schema IN('content','post_secrets')
 ORDER BY 1,2,3,4,5,6,7;
SQL
done
cmp -s "$cluster/sticky_upgrade.fingerprint" "$cluster/sticky_restore.fingerprint" || {
    printf 'Sticky-retention live/restore data, ownership, ACL or trigger fingerprint mismatch.\n' >&3
    exit 1
}
printf '0125 populated upgrade, historical state, owner/ACL/trigger, runtime denials, atomic sticky pruning and administrator restore passed.\n' >&3
