#!/usr/bin/env bash
# Owned disposable cluster: populated 0156 announcement upgrade and administrator restore.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
pg_bin=${PG_BIN:-/usr/lib/postgresql/16/bin}
cluster=$(mktemp -d /tmp/board-announcement-upgrade.XXXXXXXX)
runner=()
if [[ $(id -u) = 0 ]]; then
    chown postgres:postgres "$cluster"
    runner=(runuser -u postgres --)
fi
started=0
cleanup() {
    status=$?
    trap - EXIT
    [[ $cluster =~ ^/tmp/board-announcement-upgrade\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
    [[ $(readlink -f "$cluster") = "$cluster" ]] || exit 1
    if [[ $started = 1 ]]; then
        "${runner[@]}" "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop >/dev/null 2>&1 || status=1
        if "${runner[@]}" "$pg_bin/pg_ctl" -D "$cluster/data" status >/dev/null 2>&1; then server_status=0; else server_status=$?; fi
        if [[ $server_status != 3 ]]; then echo "Cluster stop unconfirmed; preserved $cluster" >&2; exit 1; fi
    fi
    rm -rf -- "$cluster"
    exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
"${runner[@]}" "$pg_bin/initdb" -D "$cluster/data" --username=postgres --auth=trust --encoding=UTF8 --no-locale >/dev/null
started=1
# Keep the owned listener below the default Linux ephemeral-port range.
if ! "${runner[@]}" "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" -o "-c listen_addresses='127.0.0.1' -c port=15454 -c unix_socket_directories='' -c statement_timeout=30000 -c lock_timeout=5000" -w start >/dev/null; then
    echo "Owned qualification database failed to start:" >&2
    [[ ! -f "$cluster/server.log" ]] || cat "$cluster/server.log" >&2
    exit 1
fi
psql=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h 127.0.0.1 -p 15454 -U postgres)
"${psql[@]}" -d postgres -f - < deploy/roles.sql >/dev/null
"${psql[@]}" -d postgres <<'SQL'
CREATE DATABASE announcement_upgrade OWNER board_migrator;
REVOKE ALL ON DATABASE announcement_upgrade FROM PUBLIC;
GRANT CONNECT ON DATABASE announcement_upgrade TO board_migrator,board_public;
SQL
migrator=("${psql[@]}" -U board_migrator -d announcement_upgrade)
for migration in migrations/*.sql; do
    [[ $(basename "$migration") < 0157_ ]] || break
    "${migrator[@]}" --single-transaction -f - < "$migration" >/dev/null
done
"${migrator[@]}" <<'SQL'
INSERT INTO site_private.pages(key,title,body,revision) VALUES('faq','Owned retained FAQ','Owned retained body',3);
INSERT INTO site_private.news(id,published_at,title,body,author) VALUES(1,CURRENT_TIMESTAMP-INTERVAL '1 hour','Owned retained news','Owned retained news body','Owned editor');
CREATE TABLE public.announcement_saved_pages AS SELECT key,to_jsonb(p) saved FROM site_private.pages p;
CREATE TABLE public.announcement_saved_news AS SELECT id,to_jsonb(n) saved FROM site_private.news n;
DO $$ BEGIN
 BEGIN
  INSERT INTO site_private.pages(key,title,body,revision) VALUES('announcement','Before upgrade','Must be rejected',1);
  RAISE EXCEPTION 'Old schema admitted announcement';
 EXCEPTION WHEN check_violation THEN NULL;
 END;
END $$;
SQL
# Structural drift must fail atomically without broadening the old schema.
{
    printf '%s\n' 'BEGIN;' 'ALTER TABLE site_private.pages DROP CONSTRAINT pages_check;' 'ALTER TABLE site_private.pages ADD CONSTRAINT pages_check CHECK (true);'
    cat migrations/0157_global_announcement.sql
    printf '%s\n' 'COMMIT;'
} > "$cluster/drift.sql"
if "${migrator[@]}" -f - < "$cluster/drift.sql" >"$cluster/expected-drift.log" 2>&1; then
    echo 'Unexpected page-key drift was accepted.' >&2; exit 1
fi
"${migrator[@]}" --single-transaction -f - < migrations/0157_global_announcement.sql >/dev/null
"${migrator[@]}" <<'SQL'
INSERT INTO site_private.pages(key,title,body,revision) VALUES('announcement','Owned announcement','Owned <b>plain announcement</b>',1);
SQL
cat > "$cluster/assertions.sql" <<'SQL'
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM public.announcement_saved_pages old LEFT JOIN site_private.pages current USING(key) WHERE old.saved IS DISTINCT FROM to_jsonb(current))
 OR EXISTS(SELECT 1 FROM public.announcement_saved_news old FULL JOIN site_private.news current USING(id) WHERE old.saved IS DISTINCT FROM to_jsonb(current)) THEN
 RAISE EXCEPTION 'Existing site content changed'; END IF;
 IF (SELECT count(*) FROM site_private.pages)<>2 OR (SELECT count(*) FROM site_private.news)<>1 THEN
 RAISE EXCEPTION 'Unexpected site content'; END IF;
 IF NOT EXISTS(SELECT 1 FROM site_private.pages WHERE key='announcement' AND board IS NULL AND revision=1 AND published
 AND body='Owned <b>plain announcement</b>') THEN RAISE EXCEPTION 'Announcement changed'; END IF;
 IF has_table_privilege('board_public','site_private.pages','SELECT,INSERT,UPDATE,DELETE')
 OR has_table_privilege('board_public','content.published_site_pages','INSERT,UPDATE,DELETE')
 OR NOT has_table_privilege('board_public','content.published_site_pages','SELECT') THEN RAISE EXCEPTION 'Announcement permissions changed'; END IF;
 BEGIN
  INSERT INTO site_private.pages(key,title,body,revision) VALUES('unknown','Unknown','Not admitted',1);
  RAISE EXCEPTION 'Unexpected page key admitted';
 EXCEPTION WHEN check_violation THEN NULL;
 END;
END $$;
SQL
"${migrator[@]}" -f - < "$cluster/assertions.sql"
"$pg_bin/pg_dump" -h 127.0.0.1 -p 15454 -U postgres -d announcement_upgrade --format=custom -f "$cluster/owned.dump"
"${psql[@]}" -d postgres <<'SQL'
CREATE DATABASE announcement_restored OWNER board_migrator;
REVOKE ALL ON DATABASE announcement_restored FROM PUBLIC;
GRANT CONNECT ON DATABASE announcement_restored TO board_migrator,board_public;
SQL
"$pg_bin/pg_restore" -h 127.0.0.1 -p 15454 -U postgres -d announcement_restored --exit-on-error "$cluster/owned.dump"
"${psql[@]}" -d announcement_restored -f - < "$cluster/assertions.sql"
for database in announcement_upgrade announcement_restored; do
    "${psql[@]}" -U board_public -d "$database" -At -c "SELECT body FROM content.published_site_pages WHERE key='announcement'" | grep -Fxq 'Owned <b>plain announcement</b>'
    "${psql[@]}" -U board_migrator -d "$database" -c "UPDATE site_private.pages SET published=false,revision=revision+1 WHERE key='announcement'" >/dev/null
    [[ $("${psql[@]}" -U board_public -d "$database" -At -c "SELECT count(*) FROM content.published_site_pages WHERE key='announcement'") = 0 ]]
done
echo 'Populated announcement upgrade, drift rollback, public projection and restore passed.'
