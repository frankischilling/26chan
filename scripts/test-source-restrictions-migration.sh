#!/usr/bin/env bash
# Owned PG16: populated upgrade, actual service roles and private issuance.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || exit 1
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-restrictions.XXXXXXXX)
startup_attempted=0
cleanup() {
 status=$?
 trap - EXIT
 [[ $cluster =~ ^/tmp/board-restrictions\.[[:alnum:]]{8}$ && ! -L $cluster && $(readlink -f "$cluster") = "$cluster" ]] || exit 1
 if [[ $startup_attempted = 1 ]]; then
  runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop >/dev/null 2>&1 || status=1
  if runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" status >/dev/null 2>&1; then exit 1; else [[ $? = 3 ]] || exit 1; fi
 fi
 if [[ $status = 0 ]]; then rm -rf -- "$cluster";
 else printf 'Stopped restriction cluster retained for inspection: %s\n' "$cluster"; fi
 exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
chown postgres:postgres "$cluster"
runuser -u postgres -- "$pg_bin/initdb" -D "$cluster/data" --auth=trust --encoding=UTF8 --no-locale >/dev/null
startup_attempted=1
runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" \
 -o "-c listen_addresses='' -c unix_socket_directories='$cluster'" -w start >/dev/null
psql=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h "$cluster")
admin=(runuser -u postgres -- "${psql[@]}")
"${admin[@]}" -d postgres -f deploy/roles.sql
"${admin[@]}" -d postgres <<'SQL'
CREATE DATABASE restriction_upgrade OWNER board_migrator;
REVOKE ALL ON DATABASE restriction_upgrade FROM PUBLIC;
GRANT CONNECT ON DATABASE restriction_upgrade TO board_migrator,board_staff,board_public,board_auth;
-- As in the development provisioner, activate only the service login used by
-- this private Unix-socket fixture. Private owner identities remain NOLOGIN.
ALTER ROLE board_staff LOGIN;
SQL
operator=("${psql[@]}" -U board_migrator -d restriction_upgrade)
root=("${admin[@]}" -d restriction_upgrade)
staff=("${psql[@]}" -U board_staff -d restriction_upgrade)
for migration in migrations/*.sql; do
 [[ $(basename "$migration") < 0134_ ]] || break
 "${operator[@]}" --single-transaction -f "$migration"
done
"${root[@]}" <<'SQL'
SET ROLE board_migrator;
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('rest134','Owned restrictions','Synthetic fixture',1000,100,100,100,10);
INSERT INTO content.threads(id,board) VALUES(13400001,'rest134');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(13400001,'rest134',13400001,'Synthetic name','Synthetic subject','Preserved original text');
INSERT INTO post_secrets.posting_history(post_id,board,thread_id,actor_hash,request_at,automatic_identity)
VALUES(13400001,'rest134',13400001,decode(repeat('42',32),'hex'),extract(epoch FROM clock_timestamp())::bigint,
 '00000000-0000-0000-0000-000000013401');
INSERT INTO staff_identity.accounts(id,role,allow_boards,deny_boards) OVERRIDING SYSTEM VALUE
VALUES(13400001,'moderator',ARRAY['rest134'],'{}');
INSERT INTO staff_identity.credentials(id,account_id,credential) VALUES(decode('1340','hex'),13400001,'{}');
INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id)
VALUES(decode(repeat('a1',32),'hex'),decode(repeat('b1',32),'hex'),13400001,decode('1340','hex'));
CREATE TABLE public.restriction_posts_before AS SELECT to_jsonb(p) AS value FROM content.posts p;
CREATE TABLE public.restriction_history_before AS SELECT to_jsonb(h) AS value FROM post_secrets.posting_history h;
ALTER TABLE content.boards DISABLE ROW LEVEL SECURITY;
SQL
if "${operator[@]}" --single-transaction -f migrations/0134_source_restrictions.sql >"$cluster/preflight.log" 2>&1; then
 echo 'Restriction migration accepted prior visibility drift' >&2; exit 1
fi
grep -q 'Transfer catalog is not ready before restrictions' "$cluster/preflight.log"
"${root[@]}" <<'SQL'
DO $$ BEGIN IF to_regnamespace('restrictions') IS NOT NULL THEN RAISE EXCEPTION 'Failed preflight changed schema'; END IF; END $$;
ALTER TABLE content.boards ENABLE ROW LEVEL SECURITY;
SQL
"${operator[@]}" --single-transaction -f migrations/0134_source_restrictions.sql
python3 scripts/ban-reference-sql.py > "$cluster/source-comparison.sql"
chown postgres:postgres "$cluster/source-comparison.sql"
"${root[@]}" --single-transaction -f "$cluster/source-comparison.sql"
echo 'SQL restriction policy matches all 13,840 executed original cases.'
"${root[@]}" <<'SQL'
DO $$ DECLARE r text; BEGIN
 IF content.restrictions_ready() IS DISTINCT FROM true OR content.text_thread_transfer_ready() IS DISTINCT FROM true
    OR content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Integrated readiness failed'; END IF;
 IF EXISTS(SELECT to_jsonb(p) FROM content.posts p EXCEPT SELECT value FROM public.restriction_posts_before)
    OR EXISTS(SELECT value FROM public.restriction_posts_before EXCEPT SELECT to_jsonb(p) FROM content.posts p)
    OR EXISTS(SELECT to_jsonb(h) FROM post_secrets.posting_history h EXCEPT SELECT value FROM public.restriction_history_before)
    OR EXISTS(SELECT value FROM public.restriction_history_before EXCEPT SELECT to_jsonb(h) FROM post_secrets.posting_history h)
 THEN RAISE EXCEPTION 'Restriction upgrade changed historical posts or private equality'; END IF;
 FOREACH r IN ARRAY ARRAY['board_public','board_staff','board_auth','board_media','board_media_read','board_media_intake','board_monitor'] LOOP
  IF pg_has_role(r,'board_restriction_owner','MEMBER') OR has_schema_privilege(r,'restrictions','CREATE')
    OR has_table_privilege(r,'restrictions.bans','SELECT,INSERT,UPDATE,DELETE')
    OR has_table_privilege(r,'restrictions.audit','SELECT,INSERT,UPDATE,DELETE')
    OR has_function_privilege(r,'restrictions.catalog_hash()','EXECUTE')
    OR has_function_privilege(r,'restrictions.staff_authority(bytea,bytea,integer,text,boolean)','EXECUTE')
  THEN RAISE EXCEPTION 'Runtime restriction authority leaked to %',r; END IF;
 END LOOP;
END $$;
SQL
"${staff[@]}" <<'SQL'
DO $$ BEGIN
 IF content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Healthy staff control failed'; END IF;
 BEGIN
  PERFORM actor_hash FROM restrictions.bans;
  RAISE EXCEPTION 'Staff read private target identity';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN
  UPDATE restrictions.capacity SET used_bytes=0;
  RAISE EXCEPTION 'Staff wrote private capacity';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN
  EXECUTE 'SET LOCAL ROLE board_restriction_owner';
  RAISE EXCEPTION 'Staff assumed private owner';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN
  PERFORM content.issue_post_restriction(decode(repeat('a1',32),'hex'),decode(repeat('ff',32),'hex'),900,
    'rest134',13400001,true,false,86400,'Synthetic public reason','Synthetic private reason');
  RAISE EXCEPTION 'Invalid CSRF was accepted';
 EXCEPTION WHEN SQLSTATE 'P134A' THEN NULL; END;
 BEGIN
  PERFORM content.issue_post_restriction(decode(repeat('a1',32),'hex'),decode(repeat('b1',32),'hex'),900,
    'test',13400001,true,false,86400,'Synthetic public reason','Synthetic private reason');
  RAISE EXCEPTION 'Unpermitted board was accepted';
 EXCEPTION WHEN SQLSTATE 'P134A' THEN NULL; END;
END $$;
SELECT content.issue_post_restriction(decode(repeat('a1',32),'hex'),decode(repeat('b1',32),'hex'),900,
 'rest134',13400001,true,false,86400,'Synthetic public reason','Synthetic private reason') AS ban_id \gset
SELECT content.unban_restriction(decode(repeat('a1',32),'hex'),decode(repeat('b1',32),'hex'),900,:ban_id) AS retired \gset
\if :retired
\else
 \quit 1
\endif
SELECT content.unban_restriction(decode(repeat('a1',32),'hex'),decode(repeat('b1',32),'hex'),900,:ban_id) AS repeated \gset
\if :repeated
 \quit 1
\endif
SELECT content.issue_post_restriction(decode(repeat('a1',32),'hex'),decode(repeat('b1',32),'hex'),900,
 'rest134',13400001,false,true,0,'Synthetic warning','') AS warning_id \gset
SQL
"${root[@]}" <<'SQL'
DO $$ BEGIN
 IF (SELECT count(*) FROM restrictions.bans)<>2 OR (SELECT count(*) FROM restrictions.audit)<>3
    OR NOT EXISTS(SELECT 1 FROM restrictions.bans WHERE global AND NOT warning AND NOT active AND retirement='unban'
      AND retired_by=13400001 AND actor_hash=decode(repeat('42',32),'hex')
      AND automatic_identity='00000000-0000-0000-0000-000000013401')
    OR NOT EXISTS(SELECT 1 FROM restrictions.bans WHERE warning AND active AND ends_at=starts_at)
    OR (SELECT used_bytes FROM restrictions.capacity)<>(SELECT sum(charged_bytes) FROM restrictions.bans)
 THEN RAISE EXCEPTION 'Private issuance, unban, warning or audit integrity failed'; END IF;
END $$;
UPDATE restrictions.capacity SET byte_limit=used_bytes;
SQL
"${staff[@]}" <<'SQL'
DO $$ BEGIN
 BEGIN
  PERFORM content.issue_post_restriction(decode(repeat('a1',32),'hex'),decode(repeat('b1',32),'hex'),900,
    'rest134',13400001,false,false,NULL,'Capacity refusal','');
  RAISE EXCEPTION 'Capacity refusal was bypassed';
 EXCEPTION WHEN SQLSTATE 'P134C' THEN NULL; END;
END $$;
SQL
"${root[@]}" <<'SQL'
DO $$ BEGIN IF (SELECT count(*) FROM restrictions.bans)<>2 OR (SELECT count(*) FROM restrictions.audit)<>3
 THEN RAISE EXCEPTION 'Refused issuance changed durable state'; END IF; END $$;
GRANT SELECT ON restrictions.bans TO board_public;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM false THEN RAISE EXCEPTION 'Privacy drift remained ready'; END IF; END $$;
REVOKE SELECT ON restrictions.bans FROM board_public;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restored privacy did not qualify'; END IF; END $$;
CREATE FUNCTION public.restriction_probe_trigger() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NEW; END $$;
CREATE TRIGGER restriction_unreviewed_probe BEFORE INSERT ON restrictions.bans FOR EACH ROW EXECUTE FUNCTION public.restriction_probe_trigger();
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM false THEN RAISE EXCEPTION 'Unreviewed trigger remained ready'; END IF; END $$;
DROP TRIGGER restriction_unreviewed_probe ON restrictions.bans;
DROP FUNCTION public.restriction_probe_trigger();
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restored trigger catalog did not qualify'; END IF; END $$;
SET SESSION AUTHORIZATION board_public;
DO $$ BEGIN
 IF content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Healthy public control failed'; END IF;
 BEGIN
  PERFORM content.issue_post_restriction(decode(repeat('a1',32),'hex'),decode(repeat('b1',32),'hex'),900,
    'rest134',13400001,true,false,86400,'Public forgery','');
  RAISE EXCEPTION 'Public role issued a restriction';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN
  PERFORM actor_hash FROM restrictions.bans;
  RAISE EXCEPTION 'Public role read private identities';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SQL
echo 'Source restriction populated upgrade, role denials, private binding, issuance/unban/warning, capacity and catalog checks passed.'

# Preserve populated reports and refuse an unreviewed admission definition.
"${root[@]}" <<'SQL'
SET ROLE board_migrator;
INSERT INTO content.reports(board,post_id,reason) VALUES('rest134',13400001,'Preserved synthetic report');
RESET ROLE;
CREATE TABLE public.report_restriction_reports_before AS SELECT to_jsonb(r) AS value FROM content.reports r;
CREATE TABLE public.report_admission_before AS SELECT pg_get_functiondef('content.admit_report(text,bigint,text,bytea)'::regprocedure) AS definition;
DO $$ BEGIN
 EXECUTE replace((SELECT definition FROM public.report_admission_before),'RETURN v_report;','RETURN v_report + 0;');
END $$;
SQL
if "${operator[@]}" --single-transaction -f migrations/0135_report_restrictions.sql >"$cluster/report-preflight.log" 2>&1; then
 echo 'Report restrictions accepted unreviewed admission definition' >&2; exit 1
fi
grep -q 'Report admission definition changed before restrictions' "$cluster/report-preflight.log"
"${root[@]}" <<'SQL'
DO $$ BEGIN
 IF to_regprocedure('content.bind_report_restrictions(bytea,bytea,bytea,text,bigint,bigint)') IS NOT NULL THEN
  RAISE EXCEPTION 'Failed report preflight changed schema'; END IF;
 EXECUTE (SELECT definition FROM public.report_admission_before);
END $$;
SQL
"${operator[@]}" --single-transaction -f migrations/0135_report_restrictions.sql
python3 scripts/report-ban-reference-sql.py > "$cluster/report-source-comparison.sql"
chown postgres:postgres "$cluster/report-source-comparison.sql"
"${root[@]}" --single-transaction -f "$cluster/report-source-comparison.sql"
"${root[@]}" <<'SQL'
DO $$ BEGIN
 IF content.restrictions_ready() IS DISTINCT FROM true OR content.report_snapshots_ready() IS DISTINCT FROM true
    OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Report restriction dependencies failed'; END IF;
 IF EXISTS(SELECT to_jsonb(r) FROM content.reports r EXCEPT SELECT value FROM public.report_restriction_reports_before)
    OR EXISTS(SELECT value FROM public.report_restriction_reports_before EXCEPT SELECT to_jsonb(r) FROM content.reports r)
 THEN RAISE EXCEPTION 'Report restriction upgrade changed retained reports'; END IF;
 IF has_function_privilege('board_public','content.assert_reporting_restrictions(text,bigint,bytea)','EXECUTE')
    OR has_function_privilege('board_staff','content.assert_reporting_restrictions(text,bigint,bytea)','EXECUTE')
    OR has_function_privilege('board_auth','content.bind_report_restrictions(bytea,bytea,bytea,text,bigint,bigint)','EXECUTE')
 THEN RAISE EXCEPTION 'Report restriction helper authority leaked'; END IF;
END $$;
ALTER TABLE content.reports DISABLE TRIGGER source_report_restriction_guard;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM false THEN RAISE EXCEPTION 'Disabled report guard remained ready'; END IF; END $$;
ALTER TABLE content.reports ENABLE TRIGGER source_report_restriction_guard;
DO $$ DECLARE definition text; BEGIN
 SELECT pg_get_functiondef('content.admit_report(text,bigint,text,bytea)'::regprocedure) INTO definition;
 EXECUTE replace(definition,'PERFORM content.assert_reporting_restrictions(p_board,p_post,p_actor);','NULL;');
 IF content.restrictions_ready() IS DISTINCT FROM false THEN RAISE EXCEPTION 'Report bypass remained ready'; END IF;
 EXECUTE definition;
 IF content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restored report guard did not qualify'; END IF;
END $$;
SQL
echo 'SQL report policy matches all 3,456 executed source cases; populated upgrade and report guard drift checks passed.'

"${root[@]}" -c 'ALTER TABLE content.boards DISABLE ROW LEVEL SECURITY'
if "${operator[@]}" --single-transaction -f migrations/0136_thread_ban_batches.sql >"$cluster/thread-preflight.log" 2>&1; then
 echo 'Thread batch upgrade accepted prior transfer drift' >&2; exit 1
fi
grep -q 'Transfer is not ready before thread batches' "$cluster/thread-preflight.log"
"${root[@]}" <<'SQL'
DO $$ BEGIN IF to_regclass('restrictions.pending_thread_batches') IS NOT NULL THEN
 RAISE EXCEPTION 'Failed thread preflight changed schema'; END IF; END $$;
ALTER TABLE content.boards ENABLE ROW LEVEL SECURITY;
SQL
"${operator[@]}" --single-transaction -f migrations/0136_thread_ban_batches.sql
"${root[@]}" <<'SQL'
DO $$ BEGIN
 IF content.restrictions_ready() IS DISTINCT FROM true OR content.text_thread_transfer_ready() IS DISTINCT FROM true
    OR content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Thread batch readiness failed'; END IF;
 IF has_function_privilege('board_public','content.issue_thread_restrictions(bytea,bytea,integer,text,bigint,text,text,integer,integer)','EXECUTE')
    OR has_function_privilege('board_staff','restrictions.thread_batch_subjects(text,bigint)','EXECUTE')
    OR has_table_privilege('board_staff','restrictions.pending_thread_batches','SELECT,INSERT,UPDATE,DELETE')
 THEN RAISE EXCEPTION 'Thread batch private authority leaked'; END IF;
END $$;
ALTER TABLE restrictions.pending_thread_batches DISABLE TRIGGER complete_source_thread_batch;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM false THEN
 RAISE EXCEPTION 'Disabled deferred deletion promise stayed ready'; END IF; END $$;
ALTER TABLE restrictions.pending_thread_batches ENABLE TRIGGER complete_source_thread_batch;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restored promise stayed unready'; END IF; END $$;
SQL
echo 'Thread batch populated upgrade, private-role denials and deferred guard drift checks passed.'
"${root[@]}" -c 'SET ROLE board_restriction_owner; UPDATE restrictions.capacity SET byte_limit=67108864 WHERE singleton'
python3 scripts/thread-ban-reference-sql.py > "$cluster/thread-source-comparison.sql"
chown postgres:postgres "$cluster/thread-source-comparison.sql"
"${root[@]}" -f "$cluster/thread-source-comparison.sql"
echo 'Thread batch SQL matched all 64 valid source traces and rejected invalid/root-forged inputs.'

# Qualify both independent bounds, including a huge single-address thread.
"${root[@]}" <<'SQL'
SET ROLE board_migrator;
INSERT INTO content.threads(id,board) VALUES(14750000,'tbref136'),(14770000,'tbref136');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
 SELECT 14750000+i,'tbref136',14750000,'Anonymous','','Owned post-bound fixture' FROM generate_series(0,10000) i;
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
 SELECT 14770000+i,'tbref136',14770000,'Anonymous','','Owned actor-bound fixture' FROM generate_series(0,1000) i;
INSERT INTO post_secrets.posting_history(post_id,board,thread_id,actor_hash,request_at)
 SELECT p.id,p.board,p.thread_id,
  CASE WHEN p.thread_id=14750000 THEN decode(repeat('8a',32),'hex')
       ELSE sha256(convert_to('owned-bound:'||p.id,'UTF8')) END,
  floor(extract(epoch FROM now()))::bigint
 FROM content.posts p WHERE p.board='tbref136' AND p.thread_id IN(14750000,14770000);
SET ROLE board_staff;
DO $$ DECLARE target bigint; BEGIN
 FOREACH target IN ARRAY ARRAY[14750000::bigint,14770000::bigint] LOOP
  BEGIN
   PERFORM * FROM content.issue_thread_restrictions(decode(repeat('a1',32),'hex'),decode(repeat('b1',32),'hex'),900,
    'tbref136',target,'Owned OP bound','Owned reply bound',3,0);
   RAISE EXCEPTION 'Thread batch exceeded its bound';
  EXCEPTION WHEN SQLSTATE 'P134C' THEN NULL; END;
 END LOOP;
END $$;
SET ROLE board_restriction_owner;
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM restrictions.bans WHERE board='tbref136' AND post_id BETWEEN 14750000 AND 14771000)
    OR EXISTS(SELECT 1 FROM restrictions.pending_thread_batches) THEN
  RAISE EXCEPTION 'Rejected batch retained restrictions or credentials'; END IF;
END $$;
RESET ROLE;
SQL
echo 'Thread batch rejects more than 10,000 live posts or 1,000 distinct identities without retained bans.'

# Optional local Rust lane against the same disposable, socket-only database.
# Ordinary CI runs this integration binary with the full workspace suite.
if [[ ${RUN_RESTRICTION_RUST_TESTS:-0} = 1 ]]; then
 "${root[@]}" <<'SQL'
UPDATE restrictions.capacity SET byte_limit=67108864;
ALTER ROLE board_public LOGIN;
ALTER ROLE board_auth LOGIN;
SQL
 export MIGRATION_DATABASE_URL="postgresql://board_migrator@localhost/restriction_upgrade?host=$cluster"
 export TEST_PUBLIC_DATABASE_URL="postgresql://board_public@localhost/restriction_upgrade?host=$cluster"
 export STAFF_DATABASE_URL="postgresql://board_staff@localhost/restriction_upgrade?host=$cluster"
 export AUTH_DATABASE_URL="postgresql://board_auth@localhost/restriction_upgrade?host=$cluster"
 cargo test -p board-store --test restrictions --features database-tests --locked -- --nocapture
 cargo test -p board-store --test report_target --features database-tests --locked -- --nocapture
 cargo test -p board-public --test restrictions --features database-tests --locked -- --nocapture
 cargo test -p board-staff --test restrictions --features database-tests --locked -- --nocapture
fi
