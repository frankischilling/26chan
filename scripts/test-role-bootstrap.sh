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
BEGIN;
SET ROLE board_migrator;
DO $$ BEGIN
  IF (SELECT title FROM content.boards WHERE slug='vp') IS DISTINCT FROM 'Pokémon' THEN
    RAISE EXCEPTION 'Imported source board name was decoded incorrectly';
  END IF;
END $$;
UPDATE content.boards SET title='Owned operator title' WHERE slug='vp';
\i migrations/0064_board_reference_encoding.sql
DO $$ BEGIN
  IF (SELECT title FROM content.boards WHERE slug='vp') IS DISTINCT FROM 'Owned operator title' THEN
    RAISE EXCEPTION 'Board encoding correction overwrote an operator title';
  END IF;
END $$;
ROLLBACK;
DO $$
BEGIN
  IF EXISTS (SELECT to_jsonb(p)-ARRAY['country','country_name','board_flag','flag_name','capcode','dice_result','fortune_text','fortune_color','wordfilter_payload','wordfilter_search','staff_authorized_limits'] FROM content.posts p EXCEPT SELECT to_jsonb(p) FROM public.owned_count_posts_before p)
     OR EXISTS (SELECT to_jsonb(p) FROM public.owned_count_posts_before p EXCEPT SELECT to_jsonb(p)-ARRAY['country','country_name','board_flag','flag_name','capcode','dice_result','fortune_text','fortune_color','wordfilter_payload','wordfilter_search','staff_authorized_limits'] FROM content.posts p)
     OR EXISTS (SELECT * FROM content.threads EXCEPT SELECT * FROM public.owned_count_threads_before)
     OR EXISTS (SELECT * FROM public.owned_count_threads_before EXCEPT SELECT * FROM content.threads)
     OR EXISTS (SELECT 1 FROM content.posts WHERE country IS NOT NULL OR country_name IS NOT NULL OR board_flag IS NOT NULL OR flag_name IS NOT NULL OR capcode IS NOT NULL OR dice_result IS NOT NULL OR fortune_text IS NOT NULL OR fortune_color IS NOT NULL OR wordfilter_payload IS NOT NULL OR wordfilter_search IS NOT NULL OR staff_authorized_limits)
     OR EXISTS (SELECT 1 FROM post_secrets.poster_contexts)
     OR content.unique_posters('countold',8800001) IS NOT NULL THEN
    RAISE NOTICE 'Historical upgrade diagnostics: %', (
      SELECT jsonb_build_object(
        'posts_forward_changed', EXISTS(SELECT to_jsonb(p)-ARRAY['country','country_name','board_flag','flag_name','capcode','dice_result','fortune_text','fortune_color','wordfilter_payload','wordfilter_search','staff_authorized_limits'] FROM content.posts p EXCEPT SELECT to_jsonb(p) FROM public.owned_count_posts_before p),
        'posts_reverse_changed', EXISTS(SELECT to_jsonb(p) FROM public.owned_count_posts_before p EXCEPT SELECT to_jsonb(p)-ARRAY['country','country_name','board_flag','flag_name','capcode','dice_result','fortune_text','fortune_color','wordfilter_payload','wordfilter_search','staff_authorized_limits'] FROM content.posts p),
        'threads_forward_changed', EXISTS(SELECT * FROM content.threads EXCEPT SELECT * FROM public.owned_count_threads_before),
        'threads_reverse_changed', EXISTS(SELECT * FROM public.owned_count_threads_before EXCEPT SELECT * FROM content.threads),
        'new_metadata_present', EXISTS(SELECT 1 FROM content.posts WHERE country IS NOT NULL OR country_name IS NOT NULL OR board_flag IS NOT NULL OR flag_name IS NOT NULL OR capcode IS NOT NULL OR dice_result IS NOT NULL OR fortune_text IS NOT NULL OR fortune_color IS NOT NULL OR wordfilter_payload IS NOT NULL OR wordfilter_search IS NOT NULL),
        'poster_context_present', EXISTS(SELECT 1 FROM post_secrets.poster_contexts),
        'historical_count_known', content.unique_posters('countold',8800001) IS NOT NULL
      )
    );
    RAISE EXCEPTION 'Poster count upgrade changed history or invented identity';
  END IF;
  IF (SELECT count(*) FROM content.boards WHERE source_order<1000 AND word_filter_enabled)<>79
     OR (SELECT count(*) FROM content.boards WHERE source_order<1000 AND NOT word_filter_enabled)<>3
     OR NOT has_column_privilege('board_attachment_owner','content.boards','word_filter_enabled','SELECT')
     OR NOT has_column_privilege('board_attachment_owner','content.boards','word_filter_profile','SELECT')
     OR EXISTS (SELECT 1 FROM unnest(ARRAY['board_public','board_staff','board_auth','board_media',
            'board_media_read','board_media_intake','board_monitor']) runtime(name)
         WHERE has_column_privilege(name,'content.posts','wordfilter_payload','INSERT,UPDATE')
            OR has_column_privilege(name,'content.posts','wordfilter_search','INSERT,UPDATE')
            OR (name<>'board_auth' AND has_function_privilege(name,
                'staff_identity.issue_wordfiltered_post_authority(bytea,bytea,bytea,integer,boolean,bigint,text,bigint,text,text,text,timestamptz,bytea,text)','EXECUTE')))
     OR NOT has_function_privilege('board_auth',
         'staff_identity.issue_wordfiltered_post_authority(bytea,bytea,bytea,integer,boolean,bigint,text,bigint,text,text,text,timestamptz,bytea,text)','EXECUTE') THEN
    RAISE EXCEPTION 'Wordfilter policy or authority grants differ';
  END IF;
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname='board_admission_owner'
      AND (rolcanlogin OR rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls))
     OR EXISTS (SELECT 1 FROM pg_auth_members m JOIN pg_roles r ON r.oid=m.member WHERE r.rolname='board_admission_owner')
     OR has_schema_privilege('board_admission_owner','content','CREATE')
     OR has_schema_privilege('board_admission_owner','admission','CREATE')
     OR has_schema_privilege('board_admission_owner','staff_identity','USAGE')
     OR has_schema_privilege('board_admission_owner','post_secrets','USAGE')
     OR has_schema_privilege('board_admission_owner','deployment','USAGE')
     OR has_schema_privilege('board_admission_owner','media','USAGE')
     OR has_any_column_privilege('board_admission_owner','content.posts','SELECT,INSERT,UPDATE,REFERENCES')
     OR has_any_column_privilege('board_admission_owner','content.threads','SELECT,INSERT,UPDATE,REFERENCES')
     OR has_table_privilege('board_admission_owner','admission.rules','INSERT,UPDATE,DELETE,TRUNCATE,TRIGGER')
     OR has_table_privilege('board_admission_owner','admission.logs','UPDATE,DELETE,TRUNCATE,TRIGGER')
     OR EXISTS (SELECT 1 FROM information_schema.columns c WHERE c.table_schema='content' AND c.table_name='boards'
          AND c.column_name NOT IN ('slug','staff_only') AND has_column_privilege('board_admission_owner','content.boards',c.column_name,'SELECT')) THEN
    RAISE EXCEPTION 'Content admission function owner exceeds required authority';
  END IF;
  IF EXISTS (SELECT 1 FROM unnest(ARRAY['board_public','board_staff','board_auth','board_media',
       'board_media_read','board_media_intake','board_monitor']) runtime(name)
       WHERE has_schema_privilege(name,'admission','USAGE')
          OR has_any_column_privilege(name,'admission.rules','SELECT,INSERT,UPDATE,REFERENCES')
          OR has_any_column_privilege(name,'admission.hits','SELECT,INSERT,UPDATE,REFERENCES')
          OR has_any_column_privilege(name,'admission.logs','SELECT,INSERT,UPDATE,REFERENCES')
          OR has_any_column_privilege(name,'admission.bans','SELECT,INSERT,UPDATE,REFERENCES')
          OR has_table_privilege(name,'admission.capacity','SELECT,UPDATE')
          OR (name<>'board_public' AND has_function_privilege(name,
              'content.record_content_admission(text,text,bigint,bigint,text,bigint,text,text,text,text)','EXECUTE')))
     OR NOT has_function_privilege('board_public',
          'content.record_content_admission(text,text,bigint,bigint,text,bigint,text,text,text,text)','EXECUTE')
     OR has_function_privilege('board_public','content.stamp_content_autosage()','EXECUTE')
     OR EXISTS(SELECT 1 FROM admission.rules)
     OR EXISTS(SELECT 1 FROM admission.hits)
     OR EXISTS(SELECT 1 FROM admission.logs)
     OR EXISTS(SELECT 1 FROM admission.bans) THEN
    RAISE EXCEPTION 'Content admission runtime grants or initial private state differ';
  END IF;
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname='board_robot9000_owner'
      AND (rolcanlogin OR rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls))
     OR EXISTS (SELECT 1 FROM pg_auth_members m JOIN pg_roles r ON r.oid=m.member WHERE r.rolname='board_robot9000_owner')
     OR has_schema_privilege('board_robot9000_owner','content','CREATE')
     OR has_schema_privilege('board_robot9000_owner','post_secrets','CREATE')
     OR has_schema_privilege('board_robot9000_owner','staff_identity','USAGE')
     OR has_schema_privilege('board_robot9000_owner','deployment','USAGE')
     OR has_schema_privilege('board_robot9000_owner','media','USAGE')
     OR has_table_privilege('board_robot9000_owner','content.posts','SELECT,INSERT,UPDATE,DELETE')
     OR has_any_column_privilege('board_robot9000_owner','post_secrets.deletion','SELECT,INSERT,UPDATE')
     OR has_table_privilege('board_robot9000_owner','post_secrets.deletion','DELETE,TRUNCATE,TRIGGER')
     OR EXISTS (SELECT 1 FROM information_schema.columns c WHERE c.table_schema='content' AND c.table_name='boards'
          AND c.column_name NOT IN ('slug','robot9000','robot9000_state_limit','staff_only')
          AND has_column_privilege('board_robot9000_owner','content.boards',c.column_name,'SELECT'))
     OR EXISTS (SELECT 1 FROM information_schema.columns c WHERE c.table_schema='content' AND c.table_name='boards'
          AND c.column_name<>'slug' AND has_column_privilege('board_robot9000_owner','content.boards',c.column_name,'UPDATE'))
     OR has_table_privilege('board_robot9000_owner','post_secrets.robot9000_texts','DELETE,TRUNCATE,TRIGGER,REFERENCES')
     OR has_table_privilege('board_robot9000_owner','post_secrets.robot9000_mutes','DELETE,TRUNCATE,TRIGGER,REFERENCES')
     OR EXISTS (SELECT 1 FROM unnest(ARRAY['SELECT','INSERT','UPDATE']) privilege(name)
          WHERE NOT has_table_privilege('board_robot9000_owner','post_secrets.robot9000_texts',name)
             OR NOT has_table_privilege('board_robot9000_owner','post_secrets.robot9000_mutes',name)) THEN
    RAISE EXCEPTION 'Robot9000 function owner exceeds required authority';
  END IF;
  IF EXISTS (SELECT 1 FROM unnest(ARRAY['board_public','board_staff','board_auth','board_media',
       'board_media_read','board_media_intake','board_monitor']) runtime(name)
       WHERE has_any_column_privilege(name,'post_secrets.robot9000_texts','SELECT,INSERT,UPDATE')
          OR has_any_column_privilege(name,'post_secrets.robot9000_mutes','SELECT,INSERT,UPDATE')
          OR has_table_privilege(name,'post_secrets.robot9000_texts','DELETE,TRUNCATE,TRIGGER')
          OR has_table_privilege(name,'post_secrets.robot9000_mutes','DELETE,TRUNCATE,TRIGGER')
          OR (name<>'board_public' AND has_function_privilege(name,
              'content.check_robot9000(text,bytea,bytea,double precision,timestamptz)','EXECUTE')))
     OR NOT has_function_privilege('board_public',
          'content.check_robot9000(text,bytea,bytea,double precision,timestamptz)','EXECUTE')
     OR EXISTS (SELECT 1 FROM post_secrets.robot9000_texts)
     OR EXISTS (SELECT 1 FROM post_secrets.robot9000_mutes) THEN
    RAISE EXCEPTION 'Robot9000 runtime grants or historical state differ';
  END IF;
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname='board_staff_post_owner'
      AND (rolcanlogin OR rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls))
     OR EXISTS (SELECT 1 FROM pg_auth_members m JOIN pg_roles r ON r.oid=m.member WHERE r.rolname='board_staff_post_owner')
     OR has_schema_privilege('board_staff_post_owner','content','CREATE')
     OR has_schema_privilege('board_staff_post_owner','staff_identity','CREATE')
     OR has_schema_privilege('board_staff_post_owner','deployment','USAGE')
     OR has_schema_privilege('board_staff_post_owner','media','USAGE')
     OR has_table_privilege('board_staff_post_owner','content.posts','INSERT,UPDATE,DELETE')
     OR has_any_column_privilege('board_staff_post_owner','staff_identity.credentials','INSERT,UPDATE')
     OR has_table_privilege('board_staff_post_owner','staff_identity.credentials','DELETE,TRUNCATE,TRIGGER')
     OR EXISTS (SELECT 1 FROM information_schema.columns c WHERE c.table_schema='staff_identity' AND c.table_name='credentials'
          AND c.column_name NOT IN ('id','account_id') AND has_column_privilege('board_staff_post_owner','staff_identity.credentials',c.column_name,'SELECT'))
     OR NOT has_column_privilege('board_staff_post_owner','staff_identity.credentials','id','SELECT')
     OR NOT has_column_privilege('board_staff_post_owner','staff_identity.credentials','account_id','SELECT')
     OR has_column_privilege('board_staff_post_owner','staff_identity.accounts','role','UPDATE')
     OR has_column_privilege('board_staff_post_owner','staff_identity.accounts','public_capcode','UPDATE')
     OR has_column_privilege('board_staff_post_owner','staff_identity.sessions','csrf_hash','UPDATE')
     OR has_column_privilege('board_staff_post_owner','staff_identity.sessions','expires_at','UPDATE')
     OR EXISTS (SELECT 1 FROM information_schema.columns c WHERE c.table_schema='post_secrets' AND c.table_name='staff_post_intents'
          AND c.column_name NOT IN ('token_hash','authorized_limits','comment_limit','comment','wordfilter_payload','wordfilter_search',
              'name','capcode','source_options','prepared_trip','source_name_allowed')
          AND has_column_privilege('board_staff_post_owner','post_secrets.staff_post_intents',c.column_name,'UPDATE'))
     OR EXISTS (SELECT 1 FROM information_schema.columns c WHERE c.table_schema='content' AND c.table_name='boards'
          AND c.column_name NOT IN ('slug','max_comment_chars','max_authorized_comment_chars','forced_anon','strip_tripcode')
          AND has_column_privilege('board_staff_post_owner','content.boards',c.column_name,'SELECT'))
     OR has_any_column_privilege('board_staff_post_owner','content.boards','INSERT,UPDATE')
     OR NOT has_column_privilege('board_staff_post_owner','content.boards','max_authorized_comment_chars','SELECT')
     OR NOT has_column_privilege('board_staff_post_owner','content.boards','forced_anon','SELECT')
     OR NOT has_column_privilege('board_staff_post_owner','content.boards','strip_tripcode','SELECT')
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
  IF (SELECT count(*) FROM content.boards WHERE source_order<1000 AND max_authorized_comment_chars=10000)<>81
     OR (SELECT max_authorized_comment_chars FROM content.boards WHERE slug='j')<>50000
     OR EXISTS(SELECT 1 FROM unnest(ARRAY['board_public','board_staff','board_auth','board_media',
           'board_media_read','board_media_intake','board_monitor']) runtime(name)
         WHERE has_column_privilege(name,'content.posts','staff_authorized_limits','INSERT,UPDATE')
            OR has_column_privilege(name,'content.boards','max_authorized_comment_chars','UPDATE')
            OR (name<>'board_auth' AND has_function_privilege(name,
              'staff_identity.issue_limited_post_authority(bytea,bytea,bytea,integer,boolean,bigint,text,bigint,text,text,text,timestamptz,boolean,integer,bytea,text)','EXECUTE')))
     OR NOT has_function_privilege('board_auth',
         'staff_identity.issue_limited_post_authority(bytea,bytea,bytea,integer,boolean,bigint,text,bigint,text,text,text,timestamptz,boolean,integer,bytea,text)','EXECUTE') THEN
    RAISE EXCEPTION 'Authorized-post policy or proof grants differ';
  END IF;
  IF EXISTS (SELECT 1 FROM unnest(ARRAY['board_public','board_staff','board_auth','board_media',
      'board_media_read','board_media_intake','board_monitor']) runtime(name)
      CROSS JOIN (VALUES('staff_identity.source_public_capcode(text,text[],text[],text[],text)'),
        ('staff_identity.source_capcode_name_allowed(text,text[],text[],text[])'),
        ('content.staff_display_name_size(text,text)')) helper(signature)
      WHERE has_function_privilege(runtime.name,helper.signature,'EXECUTE')
        OR (runtime.name<>'board_auth' AND has_function_privilege(runtime.name,
            'staff_identity.issue_source_post_authority(bytea,bytea,bytea,integer,boolean,bigint,text,bigint,text,text,text,timestamptz,boolean,integer,bytea,text,text,text,boolean)','EXECUTE')))
      OR NOT has_function_privilege('board_auth',
        'staff_identity.issue_source_post_authority(bytea,bytea,bytea,integer,boolean,bigint,text,bigint,text,text,text,timestamptz,boolean,integer,bytea,text,text,text,boolean)','EXECUTE') THEN
    RAISE EXCEPTION 'Source identity helpers or issuer exceed runtime authority';
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
-- The posting owner needs credential identifiers to lock a live session, but
-- must not read the passkey document or change credential identity.
SET ROLE board_staff_post_owner;
SELECT id,account_id FROM staff_identity.credentials LIMIT 0;
SELECT slug,max_comment_chars,max_authorized_comment_chars,forced_anon,strip_tripcode FROM content.boards WHERE slug='j';
DO $$
BEGIN
  IF (SELECT max_authorized_comment_chars FROM content.boards WHERE slug='j') IS DISTINCT FROM 50000 THEN
    RAISE EXCEPTION 'Posting owner cannot read the private board budget'; END IF;
  BEGIN
    PERFORM title FROM content.boards LIMIT 0;
    RAISE EXCEPTION 'Posting owner read unrelated board metadata';
  EXCEPTION WHEN insufficient_privilege THEN NULL;
  END;
  BEGIN
    PERFORM credential FROM staff_identity.credentials LIMIT 0;
    RAISE EXCEPTION 'Staff posting owner read a passkey document';
  EXCEPTION WHEN insufficient_privilege THEN NULL;
  END;
  BEGIN
    UPDATE staff_identity.credentials SET account_id=account_id WHERE false;
    RAISE EXCEPTION 'Staff posting owner changed credential identity';
  EXCEPTION WHEN insufficient_privilege THEN NULL;
  END;
  BEGIN
    DELETE FROM staff_identity.credentials WHERE false;
    RAISE EXCEPTION 'Staff posting owner deleted a credential';
  EXCEPTION WHEN insufficient_privilege THEN NULL;
  END;
END $$;
RESET ROLE;
SET ROLE board_robot9000_owner;
SELECT digest FROM post_secrets.robot9000_texts LIMIT 0;
SELECT actor FROM post_secrets.robot9000_mutes LIMIT 0;
DO $$ BEGIN
  BEGIN
    PERFORM password_hash FROM post_secrets.deletion LIMIT 0;
    RAISE EXCEPTION 'Robot9000 owner read a deletion secret';
  EXCEPTION WHEN insufficient_privilege THEN NULL;
  END;
  BEGIN
    PERFORM credential FROM staff_identity.credentials LIMIT 0;
    RAISE EXCEPTION 'Robot9000 owner read a staff credential';
  EXCEPTION WHEN insufficient_privilege THEN NULL;
  END;
  BEGIN
    UPDATE content.boards SET robot9000=false WHERE false;
    RAISE EXCEPTION 'Robot9000 owner changed board policy';
  EXCEPTION WHEN insufficient_privilege THEN NULL;
  END;
END $$;
RESET ROLE;
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
printf 'Fresh role bootstrap passed: all migrations applied as owner; historical content preserved; content admission, staff-post, poster-count and Robot9000 owners, reader, observer and intake remain NOLOGIN with restricted grants. Private cluster removed.\n'
