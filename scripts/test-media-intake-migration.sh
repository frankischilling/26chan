#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root against the disposable development cluster.' >&2; exit 1; }
source .local/database.env
source .local/media.env
source .local/media-reader.env
source .local/intake.env
cluster=$(cat .local/cluster-path)
[[ $cluster =~ ^/tmp/board-postgres\.[[:alnum:]]+$ && -d $cluster && ! -L $cluster ]] || exit 1
pg_bin=/usr/lib/postgresql/16/bin
admin=(runuser -u postgres -- "$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h /tmp -p 55432 -d postgres)
actual=$("${admin[@]}" -At -c 'SHOW data_directory')
[[ $actual = "$cluster" ]] || { echo 'Port 55432 belongs to a different cluster.' >&2; exit 1; }
upgrade_db="imageboard_intake_upgrade_$(date +%s)_$RANDOM"
[[ $upgrade_db =~ ^imageboard_intake_upgrade_[0-9]+_[0-9]+$ ]] || exit 1
created=0
cleanup() {
  if [[ $created = 1 ]]; then
    "${admin[@]}" -v upgrade_db="$upgrade_db" <<'SQL'
DROP DATABASE :"upgrade_db";
SQL
    created=0
  fi
}
trap cleanup EXIT
"${admin[@]}" -v upgrade_db="$upgrade_db" <<'SQL'
CREATE DATABASE :"upgrade_db" OWNER board_migrator;
REVOKE ALL ON DATABASE :"upgrade_db" FROM PUBLIC;
GRANT CONNECT ON DATABASE :"upgrade_db" TO board_migrator, board_media, board_media_read, board_media_intake;
SQL
created=1
upgrade_url="${MIGRATION_DATABASE_URL%/imageboard}/$upgrade_db"
db=("$pg_bin/psql" "$upgrade_url" -Xq -v ON_ERROR_STOP=1)
for migration in migrations/000{1,2,3,4,5,6,7,8,9}_*.sql migrations/0010_*.sql; do
  "${db[@]}" --single-transaction -f "$migration"
done
"${db[@]}" <<'SQL'
INSERT INTO media.jobs(id,filename,expires_at)
VALUES (repeat('1',32),'legacy receiving.png',clock_timestamp() + interval '5 minutes');
INSERT INTO media.jobs(id,filename,state,input_bytes,expires_at)
VALUES (repeat('2',32),'legacy queued.png','queued',9,clock_timestamp() + interval '1 hour');
INSERT INTO media.jobs(id,filename,state,input_bytes,attempts,lease_token,output_sha256,output_bytes)
VALUES (repeat('3',32),'legacy published.png','published',9,1,repeat('4',32),repeat('a',64),100);
INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at)
VALUES (repeat('5',32),repeat('3',32),repeat('4',32),repeat('a',64),100,1,1,'approved',clock_timestamp()),
       (repeat('6',32),repeat('3',32),repeat('7',32),repeat('b',64),101,1,1,'pending',NULL);
CREATE TABLE public.intake_jobs_baseline AS SELECT * FROM media.jobs;
CREATE TABLE public.intake_assets_baseline AS SELECT * FROM media.assets;
SQL
# Exercise transactional rollback before the successful upgrade, using the real
# migration login. No new schema, handle or grant may survive this transaction.
"${db[@]}" -c BEGIN -f migrations/0011_media_intake.sql -c ROLLBACK
"${db[@]}" <<'SQL'
DO $$ BEGIN
  IF to_regnamespace('media_intake') IS NOT NULL THEN
    RAISE EXCEPTION 'Rolled-back intake migration left its schema installed';
  END IF;
  IF EXISTS (
    (SELECT * FROM media.jobs EXCEPT SELECT * FROM public.intake_jobs_baseline)
    UNION ALL (SELECT * FROM public.intake_jobs_baseline EXCEPT SELECT * FROM media.jobs)
  ) OR EXISTS (
    (SELECT * FROM media.assets EXCEPT SELECT * FROM public.intake_assets_baseline)
    UNION ALL (SELECT * FROM public.intake_assets_baseline EXCEPT SELECT * FROM media.assets)
  ) THEN RAISE EXCEPTION 'Rollback changed existing queue or approval data'; END IF;
END $$;
SQL
"${db[@]}" --single-transaction -f migrations/0011_media_intake.sql
"${db[@]}" <<'SQL'
DO $$ BEGIN
  IF EXISTS (SELECT 1 FROM media_intake.handles) THEN
    RAISE EXCEPTION 'Upgrade fabricated capabilities for existing jobs';
  END IF;
  IF EXISTS (
    (SELECT * FROM media.jobs EXCEPT SELECT * FROM public.intake_jobs_baseline)
    UNION ALL (SELECT * FROM public.intake_jobs_baseline EXCEPT SELECT * FROM media.jobs)
  ) OR EXISTS (
    (SELECT * FROM media.assets EXCEPT SELECT * FROM public.intake_assets_baseline)
    UNION ALL (SELECT * FROM public.intake_assets_baseline EXCEPT SELECT * FROM media.assets)
  ) THEN RAISE EXCEPTION 'Upgrade changed existing queue or approval data'; END IF;
END $$;
SQL
intake_url="${INTAKE_DATABASE_URL%/imageboard}/$upgrade_db"
"$pg_bin/psql" "$intake_url" -Xq -v ON_ERROR_STOP=1 <<'SQL'
SELECT media_intake.ready() AS ready \gset
\if :ready
\else
  SELECT 1/0;
\endif
SELECT id, capability FROM media_intake.reserve('upgrade-synthetic.png') \gset upload_
SELECT media_intake.begin_upload(:'upload_id', :'upload_capability');
SELECT media_intake.finish_upload(:'upload_id', :'upload_capability', 9);
SELECT state = 'queued' AND input_bytes = 9 AND output_id IS NULL AS queued
FROM media_intake.status(:'upload_id', :'upload_capability') \gset
\if :queued
\else
  SELECT 1/0;
\endif
SQL
umask 077
for statement in \
  "SELECT * FROM media_intake.status(repeat('1',32),repeat('0',64))" \
  "SELECT media_intake.begin_upload(repeat('2',32),repeat('0',64))"; do
  if "$pg_bin/psql" "$intake_url" -Xq -v ON_ERROR_STOP=1 -v VERBOSITY=sqlstate -c "$statement" > .local/intake-migration-denial.txt 2>&1; then
    echo 'Intake accessed an existing operator job after upgrade.' >&2; exit 1
  fi
  grep -q 'P0002' .local/intake-migration-denial.txt
done
if "$pg_bin/psql" "$intake_url" -Xq -v ON_ERROR_STOP=1 -v VERBOSITY=sqlstate -c 'SELECT * FROM media.jobs' > .local/intake-migration-denial.txt 2>&1; then
  echo 'Intake can access the queue table after upgrade.' >&2; exit 1
fi
grep -q '42501' .local/intake-migration-denial.txt

# Existing processing and approval controls still work through their own login.
media_url="${MEDIA_DATABASE_URL%/imageboard}/$upgrade_db"
"$pg_bin/psql" "$media_url" -Xq -v ON_ERROR_STOP=1 <<'SQL'
BEGIN;
SELECT id FROM media.jobs WHERE id = repeat('2',32) AND state = 'queued' FOR UPDATE;
UPDATE media.jobs SET state = 'processing', attempts = 1, lease_token = repeat('8',32),
    expires_at = clock_timestamp() + interval '30 seconds' WHERE id = repeat('2',32);
INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height)
VALUES (repeat('9',32),repeat('2',32),repeat('8',32),repeat('c',64),99,1,1);
UPDATE media.jobs SET state = 'published', output_sha256 = repeat('c',64), output_bytes = 99,
    expires_at = NULL WHERE id = repeat('2',32);
UPDATE media.assets SET state = 'approved', approved_at = clock_timestamp() WHERE id = repeat('9',32);
COMMIT;
SQL
reader_url="${MEDIA_READ_DATABASE_URL%/imageboard}/$upgrade_db"
actual=$("$pg_bin/psql" "$reader_url" -XAt -v ON_ERROR_STOP=1 -c "SELECT string_agg(id,',' ORDER BY id) FROM media.approved_assets")
[[ $actual = 55555555555555555555555555555555,99999999999999999999999999999999 ]] || { echo 'Approved-only reader behavior changed after intake migration.' >&2; exit 1; }
cleanup
trap - EXIT
printf 'Media intake migration passed: 0010 jobs and approvals preserved, rollback safe, no fabricated handles, actual intake and processing logins work, and reader exposes approvals only. Disposable database removed.\n'
