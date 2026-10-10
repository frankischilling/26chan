#!/usr/bin/env bash
# Owned synthetic clusters only. Populated pre-0130 upgrade and current-schema administrator dump/restore.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-file-deletion.XXXXXXXX)
exec 3>&1
startup_attempted=0
cleanup() {
    status=$?
    trap - EXIT
    [[ $cluster =~ ^/tmp/board-file-deletion\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
    [[ $(readlink -f "$cluster") = "$cluster" ]] || exit 1
    if [[ $startup_attempted = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null 2>&1 || status=1
        # pg_ctl status returns 3 only for a stopped server. Any other result,
        # including an inaccessible directory, must preserve the owned files.
        if runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" status > /dev/null 2>&1; then
            server_status=0
        else
            server_status=$?
        fi
        if [[ $server_status != 3 ]]; then
            printf 'File-deletion cluster stop is unconfirmed; preserved private cluster and diagnostics at %s.\n' "$cluster" >&3
            exit 1
        fi
    fi
    rm -rf -- "$cluster"
    exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
chown postgres:postgres "$cluster"
runuser -u postgres -- "$pg_bin/initdb" -D "$cluster/data" --auth=trust --encoding=UTF8 --no-locale > /dev/null
startup_attempted=1
runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" \
    -o "-c listen_addresses='' -c unix_socket_directories='$cluster' -c statement_timeout=30000 -c lock_timeout=5000" -w start > /dev/null
psql=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h "$cluster")
exec > "$cluster/qualification.log" 2>&1
trap 'printf "File-deletion qualification failed at line %s (private diagnostics are not printed).\n" "$LINENO" >&3' ERR
# The shell opens private root-owned fixture files before dropping privileges.
runuser -u postgres -- "${psql[@]}" -d postgres -f - < deploy/roles.sql
runuser -u postgres -- "${psql[@]}" -d postgres -c 'ALTER ROLE board_staff LOGIN; ALTER ROLE board_auth LOGIN; ALTER ROLE board_media_read LOGIN'
create_database() {
    runuser -u postgres -- "${psql[@]}" -d postgres -v database="$1" <<'SQL'
CREATE DATABASE :"database" OWNER board_migrator;
REVOKE ALL ON DATABASE :"database" FROM PUBLIC;
GRANT CONNECT ON DATABASE :"database" TO board_migrator,board_public,board_staff,board_auth,board_media_read;
SQL
}
create_database file_deletion_upgrade
migrator=("${psql[@]}" -U board_migrator -d file_deletion_upgrade)
admin=(runuser -u postgres -- "${psql[@]}" -d file_deletion_upgrade)
# Populate retained soft-deleted history before 0129, then add a fresh canonical
# tombstone under 0129. Both must survive the populated 0130 upgrade unchanged.
for migration in migrations/*.sql; do
 [[ $(basename "$migration") < 0129_ ]] || break
 "${migrator[@]}" --single-transaction -f - < "$migration"
done
"${admin[@]}" <<'SQL'
CREATE FUNCTION public.capture_rows() RETURNS TABLE(relation text,value jsonb)
LANGUAGE plpgsql AS $$ DECLARE r record; BEGIN
 FOR r IN SELECT n.nspname,c.relname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname IN('content','post_secrets','staff_identity','media','media_intake','deployment','admission','poll_private') AND c.relkind='r' LOOP
  RETURN QUERY EXECUTE format('SELECT %L,to_jsonb(r) FROM %I.%I r',r.nspname||'.'||r.relname,r.nspname,r.relname);
 END LOOP;
END $$;
REVOKE ALL ON FUNCTION public.capture_rows() FROM PUBLIC;
CREATE FUNCTION public.normalized_acl(object_kind "char",owner_id oid,permissions aclitem[]) RETURNS jsonb
LANGUAGE sql STABLE SET search_path=pg_catalog,pg_temp AS $$
 SELECT CASE WHEN cardinality(permissions)=0 THEN '[]'::jsonb ELSE (
 SELECT coalesce(jsonb_agg(jsonb_build_array(pg_get_userbyid(a.grantor),
   CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee)::text END,
   a.privilege_type,a.is_grantable)
   ORDER BY pg_get_userbyid(a.grantor),
   CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee)::text END,
   a.privilege_type,a.is_grantable),'[]'::jsonb)
 FROM aclexplode(coalesce(permissions,acldefault(object_kind,owner_id))) a
 ) END
$$;
REVOKE ALL ON FUNCTION public.normalized_acl("char",oid,aclitem[]) FROM PUBLIC;

INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,image_limit)
VALUES('filedel','Owned file fixture','Synthetic',1000,100,100,100,10,100),
 ('fileother','Other owned fixture','Synthetic',1000,100,100,100,10,100);
BEGIN;
SET LOCAL ROLE board_migrator;
SELECT set_config('board.posting_actor',repeat('11',32),true);
SELECT set_config('board.poster_fingerprint',repeat('12',32),true);
SELECT set_config('board.poster_epoch',repeat('13',32),true);
INSERT INTO content.threads(id,board) VALUES(13000001,'filedel'),(13000010,'fileother');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
SELECT i,'filedel',13000001,'Saved name','Saved subject','Saved body'
FROM unnest(ARRAY[13000001,13000002,13000003,13000004,13000005]) i;
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(13000010,'fileother',13000010,'Other name','Other subject','Other body');
COMMIT;
INSERT INTO media.jobs(id,filename,state,input_bytes,attempts,lease_token,output_sha256,output_bytes)
SELECT repeat(i::text,32),'owned.png','published',1,1,repeat('c',32),repeat(i::text,64),1
FROM generate_series(1,4) i;
INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at)
SELECT repeat((i+4)::text,32),repeat(i::text,32),repeat('c',32),repeat(i::text,64),1,1,1,'approved',clock_timestamp()
FROM generate_series(1,4) i;
INSERT INTO media_intake.handles(job_id,capability_hash)
SELECT repeat(i::text,32),sha256(convert_to(repeat('e',64),'UTF8')) FROM generate_series(1,4) i;
INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler,file_deleted)
SELECT CASE i WHEN 1 THEN 13000002 WHEN 2 THEN 13000004 WHEN 3 THEN 13000005 ELSE 13000010 END,
 repeat(i::text,32),repeat((i+4)::text,32),'owned.png',1,1,1,false,i=2 FROM generate_series(1,4) i;
BEGIN;
SELECT slug FROM content.boards ORDER BY slug FOR UPDATE;
UPDATE content.posts SET deleted=true WHERE id=13000004;
COMMIT;
-- Historical reports include rows without membership and saved clear history.
INSERT INTO content.reports(id,board,post_id,reason,state,created_at) OVERRIDING SYSTEM VALUE
SELECT 13001000+i,CASE WHEN i=7 THEN 'fileother' ELSE 'filedel' END,
 CASE WHEN i<=3 THEN 13000002 WHEN i=4 THEN 13000003 WHEN i=5 THEN 13000001 WHEN i=6 THEN 13000004 ELSE 13000010 END,
 'Owned report',CASE i WHEN 2 THEN 'resolved' WHEN 3 THEN 'dismissed' ELSE 'open' END,'2020-01-01 UTC'
FROM generate_series(1,7) i;
UPDATE content.reports SET reporter_cleared_at='2020-01-02 UTC',group_cleared_at='2020-01-01 UTC',
 group_cleared_by=1,group_clear_inherited=false WHERE id=13001003;
BEGIN;
SELECT slug FROM content.boards ORDER BY slug FOR UPDATE;
INSERT INTO post_secrets.report_membership(report_id,actor_hash,board,post_id,thread_id,reported_at)
SELECT id,decode(repeat('22',32),'hex'),board,post_id,
 CASE WHEN board='fileother' THEN 13000010 ELSE 13000001 END,'2020-01-01 UTC'
FROM content.reports WHERE id IN(13001001,13001004,13001005,13001007);
COMMIT;
INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,created_at,network_at,address_at,environment_at,expires_at)
VALUES(decode(repeat('33',32),'hex'),decode(repeat('44',32),'hex'),decode(repeat('55',32),'hex'),decode(repeat('66',32),'hex'),1,1,1,1,4102444800);
INSERT INTO post_secrets.anonymous_reports(report_id,token_hash)
SELECT id,decode(repeat('33',32),'hex') FROM content.reports;
INSERT INTO post_secrets.report_weight_evidence(report_id,evaluator_version,evaluated_at)
SELECT id,1,'2020-01-01 UTC' FROM content.reports;
INSERT INTO content.moderation_audit(account_id,board,target_id,action,created_at)
VALUES(1,'filedel',13000002,'resolve','2020-01-01 UTC');
SQL
"${migrator[@]}" --single-transaction -f - < migrations/0129_fresh_content_erasure.sql
"${admin[@]}" <<'SQL'
BEGIN;
SELECT slug FROM content.boards WHERE slug='filedel' FOR UPDATE;
SET LOCAL ROLE board_public;
UPDATE content.posts SET deleted=true WHERE id=13000005;
RESET ROLE;
COMMIT;
CREATE TABLE public.before_rows AS SELECT * FROM public.capture_rows();
SQL
# Transactional DDL rollback must restore both schema and populated data.
{ printf 'BEGIN;\n'; cat migrations/0130_file_deletion_actions.sql; printf '\nROLLBACK;\n'; } | "${migrator[@]}"
"${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM information_schema.columns WHERE table_schema='content' AND table_name='posts' AND column_name='file_deleted')
 OR EXISTS(TABLE public.before_rows EXCEPT ALL SELECT * FROM public.capture_rows())
 OR EXISTS(SELECT * FROM public.capture_rows() EXCEPT ALL TABLE public.before_rows)
 THEN RAISE EXCEPTION 'Migration rollback changed schema or populated data'; END IF;
END $$;
SQL
"${migrator[@]}" --single-transaction -f - < migrations/0130_file_deletion_actions.sql
"${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(TABLE public.before_rows EXCEPT ALL SELECT relation,CASE WHEN relation='content.posts' THEN value-'file_deleted' ELSE value END FROM public.capture_rows())
 OR EXISTS(SELECT relation,CASE WHEN relation='content.posts' THEN value-'file_deleted' ELSE value END FROM public.capture_rows() EXCEPT ALL TABLE public.before_rows)
 OR EXISTS(SELECT 1 FROM content.posts WHERE file_deleted)
 OR NOT EXISTS(SELECT 1 FROM information_schema.columns WHERE table_schema='content' AND table_name='posts'
  AND column_name='file_deleted' AND is_nullable='NO' AND column_default='false')
 THEN RAISE EXCEPTION '0130 changed historical payloads, reports or media flags'; END IF;
 IF NOT EXISTS(SELECT 1 FROM content.posts WHERE id=13000004 AND deleted AND NOT content_erased AND comment='Saved body')
 OR NOT EXISTS(SELECT 1 FROM content.post_media WHERE post_id=13000004 AND file_deleted)
 OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=13000005 AND deleted AND content_erased AND comment='' AND NOT file_deleted)
 THEN RAISE EXCEPTION 'Historical or erased fixtures are missing'; END IF;
END $$;
DROP TABLE public.before_rows;
CREATE TABLE public.upgraded_rows AS SELECT * FROM public.capture_rows();
SQL
for migration in migrations/*.sql; do
 [[ $(basename "$migration") > 0130_file_deletion_actions.sql ]] || continue
 "${migrator[@]}" --single-transaction -f - < "$migration"
done
cat > "$cluster/assert-retained.sql" <<'SQL'
DO $$ BEGIN
 -- Only live targets 2/3, their thread clocks, media flag 2 and reports on
 -- exact administrative target 2 may differ from the populated upgrade.
 IF EXISTS(
  SELECT * FROM public.upgraded_rows WHERE NOT (
   (relation='content.posts' AND (value->>'id')::bigint IN(13000002,13000003)) OR
   (relation='content.threads' AND (value->>'id')::bigint=13000001) OR
   (relation='content.post_media' AND (value->>'post_id')::bigint=13000002) OR
   (relation='content.reports' AND (value->>'post_id')::bigint=13000002) OR
   (relation IN('post_secrets.report_membership','post_secrets.anonymous_reports','post_secrets.report_weight_evidence') AND (value->>'report_id')::bigint BETWEEN 13001001 AND 13001003) OR
   (relation='post_secrets.report_group' AND (value->>'post_id')::bigint=13000002))
  EXCEPT ALL SELECT * FROM public.capture_rows())
 OR EXISTS(
  SELECT * FROM public.capture_rows() WHERE NOT (
   (relation='content.posts' AND (value->>'id')::bigint IN(13000002,13000003)) OR
   (relation='content.threads' AND (value->>'id')::bigint=13000001) OR
   (relation='content.post_media' AND (value->>'post_id')::bigint=13000002) OR
   (relation='content.reports' AND (value->>'post_id')::bigint=13000002) OR
   (relation IN('post_secrets.report_membership','post_secrets.anonymous_reports','post_secrets.report_weight_evidence') AND (value->>'report_id')::bigint BETWEEN 13001001 AND 13001003) OR
   (relation='post_secrets.report_group' AND (value->>'post_id')::bigint=13000002))
  EXCEPT ALL SELECT * FROM public.upgraded_rows)
 THEN RAISE EXCEPTION 'File action changed unrelated history, audit or consumed media identity'; END IF;
 IF EXISTS(SELECT relation,value-'file_deleted' FROM public.capture_rows() WHERE relation='content.posts'
  EXCEPT ALL SELECT relation,value-'file_deleted' FROM public.upgraded_rows WHERE relation='content.posts')
 OR EXISTS(SELECT relation,value-'file_deleted' FROM public.capture_rows() WHERE relation='content.post_media'
  EXCEPT ALL SELECT relation,value-'file_deleted' FROM public.upgraded_rows WHERE relation='content.post_media')
 OR EXISTS(SELECT 1 FROM content.posts WHERE id=13000005 AND (NOT content_erased OR NOT deleted OR file_deleted))
 OR EXISTS(SELECT 1 FROM content.post_media WHERE post_id=13000003)
 THEN RAISE EXCEPTION 'File action rewrote content, consumed identities or fabricated media'; END IF;
END $$;
SQL
cat > "$cluster/assert-reports.sql" <<'SQL'
DO $$ BEGIN
 IF EXISTS(SELECT * FROM public.upgraded_rows WHERE relation IN('content.reports','post_secrets.report_membership','post_secrets.report_group','post_secrets.anonymous_reports','post_secrets.report_weight_evidence')
  EXCEPT ALL SELECT * FROM public.capture_rows())
 OR EXISTS(SELECT * FROM public.capture_rows() WHERE relation IN('content.reports','post_secrets.report_membership','post_secrets.report_group','post_secrets.anonymous_reports','post_secrets.report_weight_evidence')
  EXCEPT ALL SELECT * FROM public.upgraded_rows)
 THEN RAISE EXCEPTION 'Public file action changed report rows or private evidence'; END IF;
END $$;
SQL
# Actual runtime connections commit first and repeated media/text-only actions.
for pass in first repeat; do
 "${admin[@]}" -Atc 'SELECT modified_at FROM content.threads WHERE id=13000001' > "$cluster/before-clock"
 "${psql[@]}" -U board_public -d file_deletion_upgrade <<'SQL'
BEGIN;
SELECT content.delete_post_attachment('filedel',13000002);
SELECT content.delete_post_attachment('filedel',13000003);
COMMIT;
SQL
 "${admin[@]}" -f - < "$cluster/assert-retained.sql"
 "${admin[@]}" -f - < "$cluster/assert-reports.sql"
 "${admin[@]}" -v prior="$(cat "$cluster/before-clock")" <<'SQL'
SELECT 1 / ((modified_at > :'prior'::timestamptz)::integer) FROM content.threads WHERE id=13000001;
DO $$ BEGIN
 IF (SELECT count(*) FROM content.posts WHERE id IN(13000002,13000003) AND file_deleted AND NOT deleted)<>2
 OR NOT EXISTS(SELECT 1 FROM content.post_media WHERE post_id=13000002 AND file_deleted)
 THEN RAISE EXCEPTION 'Public file action failed to mark live media/text targets'; END IF;
END $$;
SQL
done
# The administrative wrapper removes every report state and its private
# cascades for exactly target 2. Rollback must restore the entire transaction.
"${admin[@]}" <<'SQL'
CREATE TABLE public.rollback_rows AS SELECT * FROM public.capture_rows();
SQL
for end in ROLLBACK COMMIT; do
 {
  printf 'BEGIN; SET LOCAL ROLE board_staff; SELECT content.staff_delete_post_attachment(\x27filedel\x27,13000002); RESET ROLE;\n'
  cat <<'SQL'
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM content.reports WHERE post_id=13000002)
 OR EXISTS(SELECT 1 FROM post_secrets.report_membership WHERE post_id=13000002)
 OR EXISTS(SELECT 1 FROM post_secrets.report_group WHERE post_id=13000002)
 OR EXISTS(SELECT 1 FROM post_secrets.anonymous_reports WHERE report_id BETWEEN 13001001 AND 13001003)
 OR EXISTS(SELECT 1 FROM post_secrets.report_weight_evidence WHERE report_id BETWEEN 13001001 AND 13001003)
 OR (SELECT count(*) FROM content.reports)<>4
 THEN RAISE EXCEPTION 'Administrative exact-target cleanup or cascade failed'; END IF;
END $$;
SQL
  cat "$cluster/assert-retained.sql"
  printf '%s;\n' "$end"
 } | "${admin[@]}"
 if [[ $end = ROLLBACK ]]; then
  "${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(TABLE public.rollback_rows EXCEPT ALL SELECT * FROM public.capture_rows())
 OR EXISTS(SELECT * FROM public.capture_rows() EXCEPT ALL TABLE public.rollback_rows)
 THEN RAISE EXCEPTION 'Administrative rollback lost data or timestamps'; END IF;
END $$;
SQL
 fi
done
"${admin[@]}" -c 'DROP TABLE public.rollback_rows'
cat > "$cluster/fingerprint.sql" <<'SQL'
SELECT relation,md5(value::text) FROM public.capture_rows() ORDER BY relation,value::text;
SELECT n.nspname,p.proname,pg_get_function_identity_arguments(p.oid),pg_get_userbyid(p.proowner),p.proconfig::text,public.normalized_acl('f',p.proowner,p.proacl),md5(pg_get_functiondef(p.oid)) FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname IN('content','post_secrets','media') ORDER BY 1,2,3;
SELECT t.tgrelid::regclass::text,t.tgname,t.tgenabled,pg_get_triggerdef(t.oid) FROM pg_trigger t WHERE NOT t.tgisinternal ORDER BY 1,2;
SELECT c.conrelid::regclass::text,c.conname,pg_get_constraintdef(c.oid,true) FROM pg_constraint c JOIN pg_namespace n ON n.oid=c.connamespace WHERE n.nspname IN('content','post_secrets','media') ORDER BY 1,2;
SELECT table_schema,table_name,column_name,grantor,grantee,privilege_type,is_grantable FROM information_schema.column_privileges WHERE table_schema IN('content','post_secrets','media') ORDER BY 1,2,3,4,5,6,7;
SELECT n.nspname,pg_get_userbyid(n.nspowner),public.normalized_acl('n',n.nspowner,n.nspacl) FROM pg_namespace n WHERE n.nspname IN('content','post_secrets','media') ORDER BY 1;
SELECT n.nspname,c.relname,c.relkind,pg_get_userbyid(c.relowner),public.normalized_acl(CASE WHEN c.relkind='S' THEN 's' ELSE 'r' END::"char",c.relowner,c.relacl),c.relrowsecurity,c.relforcerowsecurity,c.reloptions::text,CASE WHEN c.relkind='v' THEN pg_get_viewdef(c.oid,true) END FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname IN('content','post_secrets','media') AND c.relkind IN('r','p','v','m','f','S') ORDER BY 1,2;
SELECT schemaname,tablename,policyname,permissive,roles::text,cmd,qual,with_check FROM pg_policies WHERE schemaname IN('content','post_secrets','media') ORDER BY 1,2,3;
SQL
for phase in live restored; do
 database=file_deletion_upgrade
 if [[ $phase = restored ]]; then
  runuser -u postgres -- "$pg_bin/pg_dump" -h "$cluster" -U postgres -d "$database" --format=custom > "$cluster/current.dump"
  create_database file_deletion_restore
  runuser -u postgres -- "$pg_bin/pg_restore" -h "$cluster" --dbname=file_deletion_restore --single-transaction --exit-on-error < "$cluster/current.dump"
  database=file_deletion_restore
 fi
 admin=(runuser -u postgres -- "${psql[@]}" -d "$database")
 # Actual login denials run against both the live and restored populated DB.
 # Only 42501 is accepted; sentinels and unrelated SQL failures propagate.
 for role in board_public board_staff; do
  "${psql[@]}" -U "$role" -d "$database" <<'SQL'
BEGIN;
DO $$ DECLARE command text; BEGIN
 FOREACH command IN ARRAY ARRAY[
  'SELECT post_secrets.retire_staff_file_report_membership(''filedel'',13000003)',
  'DELETE FROM content.reports WHERE board=''filedel'' AND post_id=13000003',
  'UPDATE content.posts SET file_deleted=false WHERE board=''filedel'' AND id=13000003'
 ] LOOP
  BEGIN
   EXECUTE command;
   RAISE EXCEPTION 'Runtime accepted forbidden file/report mutation' USING ERRCODE='ZX130';
  EXCEPTION WHEN SQLSTATE '42501' THEN NULL;
  END;
 END LOOP;
END $$;
ROLLBACK;
SQL
 done
 "${psql[@]}" -U board_staff -d "$database" <<'SQL'
BEGIN;
SELECT content.staff_delete_post_attachment('filedel',13000003);
ROLLBACK;
SQL
 "${psql[@]}" -U board_public -d "$database" <<'SQL'
BEGIN;
DO $$ BEGIN
 BEGIN
  PERFORM content.staff_delete_post_attachment('filedel',13000003);
  RAISE EXCEPTION 'Public role accepted administrative file deletion' USING ERRCODE='ZX130';
 EXCEPTION WHEN SQLSTATE '42501' THEN NULL;
 END;
END $$;
ROLLBACK;
SQL
 "${admin[@]}" -f - < "$cluster/assert-retained.sql"
 "${admin[@]}" <<'SQL'
CREATE TEMP TABLE phase_rows AS SELECT * FROM public.capture_rows();
BEGIN;
SET LOCAL ROLE board_public;
SELECT content.delete_post_attachment('filedel',13000003);
RESET ROLE;
DO $$ BEGIN
 IF EXISTS(SELECT * FROM phase_rows WHERE relation IN('content.reports','post_secrets.report_membership','post_secrets.report_group','post_secrets.anonymous_reports','post_secrets.report_weight_evidence') EXCEPT ALL SELECT * FROM public.capture_rows())
 THEN RAISE EXCEPTION 'Repeated text-only action changed reports after restore'; END IF;
END $$;
SET LOCAL ROLE board_staff;
SELECT content.staff_delete_post_attachment('filedel',13000003);
RESET ROLE;
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM content.reports WHERE post_id=13000003)
 OR EXISTS(SELECT 1 FROM post_secrets.report_group WHERE post_id=13000003)
 OR EXISTS(SELECT 1 FROM post_secrets.anonymous_reports WHERE report_id=13001004)
 OR EXISTS(SELECT 1 FROM post_secrets.report_weight_evidence WHERE report_id=13001004)
 OR (SELECT count(*) FROM content.reports)<>3
 THEN RAISE EXCEPTION 'Administrative text-only cleanup failed after restore'; END IF;
END $$;
-- Whole deletion must reset an already-set file marker to the canonical 0129
-- payload. Existing tombstones must reject attempts to turn that marker on.
SET LOCAL ROLE board_public;
UPDATE content.posts SET deleted=true WHERE id=13000003;
RESET ROLE;
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM content.posts WHERE id=13000003 AND content_erased AND deleted
  AND NOT file_deleted AND name='' AND subject='' AND comment='' AND created_at='epoch'::timestamptz)
 THEN RAISE EXCEPTION 'Whole deletion retained a live file marker'; END IF;
 BEGIN
  UPDATE content.posts SET file_deleted=true WHERE id=13000005;
  RAISE EXCEPTION 'Erased post accepted a noncanonical file marker' USING ERRCODE='ZX130';
 EXCEPTION WHEN check_violation THEN NULL;
 END;
END $$;
ROLLBACK;
DO $$ BEGIN
 IF EXISTS(TABLE phase_rows EXCEPT ALL SELECT * FROM public.capture_rows())
 OR EXISTS(SELECT * FROM public.capture_rows() EXCEPT ALL TABLE phase_rows)
 THEN RAISE EXCEPTION 'Restored rollback changed populated data'; END IF;
END $$;
SQL
 "${psql[@]}" -U board_media_read -d "$database" <<'SQL'
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM media.approved_assets WHERE id IN(repeat('5',32),repeat('6',32),repeat('7',32)))
 OR NOT EXISTS(SELECT 1 FROM media.approved_assets WHERE id=repeat('8',32))
 THEN RAISE EXCEPTION 'Deleted media visibility changed after restore'; END IF;
END $$;
SQL
 "${psql[@]}" -U board_public -d "$database" <<'SQL'
BEGIN;
SELECT slug FROM content.boards WHERE slug='filedel' FOR UPDATE;
DO $$ BEGIN
 BEGIN
  PERFORM content.insert_post_attachment(13000006,'filedel',13000001,'Replay','','Replay body',repeat('1',32),repeat('e',64),false);
  RAISE EXCEPTION 'Consumed media capability was reusable' USING ERRCODE='ZX130';
 EXCEPTION WHEN SQLSTATE 'P0001' THEN
  IF SQLERRM<>'Attachment is not ready or was already used.' THEN RAISE; END IF;
 END;
 IF EXISTS(SELECT 1 FROM content.posts WHERE id=13000006)
 THEN RAISE EXCEPTION 'Rejected replay left a post'; END IF;
END $$;
ROLLBACK;
SQL
 "${admin[@]}" -At -f - < "$cluster/fingerprint.sql" > "$cluster/$phase.fingerprint"
done
cmp -s "$cluster/live.fingerprint" "$cluster/restored.fingerprint" || { printf 'File deletion dump/restore fingerprint mismatch.\n' >&3; exit 1; }
printf '0130 populated upgrade, historical preservation, repeated public actions, exact administrative cleanup, rollback and administrator restore passed.\n' >&3
