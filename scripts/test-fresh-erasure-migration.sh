#!/usr/bin/env bash
# Owned synthetic clusters only. Populated pre-0123 upgrade and current-schema administrator dump/restore.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-fresh-erasure.XXXXXXXX)
exec 3>&1
started=0
cleanup() {
    status=$?
    trap - EXIT
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null || status=1
    fi
    [[ $cluster =~ ^/tmp/board-fresh-erasure\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
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
trap 'printf "Fresh-erasure qualification failed at line %s (private diagnostics removed during cleanup).\n" "$LINENO" >&3' ERR
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
python3 - "$cluster" <<'PYREADINESS'
import pathlib,re,sys
root=pathlib.Path(sys.argv[1])
source=pathlib.Path('crates/store/src/content_erasure.rs').read_text()
match=re.search(r'pub const READINESS_SQL: &str = r#"(.*?)"#;',source,re.S)
if not match: raise SystemExit('Cannot extract current content erasure readiness')
query=match.group(1)
(root/'readiness.sql').write_text("DO $check$ BEGIN IF ("+query+") IS DISTINCT FROM true THEN RAISE EXCEPTION 'Current erasure readiness failed'; END IF; END $check$;")
changes=[]
for function in ('guard_post_erasure','guard_thread_erasure','erase_thread_descendants','guard_erased_author_link'):
 for suffix in ('SECURITY INVOKER','SET search_path=public','OWNER TO board_migrator'):
  changes.append(f'ALTER FUNCTION post_secrets.{function}() {suffix}')
 changes.append(f'GRANT EXECUTE ON FUNCTION post_secrets.{function}() TO PUBLIC')
 changes.append(f'GRANT EXECUTE ON FUNCTION post_secrets.{function}() TO board_migrator')
for table,trigger in [('content.posts','z_guard_post_erasure'),('content.threads','z_guard_thread_erasure'),('content.threads','erase_thread_descendants')]+[(t,'z_guard_erased_author_link') for t in ('post_secrets.deletion','post_secrets.anonymous_posts','post_secrets.op_replies','post_secrets.op_peers','post_secrets.poster_contexts','post_secrets.posting_history','staff_identity.discussion_posts')]:
 changes.append(f'ALTER TABLE {table} DISABLE TRIGGER {trigger}')
changes += [
 'ALTER TABLE content.posts DROP CONSTRAINT posts_erased_payload',
 'ALTER TABLE content.threads DROP CONSTRAINT threads_erased_payload',
 'ALTER ROLE board_content_erasure_owner LOGIN',
 'ALTER ROLE board_content_erasure_owner BYPASSRLS',
 'GRANT UPDATE(content_erased) ON content.posts TO board_public',
 'GRANT UPDATE(content_erased) ON content.threads TO board_staff',
 'REVOKE EXECUTE ON FUNCTION content.board_flag_label(text,text) FROM board_content_erasure_owner',
 'REVOKE DELETE ON post_secrets.deletion FROM board_content_erasure_owner',
 'REVOKE UPDATE(slug) ON content.boards FROM board_content_erasure_owner',
 'REVOKE SELECT(content_erased) ON content.posts FROM board_content_erasure_owner',
 'REVOKE SELECT(content_erased) ON content.threads FROM board_content_erasure_owner',
 'REVOKE SELECT(post_id) ON post_secrets.deletion FROM board_content_erasure_owner',
 'REVOKE USAGE ON SCHEMA staff_identity FROM board_content_erasure_owner',
 'DROP POLICY content_erasure_board_read ON content.boards',
 'DROP POLICY content_erasure_board_lock ON content.boards',
 'GRANT board_content_erasure_owner TO board_migrator WITH INHERIT TRUE',
 'GRANT board_content_erasure_owner TO board_migrator WITH SET FALSE',
 'GRANT board_content_erasure_owner TO board_auth',
 'GRANT board_auth TO board_content_erasure_owner',
]
with (root/'drift.sql').open('w') as out:
 for i,change in enumerate(changes):
  out.write('BEGIN; '+change+';\n')
  for role in ('board_public','board_staff'):
   out.write(f"SET LOCAL ROLE {role}; DO $check$ BEGIN IF ({query}) IS DISTINCT FROM false THEN RAISE EXCEPTION 'Readiness accepted erasure drift {i}'; END IF; END $check$; RESET ROLE;\n")
  out.write('ROLLBACK;\n')
PYREADINESS
create_database fresh_erasure_upgrade
migrator=("${psql[@]}" -U board_migrator -d fresh_erasure_upgrade)
admin=(runuser -u postgres -- "${psql[@]}" -d fresh_erasure_upgrade)
for migration in migrations/*.sql; do
 [[ $(basename "$migration") < 0123_ ]] || break
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
-- pg_dump may omit an explicit default ACL. Compare its effective grant
-- tuples, preserving grantor, grantee, privilege and grant option; an explicit
-- empty ACL must remain different from NULL/default privileges.
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
DO $$ DECLARE owner_id oid:='board_migrator'::regrole; kind "char"; BEGIN
 FOREACH kind IN ARRAY ARRAY['r','s','f']::"char"[] LOOP
  IF public.normalized_acl(kind,owner_id,NULL) IS DISTINCT FROM public.normalized_acl(kind,owner_id,acldefault(kind,owner_id))
  OR public.normalized_acl(kind,owner_id,NULL)=public.normalized_acl(kind,owner_id,'{}'::aclitem[])
  THEN RAISE EXCEPTION 'ACL normalization lost default/empty distinction'; END IF;
 END LOOP;
 IF public.normalized_acl('r',owner_id,NULL)=public.normalized_acl('r',owner_id,
     acldefault('r',owner_id)||ARRAY['board_public=r/board_migrator'::aclitem])
 OR public.normalized_acl('r',owner_id,ARRAY['board_public=r/board_migrator'::aclitem])=
    public.normalized_acl('r',owner_id,ARRAY['board_public=r*/board_migrator'::aclitem])
 OR NOT public.normalized_acl('f',owner_id,NULL) @> '[ ["board_migrator","PUBLIC","EXECUTE",false] ]'::jsonb
 THEN RAISE EXCEPTION 'ACL normalization lost a grant, grant option or PUBLIC default'; END IF;
END $$;
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,image_limit)
VALUES('erasure','Owned erasure fixture','Synthetic',1000,100,100,100,10,100);
BEGIN;
SET LOCAL ROLE board_migrator;
SELECT set_config('board.posting_actor',repeat('11',32),true);
SELECT set_config('board.poster_fingerprint',repeat('12',32),true);
SELECT set_config('board.poster_epoch',repeat('13',32),true);
INSERT INTO content.threads(id,board) VALUES(12300001,'erasure'),(12300010,'erasure'),(12300020,'erasure');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
SELECT i,'erasure',12300001,'Saved name','Saved subject','Saved body' FROM unnest(ARRAY[12300001,12300002,12300003]) i;
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(12300010,'erasure',12300010,'Historical OP','Historical subject','Historical body'),
(12300011,'erasure',12300010,'Historical child','','Historical child body'),
(12300020,'erasure',12300020,'Surviving OP','','Surviving body');
COMMIT;
-- Populate optional payload fields before 0123, so the erasure checks exercise
-- non-default bytes as well as the ordinary name/subject/comment fields.
UPDATE content.posts SET trip='!0123456789',poster_id='abcdefgh',
 wordfilter_payload=decode('5746303100000000000000','hex'),wordfilter_search='Saved searchable body',
 comment_format=40,image_spoiler=true WHERE thread_id=12300001;
UPDATE content.posts SET json_op_poster_id='abcdefgh',staff_authorized_limits=true,
 wordfilter_payload=decode('5746303200000000000000','hex'),dice_result='2d6 = 7' WHERE id=12300001;
UPDATE content.posts SET country='US',country_name='United States' WHERE id=12300002;
UPDATE content.posts SET board_flag_type='test',board_flag='FL1',flag_name='Flag 1',
 fortune_text='Synthetic fortune',fortune_color='#123456' WHERE id=12300003;
BEGIN;
SET LOCAL ROLE board_migrator;
SELECT slug FROM content.boards WHERE slug='erasure' FOR UPDATE;
INSERT INTO post_secrets.deletion(post_id,password_hash)
SELECT id,'synthetic-password-proof' FROM content.posts WHERE board='erasure';
COMMIT;
INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,created_at,network_at,address_at,environment_at,expires_at,posts,threads)
VALUES(decode(repeat('33',32),'hex'),decode(repeat('44',32),'hex'),decode(repeat('55',32),'hex'),decode(repeat('66',32),'hex'),1,1,1,1,4102444800,6,3);
INSERT INTO post_secrets.anonymous_posts(post_id,token_hash,password_proof)
SELECT id,decode(repeat('33',32),'hex'),decode(repeat('77',32),'hex') FROM content.posts WHERE board='erasure';
INSERT INTO post_secrets.op_peers(thread_id,peer) VALUES(12300001,'192.0.2.123');
INSERT INTO post_secrets.op_replies(post_id,thread_id) VALUES(12300002,12300001),(12300003,12300001);
INSERT INTO staff_identity.accounts(id,role) OVERRIDING SYSTEM VALUE VALUES(12300001,'admin');
INSERT INTO staff_identity.credentials(id,account_id,credential) VALUES(decode('1234','hex'),12300001,'{}');
INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id)
VALUES(decode(repeat('88',32),'hex'),decode(repeat('99',32),'hex'),12300001,decode('1234','hex'));
INSERT INTO staff_identity.discussion_posts(post_id,account_id) VALUES(12300002,12300001),(12300003,12300001);
INSERT INTO content.moderation_audit(account_id,board,target_id,action,created_at)
VALUES(12300001,'erasure',12300002,'resolve','2020-01-01 UTC');
INSERT INTO content.reports(id,board,post_id,reason) OVERRIDING SYSTEM VALUE
VALUES(12301001,'erasure',12300002,'Synthetic report'),(12301002,'erasure',12300020,'Unrelated report');
INSERT INTO post_secrets.anonymous_reports(report_id,token_hash)
VALUES(12301001,decode(repeat('33',32),'hex')),(12301002,decode(repeat('33',32),'hex'));
INSERT INTO media.jobs(id,filename,state,input_bytes,attempts,lease_token,output_sha256,output_bytes)
VALUES(repeat('a',32),'owned.png','published',1,1,repeat('c',32),repeat('d',64),1);
INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at)
VALUES(repeat('b',32),repeat('a',32),repeat('c',32),repeat('d',64),1,1,1,'approved',clock_timestamp());
INSERT INTO media_intake.handles(job_id,capability_hash)
VALUES(repeat('a',32),sha256(convert_to(repeat('e',64),'UTF8')));
INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler)
VALUES(12300002,repeat('a',32),repeat('b',32),'owned.png',1,1,1,false);
BEGIN;
SELECT slug FROM content.boards ORDER BY slug FOR UPDATE;
UPDATE content.posts SET deleted=true WHERE id=12300003;
UPDATE content.threads SET deleted=true WHERE id=12300010;
COMMIT;
CREATE TABLE public.before_rows AS SELECT * FROM public.capture_rows();
CREATE TABLE public.runtime_grants_before AS
SELECT table_schema,table_name,column_name,grantor,grantee,privilege_type,is_grantable
FROM information_schema.column_privileges WHERE grantee IN('board_public','board_staff','board_auth','board_media','board_media_read','board_media_intake','board_monitor');
SQL
"${migrator[@]}" --single-transaction -f - < migrations/0123_fresh_content_erasure.sql
"${admin[@]}" <<'SQL'
DO $$ BEGIN
 -- Ignore only the two explicitly additive markers. Every historical payload,
 -- private relation, policy row and media-consumption record must be identical.
 IF EXISTS(TABLE public.before_rows EXCEPT ALL SELECT relation,CASE WHEN relation IN('content.posts','content.threads') THEN value-'content_erased' ELSE value END FROM public.capture_rows())
 OR EXISTS(SELECT relation,CASE WHEN relation IN('content.posts','content.threads') THEN value-'content_erased' ELSE value END FROM public.capture_rows() EXCEPT ALL TABLE public.before_rows)
 THEN RAISE EXCEPTION 'Migration changed retained historical rows'; END IF;
 IF EXISTS(SELECT 1 FROM content.posts WHERE content_erased) OR EXISTS(SELECT 1 FROM content.threads WHERE content_erased)
 OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=12300003 AND deleted AND comment='Saved body')
 OR NOT EXISTS(SELECT 1 FROM content.threads WHERE id=12300010 AND deleted)
 THEN RAISE EXCEPTION 'Migration swept historical soft-deleted payloads'; END IF;
 IF EXISTS(TABLE public.runtime_grants_before EXCEPT SELECT table_schema,table_name,column_name,grantor,grantee,privilege_type,is_grantable FROM information_schema.column_privileges WHERE grantee IN('board_public','board_staff','board_auth','board_media','board_media_read','board_media_intake','board_monitor') AND column_name<>'content_erased')
 OR EXISTS(SELECT table_schema,table_name,column_name,grantor,grantee,privilege_type,is_grantable FROM information_schema.column_privileges WHERE grantee IN('board_public','board_staff','board_auth','board_media','board_media_read','board_media_intake','board_monitor') AND column_name<>'content_erased' EXCEPT TABLE public.runtime_grants_before)
 THEN RAISE EXCEPTION 'Migration changed existing runtime grants'; END IF;
END $$;
DROP TABLE public.runtime_grants_before;
DROP TABLE public.before_rows;
-- Byte-complete evidence retained independently of the erased target.
CREATE TABLE public.independent_rows AS SELECT * FROM public.capture_rows() WHERE relation IN
('content.moderation_audit','post_secrets.anonymous_sessions','post_secrets.posting_thread_actions',
 'staff_identity.accounts','staff_identity.credentials','staff_identity.sessions','media.jobs','media.assets','media_intake.handles','content.post_media');
CREATE TABLE public.unrelated_rows AS SELECT * FROM public.capture_rows() WHERE
(relation='content.posts' AND (value->>'id')::bigint IN(12300010,12300011,12300020)) OR
(relation='content.threads' AND (value->>'id')::bigint IN(12300010,12300020));
SQL
for migration in migrations/*.sql; do
 [[ $(basename "$migration") > 0123_fresh_content_erasure.sql ]] || continue
 "${migrator[@]}" --single-transaction -f - < "$migration"
done
cat > "$cluster/assert-erased.sql" <<'SQL'
DO $$ DECLARE relation_name text; BEGIN
 IF (SELECT count(*) FROM content.posts WHERE thread_id=12300001)<>3
 OR EXISTS(SELECT 1 FROM content.posts WHERE thread_id=12300001 AND
  (NOT deleted OR NOT content_erased OR name<>'' OR subject<>'' OR comment<>''
   OR trip IS NOT NULL OR poster_id IS NOT NULL OR json_op_poster_id IS NOT NULL OR capcode IS NOT NULL
   OR country IS NOT NULL OR country_name IS NOT NULL OR board_flag IS NOT NULL OR flag_name IS NOT NULL
   OR wordfilter_payload IS NOT NULL OR wordfilter_search IS NOT NULL OR dice_result IS NOT NULL
   OR fortune_text IS NOT NULL OR fortune_color IS NOT NULL OR comment_format<>0
   OR staff_authorized_limits OR image_spoiler OR board_flag_type<>'pol' OR created_at<>'epoch'::timestamptz))
 OR NOT EXISTS(SELECT 1 FROM content.threads WHERE id=12300001 AND content_erased AND deleted
  AND created_at='epoch'::timestamptz AND bumped_at='epoch'::timestamptz
  AND modified_at='epoch'::timestamptz AND http_modified_at='epoch'::timestamptz
  AND reply_count=0 AND NOT sticky AND NOT closed AND NOT permasage AND NOT permaage AND NOT undead
  AND sticky_rank=0 AND archived_at IS NULL AND archive_expires_at IS NULL)
 THEN RAISE EXCEPTION 'Fresh thread deletion did not erase every descendant payload'; END IF;
 FOREACH relation_name IN ARRAY ARRAY['post_secrets.deletion','post_secrets.anonymous_posts',
 'post_secrets.op_replies','post_secrets.poster_contexts','post_secrets.posting_history','staff_identity.discussion_posts'] LOOP
  IF EXISTS(SELECT 1 FROM public.capture_rows() WHERE relation=relation_name AND (value->>'post_id')::bigint BETWEEN 12300001 AND 12300003)
  THEN RAISE EXCEPTION 'Linked author authority survived in %',relation_name; END IF;
 END LOOP;
 IF EXISTS(SELECT 1 FROM post_secrets.op_peers WHERE thread_id=12300001)
 OR EXISTS(SELECT 1 FROM content.reports WHERE post_id BETWEEN 12300001 AND 12300003)
 OR EXISTS(SELECT 1 FROM post_secrets.anonymous_reports WHERE report_id=12301001)
 OR NOT EXISTS(SELECT 1 FROM content.reports WHERE id=12301002)
 OR NOT EXISTS(SELECT 1 FROM post_secrets.anonymous_reports WHERE report_id=12301002)
 THEN RAISE EXCEPTION 'Deletion changed the wrong report or author evidence'; END IF;
 IF EXISTS(TABLE public.independent_rows EXCEPT ALL SELECT * FROM public.capture_rows())
 OR EXISTS(SELECT * FROM public.capture_rows() WHERE relation IN
 ('content.moderation_audit','post_secrets.anonymous_sessions','post_secrets.posting_thread_actions',
 'staff_identity.accounts','staff_identity.credentials','staff_identity.sessions','media.jobs','media.assets','media_intake.handles','content.post_media') EXCEPT ALL TABLE public.independent_rows)
 OR EXISTS(TABLE public.unrelated_rows EXCEPT ALL SELECT * FROM public.capture_rows())
 THEN RAISE EXCEPTION 'Deletion changed independent evidence or unrelated historical content'; END IF;
END $$;
SQL
cat > "$cluster/fingerprint.sql" <<'SQL'
SELECT relation,md5(value::text) FROM public.capture_rows() ORDER BY relation,value::text;
SELECT n.nspname,p.proname,pg_get_function_identity_arguments(p.oid),pg_get_userbyid(p.proowner),p.proconfig::text,public.normalized_acl('f',p.proowner,p.proacl),md5(pg_get_functiondef(p.oid)) FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname IN('content','post_secrets') ORDER BY 1,2,3;
SELECT t.tgrelid::regclass::text,t.tgname,t.tgenabled,pg_get_triggerdef(t.oid) FROM pg_trigger t WHERE NOT t.tgisinternal ORDER BY 1,2;
SELECT c.conrelid::regclass::text,c.conname,pg_get_constraintdef(c.oid,true) FROM pg_constraint c JOIN pg_namespace n ON n.oid=c.connamespace WHERE n.nspname IN('content','post_secrets') ORDER BY 1,2;
SELECT table_schema,table_name,column_name,grantor,grantee,privilege_type,is_grantable FROM information_schema.column_privileges WHERE table_schema IN('content','post_secrets','staff_identity') ORDER BY 1,2,3,4,5,6,7;
SELECT n.nspname,c.relname,c.relkind,pg_get_userbyid(c.relowner),CASE WHEN c.relkind IN('r','p','v','m','f','S') THEN public.normalized_acl(CASE WHEN c.relkind='S' THEN 's' ELSE 'r' END::"char",c.relowner,c.relacl) ELSE to_jsonb(c.relacl::text) END,c.relrowsecurity,c.relforcerowsecurity FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname IN('content','post_secrets','staff_identity') ORDER BY 1,2;
SQL
for phase in live restored; do
 database=fresh_erasure_upgrade
 if [[ $phase = restored ]]; then
  runuser -u postgres -- "$pg_bin/pg_dump" -h "$cluster" -U postgres -d "$database" --format=custom > "$cluster/current.dump"
  create_database fresh_erasure_restore
  runuser -u postgres -- "$pg_bin/pg_restore" -h "$cluster" --dbname=fresh_erasure_restore --single-transaction --exit-on-error < "$cluster/current.dump"
  database=fresh_erasure_restore
 fi
 admin=(runuser -u postgres -- "${psql[@]}" -d "$database")
 for role in board_public board_staff; do
  "${psql[@]}" -U "$role" -d "$database" -f - < "$cluster/readiness.sql"
 done
 "${admin[@]}" -f - < "$cluster/drift.sql"
 # Dropped owner policies must fail closed at execution too, including on a
 # private board where accidental RLS invisibility could otherwise skip erasure.
 for policy in content_erasure_board_read content_erasure_board_lock; do
  "${admin[@]}" -v policy="$policy" <<'SQL'
BEGIN;
SELECT slug FROM content.boards WHERE slug='erasure' FOR UPDATE;
UPDATE content.boards SET staff_only=true WHERE slug='erasure';
CREATE TEMP TABLE atomic_before ON COMMIT DROP AS SELECT * FROM public.capture_rows();
SET LOCAL ROLE board_migrator;
DROP POLICY :"policy" ON content.boards;
SET LOCAL ROLE board_staff;
DO $$ BEGIN
 BEGIN
  UPDATE content.threads SET deleted=true WHERE id=12300020 AND board='erasure';
  RAISE EXCEPTION 'Erasure succeeded without its board visibility/lock policy' USING ERRCODE='ZX123';
 EXCEPTION WHEN check_violation THEN NULL;
 END;
END $$;
RESET ROLE;
DO $$ BEGIN
 IF EXISTS(TABLE atomic_before EXCEPT ALL SELECT * FROM public.capture_rows())
 OR EXISTS(SELECT * FROM public.capture_rows() EXCEPT ALL TABLE atomic_before)
 THEN RAISE EXCEPTION 'Failed private erasure changed content or author evidence'; END IF;
END $$;
ROLLBACK;
SQL
 done
 if [[ $phase = live ]]; then
  # Each actual runtime login can delete the whole thread. Roll back the first
  # trial, then commit the second so the archive contains irreversible tombstones.
  for role in board_public board_staff; do
   { printf 'BEGIN; SELECT slug FROM content.boards WHERE slug=\x27erasure\x27 FOR UPDATE; SET LOCAL ROLE %s;\n' "$role"
     printf "UPDATE content.threads SET deleted=true WHERE id=12300001; RESET ROLE;\n"
     cat "$cluster/assert-erased.sql"
     printf 'ROLLBACK;\n'
   } | "${admin[@]}"
  done
  "${psql[@]}" -U board_public -d "$database" <<'SQL'
BEGIN;
SELECT slug FROM content.boards WHERE slug='erasure' FOR UPDATE;
UPDATE content.threads SET deleted=true WHERE id=12300001;
COMMIT;
SQL
 fi
 "${admin[@]}" -f - < "$cluster/assert-erased.sql"
 # The offline migration role cannot reverse a tombstone while guards are intact.
 "${admin[@]}" <<'SQL'
BEGIN;
SELECT slug FROM content.boards WHERE slug='erasure' FOR UPDATE;
SET LOCAL ROLE board_migrator;
DO $$ DECLARE command text; BEGIN
 FOREACH command IN ARRAY ARRAY[
 'UPDATE content.posts SET deleted=false WHERE id=12300003',
 'UPDATE content.posts SET content_erased=false WHERE id=12300003',
 'UPDATE content.threads SET deleted=false WHERE id=12300001',
 'UPDATE content.threads SET content_erased=false WHERE id=12300001',
 'INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES(12300003,''recreated-proof'')',
 'INSERT INTO post_secrets.op_peers(thread_id,peer) VALUES(12300001,''192.0.2.124'')'] LOOP
  BEGIN
   EXECUTE command;
   RAISE EXCEPTION 'Erased authority or tombstone was restored: %',command USING ERRCODE='ZX123';
  EXCEPTION WHEN check_violation THEN NULL;
  END;
 END LOOP;
END $$;
ROLLBACK;
SQL
 "${psql[@]}" -U board_media_read -d "$database" <<'SQL'
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM media.approved_assets WHERE id=repeat('b',32))
 THEN RAISE EXCEPTION 'Erased consumed media is publicly readable'; END IF;
END $$;
SQL
 "${psql[@]}" -U board_public -d "$database" <<'SQL'
BEGIN;
SELECT slug FROM content.boards WHERE slug='erasure' FOR UPDATE;
DO $$ BEGIN
 -- A live destination and a valid, unexpired capability isolate the consumed
 -- attachment check. Retained post_media must block replay even after restore.
 BEGIN
  PERFORM content.insert_post_attachment(12300021,'erasure',12300020,'Replay','','Replay body',repeat('a',32),repeat('e',64),false);
  RAISE EXCEPTION 'Consumed media capability was reusable' USING ERRCODE='ZX123';
 EXCEPTION WHEN SQLSTATE 'P0001' THEN
  IF SQLERRM<>'Attachment is not ready or was already used.' THEN RAISE; END IF;
 END;
 IF EXISTS(SELECT 1 FROM content.posts WHERE id=12300021)
 THEN RAISE EXCEPTION 'Rejected media replay left a post'; END IF;
END $$;
ROLLBACK;
SQL
 for role in board_public board_staff; do
  "${psql[@]}" -U "$role" -d "$database" -f - < "$cluster/readiness.sql"
 done
 "${admin[@]}" -At -f - < "$cluster/fingerprint.sql" > "$cluster/$phase.fingerprint"
done
cmp -s "$cluster/live.fingerprint" "$cluster/restored.fingerprint" || { printf 'Fresh erasure dump/restore fingerprint mismatch.\n' >&3; exit 1; }
printf '0123 populated upgrade, historical preservation, fresh erasure, authority removal, actual-role readiness, drift rejection and administrator restore passed.\n' >&3
