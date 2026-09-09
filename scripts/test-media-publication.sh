#!/usr/bin/env bash
# Operator-mediated qualification; this is not authenticated queue dispatch.
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run on an owned disposable Linux host as root.' >&2; exit 1; }
: "${MEDIA_VM_TEST_CONFIG:?Set the reviewed disposable guest configuration}"
source .local/database.env
source .local/media.env
source .local/media-reader.env
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(cat .local/cluster-path)
[[ $cluster =~ ^/tmp/board-postgres\.[[:alnum:]]+$ && -d $cluster ]] || exit 1
actual=$(runuser -u postgres -- "$pg_bin/psql" -XAt -h /tmp -p 55432 -d postgres -c 'SHOW data_directory')
[[ $actual = "$cluster" ]] || { echo 'Expected disposable database on port 55432.' >&2; exit 1; }
pending=$("$pg_bin/psql" "$MIGRATION_DATABASE_URL" -XAt -v ON_ERROR_STOP=1 -c "SELECT count(*) FROM media.jobs WHERE state IN ('receiving','queued','processing')")
[[ $pending = 0 ]] || { echo 'Use an idle disposable media queue.' >&2; exit 1; }
umask 077
fixture=$(mktemp -d "$PWD/.local/media-publication.XXXXXXXX")
[[ $fixture == "$PWD/.local/media-publication."* && -d $fixture ]] || exit 1
job_id=''
cleanup() {
  if [[ $job_id =~ ^[0-9a-f]{32}$ ]]; then
    "$pg_bin/psql" "$MIGRATION_DATABASE_URL" -Xq -v ON_ERROR_STOP=1 -v job_id="$job_id" <<'SQL'
DELETE FROM media.assets WHERE job_id=:'job_id';
DELETE FROM media.jobs WHERE id=:'job_id';
SQL
  fi
  # Only this invocation's mktemp directory, verified against the fixed prefix.
  [[ $fixture == "$PWD/.local/media-publication."* && -d $fixture ]] && rm -rf -- "$fixture"
}
trap cleanup EXIT
mkdir "$fixture/quarantine"
python3 - "$fixture/input.png" <<'PY'
import pathlib, sys
sys.path.insert(0, 'tests/media')
from test_vm import red_png
pathlib.Path(sys.argv[1]).write_bytes(red_png())
PY
writer=(env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin APP_ENV=development MEDIA_DATABASE_URL="$MEDIA_DATABASE_URL" MEDIA_QUARANTINE_DIR="$fixture/quarantine")
intake=$("${writer[@]}" target/debug/board-media-admin intake "$fixture/input.png" synthetic.png)
job_id=$(python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])' <<<"$intake")
[[ $job_id =~ ^[0-9a-f]{32}$ ]] || exit 1
"${writer[@]}" target/debug/media-publish claim "$fixture/lease.json"
# The root runner sees no database environment, manifest or publication storage.
env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin python3 scripts/media/run-job.py \
  "$MEDIA_VM_TEST_CONFIG" "$fixture/quarantine/$job_id.input" "$fixture/quarantine/result.disk"
asset_id=$("${writer[@]}" target/debug/media-publish publish "$fixture/lease.json" "$fixture/quarantine/result.disk" "$fixture/objects")
[[ $asset_id =~ ^[0-9a-f]{32}$ && $asset_id != "$job_id" ]] || exit 1
env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin APP_ENV=development MEDIA_READ_DATABASE_URL="$MEDIA_READ_DATABASE_URL" \
  target/debug/media-read "$asset_id" "$fixture/objects" "$fixture/read.png"
cmp "$fixture/objects/$asset_id.png" "$fixture/read.png"
"$pg_bin/psql" "$MIGRATION_DATABASE_URL" -Xq -v ON_ERROR_STOP=1 -v job_id="$job_id" -v asset_id="$asset_id" <<'SQL'
SELECT 1 / count(*) FROM media.assets WHERE id=:'asset_id' AND job_id=:'job_id' AND state='approved' AND width=1 AND height=1;
DELETE FROM media.jobs WHERE id=:'job_id' AND state='published';
SQL
env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin APP_ENV=development MEDIA_READ_DATABASE_URL="$MEDIA_READ_DATABASE_URL" \
  target/debug/media-read "$asset_id" "$fixture/objects" "$fixture/read-after-queue-cleanup.png"
cmp "$fixture/read.png" "$fixture/read-after-queue-cleanup.png"
printf 'Actual VM publication passed: intake, private lease manifest, stopped-guest validation, durable approval, restricted read and read after queue removal. Operator handoff only.\n'
