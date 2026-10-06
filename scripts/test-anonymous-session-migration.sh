#!/usr/bin/env bash
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-anonymous.XXXXXXXX)
[[ $cluster =~ ^/tmp/board-anonymous\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
started=0
cleanup() {
    if [[ $started = 1 ]]; then
        actual=$(runuser -u postgres -- "$pg_bin/psql" -XAt -h "$cluster" -d postgres -c 'SHOW data_directory')
        [[ $actual = "$cluster/data" ]] || exit 1
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop >/dev/null
    fi
    [[ $cluster =~ ^/tmp/board-anonymous\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
    rm -rf -- "$cluster"
}
trap cleanup EXIT
chown postgres:postgres "$cluster"
runuser -u postgres -- "$pg_bin/initdb" -D "$cluster/data" --auth=trust --encoding=UTF8 --no-locale >/dev/null
runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" \
    -o "-c listen_addresses='' -c unix_socket_directories='$cluster'" -w start >/dev/null
started=1
db=(runuser -u postgres -- "$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h "$cluster")
"${db[@]}" -d postgres -f - < deploy/roles.sql
"${db[@]}" -d postgres <<'SQL'
CREATE DATABASE anonymous_upgrade OWNER board_migrator;
REVOKE ALL ON DATABASE anonymous_upgrade FROM PUBLIC;
GRANT CONNECT ON DATABASE anonymous_upgrade TO board_migrator,board_public;
SQL
# Phase 1: exercise the actual 0064 -> 0065 upgrade against historical rows.
# Later migrations depend on anonymous tables and must not run before 0065.
for migration in migrations/*.sql; do
    [[ $migration < migrations/0065_anonymous_sessions.sql ]] || break
    "${db[@]}" -d anonymous_upgrade --single-transaction -c 'SET ROLE board_migrator' -f - < "$migration"
done
"${db[@]}" -d anonymous_upgrade <<'SQL'
SET ROLE board_migrator;
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('anonold','Owned anonymous upgrade','Synthetic history',1000,100,100,100,10);
INSERT INTO content.threads(id,board,created_at,modified_at)
VALUES(8800101,'anonold','2026-01-01Z','2026-01-02Z');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(8800101,'anonold',8800101,'Historical anonymous name','Owned subject','Owned historical comment','2026-01-01Z');
INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES(8800101,'owned-historical-hash');
CREATE TABLE public.owned_anonymous_posts_before AS SELECT * FROM content.posts;
CREATE TABLE public.owned_anonymous_threads_before AS SELECT * FROM content.threads;
CREATE TABLE public.owned_anonymous_deletion_before AS SELECT * FROM post_secrets.deletion;
SQL
"${db[@]}" -d anonymous_upgrade --single-transaction -c 'SET ROLE board_migrator' -f - < migrations/0065_anonymous_sessions.sql
"${db[@]}" -d anonymous_upgrade <<'SQL'
SET ROLE board_migrator;
DO $$ BEGIN
    IF EXISTS(SELECT * FROM content.posts EXCEPT SELECT * FROM public.owned_anonymous_posts_before)
        OR EXISTS(SELECT * FROM public.owned_anonymous_posts_before EXCEPT SELECT * FROM content.posts)
        OR EXISTS(SELECT * FROM content.threads EXCEPT SELECT * FROM public.owned_anonymous_threads_before)
        OR EXISTS(SELECT * FROM public.owned_anonymous_threads_before EXCEPT SELECT * FROM content.threads)
        OR EXISTS(SELECT * FROM post_secrets.deletion EXCEPT SELECT * FROM public.owned_anonymous_deletion_before)
        OR EXISTS(SELECT * FROM public.owned_anonymous_deletion_before EXCEPT SELECT * FROM post_secrets.deletion)
        OR EXISTS(SELECT 1 FROM post_secrets.anonymous_sessions)
        OR EXISTS(SELECT 1 FROM post_secrets.anonymous_posts)
        OR EXISTS(SELECT 1 FROM post_secrets.anonymous_reports) THEN
        RAISE EXCEPTION 'Anonymous upgrade changed history or invented ownership';
    END IF;
END $$;
SQL

# Phase 2: qualify current runtime behavior only after every later migration
# has been applied in order. Keep the historical row comparison above at the
# 0065 boundary: later migrations legitimately extend the content row shape.
for migration in migrations/*.sql; do
    [[ $migration > migrations/0065_anonymous_sessions.sql ]] || continue
    "${db[@]}" -d anonymous_upgrade --single-transaction -c 'SET ROLE board_migrator' -f - < "$migration"
done
"${db[@]}" -d anonymous_upgrade <<'SQL'
DO $$ BEGIN
    IF EXISTS(SELECT 1 FROM post_secrets.anonymous_sessions)
        OR EXISTS(SELECT 1 FROM post_secrets.anonymous_posts)
        OR EXISTS(SELECT 1 FROM post_secrets.anonymous_reports)
        OR EXISTS(SELECT 1 FROM post_secrets.posting_history WHERE post_id=8800101)
        OR EXISTS(SELECT 1 FROM post_secrets.report_membership) THEN
        RAISE EXCEPTION 'Later migrations invented historical activity or ownership';
    END IF;
END $$;
DO $$ DECLARE runtime text; relation text; BEGIN
    IF EXISTS(SELECT 1 FROM pg_roles WHERE rolname='board_anonymous_owner'
        AND (rolcanlogin OR rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls))
        OR EXISTS(SELECT 1 FROM pg_auth_members m JOIN pg_roles r ON r.oid=m.member WHERE r.rolname='board_anonymous_owner')
        OR NOT EXISTS(SELECT 1 FROM pg_auth_members m JOIN pg_roles r ON r.oid=m.member JOIN pg_roles granted ON granted.oid=m.roleid
            WHERE r.rolname='board_migrator' AND granted.rolname='board_anonymous_owner' AND NOT m.inherit_option AND m.set_option)
        OR EXISTS(SELECT 1 FROM pg_auth_members m JOIN pg_roles r ON r.oid=m.member JOIN pg_roles granted ON granted.oid=m.roleid
            WHERE granted.rolname='board_anonymous_owner' AND r.rolname<>'board_migrator')
        OR has_schema_privilege('board_anonymous_owner','content','CREATE')
        OR has_schema_privilege('board_anonymous_owner','post_secrets','CREATE')
        OR has_schema_privilege('board_anonymous_owner','staff_identity','USAGE')
        OR has_schema_privilege('board_anonymous_owner','deployment','USAGE')
        OR has_schema_privilege('board_anonymous_owner','media','USAGE')
        OR has_any_column_privilege('board_anonymous_owner','content.posts','INSERT,UPDATE')
        OR has_table_privilege('board_anonymous_owner','content.posts','DELETE,TRUNCATE,TRIGGER') THEN
        RAISE EXCEPTION 'Anonymous owner exceeds its required authority';
    END IF;
    FOREACH runtime IN ARRAY ARRAY['board_public','board_staff','board_auth','board_media','board_media_read','board_media_intake','board_monitor'] LOOP
        FOREACH relation IN ARRAY ARRAY['anonymous_policy','anonymous_sessions','anonymous_posts','anonymous_reports'] LOOP
            IF has_any_column_privilege(runtime,'post_secrets.'||relation,'SELECT,INSERT,UPDATE,REFERENCES')
                OR has_table_privilege(runtime,'post_secrets.'||relation,'DELETE,TRUNCATE,TRIGGER') THEN
                RAISE EXCEPTION 'Runtime has direct anonymous state access';
            END IF;
        END LOOP;
        IF has_function_privilege(runtime,'post_secrets.advance_anonymous_session(bytea,bytea,bytea,bytea,boolean,smallint,bigint)','EXECUTE') THEN
            RAISE EXCEPTION 'Runtime has unrestricted anonymous activity authority';
        END IF;
    END LOOP;
END $$;
SQL
# SET ROLE from a superuser session cannot prove a denied SET ROLE operation.
# Connect as the actual restricted public login in this private trust cluster.
"${db[@]}" -d anonymous_upgrade -U board_public <<'SQL'
DO $$ BEGIN
    BEGIN PERFORM token_hash FROM post_secrets.anonymous_sessions LIMIT 0;
        RAISE EXCEPTION 'Public role read private activity'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
    BEGIN UPDATE post_secrets.anonymous_sessions SET verified_level=1 WHERE false;
        RAISE EXCEPTION 'Public role set verification'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
    BEGIN PERFORM post_secrets.advance_anonymous_session(NULL,NULL,NULL,NULL,true,1::smallint,0);
        RAISE EXCEPTION 'Public role advanced arbitrary activity'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
    BEGIN EXECUTE 'SET ROLE board_anonymous_owner';
        RAISE EXCEPTION 'Public role assumed anonymous owner'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
    IF EXISTS(SELECT 1 FROM content.anonymous_session(decode(repeat('01',32),'hex')))
        OR content.anonymous_post_proof(decode(repeat('01',32),'hex'),'anonold',8800101) IS NOT NULL THEN
        RAISE EXCEPTION 'Unknown capability acquired historical ownership';
    END IF;
END $$;
SQL
"${db[@]}" -d anonymous_upgrade <<'SQL'
SET ROLE board_anonymous_owner;
DO $$ BEGIN
    BEGIN PERFORM credential FROM staff_identity.credentials LIMIT 0;
        RAISE EXCEPTION 'Anonymous owner read a staff credential'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
    BEGIN PERFORM comment FROM content.posts LIMIT 0;
        RAISE EXCEPTION 'Anonymous owner read unrestricted post content'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
    BEGIN UPDATE content.boards SET staff_only=false WHERE false;
        RAISE EXCEPTION 'Anonymous owner changed staff policy'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
RESET ROLE;
SET ROLE board_migrator;
UPDATE post_secrets.anonymous_policy SET session_limit=1;
RESET ROLE;
SET ROLE board_public;
-- Runtime fixtures use the same actor gates and transaction-local context as
-- ordinary posting; only the historical migrator fixture above is actorless.
BEGIN ISOLATION LEVEL READ COMMITTED;
SELECT content.lock_posting_actor(decode(repeat('01',32),'hex'),true);
SELECT set_config('board.posting_actor',repeat('01',32),true);
INSERT INTO content.threads(id,board) VALUES(8800102,'anonold');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES(8800102,'anonold',8800102,'Anonymous','','Owned successful activity');
INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES(8800102,'owned-current-hash');
SELECT content.register_anonymous_post(decode(repeat('01',32),'hex'),decode(repeat('02',32),'hex'),decode(repeat('03',32),'hex'),decode(repeat('04',32),'hex'),true,'anonold',8800102,extract(epoch FROM clock_timestamp())::bigint);
COMMIT;
BEGIN ISOLATION LEVEL READ COMMITTED;
DO $$ BEGIN
    BEGIN
        PERFORM content.lock_posting_actor(decode(repeat('05',32),'hex'),true);
        PERFORM set_config('board.posting_actor',repeat('05',32),true);
        INSERT INTO content.threads(id,board) VALUES(8800103,'anonold');
        INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES(8800103,'anonold',8800103,'Anonymous','','Owned capacity failure');
        INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES(8800103,'owned-current-hash');
        PERFORM content.register_anonymous_post(decode(repeat('05',32),'hex'),decode(repeat('02',32),'hex'),decode(repeat('03',32),'hex'),decode(repeat('04',32),'hex'),true,'anonold',8800103,extract(epoch FROM clock_timestamp())::bigint);
        RAISE EXCEPTION 'Anonymous capacity failed open';
    EXCEPTION WHEN SQLSTATE '53300' THEN NULL; END;
    IF EXISTS(SELECT 1 FROM content.posts WHERE id=8800103) OR EXISTS(SELECT 1 FROM content.threads WHERE id=8800103) THEN
        RAISE EXCEPTION 'Capacity failure partially committed content';
    END IF;
END $$;
COMMIT;
RESET ROLE;
SET ROLE board_migrator;
DO $$ BEGIN
    IF EXISTS(SELECT 1 FROM post_secrets.posting_history WHERE post_id=8800103 OR actor_hash=decode(repeat('05',32),'hex'))
        OR EXISTS(SELECT 1 FROM post_secrets.posting_thread_actions WHERE actor_hash=decode(repeat('05',32),'hex')) THEN
        RAISE EXCEPTION 'Anonymous capacity failure leaked posting history or actions';
    END IF;
END $$;
DELETE FROM post_secrets.anonymous_policy;
RESET ROLE;
SET ROLE board_public;
BEGIN ISOLATION LEVEL READ COMMITTED;
DO $$ BEGIN
    BEGIN
        PERFORM content.lock_posting_actor(decode(repeat('05',32),'hex'),true);
        PERFORM set_config('board.posting_actor',repeat('05',32),true);
        INSERT INTO content.threads(id,board) VALUES(8800103,'anonold');
        INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES(8800103,'anonold',8800103,'Anonymous','','Owned unavailable policy');
        INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES(8800103,'owned-current-hash');
        PERFORM content.register_anonymous_post(decode(repeat('05',32),'hex'),decode(repeat('02',32),'hex'),decode(repeat('03',32),'hex'),decode(repeat('04',32),'hex'),true,'anonold',8800103,extract(epoch FROM clock_timestamp())::bigint);
        RAISE EXCEPTION 'Missing anonymous policy failed open';
    EXCEPTION WHEN SQLSTATE '55000' THEN NULL; END;
    IF EXISTS(SELECT 1 FROM content.posts WHERE id=8800103) THEN RAISE EXCEPTION 'Missing policy partially committed content'; END IF;
END $$;
COMMIT;
RESET ROLE;
SET ROLE board_migrator;
DO $$ BEGIN
    IF EXISTS(SELECT 1 FROM post_secrets.posting_history WHERE post_id=8800103 OR actor_hash=decode(repeat('05',32),'hex'))
        OR EXISTS(SELECT 1 FROM post_secrets.posting_thread_actions WHERE actor_hash=decode(repeat('05',32),'hex')) THEN
        RAISE EXCEPTION 'Missing anonymous policy leaked posting history or actions';
    END IF;
END $$;
INSERT INTO post_secrets.anonymous_policy VALUES(true,100000);
INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,created_at,network_at,address_at,environment_at,expires_at)
SELECT sha256(convert_to(n::text,'UTF8')),sha256(convert_to(n::text,'UTF8')),sha256(convert_to(n::text,'UTF8')),sha256(convert_to(n::text,'UTF8')),1,1,1,1,1 FROM generate_series(1,65) n;
RESET ROLE;
SET ROLE board_public;
BEGIN ISOLATION LEVEL READ COMMITTED;
SELECT content.lock_posting_actor(decode(repeat('05',32),'hex'),true);
SELECT set_config('board.posting_actor',repeat('05',32),true);
INSERT INTO content.threads(id,board) VALUES(8800103,'anonold');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES(8800103,'anonold',8800103,'Anonymous','','Owned bounded cleanup');
INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES(8800103,'owned-current-hash');
SELECT content.register_anonymous_post(decode(repeat('05',32),'hex'),decode(repeat('02',32),'hex'),decode(repeat('03',32),'hex'),decode(repeat('04',32),'hex'),true,'anonold',8800103,extract(epoch FROM clock_timestamp())::bigint);
COMMIT;
RESET ROLE;
SET ROLE board_migrator;
DO $$ BEGIN
    IF (SELECT count(*) FROM post_secrets.anonymous_sessions WHERE expires_at=1)<>1
        OR (SELECT count(*) FROM post_secrets.anonymous_sessions)<>3
        OR (SELECT count(*) FROM post_secrets.anonymous_posts)<>2
        OR EXISTS(SELECT 1 FROM post_secrets.anonymous_posts WHERE post_id=8800101) THEN
        RAISE EXCEPTION 'Anonymous cleanup was unbounded or invented historical ownership';
    END IF;
END $$;
RESET ROLE;
SET ROLE board_public;
BEGIN ISOLATION LEVEL READ COMMITTED;
-- Current public admission registers the report and anonymous activity atomically.
SELECT content.admit_report('anonold',8800102,'Owned restored report',decode(repeat('06',32),'hex'),
 decode(repeat('01',32),'hex'),decode(repeat('02',32),'hex'),decode(repeat('03',32),'hex'),
 decode(repeat('04',32),'hex'),false,extract(epoch FROM clock_timestamp())::bigint);
COMMIT;
SQL
# The complete dump needs this owned cluster's administrator: the migrator
# deliberately does not inherit the private report-membership owner's reads.
runuser -u postgres -- "$pg_bin/pg_dump" -h "$cluster" -U postgres -d anonymous_upgrade --format=custom > "$cluster/anonymous.dump"
"${db[@]}" -d postgres -c 'CREATE DATABASE anonymous_restore OWNER board_migrator'
# Let the shell open the private dump; do not relax its permissions for postgres.
runuser -u postgres -- "$pg_bin/pg_restore" -h "$cluster" -d anonymous_restore --exit-on-error < "$cluster/anonymous.dump"
fingerprint="SELECT md5((SELECT coalesce(string_agg(to_jsonb(s)::text,E'\\n' ORDER BY encode(s.token_hash,'hex')),'') FROM post_secrets.anonymous_sessions s)||(SELECT coalesce(string_agg(to_jsonb(p)::text,E'\\n' ORDER BY p.post_id),'') FROM post_secrets.anonymous_posts p)||(SELECT coalesce(string_agg(to_jsonb(r)::text,E'\\n' ORDER BY r.report_id),'') FROM post_secrets.anonymous_reports r)||(SELECT to_jsonb(p)::text FROM post_secrets.anonymous_policy p))"
before=$("${db[@]}" -d anonymous_upgrade -At -c "$fingerprint")
after=$("${db[@]}" -d anonymous_restore -At -c "$fingerprint")
[[ $before = "$after" && -n $before ]] || { echo 'Restored anonymous state differs.' >&2; exit 1; }
proof="SELECT encode(content.anonymous_post_proof(decode(repeat('01',32),'hex'),'anonold',8800102),'hex')"
before=$("${db[@]}" -d anonymous_upgrade -U board_public -At -c "$proof")
after=$("${db[@]}" -d anonymous_restore -U board_public -At -c "$proof")
[[ $before = "$after" && ${#before} = 64 ]] || { echo 'Restored anonymous capability is unavailable.' >&2; exit 1; }
"${db[@]}" -d anonymous_restore -U board_public <<'SQL'
DO $$ BEGIN
    BEGIN PERFORM token_hash FROM post_secrets.anonymous_sessions LIMIT 0;
        RAISE EXCEPTION 'Restore exposed private anonymous state'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
    BEGIN UPDATE post_secrets.anonymous_sessions SET verified_level=1 WHERE false;
        RAISE EXCEPTION 'Restore exposed verification writes'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SQL
cleanup
trap - EXIT
echo 'Anonymous 0065 upgrade and current runtime passed: history preserved, actual role denials, atomic capacity/policy failures, bounded cleanup and restored private activity/ownership.'
