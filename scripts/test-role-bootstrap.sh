#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-role-bootstrap.XXXXXXXX)
[[ $cluster =~ ^/tmp/board-role-bootstrap\.[[:alnum:]]+$ && -d $cluster && ! -L $cluster ]] || exit 1
started=0
cleanup() {
  if [[ $started = 1 ]]; then
    runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null
    started=0
  fi
  [[ $cluster =~ ^/tmp/board-role-bootstrap\.[[:alnum:]]+$ && -d $cluster && ! -L $cluster ]] || exit 1
  rm -rf -- "$cluster"
}
trap cleanup EXIT
chown postgres:postgres "$cluster"
runuser -u postgres -- "$pg_bin/initdb" -D "$cluster/data" --auth=trust --encoding=UTF8 --no-locale > /dev/null
# Only a Unix socket inside this private generated directory; no TCP listener.
runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" \
  -o "-c listen_addresses='' -c unix_socket_directories='$cluster'" -w start > /dev/null
started=1
db=(runuser -u postgres -- "$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h "$cluster")
"${db[@]}" -d postgres -f deploy/roles.sql
"${db[@]}" -d postgres <<'SQL'
CREATE DATABASE bootstrap_test OWNER board_migrator;
REVOKE ALL ON DATABASE bootstrap_test FROM PUBLIC;
GRANT CONNECT ON DATABASE bootstrap_test TO board_migrator,board_public;
SQL
for migration in migrations/*.sql; do
  if [[ $migration = migrations/0040_poster_counts.sql ]]; then
    "${db[@]}" -d bootstrap_test <<'SQL'
SET ROLE board_migrator;
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('countold','Owned count upgrade','Synthetic history',1000,100,100,100,10);
INSERT INTO content.threads(id,board,created_at,modified_at)
VALUES(8800001,'countold','2026-01-01Z','2026-01-02Z');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(8800001,'countold',8800001,'Historical name','Historical subject','Synthetic history','2026-01-01Z');
CREATE TABLE public.owned_count_posts_before AS SELECT * FROM content.posts;
CREATE TABLE public.owned_count_threads_before AS SELECT * FROM content.threads;
SQL
  fi
  "${db[@]}" -d bootstrap_test --single-transaction -c 'SET ROLE board_migrator' -f "$migration"
done
"${db[@]}" -d bootstrap_test <<'SQL'
DO $$
BEGIN
  IF EXISTS (SELECT to_jsonb(p)-ARRAY['country','country_name','board_flag','flag_name','capcode'] FROM content.posts p EXCEPT SELECT to_jsonb(p) FROM public.owned_count_posts_before p)
     OR EXISTS (SELECT to_jsonb(p) FROM public.owned_count_posts_before p EXCEPT SELECT to_jsonb(p)-ARRAY['country','country_name','board_flag','flag_name','capcode'] FROM content.posts p)
     OR EXISTS (SELECT * FROM content.threads EXCEPT SELECT * FROM public.owned_count_threads_before)
     OR EXISTS (SELECT * FROM public.owned_count_threads_before EXCEPT SELECT * FROM content.threads)
     OR EXISTS (SELECT 1 FROM content.posts WHERE country IS NOT NULL OR country_name IS NOT NULL OR board_flag IS NOT NULL OR flag_name IS NOT NULL OR capcode IS NOT NULL)
     OR EXISTS (SELECT 1 FROM post_secrets.poster_contexts)
     OR content.unique_posters('countold',8800001) IS NOT NULL THEN
    RAISE EXCEPTION 'Poster count upgrade changed history or invented identity';
  END IF;
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname='board_staff_post_owner'
      AND (rolcanlogin OR rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls))
     OR EXISTS (SELECT 1 FROM pg_auth_members m JOIN pg_roles r ON r.oid=m.member WHERE r.rolname='board_staff_post_owner')
     OR has_schema_privilege('board_staff_post_owner','content','CREATE')
     OR has_schema_privilege('board_staff_post_owner','staff_identity','CREATE')
     OR has_schema_privilege('board_staff_post_owner','deployment','USAGE')
     OR has_schema_privilege('board_staff_post_owner','media','USAGE')
     OR has_table_privilege('board_staff_post_owner','content.posts','INSERT,UPDATE,DELETE')
     OR has_any_column_privilege('board_staff_post_owner','staff_identity.credentials','SELECT,INSERT,UPDATE')
     OR has_column_privilege('board_staff_post_owner','staff_identity.accounts','role','UPDATE')
     OR has_column_privilege('board_staff_post_owner','staff_identity.accounts','public_capcode','UPDATE')
     OR has_column_privilege('board_staff_post_owner','staff_identity.sessions','csrf_hash','UPDATE')
     OR has_column_privilege('board_staff_post_owner','staff_identity.sessions','expires_at','UPDATE')
     OR has_column_privilege('board_staff_post_owner','post_secrets.staff_post_intents','capcode','UPDATE')
     OR NOT has_column_privilege('board_staff_post_owner','post_secrets.staff_post_intents','token_hash','UPDATE') THEN
    RAISE EXCEPTION 'Staff posting function owner exceeds required authority';
  END IF;
  IF EXISTS (SELECT 1 FROM unnest(ARRAY['board_public','board_staff','board_auth','board_media',
       'board_media_read','board_media_intake','board_monitor']) runtime(name)
       WHERE has_any_column_privilege(name,'post_secrets.staff_post_intents','SELECT,INSERT,UPDATE')
          OR has_table_privilege(name,'post_secrets.staff_post_intents','DELETE,TRUNCATE,TRIGGER')
          OR (name<>'board_auth' AND has_function_privilege(name,
              'staff_identity.issue_post_authority(bytea,bytea,bytea,integer,boolean,bigint,text,bigint,text,text,text,timestamptz)','EXECUTE'))
          OR (name<>'board_staff' AND has_function_privilege(name,
              'content.consume_staff_post_authority(bytea,bigint,text,bigint,text,text,text,timestamptz)','EXECUTE')))
     OR NOT has_function_privilege('board_auth',
          'staff_identity.issue_post_authority(bytea,bytea,bytea,integer,boolean,bigint,text,bigint,text,text,text,timestamptz)','EXECUTE')
     OR NOT has_function_privilege('board_staff',
          'content.consume_staff_post_authority(bytea,bigint,text,bigint,text,text,text,timestamptz)','EXECUTE')
     OR has_column_privilege('board_staff','content.posts','capcode','INSERT,UPDATE')
     OR has_column_privilege('board_auth','staff_identity.accounts','public_capcode','UPDATE') THEN
    RAISE EXCEPTION 'Staff posting runtime grants differ';
  END IF;
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname='board_poster_count_owner'
      AND (rolcanlogin OR rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls))
     OR EXISTS (SELECT 1 FROM pg_auth_members m JOIN pg_roles r ON r.oid=m.member WHERE r.rolname='board_poster_count_owner')
     OR has_schema_privilege('board_poster_count_owner','content','CREATE')
     OR has_schema_privilege('board_poster_count_owner','staff_identity','USAGE')
     OR has_schema_privilege('board_poster_count_owner','deployment','USAGE')
     OR has_table_privilege('board_poster_count_owner','post_secrets.poster_contexts','UPDATE,TRUNCATE,TRIGGER')
     OR has_table_privilege('board_public','post_secrets.poster_contexts','SELECT,INSERT,UPDATE,DELETE')
     OR NOT has_function_privilege('board_public','content.unique_posters(text,bigint)','EXECUTE')
     OR has_function_privilege('board_public','content.record_poster_context()','EXECUTE') THEN
    RAISE EXCEPTION 'Poster count function owner exceeds required authority';
  END IF;
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname='board_media_retention_owner'
      AND (rolcanlogin OR rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls))
     OR EXISTS (SELECT 1 FROM pg_auth_members m JOIN pg_roles r ON r.oid=m.member WHERE r.rolname='board_media_retention_owner')
     OR has_schema_privilege('board_media_retention_owner','media','CREATE')
     OR has_schema_privilege('board_media_retention_owner','content','USAGE')
     OR has_schema_privilege('board_media_retention_owner','staff_identity','USAGE')
     OR has_schema_privilege('board_media_retention_owner','media_intake','USAGE')
     OR has_schema_privilege('board_media_retention_owner','deployment','USAGE')
     OR has_column_privilege('board_media_retention_owner','media.jobs','lease_token','SELECT,INSERT,UPDATE')
     OR has_table_privilege('board_media_retention_owner','media.assets','INSERT,DELETE,TRUNCATE')
     OR has_any_column_privilege('board_media_retention_owner','media.assets','INSERT,REFERENCES') THEN
    RAISE EXCEPTION 'Retention function owner exceeds required authority';
  END IF;
  IF NOT has_function_privilege('board_media','media.retire_output(text)','EXECUTE')
     OR has_function_privilege('board_public','media.retire_output(text)','EXECUTE')
     OR has_function_privilege('board_staff','media.retire_output(text)','EXECUTE')
     OR has_function_privilege('board_media_read','media.retire_output(text)','EXECUTE') THEN
    RAISE EXCEPTION 'Retention function execution grants differ';
  END IF;
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname='board_attachment_owner'
      AND (rolcanlogin OR rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls))
     OR EXISTS (SELECT 1 FROM pg_auth_members m JOIN pg_roles r ON r.oid=m.member
       WHERE r.rolname='board_attachment_owner')
     OR has_schema_privilege('board_attachment_owner','content','CREATE')
     OR has_schema_privilege('board_attachment_owner','staff_identity','USAGE')
     OR has_schema_privilege('board_attachment_owner','deployment','USAGE')
     OR has_any_column_privilege('board_attachment_owner','media.assets','INSERT,UPDATE,REFERENCES')
     OR has_column_privilege('board_attachment_owner','media.jobs','lease_token','SELECT,INSERT,UPDATE') THEN
    RAISE EXCEPTION 'Attachment function owner exceeds required authority';
  END IF;
  IF has_any_column_privilege('board_public','content.post_media','SELECT,INSERT,UPDATE,REFERENCES')
     OR has_table_privilege('board_public','content.post_media','DELETE,TRUNCATE,TRIGGER')
     OR NOT has_table_privilege('board_public','content.visible_post_media','SELECT')
     OR NOT has_function_privilege('board_public',
       'content.insert_post_attachment(bigint,text,bigint,text,text,text,text,text,boolean)','EXECUTE') THEN
    RAISE EXCEPTION 'Public attachment grants differ';
  END IF;
  IF (SELECT rolcanlogin FROM pg_roles WHERE rolname='board_media_read') THEN
    RAISE EXCEPTION 'Unqualified staging reader is login-enabled';
  END IF;
  IF NOT has_table_privilege('board_media_read','media.approved_assets','SELECT')
     OR has_table_privilege('board_media_read','media.assets','SELECT')
     OR has_table_privilege('board_media_read','media.jobs','SELECT') THEN
    RAISE EXCEPTION 'Bootstrap reader grants differ';
  END IF;
  IF (SELECT rolcanlogin FROM pg_roles WHERE rolname='board_monitor') THEN
    RAISE EXCEPTION 'Unqualified observer is login-enabled';
  END IF;
  IF NOT has_schema_privilege('board_monitor','monitoring','USAGE')
     OR NOT has_table_privilege('board_monitor','monitoring.media_queue','SELECT')
     OR has_schema_privilege('board_monitor','media','USAGE')
     OR has_table_privilege('board_monitor','media.jobs','SELECT,INSERT,UPDATE,DELETE')
     OR has_table_privilege('board_monitor','media.queue_policy','SELECT,UPDATE') THEN
    RAISE EXCEPTION 'Bootstrap observer grants differ';
  END IF;
END $$;
DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname IN ('board_media_intake','board_media_intake_owner')
      AND (rolcanlogin OR rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls)) THEN
    RAISE EXCEPTION 'Unqualified intake roles have login or elevated flags';
  END IF;
  IF EXISTS (SELECT 1 FROM pg_auth_members m JOIN pg_roles r ON r.oid = m.member
      WHERE r.rolname IN ('board_media_intake','board_media_intake_owner')) THEN
    RAISE EXCEPTION 'Intake roles have membership';
  END IF;
  IF NOT has_schema_privilege('board_media_intake','media_intake','USAGE')
     OR has_schema_privilege('board_media_intake','media_intake','CREATE')
     OR has_schema_privilege('board_media_intake','media','USAGE')
     OR has_any_column_privilege('board_media_intake','media_intake.handles','SELECT,INSERT,UPDATE,REFERENCES')
     OR has_any_column_privilege('board_media_intake','media.jobs','SELECT,INSERT,UPDATE,REFERENCES')
     OR NOT has_function_privilege('board_media_intake','media_intake.reserve(text)','EXECUTE') THEN
    RAISE EXCEPTION 'Bootstrap intake grants differ';
  END IF;
  IF has_column_privilege('board_media_intake_owner','media.jobs','lease_token','SELECT,INSERT,UPDATE')
     OR has_column_privilege('board_media_intake_owner','media.jobs','attempts','SELECT,INSERT,UPDATE')
     OR has_column_privilege('board_media_intake_owner','media.jobs','output_sha256','SELECT,INSERT,UPDATE')
     OR has_any_column_privilege('board_media_intake_owner','media.assets','INSERT,UPDATE,REFERENCES')
     OR has_table_privilege('board_media_intake_owner','media.jobs','DELETE,TRUNCATE,TRIGGER')
     OR has_schema_privilege('board_media_intake_owner','media_intake','CREATE') THEN
    RAISE EXCEPTION 'Intake owner exceeds required authority';
  END IF;
END $$;
SET ROLE board_media_intake;
SELECT media_intake.ready();
SELECT id IS NOT NULL AND capability IS NOT NULL AS reserved FROM media_intake.reserve('bootstrap-synthetic.png');
RESET ROLE;
SET ROLE board_media_read;
SELECT count(*) AS initially_approved FROM media.approved_assets;
RESET ROLE;
SET ROLE board_monitor;
SELECT capacity, receiving, queued, processing FROM monitoring.media_queue;
SQL
cleanup
trap - EXIT
printf 'Fresh role bootstrap passed: all migrations applied as owner; historical content preserved; staff-post and poster-count owners, reader, observer and intake remain NOLOGIN with restricted grants. Private cluster removed.\n'
