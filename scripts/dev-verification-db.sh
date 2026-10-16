#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run with sudo against the disposable development cluster.' >&2; exit 1; }
[[ -f .local/database.env && -f .local/cluster-path ]] || { echo 'Run scripts/dev-db.sh first.' >&2; exit 1; }
for name in verification-tests verification-issuer verification-mail; do
  [[ ! -e .local/$name.env && ! -e .local/$name.ps1 ]] || { echo 'Verification credentials already exist; reuse them.' >&2; exit 1; }
done
cluster=$(cat .local/cluster-path)
[[ $cluster =~ ^/tmp/board-postgres\.[[:alnum:]]+$ && -d $cluster && ! -L $cluster ]] || { echo 'Expected a disposable cluster path.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
actual=$(runuser -u postgres -- "$pg_bin/psql" -XAt -h /tmp -p 55432 -d postgres -c 'SHOW data_directory')
[[ $actual = "$cluster" ]] || { echo 'Port 55432 belongs to a different cluster.' >&2; exit 1; }
BOARD_VERIFIER_PASSWORD=$(openssl rand -hex 24)
BOARD_MAILER_PASSWORD=$(openssl rand -hex 24)
export BOARD_VERIFIER_PASSWORD BOARD_MAILER_PASSWORD
runuser -u postgres -- "$pg_bin/psql" -X -v ON_ERROR_STOP=1 -h /tmp -p 55432 -d postgres <<'SQL'
\getenv verifier_password BOARD_VERIFIER_PASSWORD
\getenv mailer_password BOARD_MAILER_PASSWORD
SELECT 'CREATE ROLE board_verifier NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS'
WHERE NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='board_verifier')
\gexec
SELECT 'CREATE ROLE board_mailer NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS'
WHERE NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='board_mailer')
\gexec
ALTER ROLE board_verifier LOGIN PASSWORD :'verifier_password';
ALTER ROLE board_mailer LOGIN PASSWORD :'mailer_password';
ALTER ROLE board_verifier SET statement_timeout='5s';
ALTER ROLE board_verifier SET lock_timeout='2s';
ALTER ROLE board_verifier SET idle_in_transaction_session_timeout='5s';
ALTER ROLE board_verifier SET search_path=pg_catalog;
ALTER ROLE board_mailer SET statement_timeout='5s';
ALTER ROLE board_mailer SET lock_timeout='2s';
ALTER ROLE board_mailer SET idle_in_transaction_session_timeout='5s';
ALTER ROLE board_mailer SET search_path=pg_catalog;
GRANT CONNECT ON DATABASE imageboard TO board_verifier,board_mailer;
SQL
umask 077
set -o noclobber
issuer="postgres://board_verifier:$BOARD_VERIFIER_PASSWORD@127.0.0.1:55432/imageboard"
mailer="postgres://board_mailer:$BOARD_MAILER_PASSWORD@127.0.0.1:55432/imageboard"
printf 'export TEST_VERIFICATION_DATABASE_URL=%q\nexport TEST_VERIFICATION_MAIL_DATABASE_URL=%q\n' "$issuer" "$mailer" > .local/verification-tests.env
printf '\044env:TEST_VERIFICATION_DATABASE_URL = '\''%s'\''\n\044env:TEST_VERIFICATION_MAIL_DATABASE_URL = '\''%s'\''\n' "$issuer" "$mailer" > .local/verification-tests.ps1
printf 'export VERIFICATION_DATABASE_URL=%q\n' "$issuer" > .local/verification-issuer.env
printf '\044env:VERIFICATION_DATABASE_URL = '\''%s'\''\n' "$issuer" > .local/verification-issuer.ps1
printf 'export VERIFICATION_MAIL_DATABASE_URL=%q\n' "$mailer" > .local/verification-mail.env
printf '\044env:VERIFICATION_MAIL_DATABASE_URL = '\''%s'\''\n' "$mailer" > .local/verification-mail.ps1
unset BOARD_VERIFIER_PASSWORD BOARD_MAILER_PASSWORD issuer mailer
echo 'Separate disposable verification database credentials written to ignored .local files.'
