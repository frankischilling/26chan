#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run with sudo against the disposable development cluster.' >&2; exit 1; }
[[ -f .local/database.env && -f .local/cluster-path ]] || { echo 'Run scripts/dev-db.sh first.' >&2; exit 1; }
[[ ! -e .local/intake.env && ! -e .local/intake.ps1 ]] || { echo 'Intake credentials already exist; reuse them.' >&2; exit 1; }
cluster=$(cat .local/cluster-path)
[[ $cluster =~ ^/tmp/board-postgres\.[[:alnum:]]+$ && -d $cluster && ! -L $cluster ]] || { echo 'Expected a disposable cluster path.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
actual=$(runuser -u postgres -- "$pg_bin/psql" -XAt -h /tmp -p 55432 -d postgres -c 'SHOW data_directory')
[[ $actual = "$cluster" ]] || { echo 'Port 55432 belongs to a different cluster.' >&2; exit 1; }
BOARD_INTAKE_PASSWORD=$(openssl rand -hex 24)
export BOARD_INTAKE_PASSWORD
runuser -u postgres -- "$pg_bin/psql" -X -v ON_ERROR_STOP=1 -h /tmp -p 55432 -d postgres <<'SQL'
\getenv intake_password BOARD_INTAKE_PASSWORD
SELECT 'CREATE ROLE board_media_intake NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS'
WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'board_media_intake')
\gexec
SELECT 'CREATE ROLE board_media_intake_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS'
WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'board_media_intake_owner')
\gexec
GRANT board_media_intake_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
ALTER ROLE board_media_intake LOGIN PASSWORD :'intake_password';
ALTER ROLE board_media_intake SET statement_timeout = '5s';
ALTER ROLE board_media_intake SET lock_timeout = '2s';
ALTER ROLE board_media_intake SET idle_in_transaction_session_timeout = '5s';
ALTER ROLE board_media_intake SET search_path = pg_catalog;
GRANT CONNECT ON DATABASE imageboard TO board_media_intake;
SQL
umask 077
set -o noclobber
printf 'export INTAKE_DATABASE_URL=%q\n' "postgres://board_media_intake:$BOARD_INTAKE_PASSWORD@127.0.0.1:55432/imageboard" > .local/intake.env
printf '\044env:INTAKE_DATABASE_URL = '\''postgres://board_media_intake:%s@127.0.0.1:55432/imageboard'\''\n' "$BOARD_INTAKE_PASSWORD" > .local/intake.ps1
unset BOARD_INTAKE_PASSWORD
echo 'Separate development intake credentials written to ignored .local/intake.env and .local/intake.ps1.'
