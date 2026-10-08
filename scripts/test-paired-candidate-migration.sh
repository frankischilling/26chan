#!/usr/bin/env bash
# Owned synthetic PostgreSQL 16 cluster; populated 0115 -> 0116 only.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-paired-candidate.XXXXXXXX)
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
 [[ $cluster =~ ^/tmp/board-paired-candidate\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
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
trap 'printf "Paired candidate qualification failed at line %s (private diagnostics removed during cleanup).\n" "$LINENO" >&3' ERR
runuser -u postgres -- "${psql[@]}" -d postgres -f - < deploy/roles.sql
runuser -u postgres -- "${psql[@]}" -d postgres <<'SQL'
ALTER ROLE board_staff LOGIN;
ALTER ROLE board_auth LOGIN;
ALTER ROLE board_media_read LOGIN;
ALTER ROLE board_media_intake LOGIN;
ALTER ROLE board_monitor LOGIN;
CREATE DATABASE paired_candidate OWNER board_migrator;
REVOKE ALL ON DATABASE paired_candidate FROM PUBLIC;
GRANT CONNECT ON DATABASE paired_candidate TO board_migrator,board_media,board_public,board_staff,board_auth,board_media_read,board_media_intake,board_monitor;
SQL
migrator=("${psql[@]}" -U board_migrator -d paired_candidate)
admin=(runuser -u postgres -- "${psql[@]}" -d paired_candidate)
for migration in migrations/*.sql; do
 [[ $migration < migrations/0116_ ]] || break
 "${migrator[@]}" --single-transaction -f - < "$migration"
done
# Seed both legacy history and pre-existing paired receipts before 0116.
"${migrator[@]}" <<'SQL'
INSERT INTO media.jobs(id,filename,state,input_bytes,attempts,lease_token,expires_at,output_sha256,output_bytes,failure)
SELECT lpad(n::text,32,'0'),'qualification-historical.png',state,
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
"${psql[@]}" -U board_media_intake -d paired_candidate <<'SQL'
DO $$ BEGIN
 IF current_user <> 'board_media_intake' OR session_user <> 'board_media_intake'
 OR media_intake.ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Intake login is not ready'; END IF;
END $$;
DO $$ DECLARE r record; n integer; BEGIN
 FOR n IN 1..3 LOOP
  SELECT * INTO r FROM media_intake.reserve_pair('qualification-pair-'||n);
  IF n=2 THEN
   PERFORM media_intake.begin_pair_upload(r.id,r.capability);
   PERFORM media_intake.finish_pair_upload(r.id,r.capability,58,repeat('a',64),1,repeat('b',64),1,repeat('c',64));
  ELSIF n=3 THEN PERFORM media_intake.abort_upload(r.id,r.capability); END IF;
 END LOOP;
END $$;
SQL
# Keep snapshots outside application schemas, without runtime grants.
"${admin[@]}" <<'SQL'
CREATE FUNCTION public.capture_rows() RETURNS TABLE(relation text,value jsonb)
LANGUAGE plpgsql AS $$ DECLARE r record; BEGIN
 FOR r IN SELECT n.nspname,c.relname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname NOT IN('public','information_schema') AND n.nspname !~ '^pg_' AND c.relkind='r' LOOP
  RETURN QUERY EXECUTE format('SELECT %L,to_jsonb(r)%s FROM %I.%I r',r.nspname||'.'||r.relname,
   '',r.nspname,r.relname);
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
migration=(migrations/0116_*.sql)
[[ ${#migration[@]} = 1 && -f ${migration[0]} ]]
"${migrator[@]}" -c BEGIN -f - -c ROLLBACK < "${migration[0]}"
"${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(TABLE public.before_rows EXCEPT ALL SELECT * FROM public.capture_rows())
 OR EXISTS(SELECT * FROM public.capture_rows() EXCEPT ALL TABLE public.before_rows)
 OR EXISTS(TABLE public.before_catalog EXCEPT ALL TABLE public.catalog_snapshot)
 OR EXISTS(TABLE public.catalog_snapshot EXCEPT ALL TABLE public.before_catalog)
 THEN RAISE EXCEPTION 'Rolled-back 0116 changed rows or catalog'; END IF;
END $$;
SQL
"${migrator[@]}" --single-transaction -f - < "${migration[0]}"
"${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF (SELECT count(*) FROM media.jobs) <> 8
 OR (SELECT count(*) FROM media.assets) <> 1
 OR EXISTS(TABLE public.before_rows EXCEPT ALL SELECT * FROM public.capture_rows())
 OR EXISTS(SELECT * FROM public.capture_rows() EXCEPT ALL TABLE public.before_rows)
 THEN RAISE EXCEPTION '0116 changed historical v1/paired rows'; END IF;
 IF EXISTS(SELECT * FROM public.before_catalog b
   WHERE NOT (b.kind='constraint' AND b.value->>0 IN ('media_input_shape','jobs_failure_check'))
   EXCEPT ALL TABLE public.catalog_snapshot)
 THEN RAISE EXCEPTION '0116 changed unrelated catalog or grants'; END IF;
 IF EXISTS(SELECT 1 FROM pg_proc p WHERE p.oid IN('media.claim_paired_candidate()'::regprocedure,'media.finish_paired_candidate(text,text,boolean)'::regprocedure)
   AND (p.proowner<>'board_migrator'::regrole OR NOT p.prosecdef
    OR p.proconfig IS DISTINCT FROM ARRAY['search_path=pg_catalog, pg_temp']::text[]
    OR NOT has_function_privilege('board_media',p.oid,'EXECUTE')
    OR has_function_privilege('board_media',p.oid,'EXECUTE WITH GRANT OPTION')
    OR EXISTS(SELECT 1 FROM aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a
      WHERE a.grantee NOT IN(p.proowner,'board_media'::regrole))))
 THEN RAISE EXCEPTION 'Candidate functions have broader authority'; END IF;
 IF EXISTS(SELECT 1 FROM public.catalog_snapshot a
   WHERE NOT EXISTS(SELECT 1 FROM public.before_catalog b WHERE (a.kind,a.key)=(b.kind,b.key))
   AND NOT (
     (a.kind='constraint' AND a.key IN(SELECT oid::text FROM pg_constraint
       WHERE conrelid='media.jobs'::regclass AND conname IN('media_input_shape','jobs_failure_check')))
     OR (a.kind='function' AND a.key IN('media.guard_paired_claim()'::regprocedure::oid::text,
       'media.claim_paired_candidate()'::regprocedure::oid::text,
       'media.finish_paired_candidate(text,text,boolean)'::regprocedure::oid::text))
     OR (a.kind='trigger' AND a.key IN(SELECT oid::text FROM pg_trigger
       WHERE tgrelid='media.jobs'::regclass AND tgname='media_paired_claim_guard'))
   )) THEN RAISE EXCEPTION '0116 added unexpected schema or authority'; END IF;
 IF EXISTS(SELECT 1 FROM pg_proc p WHERE p.oid='media.guard_paired_claim()'::regprocedure
   AND (p.proowner<>'board_migrator'::regrole OR p.prosecdef
     OR p.proconfig IS DISTINCT FROM ARRAY['search_path=pg_catalog']::text[]
     OR EXISTS(SELECT 1 FROM aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a
       WHERE a.grantee<>p.proowner)))
 THEN RAISE EXCEPTION 'Candidate guard has broader authority'; END IF;
END $$;
SQL
# Use actual runtime logins for candidate processing and denied callers.
"${psql[@]}" -U board_media -d paired_candidate <<'SQL'
DO $$ BEGIN
 IF current_user <> 'board_media' OR session_user <> 'board_media'
 THEN RAISE EXCEPTION 'Candidate test is not using the runtime login'; END IF;
END $$;
BEGIN ISOLATION LEVEL REPEATABLE READ;
DO $$ BEGIN
 BEGIN PERFORM media.claim_paired_candidate();
   RAISE EXCEPTION 'Unsupported isolation claimed candidate' USING ERRCODE='ZX001';
 EXCEPTION WHEN invalid_parameter_value THEN NULL; END;
END $$;
ROLLBACK;
DO $$ DECLARE j media.jobs%ROWTYPE; old_token text; pending_id text; BEGIN
 SELECT id INTO STRICT pending_id FROM media.jobs WHERE filename='qualification-pair-2';
 BEGIN UPDATE media.jobs SET expires_at=clock_timestamp()+interval '2 hours' WHERE id=pending_id;
   RAISE EXCEPTION 'Queued expiry extension admitted' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN UPDATE media.jobs SET attempts=1 WHERE id=pending_id;
   RAISE EXCEPTION 'Direct queued attempt mutation admitted' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 SELECT * INTO STRICT j FROM media.claim_paired_candidate();
 IF j.id IS DISTINCT FROM pending_id OR j.state IS DISTINCT FROM 'processing'
 OR j.attempts IS DISTINCT FROM 1 OR j.lease_token IS NULL
 OR j.input_bytes IS DISTINCT FROM 58 OR j.input_image_bytes IS DISTINCT FROM 1
 OR j.input_replay_bytes IS DISTINCT FROM 1
 THEN RAISE EXCEPTION 'Typed claim changed receipt or lease shape'; END IF;
 old_token:=j.lease_token;
 IF media.finish_paired_candidate(j.id,NULL,true) IS DISTINCT FROM false
 OR media.finish_paired_candidate(j.id,repeat('0',32),true) IS DISTINCT FROM false
 OR media.finish_paired_candidate(j.id,j.lease_token,NULL) IS DISTINCT FROM false
 OR media.finish_paired_candidate(repeat('0',32),j.lease_token,true) IS DISTINCT FROM false
 THEN RAISE EXCEPTION 'Missing/wrong candidate credentials admitted'; END IF;
 IF EXISTS(SELECT * FROM media.claim_paired_candidate()) THEN RAISE EXCEPTION 'One job claimed twice'; END IF;
 BEGIN UPDATE media.jobs SET attempts=0 WHERE id=j.id;
   RAISE EXCEPTION 'Attempt counter reset admitted' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN UPDATE media.jobs SET lease_token=repeat('f',32) WHERE id=j.id;
   RAISE EXCEPTION 'Direct lease replacement admitted' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN UPDATE media.jobs SET expires_at=clock_timestamp()+interval '1 hour' WHERE id=j.id;
   RAISE EXCEPTION 'Processing lease extension admitted' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN UPDATE media.jobs SET state='queued',lease_token=NULL,expires_at=clock_timestamp()+interval '2 hours' WHERE id=j.id;
   RAISE EXCEPTION 'Retry expiry beyond one hour admitted' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN UPDATE media.jobs SET state='failed',failure='candidate_checked',lease_token=NULL,expires_at=NULL WHERE id=j.id;
   RAISE EXCEPTION 'Direct checked marker admitted' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN UPDATE media.jobs SET state='published',output_sha256=repeat('f',64),output_bytes=1,expires_at=NULL WHERE id=j.id;
   RAISE EXCEPTION 'Candidate published through direct SQL' USING ERRCODE='ZX001';
 EXCEPTION WHEN check_violation THEN NULL; END;
 BEGIN INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height)
   VALUES(repeat('f',32),j.id,j.lease_token,repeat('f',64),1,1,1);
   RAISE EXCEPTION 'Candidate asset admitted' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 -- A normal current-lease retry remains supported, with no new attempt until claim.
 UPDATE media.jobs SET state='queued',lease_token=NULL,expires_at=clock_timestamp()+interval '1 hour' WHERE id=j.id;
 SELECT * INTO STRICT j FROM media.claim_paired_candidate();
 IF j.id IS DISTINCT FROM pending_id OR j.state IS DISTINCT FROM 'processing'
 OR j.attempts IS DISTINCT FROM 2 OR j.lease_token IS NULL
 OR j.lease_token IS NOT DISTINCT FROM old_token
 OR media.finish_paired_candidate(j.id,old_token,true) IS DISTINCT FROM false
 THEN RAISE EXCEPTION 'Second attempt is not fenced'; END IF;
 UPDATE media.jobs SET state='queued',lease_token=NULL,expires_at=clock_timestamp()+interval '1 hour' WHERE id=j.id;
 SELECT * INTO STRICT j FROM media.claim_paired_candidate();
 IF j.id IS DISTINCT FROM pending_id OR j.state IS DISTINCT FROM 'processing'
 OR j.attempts IS DISTINCT FROM 3 OR j.lease_token IS NULL
 THEN RAISE EXCEPTION 'Third attempt not counted'; END IF;
 BEGIN UPDATE media.jobs SET state='queued',lease_token=NULL,expires_at=clock_timestamp()+interval '1 hour' WHERE id=j.id;
   RAISE EXCEPTION 'Fourth attempt retry admitted' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 IF media.finish_paired_candidate(j.id,j.lease_token,true) IS DISTINCT FROM true
 THEN RAISE EXCEPTION 'Checked candidate completion failed'; END IF;
 IF media.finish_paired_candidate(j.id,j.lease_token,true) IS DISTINCT FROM false
 THEN RAISE EXCEPTION 'Checked candidate completion not fenced'; END IF;
 IF NOT EXISTS(SELECT 1 FROM media.jobs WHERE id=j.id AND state='failed' AND failure='candidate_checked'
 AND lease_token IS NULL AND expires_at IS NULL AND output_sha256 IS NULL AND output_bytes IS NULL)
 THEN RAISE EXCEPTION 'Checked candidate acquired publication state'; END IF;
 BEGIN UPDATE media.jobs SET state='queued',failure=NULL,expires_at=clock_timestamp()+interval '1 hour' WHERE id=j.id;
   RAISE EXCEPTION 'Terminal candidate revived' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN INSERT INTO media.jobs(id,filename,state,failure) VALUES(repeat('f',32),'qualification-forged.png','failed','candidate_checked');
   RAISE EXCEPTION 'Image-v1 forged candidate marker' USING ERRCODE='ZX001';
 EXCEPTION WHEN check_violation THEN NULL; END;
END $$;
SQL
for role in board_public board_staff board_auth board_media_read board_media_intake board_monitor; do
 "${psql[@]}" -U "$role" -d paired_candidate <<'SQL'
DO $$ BEGIN
 BEGIN PERFORM media.claim_paired_candidate();
   RAISE EXCEPTION 'Unscoped role claimed candidate' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN PERFORM media.finish_paired_candidate(repeat('0',32),repeat('0',32),true);
   RAISE EXCEPTION 'Unscoped role completed candidate' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SQL
done
printf 'Paired candidate migration passed: populated legacy and paired history, full rollback, scoped catalog and authority, actual-role claims, bounded retries and publication bypass denials. Private cluster will be removed.\n' >&3
