#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run with sudo against the disposable development cluster.' >&2; exit 1; }
[[ -f .local/database.env && -f .local/cluster-path ]] || { echo 'Run scripts/dev-db.sh first.' >&2; exit 1; }
[[ ! -e .local/media-reader.env && ! -e .local/media-reader.ps1 ]] || { echo 'Media reader credentials already exist; reuse them.' >&2; exit 1; }
cluster=$(cat .local/cluster-path)
[[ $cluster =~ ^/tmp/board-postgres\.[[:alnum:]]+$ && -d $cluster ]] || { echo 'Expected a disposable cluster path.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
actual=$(runuser -u postgres -- "$pg_bin/psql" -XAt -h /tmp -p 55432 -d postgres -c 'SHOW data_directory')
[[ $actual = "$cluster" ]] || { echo 'Port 55432 belongs to a different cluster.' >&2; exit 1; }
reader_password=$(openssl rand -hex 24)
runuser -u postgres -- "$pg_bin/psql" -X -v ON_ERROR_STOP=1 -h /tmp -p 55432 -d postgres -v reader_password="$reader_password" <<'SQL'
SELECT 'CREATE ROLE board_media_read LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS'
WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'board_media_read')
\gexec
ALTER ROLE board_media_read PASSWORD :'reader_password';
ALTER ROLE board_media_read SET statement_timeout = '5s';
ALTER ROLE board_media_read SET lock_timeout = '2s';
ALTER ROLE board_media_read SET idle_in_transaction_session_timeout = '5s';
ALTER ROLE board_media_read SET search_path = pg_catalog;
GRANT CONNECT ON DATABASE imageboard TO board_media_read;
SQL
umask 077
set -o noclobber
printf 'export MEDIA_READ_DATABASE_URL=%q\n' "postgres://board_media_read:$reader_password@127.0.0.1:55432/imageboard" > .local/media-reader.env
printf '\044env:MEDIA_READ_DATABASE_URL = '\''postgres://board_media_read:%s@127.0.0.1:55432/imageboard'\''\n' "$reader_password" > .local/media-reader.ps1
echo 'Separate development media reader credential written to ignored .local/media-reader.env and .local/media-reader.ps1.'
