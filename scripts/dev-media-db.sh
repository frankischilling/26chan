#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run with sudo against the disposable development cluster.' >&2; exit 1; }
[[ -f .local/database.env && -f .local/cluster-path ]] || { echo 'Run scripts/dev-db.sh first.' >&2; exit 1; }
[[ ! -e .local/media.env && ! -e .local/media.ps1 ]] || { echo 'Media credentials already exist; reuse them.' >&2; exit 1; }
cluster=$(cat .local/cluster-path)
[[ $cluster =~ ^/tmp/board-postgres\.[[:alnum:]]+$ && -d $cluster ]] || { echo 'Expected a disposable cluster path.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
actual=$(runuser -u postgres -- "$pg_bin/psql" -XAt -h /tmp -p 55432 -d postgres -c 'SHOW data_directory')
[[ $actual = "$cluster" ]] || { echo 'Port 55432 belongs to a different cluster.' >&2; exit 1; }
media_password=$(openssl rand -hex 24)
runuser -u postgres -- "$pg_bin/psql" -X -v ON_ERROR_STOP=1 -h /tmp -p 55432 -d postgres -v media_password="$media_password" <<'SQL'
SELECT 'CREATE ROLE board_media LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS'
WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'board_media')
\gexec
ALTER ROLE board_media PASSWORD :'media_password';
ALTER ROLE board_media SET statement_timeout = '5s';
ALTER ROLE board_media SET lock_timeout = '2s';
ALTER ROLE board_media SET idle_in_transaction_session_timeout = '5s';
ALTER ROLE board_media SET search_path = pg_catalog;
GRANT CONNECT ON DATABASE imageboard TO board_media;
SQL
umask 077
set -o noclobber
printf 'export MEDIA_DATABASE_URL=%q\n' "postgres://board_media:$media_password@127.0.0.1:55432/imageboard" > .local/media.env
printf '\044env:MEDIA_DATABASE_URL = '\''postgres://board_media:%s@127.0.0.1:55432/imageboard'\''\n' "$media_password" > .local/media.ps1
echo 'Separate development media credential written to ignored .local/media.env and .local/media.ps1.'
