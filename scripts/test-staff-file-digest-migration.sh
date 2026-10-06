#!/usr/bin/env bash
# Historical 0106 -> 0107 boundary, owned synthetic data only; no application/PHP execution.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-staff-digest.XXXXXXXX)
started=0
cleanup() {
 if [[ $started = 1 ]]; then
  runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null
 fi
 [[ $cluster =~ ^/tmp/board-staff-digest\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
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
exec 3>&1
exec > "$cluster/qualification.log" 2>&1
trap 'printf "Staff digest qualification failed at line %s (private diagnostics removed during cleanup).\n" "$LINENO" >&3' ERR
runuser -u postgres -- "${psql[@]}" -d postgres -f - < deploy/roles.sql
runuser -u postgres -- "${psql[@]}" -d postgres -c 'ALTER ROLE board_staff LOGIN; ALTER ROLE board_auth LOGIN'
create_database() {
 runuser -u postgres -- "${psql[@]}" -d postgres -v database="$1" <<'SQL'
CREATE DATABASE :"database" OWNER board_migrator;
REVOKE ALL ON DATABASE :"database" FROM PUBLIC;
GRANT CONNECT ON DATABASE :"database" TO board_migrator,board_public,board_staff,board_auth;
SQL
}
cat > "$cluster/fixture.sql" <<'SQL'
BEGIN;
SET LOCAL ROLE board_migrator;
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,archive_retention_seconds)
VALUES('digest','Synthetic digest fixture','Owned qualification',1000,100,100,100,10,86400);
SELECT set_config('board.posting_actor',repeat('11',32),true);
INSERT INTO content.threads(id,board) SELECT 10700000+i,'digest' FROM generate_series(1,10) i;
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
SELECT 10700000+i,'digest',10700000+i,'Synthetic','','Retained body' FROM generate_series(1,10) i;
COMMIT;
-- 1 approved; 2 removed file; 3 deleted post; 4 deleted thread;
-- 5 expired archive; 6 pending; 7 deleting; 8 missing asset;
-- 9 legacy NULL manifest; 10 retained archive.
INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at,
 md5,thumbnail_sha256,thumbnail_bytes,thumbnail_width,thumbnail_height)
SELECT lpad(i::text,32,'0'),lpad(i::text,32,'0'),lpad(i::text,32,'0'),repeat('a',64),100,500,300,
 CASE i WHEN 6 THEN 'pending' WHEN 7 THEN 'deleting' ELSE 'approved' END,
 CASE WHEN i IN(6,7) THEN NULL ELSE clock_timestamp() END,
 CASE WHEN i=9 THEN NULL ELSE repeat('a',32) END,
 CASE WHEN i=9 THEN NULL ELSE repeat('b',64) END,
 CASE WHEN i=9 THEN NULL ELSE 50 END,CASE WHEN i=9 THEN NULL ELSE 100 END,CASE WHEN i=9 THEN NULL ELSE 60 END
FROM generate_series(1,10) i WHERE i<>8;
INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler,file_deleted)
SELECT 10700000+i,lpad(i::text,32,'0'),lpad(i::text,32,'0'),'synthetic.png',100,500,300,false,i=2
FROM generate_series(1,10) i;
UPDATE content.posts SET deleted=true WHERE id=10700003;
UPDATE content.threads SET deleted=true WHERE id=10700004;
UPDATE content.threads SET archived_at=clock_timestamp()-interval '2 days',archive_expires_at=clock_timestamp()-interval '1 day' WHERE id=10700005;
UPDATE content.threads SET archived_at=clock_timestamp()-interval '1 hour',archive_expires_at=clock_timestamp()+interval '1 day' WHERE id=10700010;
SQL
cat > "$cluster/staff.sql" <<'SQL'
DO $$ BEGIN
 IF (SELECT count(*) FROM content.staff_post_media)<>10
 OR (SELECT md5 FROM content.staff_post_media WHERE post_id=10700001) IS DISTINCT FROM repeat('a',32)
 OR (SELECT md5 FROM content.staff_post_media WHERE post_id=10700010) IS DISTINCT FROM repeat('a',32)
 OR EXISTS(SELECT 1 FROM content.staff_post_media WHERE post_id BETWEEN 10700002 AND 10700009 AND md5 IS NOT NULL)
 OR EXISTS(SELECT 1 FROM content.staff_post_media WHERE md5 IS NOT NULL AND (md5 !~ '^[0-9a-f]{32}$' OR octet_length(md5)<>32 OR NOT available))
 THEN RAISE EXCEPTION 'Staff normalized digest visibility mismatch'; END IF;
END $$;
SQL
cat > "$cluster/denied.sql" <<'SQL'
DO $$ BEGIN
 BEGIN PERFORM 1 FROM media.assets LIMIT 1;
  RAISE EXCEPTION 'Runtime read raw assets' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN PERFORM 1 FROM content.post_media LIMIT 1;
  RAISE EXCEPTION 'Runtime read raw attachment table' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SQL
for mode in upgrade fresh; do
 database="staff_digest_$mode"
 create_database "$database"
 migrator=("${psql[@]}" -U board_migrator -d "$database")
 admin=(runuser -u postgres -- "${psql[@]}" -d "$database")
 for migration in migrations/*.sql; do
  [[ $migration < migrations/0107_staff_file_digest.sql ]] || break
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
CREATE VIEW public.view_metadata AS
SELECT 'relation'::text kind,jsonb_build_array(pg_get_userbyid(c.relowner),c.reloptions,
 (SELECT jsonb_agg(jsonb_build_array(pg_get_userbyid(a.grantor),CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
 ORDER BY pg_get_userbyid(a.grantor),CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
 FROM aclexplode(coalesce(c.relacl,acldefault('r',c.relowner))) a)) value
FROM pg_class c WHERE c.oid='content.staff_post_media'::regclass
UNION ALL
SELECT 'column',to_jsonb(c)-'table_catalog'-'udt_catalog' FROM information_schema.columns c
WHERE table_schema='content' AND table_name='staff_post_media' AND ordinal_position<=10
UNION ALL
SELECT 'column-grant',to_jsonb(c)-'table_catalog'-'udt_catalog' FROM information_schema.column_privileges c
WHERE table_schema='content' AND table_name='staff_post_media' AND column_name<>'md5';
SQL
 if [[ $mode = upgrade ]]; then "${admin[@]}" -f - < "$cluster/fixture.sql"; fi
 "${admin[@]}" <<'SQL'
CREATE TABLE public.before_rows AS SELECT * FROM public.capture_rows();
CREATE TABLE public.before_metadata AS TABLE public.view_metadata;
CREATE TABLE public.before_reads AS SELECT to_jsonb(m) value FROM content.staff_post_media m;
DO $$ BEGIN
 IF (SELECT array_agg(column_name::text ORDER BY ordinal_position) FROM information_schema.columns
 WHERE table_schema='content' AND table_name='staff_post_media') IS DISTINCT FROM
 ARRAY['post_id','filename','bytes','width','height','spoiler','tim','thumbnail_width','thumbnail_height','available']
 THEN RAISE EXCEPTION 'Unexpected historical ten-column view'; END IF;
END $$;
SQL
 "${migrator[@]}" --single-transaction -f - < migrations/0107_staff_file_digest.sql
 "${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(TABLE public.before_rows EXCEPT ALL SELECT * FROM public.capture_rows())
 OR EXISTS(SELECT * FROM public.capture_rows() EXCEPT ALL TABLE public.before_rows)
 OR EXISTS(TABLE public.before_metadata EXCEPT ALL TABLE public.view_metadata)
 OR EXISTS(TABLE public.view_metadata EXCEPT ALL TABLE public.before_metadata)
 OR EXISTS(TABLE public.before_reads EXCEPT ALL SELECT to_jsonb(m)-'md5' FROM content.staff_post_media m)
 OR EXISTS(SELECT to_jsonb(m)-'md5' FROM content.staff_post_media m EXCEPT ALL TABLE public.before_reads)
 THEN RAISE EXCEPTION '0107 changed retained data, old reads or metadata'; END IF;
 IF (SELECT count(*) FROM information_schema.columns WHERE table_schema='content' AND table_name='staff_post_media')<>11
 OR NOT EXISTS(SELECT 1 FROM information_schema.columns WHERE table_schema='content' AND table_name='staff_post_media'
 AND column_name='md5' AND ordinal_position=11 AND data_type='text' AND is_nullable='YES')
 OR NOT EXISTS(SELECT 1 FROM pg_class WHERE oid='content.staff_post_media'::regclass AND reloptions @> ARRAY['security_barrier=true'])
 THEN RAISE EXCEPTION 'Missing nullable additive digest or security barrier'; END IF;
END $$;
DROP TABLE public.before_rows,public.before_metadata,public.before_reads;
SQL
 if [[ $mode = fresh ]]; then "${admin[@]}" -f - < "$cluster/fixture.sql"; fi
 for phase in live restored; do
  if [[ $phase = restored ]]; then
   runuser -u postgres -- "$pg_bin/pg_dump" -h "$cluster" -d "$database" --format=custom > "$cluster/current.dump"
   create_database "${database}_restore"
   runuser -u postgres -- "$pg_bin/pg_restore" -h "$cluster" --dbname="${database}_restore" --single-transaction --exit-on-error < "$cluster/current.dump"
   database="${database}_restore"
   admin=(runuser -u postgres -- "${psql[@]}" -d "$database")
  fi
  "${psql[@]}" -U board_staff -d "$database" -f - < "$cluster/staff.sql"
  for role in board_staff board_public board_auth; do
   "${psql[@]}" -U "$role" -d "$database" -f - < "$cluster/denied.sql"
  done
  for role in board_public board_auth; do
   "${psql[@]}" -U "$role" -d "$database" <<'SQL'
DO $$ BEGIN
 BEGIN PERFORM md5 FROM content.staff_post_media LIMIT 1;
  RAISE EXCEPTION 'Nonstaff read staff digest view' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SQL
  done
  "${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM pg_class c CROSS JOIN LATERAL aclexplode(coalesce(c.relacl,acldefault('r',c.relowner))) a
 WHERE c.oid IN('media.assets'::regclass,'content.post_media'::regclass,'content.staff_post_media'::regclass) AND a.grantee=0)
 THEN RAISE EXCEPTION 'PUBLIC has raw/staff view grant'; END IF;
END $$;
-- Put the deadline strictly between transaction start and the next read's
-- wall clock. Own writes remain visible under REPEATABLE READ; no timed race.
BEGIN ISOLATION LEVEL REPEATABLE READ;
SET LOCAL ROLE board_staff;
DO $$ BEGIN
 IF (SELECT md5 FROM content.staff_post_media WHERE post_id=10700010) IS NULL THEN RAISE EXCEPTION 'Retained archive digest missing before expiry'; END IF;
END $$;
RESET ROLE;
UPDATE content.threads SET archive_expires_at=clock_timestamp() WHERE id=10700010;
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM content.threads WHERE id=10700010
 AND archive_expires_at>transaction_timestamp() AND archive_expires_at<clock_timestamp())
 THEN RAISE EXCEPTION 'Expiry fixture did not establish both clock bounds'; END IF;
END $$;
SET LOCAL ROLE board_staff;
DO $$ BEGIN
 IF (SELECT available FROM content.staff_post_media WHERE post_id=10700010) IS DISTINCT FROM true
 OR (SELECT md5 FROM content.staff_post_media WHERE post_id=10700010) IS NOT NULL
 THEN RAISE EXCEPTION 'Wall-clock digest expiry changed historical availability or disclosed expired digest'; END IF;
END $$;
ROLLBACK;
SQL
  "${admin[@]}" -At <<'SQL' > "$cluster/$mode-$phase.fingerprint"
SELECT jsonb_build_array('row',relation,value)::text FROM public.capture_rows() ORDER BY relation,value::text;
SELECT jsonb_build_array('metadata',kind,value)::text FROM public.view_metadata ORDER BY kind,value::text;
SELECT (to_jsonb(c)-'table_catalog'-'udt_catalog')::text FROM information_schema.columns c WHERE table_schema='content' AND table_name='staff_post_media' ORDER BY ordinal_position;
SELECT pg_get_viewdef('content.staff_post_media'::regclass,true);
SET ROLE board_staff;
SELECT to_jsonb(m)::text FROM content.staff_post_media m ORDER BY post_id;
SQL
 done
 cmp -s "$cluster/$mode-live.fingerprint" "$cluster/$mode-restored.fingerprint" || {
  printf 'Staff digest dump/restore fingerprint mismatch (%s); bounded synthetic metadata diff follows.\n' "$mode" >&3
  diff -u --label live --label restored "$cluster/$mode-live.fingerprint" "$cluster/$mode-restored.fingerprint" > "$cluster/fingerprint.diff" || true
  sed -n '1,100p' "$cluster/fingerprint.diff" >&3
  exit 1
 }
 printf '%s staff digest migration, role reads, expiry and administrator dump/restore passed.\n' "$mode" >&3
 done
cleanup
trap - EXIT
printf 'Staff digest qualification passed; private cluster removed.\n' >&3
