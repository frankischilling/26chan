#!/usr/bin/env bash
# Owned synthetic PostgreSQL 16 cluster; populated 0111 -> 0112 only.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-source-provenance.XXXXXXXX)
started=0
exec 3>&1
cleanup() {
 status=$?
 trap - EXIT
 if [[ $started = 1 ]]; then
  runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null || status=1
 fi
 [[ $cluster =~ ^/tmp/board-source-provenance\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
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
trap 'printf "Source provenance qualification failed at line %s (private diagnostics removed during cleanup).\n" "$LINENO" >&3' ERR
runuser -u postgres -- "${psql[@]}" -d postgres -f - < deploy/roles.sql
runuser -u postgres -- "${psql[@]}" -d postgres <<'SQL'
ALTER ROLE board_staff LOGIN;
ALTER ROLE board_auth LOGIN;
ALTER ROLE board_media_read LOGIN;
ALTER ROLE board_media_intake LOGIN;
ALTER ROLE board_monitor LOGIN;
CREATE DATABASE source_provenance OWNER board_migrator;
REVOKE ALL ON DATABASE source_provenance FROM PUBLIC;
GRANT CONNECT ON DATABASE source_provenance TO board_migrator,board_media,board_public,board_staff,board_auth,board_media_read,board_media_intake,board_monitor;
SQL
migrator=("${psql[@]}" -U board_migrator -d source_provenance)
admin=(runuser -u postgres -- "${psql[@]}" -d source_provenance)
for migration in migrations/*.sql; do
 [[ $migration < migrations/0112_ ]] || break
 "${migrator[@]}" --single-transaction -f - < "$migration"
done
"${migrator[@]}" <<'SQL'
BEGIN;
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('prov','Owned provenance fixture','Synthetic',1000,100,100,100,10);
SELECT set_config('board.posting_actor',repeat('11',32),true);
INSERT INTO content.threads(id,board) SELECT 11200000+i,'prov' FROM generate_series(1,4) i;
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
SELECT 11200000+i,'prov',11200000+i,'Synthetic','','Retained body' FROM generate_series(1,4) i;
-- Historical PNG, manual PNG, JPEG and GIF receipts all predate provenance.
INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at,
 md5,thumbnail_sha256,thumbnail_bytes,thumbnail_width,thumbnail_height)
SELECT lpad(i::text,32,'0'),lpad(i::text,32,'0'),lpad(i::text,32,'0'),repeat('a',64),100,500,300,
 'approved',clock_timestamp(),repeat('b',32),repeat('c',64),50,100,60 FROM generate_series(1,4) i;
INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler)
SELECT 11200000+i,lpad(i::text,32,'0'),lpad(i::text,32,'0'),
 CASE i WHEN 3 THEN 'synthetic.jpg' WHEN 4 THEN 'synthetic.gif' ELSE 'synthetic.png' END,
 100,500,300,false FROM generate_series(1,4) i;
INSERT INTO media.jobs(id,filename,state,input_bytes,attempts,lease_token,expires_at)
VALUES(repeat('e',32),'synthetic.png','processing',100,1,repeat('f',32),clock_timestamp()+interval '1 hour');
COMMIT;
SQL
# Helpers live outside application schemas and never receive runtime grants.
"${admin[@]}" <<'SQL'
CREATE FUNCTION public.capture_rows() RETURNS TABLE(relation text,value jsonb)
LANGUAGE plpgsql AS $$ DECLARE r record; BEGIN
 FOR r IN SELECT n.nspname,c.relname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname NOT IN('public','information_schema') AND n.nspname !~ '^pg_' AND c.relkind='r' LOOP
  RETURN QUERY EXECUTE format('SELECT %L,to_jsonb(r)%s FROM %I.%I r',r.nspname||'.'||r.relname,
   CASE WHEN r.nspname='media' AND r.relname='assets' THEN
    ' - ARRAY[''source_input_sha256'',''source_input_bytes'',''source_profile'',''source_retained_bytes'',''source_md5'']' ELSE '' END,r.nspname,r.relname);
 END LOOP;
END $$;
REVOKE ALL ON FUNCTION public.capture_rows() FROM PUBLIC;
CREATE VIEW public.catalog_snapshot AS
SELECT 'relation'::text kind,c.oid::text key,jsonb_build_array(n.nspname,c.relname,c.relkind,c.relowner,c.relacl,c.reloptions,c.relrowsecurity,c.relforcerowsecurity,
 CASE WHEN c.relkind='v' THEN pg_get_viewdef(c.oid,true) ELSE NULL END) value
FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
WHERE n.nspname NOT IN('public','information_schema') AND n.nspname !~ '^pg_'
UNION ALL
SELECT 'column',a.attrelid||':'||a.attnum,to_jsonb(a) FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace
WHERE n.nspname NOT IN('public','information_schema') AND n.nspname !~ '^pg_' AND a.attnum>0
UNION ALL
SELECT 'constraint',c.oid::text,jsonb_build_array(c.conname,c.conrelid,pg_get_constraintdef(c.oid,true)) FROM pg_constraint c JOIN pg_namespace n ON n.oid=c.connamespace
WHERE n.nspname NOT IN('public','information_schema') AND n.nspname !~ '^pg_'
UNION ALL
SELECT 'function',p.oid::text,jsonb_build_array(p.proowner,p.proacl,pg_get_functiondef(p.oid)) FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
WHERE n.nspname NOT IN('public','information_schema') AND n.nspname !~ '^pg_'
UNION ALL
SELECT 'trigger',t.oid::text,jsonb_build_array(t.tgenabled,pg_get_triggerdef(t.oid,true)) FROM pg_trigger t JOIN pg_class c ON c.oid=t.tgrelid JOIN pg_namespace n ON n.oid=c.relnamespace
WHERE n.nspname NOT IN('public','information_schema') AND n.nspname !~ '^pg_'
UNION ALL
SELECT 'schema',n.oid::text,jsonb_build_array(n.nspname,n.nspowner,n.nspacl) FROM pg_namespace n
WHERE n.nspname NOT IN('public','information_schema') AND n.nspname !~ '^pg_'
UNION ALL
SELECT 'default-acl',d.oid::text,to_jsonb(d) FROM pg_default_acl d;
CREATE TABLE public.before_rows AS SELECT * FROM public.capture_rows();
CREATE TABLE public.before_catalog AS TABLE public.catalog_snapshot;
CREATE TABLE public.before_reads AS
 SELECT 'public'::text kind,to_jsonb(m) value FROM content.visible_post_media m
 UNION ALL SELECT 'staff',to_jsonb(m) FROM content.staff_post_media m
 UNION ALL SELECT 'reader',to_jsonb(m) FROM media.approved_assets m;
SQL
migration=(migrations/0112_*.sql)
[[ ${#migration[@]} = 1 && -f ${migration[0]} ]]
"${migrator[@]}" -c BEGIN -f - -c ROLLBACK < "${migration[0]}"
"${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(TABLE public.before_rows EXCEPT ALL SELECT * FROM public.capture_rows())
 OR EXISTS(SELECT * FROM public.capture_rows() EXCEPT ALL TABLE public.before_rows)
 OR EXISTS(TABLE public.before_catalog EXCEPT ALL TABLE public.catalog_snapshot)
 OR EXISTS(TABLE public.catalog_snapshot EXCEPT ALL TABLE public.before_catalog)
 THEN RAISE EXCEPTION 'Rolled-back 0112 changed rows or catalog'; END IF;
END $$;
SQL
"${migrator[@]}" --single-transaction -f - < "${migration[0]}"
"${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(TABLE public.before_rows EXCEPT ALL SELECT * FROM public.capture_rows())
 OR EXISTS(SELECT * FROM public.capture_rows() EXCEPT ALL TABLE public.before_rows)
 OR EXISTS(TABLE public.before_catalog EXCEPT ALL TABLE public.catalog_snapshot)
 THEN RAISE EXCEPTION '0112 changed retained rows, output checksums or existing schema/ACLs'; END IF;
 IF EXISTS(SELECT 1 FROM media.assets WHERE source_input_sha256 IS NOT NULL OR source_input_bytes IS NOT NULL
 OR source_profile IS NOT NULL OR source_retained_bytes IS NOT NULL OR source_md5 IS NOT NULL)
 THEN RAISE EXCEPTION '0112 fabricated historical provenance'; END IF;
 IF EXISTS(TABLE public.before_reads EXCEPT ALL
 (SELECT 'public',to_jsonb(m) FROM content.visible_post_media m UNION ALL
 SELECT 'staff',to_jsonb(m) FROM content.staff_post_media m UNION ALL
 SELECT 'reader',to_jsonb(m) FROM media.approved_assets m))
 OR EXISTS((SELECT 'public',to_jsonb(m) FROM content.visible_post_media m UNION ALL
 SELECT 'staff',to_jsonb(m) FROM content.staff_post_media m UNION ALL
 SELECT 'reader',to_jsonb(m) FROM media.approved_assets m) EXCEPT ALL TABLE public.before_reads)
 THEN RAISE EXCEPTION '0112 changed public/staff/reader output'; END IF;
END $$;
SQL
"${admin[@]}" <<'SQL'
DO $$ DECLARE additions integer; BEGIN
 SELECT count(*) INTO additions FROM public.catalog_snapshot a
 WHERE NOT EXISTS(SELECT 1 FROM public.before_catalog b WHERE (a.kind,a.key)=(b.kind,b.key));
 IF additions<>8 THEN RAISE EXCEPTION 'Unexpected catalog addition count: %',additions; END IF;
 IF EXISTS(SELECT 1 FROM public.catalog_snapshot a
 WHERE NOT EXISTS(SELECT 1 FROM public.before_catalog b WHERE (a.kind,a.key)=(b.kind,b.key))
 AND NOT (
  (a.kind='column' AND EXISTS(SELECT 1 FROM pg_attribute x WHERE x.attrelid='media.assets'::regclass
   AND x.attrelid||':'||x.attnum=a.key AND x.attname IN('source_input_sha256','source_input_bytes','source_profile','source_retained_bytes','source_md5')
   AND NOT x.attnotnull AND NOT x.atthasdef AND x.attacl IS NULL))
  OR (a.kind='constraint' AND a.key=(SELECT oid::text FROM pg_constraint WHERE conrelid='media.assets'::regclass AND conname='source_provenance'))
  OR (a.kind='function' AND a.key='media.guard_source_provenance()'::regprocedure::oid::text)
  OR (a.kind='trigger' AND a.key=(SELECT oid::text FROM pg_trigger WHERE tgrelid='media.assets'::regclass AND tgname='media_source_provenance_immutable'))
 )) THEN RAISE EXCEPTION 'Unexpected new schema object or authority'; END IF;
 IF EXISTS(SELECT 1 FROM pg_proc p CROSS JOIN LATERAL aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a
 WHERE p.oid='media.guard_source_provenance()'::regprocedure AND a.grantee<>p.proowner)
 OR EXISTS(SELECT 1 FROM pg_proc WHERE oid='media.guard_source_provenance()'::regprocedure AND prosecdef)
 THEN RAISE EXCEPTION 'Provenance function expanded authority'; END IF;
END $$;
SQL
cat > "$cluster/tuple.sql" <<'SQL'
BEGIN;
DO $$ DECLARE base jsonb; candidate jsonb; item record; k text; mask integer; i integer;
 keys text[]:=ARRAY['source_input_sha256','source_input_bytes','source_profile','source_retained_bytes','source_md5'];
BEGIN
 SELECT to_jsonb(a)||jsonb_build_object('id',repeat('d',32),'job_id',repeat('e',32),
  'lease_token',repeat('f',32),'state','pending','approved_at',NULL,
  'source_input_sha256',repeat('1',64),'source_input_bytes',100,'source_profile','png-v1',
  'source_retained_bytes',80,'source_md5','\x'||repeat('2',32)) INTO base
 FROM media.assets a WHERE id=lpad('1',32,'0');
 -- Every one of the 30 nonempty incomplete subsets must fail.
 FOR mask IN 1..30 LOOP
  candidate:=base;
  FOR i IN 1..5 LOOP
   IF (mask & (1 << (i-1)))=0 THEN candidate:=candidate||jsonb_build_object(keys[i],NULL); END IF;
  END LOOP;
  BEGIN
   INSERT INTO media.assets SELECT * FROM jsonb_populate_record(NULL::media.assets,candidate);
   RAISE EXCEPTION 'Partial tuple accepted: %',mask USING ERRCODE='ZX001';
  EXCEPTION WHEN check_violation OR insufficient_privilege THEN NULL; END;
 END LOOP;
 FOR item IN SELECT * FROM (VALUES
  ('source_input_sha256',to_jsonb(repeat('A',64))),('source_input_sha256',to_jsonb(repeat('1',63))),
  ('source_input_sha256',to_jsonb(repeat('1',65))),('source_input_sha256',to_jsonb(repeat('z',64))),
  ('source_input_bytes','0'::jsonb),('source_input_bytes','19'::jsonb),('source_input_bytes','8388609'::jsonb),
  ('source_profile','"PNG-v1"'::jsonb),('source_profile','"jpeg-v1"'::jsonb),
  ('source_retained_bytes','0'::jsonb),('source_retained_bytes','19'::jsonb),('source_retained_bytes','101'::jsonb),
  ('source_md5',to_jsonb('\x'||repeat('2',30))),('source_md5',to_jsonb('\x'||repeat('2',34)))
 ) invalid(key,value) LOOP
  BEGIN
   INSERT INTO media.assets SELECT * FROM jsonb_populate_record(NULL::media.assets,base||jsonb_build_object(item.key,item.value));
   RAISE EXCEPTION 'Invalid tuple accepted: %',item.key USING ERRCODE='ZX001';
  EXCEPTION WHEN check_violation OR insufficient_privilege THEN NULL; END;
 END LOOP;
 FOR item IN SELECT * FROM (VALUES
  ('job_id',to_jsonb(repeat('9',32))),('lease_token',to_jsonb(repeat('9',32))),
  ('source_input_bytes','99'::jsonb),('state','"deleting"'::jsonb)
 ) invalid(key,value) LOOP
  BEGIN
   INSERT INTO media.assets SELECT * FROM jsonb_populate_record(NULL::media.assets,base||jsonb_build_object(item.key,item.value));
   RAISE EXCEPTION 'Wrong job/lease/length/state accepted: %',item.key USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 END LOOP;
 UPDATE media.jobs SET expires_at=clock_timestamp()-interval '1 second' WHERE id=repeat('e',32);
 BEGIN
  INSERT INTO media.assets SELECT * FROM jsonb_populate_record(NULL::media.assets,base);
  RAISE EXCEPTION 'Expired lease accepted' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 UPDATE media.jobs SET expires_at=clock_timestamp()+interval '1 hour' WHERE id=repeat('e',32);
 UPDATE media.jobs SET state='queued',lease_token=NULL WHERE id=repeat('e',32);
 BEGIN
  INSERT INTO media.assets SELECT * FROM jsonb_populate_record(NULL::media.assets,base);
  RAISE EXCEPTION 'Nonprocessing job accepted' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 UPDATE media.jobs SET state='processing',lease_token=repeat('f',32) WHERE id=repeat('e',32);
 -- Exact minimum/maximum, including retained=input, are valid and rollback.
 FOREACH i IN ARRAY ARRAY[20,8388608] LOOP
  UPDATE media.jobs SET input_bytes=i WHERE id=repeat('e',32);
  INSERT INTO media.assets SELECT * FROM jsonb_populate_record(NULL::media.assets,
   base||jsonb_build_object('source_input_bytes',i,'source_retained_bytes',i));
  DELETE FROM media.assets WHERE id=repeat('d',32);
 END LOOP;
 UPDATE media.jobs SET input_bytes=100 WHERE id=repeat('e',32);
 INSERT INTO media.assets SELECT * FROM jsonb_populate_record(NULL::media.assets,base);
 -- Immutable even while pending, before the older approved-row guard applies.
 FOREACH k IN ARRAY keys LOOP
  BEGIN
   EXECUTE format('UPDATE media.assets SET %I=NULL WHERE id=$1',k) USING repeat('d',32);
   RAISE EXCEPTION 'Tuple component changed: %',k USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 END LOOP;
 BEGIN
  UPDATE media.assets SET source_input_sha256=repeat('3',64),source_input_bytes=99,
   source_profile='png-v1',source_retained_bytes=79,source_md5=decode(repeat('4',32),'hex') WHERE id=repeat('d',32);
  RAISE EXCEPTION 'Complete tuple changed' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN
  UPDATE media.assets SET source_input_sha256=repeat('1',64),source_input_bytes=100,
   source_profile='png-v1',source_retained_bytes=80,source_md5=decode(repeat('2',32),'hex') WHERE id=lpad('1',32,'0');
  RAISE EXCEPTION 'Historical tuple backfilled' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 -- Manual/JPEG/GIF publication remains possible without fabricated tuples.
 UPDATE media.assets SET source_md5=source_md5 WHERE id=repeat('d',32);
 UPDATE media.assets SET state='approved',approved_at=clock_timestamp() WHERE id=repeat('d',32);
 IF NOT EXISTS(SELECT 1 FROM media.assets WHERE id=repeat('d',32) AND state='approved'
  AND source_input_sha256=repeat('1',64) AND source_input_bytes=100 AND source_profile='png-v1'
  AND source_retained_bytes=80 AND source_md5=decode(repeat('2',32),'hex')
  AND sha256=repeat('a',64) AND md5=repeat('b',32))
 THEN RAISE EXCEPTION 'Approval changed source or output identity'; END IF;
 BEGIN
  UPDATE media.assets SET source_md5=decode(repeat('3',32),'hex') WHERE id=repeat('d',32);
  RAISE EXCEPTION 'Approved source changed' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 FOR i IN 5..7 LOOP
  candidate:=base||jsonb_build_object('id',lpad(i::text,32,'0'),'job_id',lpad(i::text,32,'0'));
  FOREACH k IN ARRAY keys LOOP candidate:=candidate||jsonb_build_object(k,NULL); END LOOP;
  INSERT INTO media.assets SELECT * FROM jsonb_populate_record(NULL::media.assets,candidate);
 END LOOP;
END $$;
ROLLBACK;
SQL
for role in board_media board_migrator; do
 "${psql[@]}" -U "$role" -d source_provenance -f - < "$cluster/tuple.sql"
done
for role in board_public board_staff board_auth board_media_read board_media_intake board_monitor; do
 "${psql[@]}" -U "$role" -d source_provenance <<'SQL'
DO $$ BEGIN
 BEGIN PERFORM source_md5 FROM media.assets;
  RAISE EXCEPTION 'Runtime read private provenance' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN UPDATE media.assets SET source_md5=NULL;
  RAISE EXCEPTION 'Runtime wrote private provenance' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SQL
done
"${psql[@]}" -U board_public -d source_provenance -c 'SELECT * FROM content.visible_post_media' > "$cluster/public.read"
"${psql[@]}" -U board_staff -d source_provenance -c 'SELECT * FROM content.staff_post_media' > "$cluster/staff.read"
"${psql[@]}" -U board_media_read -d source_provenance -c 'SELECT * FROM media.approved_assets' > "$cluster/reader.read"
"${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(TABLE public.before_rows EXCEPT ALL SELECT * FROM public.capture_rows())
 OR EXISTS(SELECT * FROM public.capture_rows() EXCEPT ALL TABLE public.before_rows)
 THEN RAISE EXCEPTION 'Qualification changed retained rows or output checksums'; END IF;
END $$;
SQL
printf 'PNG provenance qualification passed: populated upgrade, rollback, retained rows and output, schema/ACLs, projections, tuple subsets/bounds, lease binding and immutability. Private cluster will be removed.\n' >&3
