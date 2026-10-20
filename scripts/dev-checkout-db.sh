#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run with sudo against the disposable development cluster.' >&2; exit 1; }
[[ -f .local/database.env && -f .local/cluster-path ]] || { echo 'Run scripts/dev-db.sh first.' >&2; exit 1; }
for name in checkout-tests checkout-adapter; do
  [[ ! -e .local/$name.env && ! -e .local/$name.ps1 ]] || { echo 'Checkout credentials already exist; reuse them.' >&2; exit 1; }
done
cluster=$(cat .local/cluster-path)
[[ $cluster =~ ^/tmp/board-postgres\.[[:alnum:]]+$ && -d $cluster && ! -L $cluster ]] || { echo 'Expected a disposable cluster path.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
actual=$(runuser -u postgres -- "$pg_bin/psql" -XAt -h /tmp -p 55432 -d postgres -c 'SHOW data_directory')
[[ $actual = "$cluster" ]] || { echo 'Port 55432 belongs to a different cluster.' >&2; exit 1; }
BOARD_CHECKOUT_PASSWORD=$(openssl rand -hex 24)
export BOARD_CHECKOUT_PASSWORD
runuser -u postgres -- "$pg_bin/psql" -X -v ON_ERROR_STOP=1 -h /tmp -p 55432 -d postgres <<'SQL'
\getenv checkout_password BOARD_CHECKOUT_PASSWORD
SELECT 'CREATE ROLE board_checkout NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS'
WHERE NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='board_checkout')
\gexec
ALTER ROLE board_checkout LOGIN PASSWORD :'checkout_password';
ALTER ROLE board_checkout SET statement_timeout='5s';
ALTER ROLE board_checkout SET lock_timeout='2s';
ALTER ROLE board_checkout SET idle_in_transaction_session_timeout='5s';
ALTER ROLE board_checkout SET search_path=pg_catalog;
GRANT CONNECT ON DATABASE imageboard TO board_checkout;
SQL
umask 077
set -o noclobber
adapter="postgres://board_checkout:$BOARD_CHECKOUT_PASSWORD@127.0.0.1:55432/imageboard"
printf 'export TEST_CHECKOUT_DATABASE_URL=%q\n' "$adapter" > .local/checkout-tests.env
printf '\044env:TEST_CHECKOUT_DATABASE_URL = '\''%s'\''\n' "$adapter" > .local/checkout-tests.ps1
printf 'export CHECKOUT_DATABASE_URL=%q\n' "$adapter" > .local/checkout-adapter.env
printf '\044env:CHECKOUT_DATABASE_URL = '\''%s'\''\n' "$adapter" > .local/checkout-adapter.ps1
unset BOARD_CHECKOUT_PASSWORD adapter
echo 'Separate disposable checkout database credentials written to ignored .local files.'
