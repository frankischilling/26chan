#!/usr/bin/env bash
# Owned synthetic clusters only. Bound qualification to populated 0108 -> 0109.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-poll-read.XXXXXXXX)
exec 3>&1
started=0
cleanup() {
    status=$?
    trap - EXIT
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null || status=1
    fi
    [[ $cluster =~ ^/tmp/board-poll-read\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
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
trap 'printf "Poll-read qualification failed at line %s (private diagnostics removed during cleanup).\n" "$LINENO" >&3' ERR
# The parent shell opens every private file before dropping privileges.
runuser -u postgres -- "${psql[@]}" -d postgres -f - < deploy/roles.sql
runuser -u postgres -- "${psql[@]}" -d postgres -c 'ALTER ROLE board_staff LOGIN; ALTER ROLE board_auth LOGIN'
create_database() {
    runuser -u postgres -- "${psql[@]}" -d postgres -v database="$1" <<'SQL'
CREATE DATABASE :"database" OWNER board_migrator;
REVOKE ALL ON DATABASE :"database" FROM PUBLIC;
GRANT CONNECT ON DATABASE :"database" TO board_migrator,board_public,board_staff,board_auth;
SQL
}
python3 - "$cluster" <<'PY'
import pathlib,re,sys
match=re.search(r'pub const POLL_READINESS_SQL: &str = r#"(.*?)"#;',pathlib.Path('crates/store/src/polls.rs').read_text(),re.S)
if not match: raise SystemExit('Cannot extract current poll readiness')
pathlib.Path(sys.argv[1],'readiness.sql').write_text(
    "DO $check$ BEGIN IF ("+match.group(1)+") IS DISTINCT FROM true THEN RAISE EXCEPTION 'Current poll readiness failed'; END IF; END $check$;\n")
PY
cat > "$cluster/capture.sql" <<'SQL'
CREATE FUNCTION public.capture_rows() RETURNS TABLE(relation text,value jsonb)
LANGUAGE plpgsql AS $$ DECLARE r record; BEGIN
 FOR r IN SELECT n.nspname,c.relname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission','poll_private') AND c.relkind='r' LOOP
  RETURN QUERY EXECUTE format('SELECT %L,to_jsonb(r) FROM %I.%I r',r.nspname||'.'||r.relname,r.nspname,r.relname);
 END LOOP;
END $$;
REVOKE ALL ON FUNCTION public.capture_rows() FROM PUBLIC;
-- ACLs are compared semantically, independent of pg_dump's array ordering.
CREATE VIEW public.capture_metadata AS
SELECT n.nspname||'.'||c.relname object,'relation'::text kind,
 jsonb_build_array(c.relkind,pg_get_userbyid(c.relowner),c.reloptions,c.relrowsecurity,c.relforcerowsecurity,
 (SELECT jsonb_agg(jsonb_build_array(pg_get_userbyid(a.grantor),
 CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
 ORDER BY a.grantor,a.grantee,a.privilege_type,a.is_grantable)
 FROM aclexplode(coalesce(c.relacl,acldefault(CASE WHEN c.relkind='S' THEN 's'::"char" ELSE 'r'::"char" END,c.relowner))) a),
 CASE WHEN c.relkind='v' THEN pg_get_viewdef(c.oid,true) END) value
FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
WHERE n.nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission','poll_private')
UNION ALL
SELECT n.nspname||'.'||c.relname,'column',jsonb_build_array(a.attname,format_type(a.atttypid,a.atttypmod),
 a.attnotnull,a.attidentity,a.attgenerated,pg_get_expr(d.adbin,d.adrelid),
 (SELECT jsonb_agg(jsonb_build_array(pg_get_userbyid(g.grantor),CASE WHEN g.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(g.grantee) END,g.privilege_type,g.is_grantable)
 ORDER BY g.grantor,g.grantee,g.privilege_type,g.is_grantable) FROM aclexplode(a.attacl) g))
FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace
LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
WHERE a.attnum>0 AND NOT a.attisdropped
AND n.nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission','poll_private')
UNION ALL
SELECT c.conrelid::regclass::text,'constraint',jsonb_build_array(c.conname,pg_get_constraintdef(c.oid,true))
FROM pg_constraint c JOIN pg_namespace n ON n.oid=c.connamespace
WHERE n.nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission','poll_private')
UNION ALL
SELECT n.nspname||'.'||c.relname,'index',to_jsonb(pg_get_indexdef(c.oid))
FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE c.relkind='i'
AND n.nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission','poll_private')
UNION ALL
SELECT t.tgrelid::regclass::text,'trigger',jsonb_build_array(t.tgname,t.tgenabled,pg_get_triggerdef(t.oid))
FROM pg_trigger t JOIN pg_class c ON c.oid=t.tgrelid JOIN pg_namespace n ON n.oid=c.relnamespace
WHERE NOT t.tgisinternal AND n.nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission','poll_private')
UNION ALL
SELECT schemaname||'.'||tablename,'policy',to_jsonb(p) FROM pg_policies p
WHERE schemaname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission','poll_private')
UNION ALL
SELECT n.nspname||'.'||p.proname,'function',jsonb_build_array(pg_get_functiondef(p.oid),pg_get_userbyid(p.proowner),
 (SELECT jsonb_agg(jsonb_build_array(pg_get_userbyid(a.grantor),CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
 ORDER BY a.grantor,a.grantee,a.privilege_type,a.is_grantable) FROM aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a))
FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
WHERE n.nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission','poll_private')
UNION ALL
SELECT n.nspname,'schema',jsonb_build_array(pg_get_userbyid(n.nspowner),
 (SELECT jsonb_agg(jsonb_build_array(pg_get_userbyid(a.grantor),CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
 ORDER BY a.grantor,a.grantee,a.privilege_type,a.is_grantable) FROM aclexplode(coalesce(n.nspacl,acldefault('n',n.nspowner))) a))
FROM pg_namespace n WHERE n.nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission','poll_private');
REVOKE ALL ON public.capture_metadata FROM PUBLIC;
SQL
cat > "$cluster/polls.sql" <<'SQL'
-- Explicit test-only fixtures, inserted after 0109 under the configuration owner.
INSERT INTO poll_private.polls(id,title,description,vote_count,published,catalogue_ordinal) VALUES
 (10900001,'Listed second','Synthetic description',7,true,2),
 (10900002,'Listed first','',0,true,1),
 (10900003,'Unpublished sentinel','Hidden description',99,false,3),
 (10900004,'Unlisted detail','Published but not in catalogue',3,true,NULL);
INSERT INTO poll_private.options(poll_id,id,ordinal,caption,score) VALUES
 (10900001,9,2,'Second',NULL),(10900001,20,1,'First',9),
 (10900002,1,1,'Zero',0),(10900003,1,1,'Hidden option',99),
 (10900004,1,1,'Unlisted option',3);
-- Bounds are safety choices, not recovered voting semantics or source seeds.
DO $$ DECLARE patch jsonb; BEGIN
 FOR patch IN SELECT value FROM jsonb_array_elements(
  '[{"id":0},{"vote_count":-1},{"vote_count":1000000001},{"catalogue_ordinal":0},{"catalogue_ordinal":201}]'::jsonb
  ||jsonb_build_array(jsonb_build_object('title',repeat('x',513)),jsonb_build_object('description',repeat('x',16385)))) LOOP
  BEGIN
   INSERT INTO poll_private.polls SELECT * FROM jsonb_populate_record(NULL::poll_private.polls,
    '{"id":10909999,"title":"Bound fixture","description":"","vote_count":0,"published":false,"catalogue_ordinal":200}'::jsonb||patch);
   RAISE EXCEPTION 'Invalid poll bound accepted' USING ERRCODE='ZX001';
  EXCEPTION WHEN check_violation THEN NULL; END;
 END LOOP;
 FOR patch IN SELECT value FROM jsonb_array_elements(
  '[{"id":0},{"ordinal":0},{"ordinal":129},{"score":-1},{"score":1000000001}]'::jsonb
  ||jsonb_build_array(jsonb_build_object('caption',repeat('x',1025)))) LOOP
  BEGIN
   INSERT INTO poll_private.options SELECT * FROM jsonb_populate_record(NULL::poll_private.options,
    '{"poll_id":10900001,"id":999,"ordinal":128,"caption":"Bound fixture","score":null}'::jsonb||patch);
   RAISE EXCEPTION 'Invalid option bound accepted' USING ERRCODE='ZX001';
  EXCEPTION WHEN check_violation THEN NULL; END;
 END LOOP;
END $$;
SQL
cat > "$cluster/runtime.sql" <<'SQL'
DO $$ DECLARE relation_name text; relation_oid oid; statement text; BEGIN
 FOREACH relation_name IN ARRAY ARRAY['poll_private.polls','poll_private.options','content.published_polls','content.published_poll_options'] LOOP
  -- Catalog lookup avoids requiring USAGE merely to resolve private names.
  SELECT c.oid INTO relation_oid FROM pg_catalog.pg_class c
  JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace
  WHERE n.nspname||'.'||c.relname=relation_name;
  IF relation_oid IS NULL
  OR pg_catalog.has_table_privilege(current_user,relation_oid,'INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER') IS DISTINCT FROM false
  OR pg_catalog.has_any_column_privilege(current_user,relation_oid,'INSERT,UPDATE,REFERENCES') IS DISTINCT FROM false
  THEN RAISE EXCEPTION 'Runtime poll write authority present or relation missing: %',relation_name; END IF;
  IF relation_name LIKE 'poll_private.%' OR current_user<>'board_public' THEN
   BEGIN
    EXECUTE 'SELECT * FROM '||relation_name||' LIMIT 1';
    RAISE EXCEPTION 'Runtime private poll read accepted' USING ERRCODE='ZX001';
   EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  END IF;
  FOREACH statement IN ARRAY ARRAY[
   'INSERT INTO '||relation_name||' DEFAULT VALUES',
   'UPDATE '||relation_name||' SET id=id WHERE false',
   'DELETE FROM '||relation_name||' WHERE false'] LOOP
   BEGIN
    EXECUTE statement;
    RAISE EXCEPTION 'Runtime poll write accepted' USING ERRCODE='ZX001';
   EXCEPTION
    WHEN insufficient_privilege THEN NULL;
    -- PostgreSQL can reject the joined view during rewrite before ACL checks.
    -- Its write ACLs were independently checked above; no other target qualifies.
    WHEN SQLSTATE '55000' THEN
     IF relation_name<>'content.published_poll_options' THEN RAISE; END IF;
   END;
  END LOOP;
 END LOOP;
END $$;
SQL
cat > "$cluster/public.sql" <<'SQL'
DO $$ BEGIN
 IF (SELECT array_agg(id ORDER BY catalogue_ordinal) FROM
  (SELECT id,catalogue_ordinal FROM content.published_polls WHERE catalogue_ordinal IS NOT NULL ORDER BY catalogue_ordinal LIMIT 201) p)
  IS DISTINCT FROM ARRAY[10900002,10900001]::bigint[]
 OR (SELECT array_agg(id ORDER BY ordinal) FROM
  (SELECT id,ordinal FROM content.published_poll_options WHERE poll_id=10900001 ORDER BY ordinal LIMIT 129) o)
  IS DISTINCT FROM ARRAY[20,9]::bigint[]
 OR (SELECT count(*) FROM content.published_polls)<>3
 OR (SELECT count(*) FROM content.published_poll_options)<>4
 OR EXISTS(SELECT 1 FROM content.published_polls WHERE id=10900003)
 OR EXISTS(SELECT 1 FROM content.published_poll_options WHERE poll_id=10900003)
 OR NOT EXISTS(SELECT 1 FROM content.published_polls WHERE id=10900004 AND catalogue_ordinal IS NULL)
 OR NOT EXISTS(SELECT 1 FROM content.published_poll_options WHERE poll_id=10900004 AND score=3)
 OR NOT EXISTS(SELECT 1 FROM content.published_poll_options WHERE poll_id=10900001 AND id=9 AND score IS NULL)
 OR NOT EXISTS(SELECT 1 FROM content.published_poll_options WHERE poll_id=10900001 AND id=20 AND score=9)
 THEN RAISE EXCEPTION 'Poll projection, explicit order, hidden filtering or nullable score changed'; END IF;
END $$;
SQL
for mode in upgrade fresh; do
 database="poll_read_$mode"
 create_database "$database"
 migrator=("${psql[@]}" -U board_migrator -d "$database")
 admin=(runuser -u postgres -- "${psql[@]}" -d "$database")
 for migration in migrations/*.sql; do
  [[ $migration < migrations/0109_poll_read_projection.sql ]] || break
  "${migrator[@]}" --single-transaction -f - < "$migration"
 done
 "${admin[@]}" -f - < "$cluster/capture.sql"
 if [[ $mode = upgrade ]]; then
  "${migrator[@]}" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('pollfix','Retained board','Synthetic unrelated rows',1000,100,100,100,10);
BEGIN;
SELECT set_config('board.posting_actor',repeat('19',32),true);
INSERT INTO content.threads(id,board) VALUES(10901001,'pollfix');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(10901001,'pollfix',10901001,'Retained name','Retained subject','Retained body');
COMMIT;
INSERT INTO content.reports(id,board,post_id,reason) OVERRIDING SYSTEM VALUE
VALUES(10901001,'pollfix',10901001,'Retained report');
INSERT INTO content.moderation_audit(account_id,board,target_id,action,created_at)
VALUES(1,'pollfix',10901001,'resolve','2020-01-01 UTC');
SQL
  # A preexisting /polls board must block 0109 atomically, never be renamed.
  "${migrator[@]}" -c "INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES('polls','Conflict fixture','Synthetic',1000,100,100,100,10)"
  "${admin[@]}" -c 'CREATE TABLE public.conflict_rows AS SELECT * FROM public.capture_rows()'
  if "${migrator[@]}" --single-transaction -f - < migrations/0109_poll_read_projection.sql; then
   echo '0109 accepted a conflicting polls board' >&2; exit 1
  fi
  "${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF to_regnamespace('poll_private') IS NOT NULL
 OR EXISTS(SELECT 1 FROM pg_constraint WHERE conrelid='content.boards'::regclass AND conname='boards_reserved_polls_route')
 OR EXISTS(TABLE public.conflict_rows EXCEPT ALL SELECT * FROM public.capture_rows())
 OR EXISTS(SELECT * FROM public.capture_rows() EXCEPT ALL TABLE public.conflict_rows)
 THEN RAISE EXCEPTION 'Conflicting upgrade changed data or retained partial migration'; END IF;
END $$;
DROP TABLE public.conflict_rows;
SQL
  "${migrator[@]}" -c "DELETE FROM content.boards WHERE slug='polls'"
 fi
 "${admin[@]}" -c 'CREATE TABLE public.before_rows AS SELECT * FROM public.capture_rows(); CREATE TABLE public.before_metadata AS TABLE public.capture_metadata'
 "${migrator[@]}" --single-transaction -f - < migrations/0109_poll_read_projection.sql
 "${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM poll_private.polls) OR EXISTS(SELECT 1 FROM poll_private.options)
 THEN RAISE EXCEPTION '0109 seeded or backfilled polls'; END IF;
 IF EXISTS(TABLE public.before_rows EXCEPT ALL SELECT * FROM public.capture_rows())
 OR EXISTS(SELECT * FROM public.capture_rows() EXCEPT ALL TABLE public.before_rows)
 THEN RAISE EXCEPTION '0109 changed unrelated rows'; END IF;
 IF EXISTS(TABLE public.before_metadata EXCEPT ALL TABLE public.capture_metadata)
 OR EXISTS(SELECT * FROM public.capture_metadata WHERE object NOT LIKE 'poll_private%'
 AND object NOT IN ('content.published_polls','content.published_poll_options')
 AND NOT (object='content.boards' AND kind='constraint' AND value->>0='boards_reserved_polls_route')
 EXCEPT ALL TABLE public.before_metadata)
 THEN RAISE EXCEPTION '0109 changed existing definitions or authority'; END IF;
END $$;
DROP TABLE public.before_rows,public.before_metadata;
SQL
 "${psql[@]}" -U board_public -d "$database" -f - < "$cluster/readiness.sql"
 "${migrator[@]}" -f - < "$cluster/polls.sql"
 "${admin[@]}" -c 'CREATE TABLE public.expected_rows AS SELECT * FROM public.capture_rows(); CREATE TABLE public.expected_metadata AS TABLE public.capture_metadata'
 for phase in live restored; do
  if [[ $phase = restored ]]; then
   runuser -u postgres -- "$pg_bin/pg_dump" -h "$cluster" -U postgres -d "$database" --format=custom > "$cluster/current.dump"
   create_database "${database}_restore"
   runuser -u postgres -- "$pg_bin/pg_restore" -h "$cluster" --dbname="${database}_restore" --single-transaction --exit-on-error < "$cluster/current.dump"
   database="${database}_restore"
   admin=(runuser -u postgres -- "${psql[@]}" -d "$database")
  fi
  for role in board_public board_staff board_auth; do
   "${psql[@]}" -U "$role" -d "$database" -f - < "$cluster/runtime.sql"
  done
  "${psql[@]}" -U board_public -d "$database" -f - < "$cluster/readiness.sql"
  "${psql[@]}" -U board_public -d "$database" -f - < "$cluster/public.sql"
  "${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(TABLE public.expected_rows EXCEPT ALL SELECT * FROM public.capture_rows())
 OR EXISTS(SELECT * FROM public.capture_rows() EXCEPT ALL TABLE public.expected_rows)
 OR EXISTS(TABLE public.expected_metadata EXCEPT ALL TABLE public.capture_metadata)
 OR EXISTS(TABLE public.capture_metadata EXCEPT ALL TABLE public.expected_metadata)
 THEN RAISE EXCEPTION 'Poll qualification/restore changed data, definitions or ACLs'; END IF;
END $$;
SQL
 done
 printf 'Poll-read %s and administrator dump/restore qualification passed.\n' "$mode" >&3
done
