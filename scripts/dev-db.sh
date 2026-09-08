#!/usr/bin/env bash
set -euo pipefail
# Disposable PostgreSQL on Linux/WSL. Requires root, PostgreSQL 16 and openssl.
# Existing clusters are not changed. A second invocation refuses to overwrite state.
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run with sudo in a disposable development environment.' >&2; exit 1; }
[[ ! -e .local/database.env ]] || { echo '.local/database.env already exists; use the existing cluster.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
[[ -x "$pg_bin/initdb" ]] || { echo 'Install PostgreSQL 16 first.' >&2; exit 1; }
mkdir -p .local
cluster=$(mktemp -d /tmp/board-postgres.XXXXXXXX)
chown postgres:postgres "$cluster"
admin_password=$(openssl rand -hex 24)
public_password=$(openssl rand -hex 24)
migration_password=$(openssl rand -hex 24)
password_file=$(mktemp)
chmod 600 "$password_file"
printf '%s\n' "$admin_password" > "$password_file"
chown postgres:postgres "$password_file"
trap 'rm -f -- "$password_file"' EXIT
runuser -u postgres -- "$pg_bin/initdb" -D "$cluster" --auth-host=scram-sha-256 --auth-local=peer --pwfile="$password_file" > .local/initdb.log
runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster" -l "$cluster/server.log" -o '-p 55432 -h 127.0.0.1 -k /tmp' -w start
psql=(runuser -u postgres -- "$pg_bin/psql" -X -v ON_ERROR_STOP=1 -h /tmp -p 55432)
"${psql[@]}" -f deploy/roles.sql
"${psql[@]}" -v public_password="$public_password" -v migration_password="$migration_password" <<'SQL'
ALTER ROLE board_public PASSWORD :'public_password';
ALTER ROLE board_migrator PASSWORD :'migration_password';
CREATE DATABASE imageboard OWNER board_migrator;
REVOKE ALL ON DATABASE imageboard FROM PUBLIC;
GRANT CONNECT ON DATABASE imageboard TO board_public, board_migrator;
SQL
umask 077
printf 'export MIGRATION_DATABASE_URL=%q\nexport TEST_PUBLIC_DATABASE_URL=%q\nexport DATABASE_URL=%q\nexport BOARD_TEST_CLUSTER=%q\n' \
  "postgres://board_migrator:$migration_password@127.0.0.1:55432/imageboard" \
  "postgres://board_public:$public_password@127.0.0.1:55432/imageboard" \
  "postgres://board_public:$public_password@127.0.0.1:55432/imageboard" "$cluster" > .local/database.env
printf '\044env:MIGRATION_DATABASE_URL = '\''postgres://board_migrator:%s@127.0.0.1:55432/imageboard'\''\n\044env:TEST_PUBLIC_DATABASE_URL = '\''postgres://board_public:%s@127.0.0.1:55432/imageboard'\''\n\044env:DATABASE_URL = \044env:TEST_PUBLIC_DATABASE_URL\n' "$migration_password" "$public_password" > .local/database.ps1
printf '%s\n' "$cluster" > .local/cluster-path
echo 'Disposable database started on 127.0.0.1:55432. Credentials are in ignored .local files.'
