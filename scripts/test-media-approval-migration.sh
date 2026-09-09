#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root against the disposable development cluster.' >&2; exit 1; }
source .local/database.env
source .local/media-reader.env
cluster=$(cat .local/cluster-path)
[[ $cluster =~ ^/tmp/board-postgres\.[[:alnum:]]+$ && -d $cluster ]] || exit 1
pg_bin=/usr/lib/postgresql/16/bin
admin=(runuser -u postgres -- "$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h /tmp -p 55432 -d postgres)
actual=$("${admin[@]}" -At -c 'SHOW data_directory')
[[ $actual = "$cluster" ]] || { echo 'Port 55432 belongs to a different cluster.' >&2; exit 1; }
upgrade_db="imageboard_approval_upgrade_$(date +%s)_$RANDOM"
[[ $upgrade_db =~ ^imageboard_approval_upgrade_[0-9]+_[0-9]+$ ]] || exit 1
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
GRANT CONNECT ON DATABASE :"upgrade_db" TO board_migrator, board_media_read;
SQL
created=1
upgrade_url="${MIGRATION_DATABASE_URL%/imageboard}/$upgrade_db"
db=("$pg_bin/psql" "$upgrade_url" -Xq -v ON_ERROR_STOP=1)
for migration in migrations/000{1,2,3,4,5,6,7}_*.sql; do
  "${db[@]}" --single-transaction -f "$migration"
done
"${db[@]}" <<'SQL'
INSERT INTO media.jobs(id,filename,state,input_bytes,attempts,lease_token,output_sha256,output_bytes)
VALUES (repeat('1',32),'legacy synthetic receipt','published',10,1,repeat('2',32),repeat('a',64),100);
CREATE TABLE public.approval_upgrade_baseline AS SELECT * FROM media.jobs;
SQL
"${db[@]}" --single-transaction -f migrations/0008_media_assets.sql
"${db[@]}" <<'SQL'
DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM media.assets) THEN RAISE EXCEPTION 'Legacy receipt was silently approved'; END IF;
  IF (SELECT count(*) FROM media.jobs) <> 1 OR EXISTS (
    (SELECT * FROM media.jobs EXCEPT SELECT * FROM public.approval_upgrade_baseline)
    UNION ALL
    (SELECT * FROM public.approval_upgrade_baseline EXCEPT SELECT * FROM media.jobs)
  ) THEN RAISE EXCEPTION 'Approval migration changed legacy queue data'; END IF;
END $$;
INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at)
VALUES (repeat('3',32),repeat('1',32),repeat('4',32),repeat('a',64),100,1,1,'approved',clock_timestamp()),
       (repeat('5',32),repeat('1',32),repeat('6',32),repeat('b',64),101,1,1,'pending',NULL);
SQL
reader_url="${MEDIA_READ_DATABASE_URL%/imageboard}/$upgrade_db"
actual=$("$pg_bin/psql" "$reader_url" -XAt -v ON_ERROR_STOP=1 -c 'SELECT id FROM media.approved_assets')
[[ $actual = 33333333333333333333333333333333 ]] || { echo 'Reader view exposed incorrect assets.' >&2; exit 1; }
if "$pg_bin/psql" "$reader_url" -XAt -v ON_ERROR_STOP=1 -c 'SELECT * FROM media.assets' > .local/approval-migration-denial.txt 2>&1; then
  echo 'Reader can access base table after upgrade.' >&2; exit 1
fi
grep -q 'permission denied' .local/approval-migration-denial.txt
cleanup
printf 'Media approval migration passed: 0007 jobs preserved, legacy receipts remain unapproved, reader exposes only approvals and cannot read base records. Disposable database removed.\n'
