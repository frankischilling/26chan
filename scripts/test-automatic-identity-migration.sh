#!/usr/bin/env bash
# Owned synthetic clusters only. Qualify 0094 -> 0095/0096 and current dump/restore.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-automatic-identity.XXXXXXXX)
started=0
cleanup() {
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null
    fi
    [[ $cluster =~ ^/tmp/board-automatic-identity\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
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
trap 'printf "Automatic-identity qualification failed at line %s (private diagnostics removed during cleanup).\n" "$LINENO" >&3' ERR
# All fixture SQL is root-owned mode 0600. The parent shell opens stdin before
# runuser drops privileges; never pass these private paths to postgres psql.
runuser -u postgres -- "${psql[@]}" -d postgres -f deploy/roles.sql
create_database() {
 runuser -u postgres -- "${psql[@]}" -d postgres -v database="$1" <<'SQL'
CREATE DATABASE :"database" OWNER board_migrator;
REVOKE ALL ON DATABASE :"database" FROM PUBLIC;
GRANT CONNECT ON DATABASE :"database" TO board_migrator,board_public;
SQL
}
cat > "$cluster/fixtures.sql" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,staff_only,user_thread_limit)
VALUES('autoid','Automatic identity fixture','Synthetic',1000,100,100,100,10,false,1);
UPDATE content.boards SET posting_reply_seconds=17,user_thread_limit=9 WHERE slug='a';
BEGIN;
SELECT set_config('board.posting_actor',repeat('11',32),true);
INSERT INTO content.threads(id,board) VALUES(9500001,'autoid'),(9500002,'autoid');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(9500001,'autoid',9500001,'Synthetic','','Legacy registered OP'),
 (9500002,'autoid',9500002,'Synthetic','','Legacy unregistered OP'),
 (9500003,'autoid',9500001,'Synthetic','','Legacy reply');
INSERT INTO post_secrets.deletion(post_id,password_hash)
VALUES(9500001,'owned-deletion-hash'),(9500002,'owned-deletion-hash'),(9500003,'owned-deletion-hash');
COMMIT;
SQL
cat > "$cluster/legacy-registration.sql" <<'SQL'
BEGIN;
SELECT content.register_anonymous_post(decode(repeat('01',32),'hex'),decode(repeat('02',32),'hex'),
 decode(repeat('03',32),'hex'),decode(repeat('04',32),'hex'),true,'autoid',9500001,extract(epoch FROM clock_timestamp())::bigint);
SELECT content.admit_report('autoid',9500001,'Legacy registered report',decode(repeat('21',32),'hex')) AS report_id \gset
SELECT content.register_anonymous_report(decode(repeat('01',32),'hex'),decode(repeat('02',32),'hex'),
 decode(repeat('03',32),'hex'),decode(repeat('04',32),'hex'),false,'autoid',:report_id,extract(epoch FROM clock_timestamp())::bigint);
SELECT content.admit_report('autoid',9500002,'Legacy unregistered report',decode(repeat('22',32),'hex'));
COMMIT;
SQL
# Extract the shared readiness body, not a second independently maintained SQL
# approximation. Only postgres mutates catalogs; both real runtime identities
# evaluate it without any new private-schema USAGE grant.
python3 - "$cluster/readiness.sql" <<'PYREADINESS'
import pathlib, re, sys
source = pathlib.Path("crates/store/src/automatic_admission.rs").read_text()
queries = re.findall(r'pub const READINESS_SQL: &str = r#"(.*?)"#;', source, re.S)
assert len(queries) == 1
query = queries[0]
changes = [
    "DROP FUNCTION content.check_user_thread_quota(bytea,text,bigint,bytea,boolean,bigint)",
    "REVOKE EXECUTE ON FUNCTION post_secrets.resolve_automatic_identity(bytea,boolean,bigint,boolean) FROM board_posting_cooldown_owner",
    "REVOKE EXECUTE ON FUNCTION post_secrets.lookup_automatic_identity(bytea,bigint) FROM board_report_admission_owner",
    "GRANT SELECT(automatic_identity) ON post_secrets.anonymous_sessions TO board_public",
    "GRANT UPDATE(automatic_identity) ON post_secrets.posting_history TO board_public",
    "GRANT SELECT(automatic_identity) ON post_secrets.report_membership TO board_staff",
    "GRANT UPDATE(registration_xid) ON post_secrets.report_membership TO board_anonymous_owner",
    "ALTER TABLE post_secrets.posting_history ALTER COLUMN registration_xid DROP DEFAULT",
    "ALTER TABLE post_secrets.report_membership ALTER COLUMN registration_xid SET DEFAULT '1'::xid8",
    "ALTER TABLE post_secrets.anonymous_sessions ALTER COLUMN automatic_identity SET DEFAULT gen_random_uuid()",
    "ALTER TABLE post_secrets.anonymous_sessions ALTER COLUMN automatic_identity SET NOT NULL",
    "ALTER TABLE post_secrets.anonymous_sessions DROP CONSTRAINT anonymous_sessions_automatic_identity_key",
    "ALTER FUNCTION post_secrets.lookup_automatic_identity(bytea,bigint) SECURITY INVOKER",
    "ALTER FUNCTION post_secrets.resolve_automatic_identity(bytea,boolean,bigint,boolean) SET search_path=public",
    "ALTER ROLE board_anonymous_owner LOGIN",
    "ALTER ROLE board_posting_cooldown_owner BYPASSRLS",
    "ALTER ROLE board_report_admission_owner CREATEROLE",
]
with open(sys.argv[1], "w") as output:
    for change, expected in [("", True), *[(item, False) for item in changes]]:
        output.write("BEGIN;\n")
        if change:
            output.write(change + ";\n")
        for role in ("board_public", "board_staff"):
            output.write(f"SET LOCAL ROLE {role};\n")
            output.write("DO $readiness$ BEGIN IF (" + query + ") IS DISTINCT FROM " + str(expected).lower()
                + " THEN RAISE EXCEPTION 'Automatic identity readiness drift contract failed'; END IF; END $readiness$;\n")
            output.write("RESET ROLE;\n")
        output.write("ROLLBACK;\n")
PYREADINESS
for mode in upgrade fresh; do
 database="automatic_identity_$mode"
 create_database "$database"
 migrator=("${psql[@]}" -U board_migrator -d "$database")
 admin=(runuser -u postgres -- "${psql[@]}" -d "$database")
 for migration in migrations/*.sql; do
  [[ $migration != migrations/0095* ]] || break
  "${migrator[@]}" --single-transaction -f "$migration"
 done
 if [[ $mode = upgrade ]]; then
  "${migrator[@]}" -f - < "$cluster/fixtures.sql"
  "${psql[@]}" -U board_public -d "$database" -f - < "$cluster/legacy-registration.sql"
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
CREATE FUNCTION public.capture_legacy_identity_rows() RETURNS TABLE(relation text,value jsonb)
LANGUAGE sql AS $$ SELECT relation,CASE relation
 WHEN 'post_secrets.anonymous_sessions' THEN value-'automatic_identity'
 WHEN 'post_secrets.posting_history' THEN value-'automatic_identity'-'registration_xid'
 WHEN 'post_secrets.report_membership' THEN value-'automatic_identity'-'registration_xid'
 ELSE value END FROM public.capture_report_rows() $$;
INSERT INTO public.report_rows_before SELECT * FROM public.capture_legacy_identity_rows();
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
 "${migrator[@]}" --single-transaction -f migrations/0095_automatic_admission_identity.sql
 "${migrator[@]}" --single-transaction -f migrations/0096_automatic_thread_quota.sql
 "${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(TABLE public.report_rows_before EXCEPT ALL SELECT * FROM public.capture_legacy_identity_rows())
 OR EXISTS(SELECT * FROM public.capture_legacy_identity_rows() EXCEPT ALL TABLE public.report_rows_before)
 THEN RAISE EXCEPTION 'Upgrade changed legacy rows'; END IF;
 IF EXISTS(SELECT 1 FROM post_secrets.anonymous_sessions WHERE automatic_identity IS NOT NULL)
 OR EXISTS(SELECT 1 FROM post_secrets.posting_history WHERE automatic_identity IS NOT NULL OR registration_xid IS NOT NULL)
 OR EXISTS(SELECT 1 FROM post_secrets.report_membership WHERE automatic_identity IS NOT NULL OR registration_xid IS NOT NULL)
 THEN RAISE EXCEPTION 'Upgrade backfilled legacy identity or provenance'; END IF;
 IF EXISTS(TABLE public.report_columns_before EXCEPT TABLE public.report_columns)
 OR EXISTS(SELECT * FROM public.report_columns WHERE NOT (
  relation='post_secrets.anonymous_sessions' AND attname='automatic_identity'
  OR relation IN('post_secrets.posting_history','post_secrets.report_membership')
   AND attname IN('automatic_identity','registration_xid')) EXCEPT TABLE public.report_columns_before)
 THEN RAISE EXCEPTION 'Unrelated column change'; END IF;
 IF EXISTS(TABLE public.report_constraints_before EXCEPT TABLE public.report_constraints)
 OR EXISTS(SELECT * FROM public.report_constraints WHERE NOT (
  relation='post_secrets.anonymous_sessions' AND conname='anonymous_sessions_automatic_identity_key')
 EXCEPT TABLE public.report_constraints_before)
 THEN RAISE EXCEPTION 'Unrelated constraint change'; END IF;
 IF EXISTS(TABLE public.report_indexes_before EXCEPT TABLE public.report_indexes)
 OR EXISTS(SELECT * FROM public.report_indexes WHERE NOT (schemaname='post_secrets' AND
  (tablename='anonymous_sessions' AND indexname='anonymous_sessions_automatic_identity_key'
   OR tablename='posting_history' AND indexname='posting_history_automatic_op'
   OR tablename='report_membership' AND indexname IN('report_membership_automatic_target','report_membership_automatic_time')))
 EXCEPT TABLE public.report_indexes_before)
 THEN RAISE EXCEPTION 'Unrelated index change'; END IF;
END $$;
-- Exactly two registration bodies change. Their owner and ACL must not change.
CREATE VIEW public.identity_preserved_authority AS
SELECT kind,object,CASE WHEN kind='function' AND object IN(
 'content.register_anonymous_post(bytea,bytea,bytea,bytea,boolean,text,bigint,bigint)',
 'content.register_anonymous_report(bytea,bytea,bytea,bytea,boolean,text,bigint,bigint)')
 THEN value-2 ELSE value END value FROM public.report_authority;
CREATE VIEW public.identity_old_authority AS
SELECT kind,object,CASE WHEN kind='function' AND object IN(
 'content.register_anonymous_post(bytea,bytea,bytea,bytea,boolean,text,bigint,bigint)',
 'content.register_anonymous_report(bytea,bytea,bytea,bytea,boolean,text,bigint,bigint)')
 THEN value-2 ELSE value END value FROM public.report_authority_before;
DO $$ BEGIN
 IF EXISTS(TABLE public.identity_old_authority EXCEPT TABLE public.identity_preserved_authority)
 OR EXISTS(SELECT * FROM public.identity_preserved_authority WHERE NOT (
  kind='function' AND object IN('post_secrets.resolve_automatic_identity(bytea,boolean,bigint,boolean)',
   'post_secrets.lookup_automatic_identity(bytea,bigint)',
   'content.check_user_thread_quota(bytea,text,bigint,bytea,boolean,bigint)')
  OR kind='relation' AND object IN('post_secrets.anonymous_sessions_automatic_identity_key',
   'post_secrets.posting_history_automatic_op','post_secrets.report_membership_automatic_target',
   'post_secrets.report_membership_automatic_time')
  OR kind='column grant' AND value->>1='board_anonymous_owner' AND value->>3='false' AND (
   object IN('post_secrets.posting_history.post_id','post_secrets.posting_history.board',
    'post_secrets.posting_history.thread_id','post_secrets.posting_history.registration_xid',
    'post_secrets.report_membership.report_id','post_secrets.report_membership.board',
    'post_secrets.report_membership.post_id','post_secrets.report_membership.registration_xid') AND value->>2='SELECT'
   OR object IN('post_secrets.posting_history.automatic_identity','post_secrets.report_membership.automatic_identity')
    AND value->>2 IN('SELECT','UPDATE')))
 EXCEPT TABLE public.identity_old_authority)
 THEN RAISE EXCEPTION 'Upgrade changed unrelated owner, ACL, role, RLS, trigger or function'; END IF;
END $$;
DROP VIEW public.identity_old_authority,public.identity_preserved_authority;
DROP TABLE public.report_rows_before,public.report_authority_before,public.report_columns_before,
 public.report_constraints_before,public.report_indexes_before;
SQL
 if [[ $mode = fresh ]]; then
  "${migrator[@]}" -f - < "$cluster/fixtures.sql"
 fi
 # A real public connection registers only newly inserted history. The fixture
 # deletion hash is an ordinary deletion secret, never a fabricated source
 # password or Pass identity. No UUID is supplied by the client.
 "${psql[@]}" -U board_public -d "$database" -v minted="$([[ $mode = fresh ]] && echo true || echo false)" <<'SQL'
BEGIN;
SELECT content.lock_posting_actor(decode(repeat('31',32),'hex'),true);
SELECT set_config('board.posting_actor',repeat('31',32),true);
INSERT INTO content.threads(id,board) VALUES(9500010,'autoid');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(9500010,'autoid',9500010,'Synthetic','','New registered OP');
INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES(9500010,'owned-deletion-hash');
SELECT content.register_anonymous_post(decode(repeat('01',32),'hex'),decode(repeat('02',32),'hex'),
 decode(repeat('03',32),'hex'),decode(repeat('04',32),'hex'),:'minted'::boolean,'autoid',9500010,extract(epoch FROM clock_timestamp())::bigint);
SELECT content.admit_report('autoid',9500003,'New registered report',decode(repeat('32',32),'hex')) AS report_id \gset
SELECT content.register_anonymous_report(decode(repeat('01',32),'hex'),decode(repeat('02',32),'hex'),
 decode(repeat('03',32),'hex'),decode(repeat('04',32),'hex'),false,'autoid',:report_id,extract(epoch FROM clock_timestamp())::bigint);
SELECT content.admit_report('autoid',9500002,'Current unregistered report',decode(repeat('33',32),'hex'));
COMMIT;
SQL
 # Age only this synthetic session, so a restored test cannot race its creation
 # second. Authority and full-row restore fingerprints capture the exact state.
 "${admin[@]}" <<'SQL'
UPDATE post_secrets.anonymous_sessions SET created_at=extract(epoch FROM clock_timestamp())::bigint-60,
 activity_at=extract(epoch FROM clock_timestamp())::bigint-1;
SQL
 for phase in live restored; do
  if [[ $phase = restored ]]; then
   runuser -u postgres -- "$pg_bin/pg_dump" -h "$cluster" -d "$database" --format=custom --file="$cluster/current.dump"
   create_database "${database}_restore"
   runuser -u postgres -- "$pg_bin/pg_restore" -h "$cluster" --dbname="${database}_restore" --single-transaction --exit-on-error < "$cluster/current.dump"
   database="${database}_restore"
   admin=(runuser -u postgres -- "${psql[@]}" -d "$database")
  fi
  "${admin[@]}" <<'SQL'
DO $$ DECLARE r text; rel text; f text; identity uuid; BEGIN
 SELECT automatic_identity INTO STRICT identity FROM post_secrets.anonymous_sessions WHERE token_hash=decode(repeat('01',32),'hex');
 IF (SELECT count(*) FROM pg_index i JOIN pg_class c ON c.oid=i.indexrelid
  JOIN pg_namespace ns ON ns.oid=c.relnamespace WHERE ns.nspname='post_secrets' AND i.indisvalid AND i.indisready
  AND c.relname IN('anonymous_sessions_automatic_identity_key','posting_history_automatic_op',
   'report_membership_automatic_target','report_membership_automatic_time'))<>4
 THEN RAISE EXCEPTION 'Private automatic indexes missing or invalid'; END IF;
 IF identity IS NULL OR NOT EXISTS(SELECT 1 FROM post_secrets.posting_history WHERE post_id=9500010 AND automatic_identity=identity AND registration_xid IS NOT NULL)
 OR NOT EXISTS(SELECT 1 FROM post_secrets.report_membership m JOIN content.reports r ON r.id=m.report_id
  WHERE r.reason='New registered report' AND m.automatic_identity=identity AND m.registration_xid IS NOT NULL)
 OR EXISTS(SELECT 1 FROM post_secrets.posting_history WHERE post_id IN(9500001,9500002,9500003) AND automatic_identity IS NOT NULL)
 OR EXISTS(SELECT 1 FROM post_secrets.report_membership m JOIN content.reports r ON r.id=m.report_id
  WHERE r.reason LIKE 'Legacy%' AND m.automatic_identity IS NOT NULL)
 THEN RAISE EXCEPTION 'New-only identity capture did not survive'; END IF;
 FOREACH rel IN ARRAY ARRAY['post_secrets.posting_history','post_secrets.report_membership'] LOOP
  IF NOT EXISTS(SELECT 1 FROM pg_attribute a JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
   WHERE a.attrelid=rel::regclass AND a.attname='registration_xid' AND a.atttypid='xid8'::regtype
    AND NOT a.attnotnull AND pg_get_expr(d.adbin,d.adrelid)='pg_current_xact_id()')
  THEN RAISE EXCEPTION 'Registration provenance default changed'; END IF;
 END LOOP;
 FOREACH r IN ARRAY ARRAY['board_public','board_staff','board_auth','board_media','board_media_read','board_media_intake','board_monitor'] LOOP
  FOREACH rel IN ARRAY ARRAY['post_secrets.anonymous_sessions','post_secrets.posting_history','post_secrets.report_membership'] LOOP
   IF has_column_privilege(r,rel,'automatic_identity','SELECT,INSERT,UPDATE')
   THEN RAISE EXCEPTION 'Runtime identity column exposed'; END IF;
  END LOOP;
  IF has_function_privilege(r,'post_secrets.resolve_automatic_identity(bytea,boolean,bigint,boolean)','EXECUTE')
   OR has_function_privilege(r,'post_secrets.lookup_automatic_identity(bytea,bigint)','EXECUTE')
  THEN RAISE EXCEPTION 'Private helper exposed'; END IF;
 END LOOP;
 FOREACH f IN ARRAY ARRAY['post_secrets.resolve_automatic_identity(bytea,boolean,bigint,boolean)',
  'post_secrets.lookup_automatic_identity(bytea,bigint)',
  'content.register_anonymous_post(bytea,bytea,bytea,bytea,boolean,text,bigint,bigint)',
  'content.register_anonymous_report(bytea,bytea,bytea,bytea,boolean,text,bigint,bigint)',
  'content.check_user_thread_quota(bytea,text,bigint,bytea,boolean,bigint)'] LOOP
  IF NOT EXISTS(SELECT 1 FROM pg_proc WHERE oid=f::regprocedure AND prosecdef
   AND proconfig @> ARRAY['search_path=pg_catalog, pg_temp']
   AND proowner=CASE WHEN f LIKE 'content.check_user_thread_quota%' THEN 'board_posting_cooldown_owner'::regrole ELSE 'board_anonymous_owner'::regrole END)
  THEN RAISE EXCEPTION 'Unsafe identity function owner or search path'; END IF;
 END LOOP;
 IF EXISTS(SELECT 1 FROM content.anonymous_session(decode(repeat('01',32),'hex')) s WHERE to_jsonb(s)?'automatic_identity')
 THEN RAISE EXCEPTION 'Public snapshot exposes private identity'; END IF;
END $$;
SQL
  "${psql[@]}" -U board_public -d "$database" <<'SQL'
BEGIN;
DO $$ DECLARE n bigint:=extract(epoch FROM clock_timestamp())::bigint; rejected boolean; BEGIN
 -- Different IP finds exactly the newly captured automatic OP.
 SELECT q.rejected INTO rejected FROM content.check_user_thread_quota(decode(repeat('40',32),'hex'),'autoid',n,decode(repeat('01',32),'hex'),false,n) q;
 IF rejected IS DISTINCT FROM true THEN RAISE EXCEPTION 'Automatic OR branch missing'; END IF;
 -- Unknown newly minted capability has no automatic history; IP remains active.
 SELECT q.rejected INTO rejected FROM content.check_user_thread_quota(decode(repeat('31',32),'hex'),'autoid',n,decode(repeat('41',32),'hex'),true,n) q;
 IF rejected IS DISTINCT FROM true THEN RAISE EXCEPTION 'New capability bypassed IP quota'; END IF;
 SELECT q.rejected INTO rejected FROM content.check_user_thread_quota(decode(repeat('40',32),'hex'),'autoid',n,decode(repeat('41',32),'hex'),true,n) q;
 IF rejected IS DISTINCT FROM false THEN RAISE EXCEPTION 'New capability invented equality'; END IF;
 BEGIN
  PERFORM content.register_anonymous_post(decode(repeat('01',32),'hex'),decode(repeat('02',32),'hex'),decode(repeat('03',32),'hex'),decode(repeat('04',32),'hex'),false,'autoid',9500002,n);
  RAISE EXCEPTION 'Old post stamping allowed' USING ERRCODE='ZX001';
 EXCEPTION WHEN check_violation THEN NULL; END;
 BEGIN
  PERFORM automatic_identity FROM post_secrets.anonymous_sessions;
  RAISE EXCEPTION 'Private identity readable' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
ROLLBACK;
SQL
  # Public-role calls under administrator-controlled rollback-only states cover
  # OR deduplication, source-new gates, provenance and all-or-nothing failures.
  "${admin[@]}" <<'SQL'
BEGIN;
CREATE TEMP TABLE identity_before AS SELECT * FROM public.capture_report_rows();
UPDATE content.boards SET user_thread_limit=2 WHERE slug='autoid';
SET LOCAL ROLE board_public;
DO $$ DECLARE n bigint:=extract(epoch FROM clock_timestamp())::bigint; BEGIN
 IF (SELECT rejected FROM content.check_user_thread_quota(decode(repeat('31',32),'hex'),'autoid',n,decode(repeat('01',32),'hex'),false,n))
 THEN RAISE EXCEPTION 'OR branch double counted one OP'; END IF;
END $$;
RESET ROLE;
UPDATE content.boards SET user_thread_limit=1 WHERE slug='autoid';
UPDATE post_secrets.anonymous_sessions SET created_at=extract(epoch FROM clock_timestamp())::bigint;
SELECT extract(epoch FROM clock_timestamp())::bigint AS same_second \gset
UPDATE post_secrets.anonymous_sessions SET created_at=:same_second;
SET LOCAL ROLE board_public;
SELECT set_config('test.same_second',:'same_second',true);
DO $$ DECLARE n bigint:=current_setting('test.same_second')::bigint; BEGIN
 IF (SELECT rejected FROM content.check_user_thread_quota(decode(repeat('40',32),'hex'),'autoid',n,decode(repeat('01',32),'hex'),false,n))
 THEN RAISE EXCEPTION 'Same-second source-new identity branch was counted'; END IF;
 IF NOT (SELECT rejected FROM content.check_user_thread_quota(decode(repeat('31',32),'hex'),'autoid',n,decode(repeat('01',32),'hex'),false,n))
 THEN RAISE EXCEPTION 'Same-second source-new bypassed IP branch'; END IF;
END $$;
RESET ROLE;
UPDATE post_secrets.anonymous_sessions SET created_at=extract(epoch FROM clock_timestamp())::bigint-700000,
 activity_at=extract(epoch FROM clock_timestamp())::bigint-604801;
SET LOCAL ROLE board_public;
DO $$ DECLARE n bigint:=extract(epoch FROM clock_timestamp())::bigint; BEGIN
 IF (SELECT rejected FROM content.check_user_thread_quota(decode(repeat('40',32),'hex'),'autoid',n,decode(repeat('01',32),'hex'),false,n))
 THEN RAISE EXCEPTION 'Idle source-new identity branch was counted'; END IF;
 IF NOT (SELECT rejected FROM content.check_user_thread_quota(decode(repeat('31',32),'hex'),'autoid',n,decode(repeat('01',32),'hex'),false,n))
 THEN RAISE EXCEPTION 'Idle source-new bypassed IP branch'; END IF;
END $$;
RESET ROLE;
ROLLBACK;
SQL
  # The old report exists only on the actual upgrade path.
  if [[ $mode = upgrade ]]; then
   "${admin[@]}" <<'SQL'
BEGIN;
SELECT id AS old_report FROM content.reports WHERE reason='Legacy unregistered report' \gset
SET LOCAL ROLE board_public;
SELECT set_config('test.old_report',:'old_report',true);
DO $$ BEGIN
 BEGIN
  PERFORM content.register_anonymous_report(decode(repeat('01',32),'hex'),decode(repeat('02',32),'hex'),decode(repeat('03',32),'hex'),decode(repeat('04',32),'hex'),false,'autoid',current_setting('test.old_report')::bigint,extract(epoch FROM clock_timestamp())::bigint);
  RAISE EXCEPTION 'Old report stamping allowed' USING ERRCODE='ZX001';
 EXCEPTION WHEN check_violation THEN NULL; END;
END $$;
ROLLBACK;
SQL
  fi
  "${admin[@]}" <<'SQL'
BEGIN;
CREATE TEMP TABLE identity_before AS SELECT * FROM public.capture_report_rows();
SELECT id AS current_report FROM content.reports WHERE reason='Current unregistered report' \gset
SET LOCAL ROLE board_public;
SELECT set_config('test.current_report',:'current_report',true);
DO $$ BEGIN
 BEGIN
  PERFORM content.register_anonymous_report(decode(repeat('01',32),'hex'),decode(repeat('02',32),'hex'),decode(repeat('03',32),'hex'),decode(repeat('04',32),'hex'),false,'autoid',current_setting('test.current_report')::bigint,extract(epoch FROM clock_timestamp())::bigint);
  RAISE EXCEPTION 'Committed current report stamping allowed' USING ERRCODE='ZX001';
 EXCEPTION WHEN check_violation THEN NULL; END;
END $$;
DO $$ DECLARE n bigint:=extract(epoch FROM clock_timestamp())::bigint; r bigint; BEGIN
 BEGIN
  PERFORM content.lock_posting_actor(decode(repeat('51',32),'hex'),true);
  PERFORM set_config('board.posting_actor',repeat('51',32),true);
  INSERT INTO content.threads(id,board) VALUES(9500090,'autoid');
  INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES(9500090,'autoid',9500090,'Synthetic','','Rollback activity');
  INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES(9500090,'owned-deletion-hash');
  PERFORM content.register_anonymous_post(decode(repeat('01',32),'hex'),decode(repeat('02',32),'hex'),decode(repeat('03',32),'hex'),decode(repeat('04',32),'hex'),false,'autoid',9500090,n);
  RAISE EXCEPTION 'Synthetic downstream rejection' USING ERRCODE='ZX002';
 EXCEPTION WHEN SQLSTATE 'ZX002' THEN NULL; END;
 BEGIN
  r:=content.admit_report('autoid',9500002,'Rollback report',decode(repeat('52',32),'hex'));
  PERFORM content.register_anonymous_report(decode(repeat('01',32),'hex'),decode(repeat('02',32),'hex'),decode(repeat('03',32),'hex'),decode(repeat('04',32),'hex'),false,'autoid',r,n);
  RAISE EXCEPTION 'Synthetic downstream rejection' USING ERRCODE='ZX002';
 EXCEPTION WHEN SQLSTATE 'ZX002' THEN NULL; END;
 BEGIN
  PERFORM content.lock_posting_actor(decode(repeat('53',32),'hex'),true);
  PERFORM set_config('board.posting_actor',repeat('53',32),true);
  INSERT INTO content.threads(id,board) VALUES(9500091,'autoid');
  INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES(9500091,'autoid',9500091,'Synthetic','','Reject expired context');
  INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES(9500091,'owned-deletion-hash');
  PERFORM content.register_anonymous_post(decode(repeat('54',32),'hex'),decode(repeat('02',32),'hex'),decode(repeat('03',32),'hex'),decode(repeat('04',32),'hex'),false,'autoid',9500091,n);
  RAISE EXCEPTION 'Unknown retained capability accepted' USING ERRCODE='ZX001';
 EXCEPTION WHEN invalid_authorization_specification THEN NULL; END;
END $$;
RESET ROLE;
DO $$ BEGIN
 IF EXISTS(TABLE identity_before EXCEPT ALL SELECT * FROM public.capture_report_rows())
 OR EXISTS(SELECT * FROM public.capture_report_rows() EXCEPT ALL TABLE identity_before)
 THEN RAISE EXCEPTION 'Rejected transaction retained history, proof, identity or session activity'; END IF;
END $$;
DELETE FROM post_secrets.anonymous_sessions;
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM post_secrets.posting_history WHERE post_id=9500010 AND automatic_identity IS NOT NULL)
 OR NOT EXISTS(SELECT 1 FROM post_secrets.report_membership m JOIN content.reports r ON r.id=m.report_id
  WHERE r.reason='New registered report' AND m.automatic_identity IS NOT NULL)
 THEN RAISE EXCEPTION 'Session cleanup destroyed captured equality'; END IF;
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
  printf 'Automatic-identity current dump/restore fingerprint mismatch (%s).\n' "$mode" >&3; exit 1;
 }
 printf '%s automatic-identity migration and current dump/restore passed.\n' "$mode" >&3
done
printf 'Legacy rows, new-only private identity, provenance rollback and public OP quota qualified.\n' >&3
