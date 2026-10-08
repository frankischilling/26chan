#!/usr/bin/env bash
# Owned synthetic PostgreSQL 16 cluster; populated 0114 -> 0115 only.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-paired-provenance.XXXXXXXX)
started=0
exec 3>&1
cleanup() {
 status=$?
 trap - EXIT
 if [[ $started = 1 ]]; then
  if ! runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null; then
   printf 'Owned PostgreSQL shutdown failed; private cluster retained for inspection.\n' >&3
   exit 1
  fi
 fi
 [[ $cluster =~ ^/tmp/board-paired-provenance\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
 [[ $(readlink -f "$cluster") = "$cluster" ]] || exit 1
 rm -rf -- "$cluster"
 exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
chown postgres:postgres "$cluster"
runuser -u postgres -- "$pg_bin/initdb" -D "$cluster/data" --auth=trust --encoding=UTF8 --no-locale > /dev/null
# A failed or interrupted startup may already own a live postmaster.
# Cleanup must attempt shutdown before considering private-file removal.
started=1
runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" \
 -o "-c listen_addresses='' -c unix_socket_directories='$cluster' -c statement_timeout=30000 -c lock_timeout=5000" -w start > /dev/null
psql=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h "$cluster")
exec > "$cluster/qualification.log" 2>&1
trap 'printf "Paired input provenance qualification failed at line %s (private diagnostics removed during cleanup).\n" "$LINENO" >&3' ERR
runuser -u postgres -- "${psql[@]}" -d postgres -f - < deploy/roles.sql
runuser -u postgres -- "${psql[@]}" -d postgres <<'SQL'
ALTER ROLE board_staff LOGIN;
ALTER ROLE board_auth LOGIN;
ALTER ROLE board_media_read LOGIN;
ALTER ROLE board_media_intake LOGIN;
ALTER ROLE board_monitor LOGIN;
CREATE DATABASE paired_provenance OWNER board_migrator;
REVOKE ALL ON DATABASE paired_provenance FROM PUBLIC;
GRANT CONNECT ON DATABASE paired_provenance TO board_migrator,board_media,board_public,board_staff,board_auth,board_media_read,board_media_intake,board_monitor;
SQL
migrator=("${psql[@]}" -U board_migrator -d paired_provenance)
admin=(runuser -u postgres -- "${psql[@]}" -d paired_provenance)
for migration in migrations/*.sql; do
 [[ $migration < migrations/0115_ ]] || break
 "${migrator[@]}" --single-transaction -f - < "$migration"
done
"${migrator[@]}" <<'SQL'
INSERT INTO media.jobs(id,filename,state,input_bytes,attempts,lease_token,expires_at,output_sha256,output_bytes,failure)
SELECT lpad(n::text,32,'0'),'historical.png',state,
 CASE WHEN state IN ('queued','processing','published') THEN 8388608 END,
 CASE WHEN state IN ('processing','published') THEN 1 ELSE 0 END,
 CASE WHEN state IN ('processing','published') THEN repeat('a',32) END,
 CASE WHEN state IN ('receiving','queued','processing') THEN clock_timestamp()+interval '1 hour' END,
 CASE WHEN state='published' THEN repeat('a',64) END,
 CASE WHEN state='published' THEN 100 END,
 CASE WHEN state='failed' THEN 'abandoned' END
FROM (VALUES (1,'receiving'),(2,'queued'),(3,'processing'),(4,'published'),(5,'failed')) AS s(n,state);
INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at)
VALUES(repeat('a',32),lpad('4',32,'0'),repeat('a',32),repeat('a',64),100,10,10,'approved',clock_timestamp());
SQL
# Helpers live outside application schemas and never receive runtime grants.
"${admin[@]}" <<'SQL'
CREATE FUNCTION public.capture_rows() RETURNS TABLE(relation text,value jsonb)
LANGUAGE plpgsql AS $$ DECLARE r record; BEGIN
 FOR r IN SELECT n.nspname,c.relname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname NOT IN('public','information_schema') AND n.nspname !~ '^pg_' AND c.relkind='r' LOOP
  RETURN QUERY EXECUTE format('SELECT %L,to_jsonb(r)%s FROM %I.%I r',r.nspname||'.'||r.relname,
   CASE WHEN r.nspname='media' AND r.relname='jobs' THEN
    ' - ARRAY[''input_kind'',''input_sha256'',''input_image_bytes'',''input_image_sha256'',''input_replay_bytes'',''input_replay_sha256'']' ELSE '' END,r.nspname,r.relname);
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
SELECT 'default-acl',d.oid::text,to_jsonb(d) FROM pg_default_acl d
UNION ALL SELECT 'policy',p.oid::text,to_jsonb(p) FROM pg_policy p;
CREATE TABLE public.before_rows AS SELECT * FROM public.capture_rows();
CREATE TABLE public.before_catalog AS TABLE public.catalog_snapshot;
SQL
migration=(migrations/0115_*.sql)
[[ ${#migration[@]} = 1 && -f ${migration[0]} ]]
"${migrator[@]}" -c BEGIN -f - -c ROLLBACK < "${migration[0]}"
"${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(TABLE public.before_rows EXCEPT ALL SELECT * FROM public.capture_rows())
 OR EXISTS(SELECT * FROM public.capture_rows() EXCEPT ALL TABLE public.before_rows)
 OR EXISTS(TABLE public.before_catalog EXCEPT ALL TABLE public.catalog_snapshot)
 OR EXISTS(TABLE public.catalog_snapshot EXCEPT ALL TABLE public.before_catalog)
 THEN RAISE EXCEPTION 'Rolled-back 0115 changed rows, schema or authority'; END IF;
END $$;
SQL
"${migrator[@]}" --single-transaction -f - < "${migration[0]}"
"${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF (SELECT count(*) FROM media.jobs) <> 5
 OR EXISTS(TABLE public.before_rows EXCEPT ALL SELECT * FROM public.capture_rows())
 OR EXISTS(SELECT * FROM public.capture_rows() EXCEPT ALL TABLE public.before_rows)
 OR EXISTS(SELECT 1 FROM media.jobs WHERE input_kind <> 'image-v1' OR input_sha256 IS NOT NULL
    OR input_image_bytes IS NOT NULL OR input_image_sha256 IS NOT NULL
    OR input_replay_bytes IS NOT NULL OR input_replay_sha256 IS NOT NULL)
 THEN RAISE EXCEPTION '0115 changed historical rows or fabricated paired provenance'; END IF;
 -- Only the old v1 function bodies and the replaced size constraint may change.
 IF EXISTS(SELECT * FROM public.before_catalog b
   WHERE NOT ((b.kind='function' AND b.key IN ('media_intake.begin_upload(text,text)'::regprocedure::oid::text,
       'media_intake.finish_upload(text,text,bigint)'::regprocedure::oid::text))
     OR (b.kind='constraint' AND b.value->>0='jobs_input_bytes_check'))
   EXCEPT ALL TABLE public.catalog_snapshot)
 THEN RAISE EXCEPTION '0115 changed unrelated catalog, policies or authority'; END IF;
 IF EXISTS(SELECT 1 FROM public.catalog_snapshot a
   WHERE NOT EXISTS(SELECT 1 FROM public.before_catalog b WHERE (a.kind,a.key)=(b.kind,b.key))
   AND NOT (
     (a.kind='column' AND EXISTS(SELECT 1 FROM pg_attribute x WHERE x.attrelid='media.jobs'::regclass
       AND x.attrelid||':'||x.attnum=a.key AND x.attname IN('input_kind','input_sha256','input_image_bytes','input_image_sha256','input_replay_bytes','input_replay_sha256')))
     OR (a.kind='constraint' AND a.key=(SELECT oid::text FROM pg_constraint WHERE conrelid='media.jobs'::regclass AND conname='media_input_shape'))
     OR (a.kind='function' AND a.key IN('media.guard_input_descriptor()'::regprocedure::oid::text,
       'media.guard_inactive_pair_asset()'::regprocedure::oid::text,
       'media_intake.reserve_pair(text)'::regprocedure::oid::text,
       'media_intake.begin_pair_upload(text,text)'::regprocedure::oid::text,
       'media_intake.finish_pair_upload(text,text,bigint,text,bigint,text,bigint,text)'::regprocedure::oid::text))
     OR (a.kind='trigger' AND EXISTS(SELECT 1 FROM pg_trigger t WHERE t.oid::text=a.key
       AND ((t.tgrelid='media.jobs'::regclass AND t.tgname='media_input_descriptor_immutable')
         OR (t.tgrelid='media.assets'::regclass AND t.tgname='media_inactive_pair_asset'))))
   )) THEN RAISE EXCEPTION '0115 added unexpected schema or authority'; END IF;
 IF EXISTS(SELECT 1 FROM pg_proc p WHERE p.oid IN (
     'media_intake.reserve_pair(text)'::regprocedure,'media_intake.begin_pair_upload(text,text)'::regprocedure,
     'media_intake.finish_pair_upload(text,text,bigint,text,bigint,text,bigint,text)'::regprocedure,
     'media_intake.begin_upload(text,text)'::regprocedure,'media_intake.finish_upload(text,text,bigint)'::regprocedure)
   AND (p.proowner <> 'board_media_intake_owner'::regrole OR NOT p.prosecdef
     OR p.proconfig IS DISTINCT FROM ARRAY['search_path=pg_catalog, pg_temp']::text[]
     OR NOT has_function_privilege('board_media_intake',p.oid,'EXECUTE')
     OR has_function_privilege('board_media_intake',p.oid,'EXECUTE WITH GRANT OPTION')
     OR EXISTS(SELECT 1 FROM aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) acl
       WHERE acl.grantee NOT IN (p.proowner,'board_media_intake'::regrole))))
 OR has_schema_privilege('board_media_intake_owner','media_intake','CREATE')
 OR EXISTS(SELECT 1 FROM pg_roles WHERE rolname='board_media_intake_owner' AND rolcanlogin)
 THEN RAISE EXCEPTION 'Paired definer privileges are not narrowly scoped'; END IF;
 IF EXISTS(SELECT 1 FROM pg_attribute a
   CROSS JOIN (VALUES('SELECT'),('INSERT'),('UPDATE'),('REFERENCES')) AS priv(name)
   WHERE a.attrelid='media.jobs'::regclass
     AND a.attname IN('input_kind','input_sha256','input_image_bytes','input_image_sha256','input_replay_bytes','input_replay_sha256')
     AND (has_column_privilege('board_media_intake',a.attrelid,a.attnum,priv.name)
       OR has_column_privilege('board_media_intake_owner',a.attrelid,a.attnum,priv.name)
         <> (priv.name='SELECT' OR (priv.name='INSERT' AND a.attname='input_kind')
           OR (priv.name='UPDATE' AND a.attname<>'input_kind'))))
 OR EXISTS(SELECT 1 FROM pg_attribute a CROSS JOIN LATERAL aclexplode(a.attacl) acl
   WHERE a.attrelid='media.jobs'::regclass
     AND a.attname IN('input_kind','input_sha256','input_image_bytes','input_image_sha256','input_replay_bytes','input_replay_sha256')
     AND (acl.grantee<>'board_media_intake_owner'::regrole OR acl.is_grantable))
 THEN RAISE EXCEPTION 'New descriptor column grants exceed the intake contract'; END IF;

END $$;
SQL
# Use actual runtime logins, rather than relying on SET ROLE or catalog assertions alone.
"${psql[@]}" -U board_media_intake -d paired_provenance <<'SQL'
DO $$ DECLARE r record; legacy record; BEGIN
 IF current_user <> 'board_media_intake' OR session_user <> 'board_media_intake'
 OR NOT media_intake.ready() THEN RAISE EXCEPTION 'Intake login is not ready'; END IF;
 BEGIN PERFORM id FROM media.jobs;
   RAISE EXCEPTION 'Intake read base jobs' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN PERFORM * FROM media_intake.handles;
   RAISE EXCEPTION 'Intake read bearer hashes' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 SELECT * INTO r FROM media_intake.reserve_pair('owned-paired.png');
 BEGIN PERFORM media_intake.begin_upload(r.id,r.capability);
   RAISE EXCEPTION 'Legacy begin accepted paired job' USING ERRCODE='ZX001';
 EXCEPTION WHEN raise_exception THEN NULL; END;
 PERFORM media_intake.begin_pair_upload(r.id,r.capability);
 BEGIN PERFORM media_intake.finish_pair_upload(r.id,repeat('0',64),NULL,NULL,NULL,NULL,NULL,NULL);
   RAISE EXCEPTION 'Invalid bearer admitted' USING ERRCODE='ZX001';
 EXCEPTION WHEN no_data_found THEN NULL; END;
 BEGIN PERFORM media_intake.finish_pair_upload(r.id,r.capability,58,repeat('a',64),1,repeat('b',64),1,NULL);
   RAISE EXCEPTION 'Partial replay descriptor admitted' USING ERRCODE='ZX001';
 EXCEPTION WHEN invalid_parameter_value THEN NULL; END;
 PERFORM media_intake.finish_pair_upload(r.id,r.capability,58,repeat('a',64),1,repeat('b',64),1,repeat('c',64));
 PERFORM media_intake.finish_pair_upload(r.id,r.capability,58,repeat('a',64),1,repeat('b',64),1,repeat('c',64));
 BEGIN PERFORM media_intake.finish_pair_upload(r.id,r.capability,58,repeat('d',64),1,repeat('b',64),1,repeat('c',64));
   RAISE EXCEPTION 'Changed retry admitted' USING ERRCODE='ZX001';
 EXCEPTION WHEN raise_exception THEN NULL; END;
 SELECT * INTO legacy FROM media_intake.reserve('owned-legacy.png');
 BEGIN PERFORM media_intake.begin_pair_upload(legacy.id,legacy.capability);
   RAISE EXCEPTION 'Paired begin accepted v1 job' USING ERRCODE='ZX001';
 EXCEPTION WHEN raise_exception THEN NULL; END;
 PERFORM media_intake.begin_upload(legacy.id,legacy.capability);
 PERFORM media_intake.finish_upload(legacy.id,legacy.capability,8388608);
 SELECT * INTO r FROM media_intake.reserve_pair('owned-png-only.png');
 PERFORM media_intake.begin_pair_upload(r.id,r.capability);
 PERFORM media_intake.finish_pair_upload(r.id,r.capability,8388664,repeat('d',64),8388608,repeat('e',64),NULL,NULL);
END $$;
SQL
"${psql[@]}" -U board_media -d paired_provenance <<'SQL'
DO $$ DECLARE pair_id text; BEGIN
 SELECT id INTO STRICT pair_id FROM media.jobs WHERE filename='owned-paired.png';
 BEGIN INSERT INTO media.jobs(id,filename,input_kind,expires_at)
   VALUES(repeat('f',32),'forged.png','paired-v2',clock_timestamp()+interval '1 hour');
   RAISE EXCEPTION 'Base role forged paired job' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN UPDATE media.jobs SET input_kind='image-v1' WHERE id=pair_id;
   RAISE EXCEPTION 'Base role changed input kind' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN UPDATE media.jobs SET input_sha256=repeat('f',64) WHERE id=pair_id;
   RAISE EXCEPTION 'Base role rewrote descriptor' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN UPDATE media.jobs SET state='processing',attempts=1,lease_token=repeat('f',32) WHERE id=pair_id;
   RAISE EXCEPTION 'Base role processed pair' USING ERRCODE='ZX001';
 EXCEPTION WHEN check_violation THEN NULL; END;
 BEGIN INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height)
   VALUES(repeat('f',32),pair_id,repeat('f',32),repeat('f',64),1,1,1);
   RAISE EXCEPTION 'Base role created paired asset' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 UPDATE media.jobs SET state='failed',failure='abandoned',expires_at=NULL WHERE id=pair_id;
 BEGIN UPDATE media.jobs SET state='queued',failure=NULL,expires_at=clock_timestamp()+interval '1 hour' WHERE id=pair_id;
   RAISE EXCEPTION 'Base role revived terminal pair' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 -- The original image-only processing path remains available to this role.
 UPDATE media.jobs SET state='processing',attempts=1,lease_token=repeat('f',32),expires_at=clock_timestamp()+interval '30 seconds'
 WHERE filename='owned-legacy.png' AND input_kind='image-v1' AND state='queued';
 IF NOT FOUND THEN RAISE EXCEPTION 'Legacy queue transition failed'; END IF;
END $$;
SQL
for role in board_public board_staff board_auth board_media board_media_read board_monitor; do
 "${psql[@]}" -U "$role" -d paired_provenance <<'SQL'
DO $$ BEGIN
 BEGIN PERFORM * FROM media_intake.reserve_pair('denied.png');
   RAISE EXCEPTION 'Unscoped role reserved pair' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN PERFORM media_intake.finish_pair_upload(repeat('0',32),repeat('0',64),57,repeat('a',64),1,repeat('b',64),NULL,NULL);
   RAISE EXCEPTION 'Unscoped role finalized pair' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SQL
done
printf 'Paired input migration passed: populated five-state history, full rollback, scoped schema and authority, actual-role intake, legacy controls and paired bypass denials. Private cluster will be removed.\n' >&3
