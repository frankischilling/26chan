#!/usr/bin/env bash
# Fresh and populated 0128 upgrade on owned temporary PostgreSQL 16 clusters.
# No live database or unrelated poll data is accessed.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-poll-vote.XXXXXXXX)
started=0
cleanup() {
  status=$?
  trap - EXIT
  if [[ $started = 1 ]]; then
    runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null || status=1
  fi
  [[ $cluster =~ ^/tmp/board-poll-vote\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
  [[ $(readlink -f "$cluster") = "$cluster" ]] || exit 1
  rm -rf -- "$cluster"
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
chown postgres:postgres "$cluster"
runuser -u postgres -- "$pg_bin/initdb" -D "$cluster/data" --auth=trust --encoding=UTF8 --no-locale > /dev/null
runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" \
  -o "-c listen_addresses='' -c unix_socket_directories='$cluster' -c statement_timeout=30000 -c lock_timeout=5000" -w start > /dev/null
started=1
psql=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h "$cluster")
exec 3>&1
exec > "$cluster/qualification.log" 2>&1
trap 'printf "Poll vote migration qualification failed at line %s; private diagnostics removed.\n" "$LINENO" >&3' ERR

# Simulate both a new installation and the upgrade of a 0109-era role set.
sed '/board_poll_owner/d' deploy/roles.sql > "$cluster/old-roles.sql"
runuser -u postgres -- "${psql[@]}" -d postgres -f - < "$cluster/old-roles.sql"
runuser -u postgres -- "${psql[@]}" -d postgres <<'SQL'
CREATE DATABASE poll_vote_upgrade OWNER board_migrator;
REVOKE ALL ON DATABASE poll_vote_upgrade FROM PUBLIC;
GRANT CONNECT ON DATABASE poll_vote_upgrade TO board_migrator,board_public;
SQL
upgrade=("${psql[@]}" -U board_migrator -d poll_vote_upgrade)
for migration in migrations/*.sql; do
  [[ $migration < migrations/0128_poll_voting.sql ]] || break
  "${upgrade[@]}" --single-transaction -f - < "$migration"
done
"${upgrade[@]}" <<'SQL'
INSERT INTO poll_private.polls(id,title,description,vote_count,published,catalogue_ordinal)
VALUES(8128001,'Retained published','Owned original',7,true,1),
      (8128002,'Retained hidden','Owned hidden',99,false,NULL),
      (8128003,'Retained unlisted','Owned unlisted',3,true,NULL);
INSERT INTO poll_private.options(poll_id,id,ordinal,caption,score)
VALUES(8128001,11,1,'Owned nullable',NULL),
      (8128001,22,2,'Owned scored',5),
      (8128002,31,1,'Hidden option',99),
      (8128003,41,1,'Unlisted option',3);
SQL
"${upgrade[@]}" -c "CREATE TABLE public.poll_baseline AS SELECT id,title,description,vote_count,published,catalogue_ordinal FROM poll_private.polls"
"${upgrade[@]}" -c "CREATE TABLE public.option_baseline AS SELECT * FROM poll_private.options"

# 0128 requires the new role: an unbootstrapped upgrade must fail atomically.
if "${upgrade[@]}" --single-transaction -f - < migrations/0128_poll_voting.sql; then
  echo '0128 accepted missing poll owner role' >&2; exit 1
fi
"${upgrade[@]}" <<'SQL'
DO $$ BEGIN
 IF to_regclass('poll_private.votes') IS NOT NULL
   OR EXISTS(SELECT 1 FROM pg_attribute WHERE attrelid='poll_private.polls'::regclass
      AND attname='accepting_votes' AND NOT attisdropped)
   OR EXISTS(TABLE public.poll_baseline EXCEPT ALL
       SELECT id,title,description,vote_count,published,catalogue_ordinal FROM poll_private.polls)
   OR EXISTS(TABLE public.option_baseline EXCEPT ALL TABLE poll_private.options)
 THEN RAISE EXCEPTION 'Failed 0128 left state or data changes'; END IF;
END $$;
SQL
runuser -u postgres -- "${psql[@]}" -d postgres -f - < deploy/poll-role.sql
"${upgrade[@]}" --single-transaction -f - < migrations/0128_poll_voting.sql
"${upgrade[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS (
   TABLE public.poll_baseline EXCEPT ALL
   SELECT id,title,description,vote_count,published,catalogue_ordinal
   FROM poll_private.polls)
 OR EXISTS (
   SELECT id,title,description,vote_count,published,catalogue_ordinal
   FROM poll_private.polls EXCEPT ALL TABLE public.poll_baseline)
 OR EXISTS(TABLE public.option_baseline EXCEPT ALL TABLE poll_private.options)
 OR EXISTS(TABLE poll_private.options EXCEPT ALL TABLE public.option_baseline)
 OR EXISTS(SELECT 1 FROM poll_private.votes)
 OR EXISTS(SELECT 1 FROM poll_private.polls WHERE accepting_votes
      OR new_vote_count<>0 OR vote_capacity<>10000)
 THEN RAISE EXCEPTION '0128 rewrote retained polls, scores or vote identities'; END IF;
END $$;
SQL

# Fresh role bootstrap is independently exercised in another database.
runuser -u postgres -- "${psql[@]}" -d postgres <<'SQL'
CREATE DATABASE poll_vote_fresh OWNER board_migrator;
REVOKE ALL ON DATABASE poll_vote_fresh FROM PUBLIC;
GRANT CONNECT ON DATABASE poll_vote_fresh TO board_migrator,board_public;
SQL
fresh=("${psql[@]}" -U board_migrator -d poll_vote_fresh)
for migration in migrations/*.sql; do
  "${fresh[@]}" --single-transaction -f - < "$migration"
done
"${fresh[@]}" -c "DO \$\$ BEGIN IF EXISTS(SELECT 1 FROM poll_private.polls) OR EXISTS(SELECT 1 FROM poll_private.votes) THEN RAISE EXCEPTION 'Fresh migration seeded ballots'; END IF; END \$\$;"

python3 - "$cluster" <<'PY'
from pathlib import Path
import re, sys
root = Path('crates/store/src')
statements = []
for filename, name in [('polls.rs', 'POLL_READINESS_SQL'), ('poll_voting.rs', 'POLL_VOTE_READINESS_SQL')]:
    contents = (root / filename).read_text()
    match = re.search(r'pub const ' + name + r': &str = r#"(.*?)"#;', contents, re.S)
    if not match:
        raise SystemExit(f'Missing {name}')
    statements.append((name, match.group(1)))
Path(sys.argv[1], 'vote-ready.sql').write_text(
    '\n'.join("\\echo " + name + "\nDO $check$ BEGIN IF (" + sql +
              ") IS DISTINCT FROM true THEN RAISE EXCEPTION 'Poll readiness failed'; END IF; END $check$;"
              for name, sql in statements) + '\n')
PY
for database in poll_vote_upgrade poll_vote_fresh; do
  if ! "${psql[@]}" -v VERBOSITY=sqlstate -U board_public -d "$database" -f - \
      < "$cluster/vote-ready.sql" > "$cluster/readiness.log" 2>&1; then
    python3 - "$cluster/readiness.log" <<'PY' >&3
from pathlib import Path
import re, sys
log = Path(sys.argv[1]).read_text()
names = re.findall(r'^POLL_(?:VOTE_)?READINESS_SQL$', log, re.M)
codes = re.findall(r'ERROR:\s*([A-Z0-9]{5})\b', log)
print('Poll readiness failed:', names[-1] if names else 'unknown check',
      'SQLSTATE', codes[-1] if codes else 'unavailable')
PY
    exit 1
  fi
  "${psql[@]}" -U board_public -d "$database" <<'SQL'
DO $$ DECLARE ledger oid; BEGIN
 SELECT c.oid INTO ledger FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname='poll_private' AND c.relname='votes';
 IF has_schema_privilege(current_user,'poll_private','USAGE')
 OR ledger IS NULL
 OR has_table_privilege(current_user,ledger,'SELECT,INSERT,UPDATE,DELETE')
 OR has_any_column_privilege(current_user,ledger,'SELECT,INSERT,UPDATE')
 OR NOT has_function_privilege(current_user,'content.cast_poll_vote(bigint,bigint,bytea)','EXECUTE')
 OR NOT has_function_privilege(current_user,'content.has_poll_vote(bigint,bytea)','EXECUTE')
 THEN RAISE EXCEPTION 'Public role poll authority is wider than intended'; END IF;
END $$;
SQL
done
printf 'Poll vote 0128 fresh/legacy role bootstrap, atomic upgrade, retained rows and public readiness passed.\n' >&3
