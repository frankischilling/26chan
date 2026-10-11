#!/usr/bin/env bash
# Owned disposable PostgreSQL: populated upgrade and runtime policy authority.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-image-policy.XXXXXXXX)
started=0
cleanup() {
    status=$?
    trap - EXIT
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop >/dev/null || status=1
    fi
    [[ $cluster =~ ^/tmp/board-image-policy\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
    [[ $(readlink -f "$cluster") = "$cluster" ]] || exit 1
    rm -rf -- "$cluster"
    exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
chown postgres:postgres "$cluster"
runuser -u postgres -- "$pg_bin/initdb" -D "$cluster/data" --auth=trust --encoding=UTF8 --no-locale >/dev/null
runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" \
    -o "-c listen_addresses='' -c unix_socket_directories='$cluster' -c statement_timeout=30000 -c lock_timeout=5000" -w start >/dev/null
started=1
psql=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h "$cluster")
runuser -u postgres -- "${psql[@]}" -d postgres -f - < deploy/roles.sql
runuser -u postgres -- "${psql[@]}" -d postgres <<'SQL'
CREATE DATABASE image_policy_upgrade OWNER board_migrator;
REVOKE ALL ON DATABASE image_policy_upgrade FROM PUBLIC;
GRANT CONNECT ON DATABASE image_policy_upgrade TO board_migrator,board_public;
SQL
migrator=("${psql[@]}" -U board_migrator -d image_policy_upgrade)
for migration in migrations/*.sql; do
    [[ $(basename "$migration") < 0132_ ]] || break
    "${migrator[@]}" --single-transaction -f - < "$migration" >/dev/null
done
has_transfer=$("${migrator[@]}" -Atc "SELECT to_regclass('post_secrets.thread_transfer_catalog') IS NOT NULL")
if [[ $has_transfer = t ]]; then
    catalog_before=$("${migrator[@]}" -Atc "SET ROLE board_thread_transfer_owner; SELECT encode(definition_hash,'hex') FROM post_secrets.thread_transfer_catalog WHERE singleton")
    [[ $catalog_before =~ ^[0-9a-f]{64}$ ]] || { echo 'Transfer catalog checkpoint is invalid.' >&2; exit 1; }
    "${migrator[@]}" -c "SET ROLE board_thread_transfer_owner; UPDATE post_secrets.thread_transfer_catalog SET definition_hash=decode(repeat('00',32),'hex')"
    if "${migrator[@]}" --single-transaction -f - < migrations/0132_source_image_board_policy.sql >"$cluster/rejected.out" 2>"$cluster/rejected.err"; then
        echo 'Image migration accepted existing transfer catalog drift.' >&2; exit 1
    fi
    grep -Fq 'Text transfer catalog is not ready for image policy.' "$cluster/rejected.err"
    "${migrator[@]}" -v old_hash="$catalog_before" <<'SQL'
SET ROLE board_thread_transfer_owner;
DO $$ BEGIN
    IF EXISTS(SELECT 1 FROM pg_attribute WHERE attrelid='content.boards'::regclass
        AND attname='max_image_bytes' AND NOT attisdropped)
        OR (SELECT definition_hash<>decode(repeat('00',32),'hex')
            FROM post_secrets.thread_transfer_catalog WHERE singleton)
    THEN RAISE EXCEPTION 'Rejected image migration changed schema or resealed drift'; END IF;
END $$;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=decode(:'old_hash','hex') WHERE singleton;
RESET ROLE;
SQL
fi
"${migrator[@]}" -f - < fixtures/demo.sql >/dev/null
{
cat <<'SQL'
INSERT INTO media.jobs(id,filename,state,input_bytes,attempts,lease_token,output_sha256,output_bytes)
VALUES(repeat('1',32),'Owned retained file.png','published',100,1,repeat('2',32),repeat('a',64),123);
INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at)
VALUES(repeat('3',32),repeat('1',32),repeat('2',32),repeat('a',64),123,10,20,'approved',clock_timestamp());
CREATE TEMP TABLE before_image_policy(name text PRIMARY KEY,payload jsonb NOT NULL);
INSERT INTO before_image_policy VALUES
('boards',(SELECT jsonb_agg(to_jsonb(b) ORDER BY slug) FROM content.boards b)),
('posts',(SELECT jsonb_agg(to_jsonb(p) ORDER BY id) FROM content.posts p)),
('threads',(SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM content.threads t)),
('assets',(SELECT jsonb_agg(to_jsonb(a) ORDER BY id) FROM media.assets a)),
('jobs',(SELECT jsonb_agg(to_jsonb(j) ORDER BY id) FROM media.jobs j));
SQL
printf 'BEGIN;\n'
cat migrations/0132_source_image_board_policy.sql
printf 'COMMIT;\n'
cat <<'SQL'
DO $check$ BEGIN
    IF (SELECT payload FROM before_image_policy WHERE name='boards') IS DISTINCT FROM
        (SELECT jsonb_agg(to_jsonb(b)-ARRAY['max_image_bytes','min_image_width','min_image_height','max_image_dimension','gif_only'] ORDER BY slug) FROM content.boards b)
    OR (SELECT payload FROM before_image_policy WHERE name='posts') IS DISTINCT FROM
        (SELECT jsonb_agg(to_jsonb(p) ORDER BY id) FROM content.posts p)
    OR (SELECT payload FROM before_image_policy WHERE name='threads') IS DISTINCT FROM
        (SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM content.threads t)
    OR (SELECT payload FROM before_image_policy WHERE name='assets') IS DISTINCT FROM
        (SELECT jsonb_agg(to_jsonb(a) ORDER BY id) FROM media.assets a)
    OR (SELECT payload FROM before_image_policy WHERE name='jobs') IS DISTINCT FROM
        (SELECT jsonb_agg(to_jsonb(j) ORDER BY id) FROM media.jobs j)
    THEN RAISE EXCEPTION 'Image policy rewrote historical board fields, posts or media'; END IF;
END $check$;
SQL
} | "${migrator[@]}"
python3 - "$cluster/policy-import.sql" <<'PY'
import json, pathlib, sys
rows=json.loads(pathlib.Path('fixtures/board-reference.json').read_text())['boards']
keys=('max_image_bytes','min_image_width','min_image_height','max_image_dimension','gif_only')
values=[]
for row in rows:
    assert row['slug'].isalnum() and all(key in row for key in keys)
    values.append("('"+row['slug']+"',"+','.join(str(row[key]).lower() for key in keys)+')')
pathlib.Path(sys.argv[1]).write_text("DO $check$ BEGIN IF EXISTS(SELECT 1 FROM (VALUES\n"+",\n".join(values)+") expected(slug,"+','.join(keys)+") LEFT JOIN content.boards b USING(slug) WHERE b.slug IS NULL OR ROW("+','.join('b.'+key for key in keys)+") IS DISTINCT FROM ROW("+','.join('expected.'+key for key in keys)+")) THEN RAISE EXCEPTION 'Source board image policy import differs'; END IF; END $check$;\n")
PY
"${migrator[@]}" -f - < "$cluster/policy-import.sql"
"${migrator[@]}" <<'SQL'
DO $check$ DECLARE helper record; runtime text; BEGIN
    SELECT * INTO STRICT helper FROM pg_proc
        WHERE oid='content.check_image_board_policy(text,bigint,integer,integer,text)'::regprocedure;
    IF helper.proowner<>'board_attachment_owner'::regrole OR helper.prosecdef
        OR helper.proconfig<>ARRAY['search_path=pg_catalog, pg_temp']
        OR EXISTS(SELECT 1 FROM aclexplode(helper.proacl) a WHERE a.grantee<>helper.proowner)
        OR has_schema_privilege('board_attachment_owner','content','CREATE')
    THEN RAISE EXCEPTION 'Image policy helper authority changed'; END IF;
    FOREACH runtime IN ARRAY ARRAY['board_public','board_staff','board_auth','board_media',
        'board_media_read','board_media_intake','board_monitor','board_staff_post_owner'] LOOP
        IF has_function_privilege(runtime,helper.oid,'EXECUTE')
            OR has_column_privilege(runtime,'content.boards','max_image_bytes','UPDATE')
            OR has_column_privilege(runtime,'content.boards','gif_only','UPDATE')
        THEN RAISE EXCEPTION 'Runtime gained image policy authority'; END IF;
    END LOOP;
END $check$;
BEGIN;
SET LOCAL ROLE board_attachment_owner;
SELECT content.check_image_board_policy('a',100,10,20,'png');
ROLLBACK;
SQL
"${psql[@]}" -U board_public -d image_policy_upgrade <<'SQL'
DO $check$ BEGIN
    BEGIN
        PERFORM content.check_image_board_policy('a',100,10,20,'png');
        RAISE EXCEPTION 'Public runtime called private image policy' USING ERRCODE='ZX001';
    EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $check$;
SQL
printf 'Image policy populated upgrade preserved historical content/media, imported all source boards and retained private runtime authority.\n'
if [[ $has_transfer = t ]]; then
    "${migrator[@]}" <<'SQL'
SET ROLE board_thread_transfer_owner;
DO $$ BEGIN
    IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'Image policy invalidated healthy transfer readiness';
    END IF;
    IF EXISTS(SELECT 1 FROM (VALUES('board_public'),('board_staff'),('board_auth'),('board_media'),
        ('board_media_read'),('board_media_intake'),('board_monitor')) runtime(name)
        WHERE has_column_privilege(runtime.name,'post_secrets.thread_transfer_catalog','definition_hash','UPDATE')
        OR has_function_privilege(runtime.name,'post_secrets.transfer_catalog_hash()','EXECUTE'))
    THEN RAISE EXCEPTION 'Runtime gained transfer catalog resealing authority'; END IF;
END $$;
BEGIN;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=decode(repeat('00',32),'hex') WHERE singleton;
DO $$ BEGIN
    IF content.text_thread_transfer_ready() IS DISTINCT FROM false THEN
        RAISE EXCEPTION 'Post-upgrade transfer catalog drift was accepted';
    END IF;
END $$;
ROLLBACK;
RESET ROLE;
SQL
    echo 'Image policy retained installed transfer readiness and rejected prior/later drift without runtime resealing grants.'
fi
