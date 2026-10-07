#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-staff-attachment.XXXXXXXX)
started=0
cleanup() {
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null
        started=0
    fi
    [[ $cluster =~ ^/tmp/board-staff-attachment\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
    [[ $(readlink -f "$cluster") = "$cluster" ]] || exit 1
    rm -rf -- "$cluster"
}
trap cleanup EXIT
chown postgres:postgres "$cluster"
runuser -u postgres -- "$pg_bin/initdb" -D "$cluster/data" --auth=trust --encoding=UTF8 --no-locale > /dev/null
runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" \
    -o "-c listen_addresses='' -c unix_socket_directories='$cluster'" -w start > /dev/null
started=1
psql=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h "$cluster")
runuser -u postgres -- "${psql[@]}" -d postgres -f deploy/roles.sql
runuser -u postgres -- "${psql[@]}" -d postgres <<'SQL'
CREATE DATABASE staff_attachment_upgrade OWNER board_migrator;
CREATE DATABASE staff_attachment_fresh OWNER board_migrator;
REVOKE ALL ON DATABASE staff_attachment_upgrade,staff_attachment_fresh FROM PUBLIC;
GRANT CONNECT ON DATABASE staff_attachment_upgrade,staff_attachment_fresh TO board_migrator,board_auth,board_staff;
-- Disposable trust-authenticated logins only.
ALTER ROLE board_auth LOGIN;
ALTER ROLE board_staff LOGIN;
SQL
migrator=("${psql[@]}" -U board_migrator -d staff_attachment_upgrade)
auth=("${psql[@]}" -U board_auth -d staff_attachment_upgrade)
staff=("${psql[@]}" -U board_staff -d staff_attachment_upgrade)
for migration in migrations/*.sql; do
    [[ $migration != migrations/0110_staff_post_attachments.sql ]] || break
    "${migrator[@]}" --single-transaction -f "$migration"
done
"${migrator[@]}" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,max_authorized_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,image_limit)
VALUES('oldfile','Owned staff attachment upgrade','Synthetic',2000,10000,100,100,100,10,100);
INSERT INTO content.threads(id,board,created_at,modified_at)
VALUES(8811001,'oldfile','2026-01-01Z','2026-01-02Z');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(8811001,'oldfile',8811001,'Historical name','Historical subject','Historical body','2026-01-01Z');
-- Preserve the imported row, then enable labels for the pending staff proof.
UPDATE content.boards SET user_ids=true WHERE slug='oldfile';
INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES(8811001,'owned-upgrade-derived-hash');
INSERT INTO post_secrets.op_peers(thread_id,peer) VALUES(8811001,'192.0.2.110');
INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at)
VALUES(repeat('a',32),repeat('a',32),repeat('a',32),repeat('a',64),100,500,300,'approved',clock_timestamp());
INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler,file_deleted)
VALUES(8811001,repeat('a',32),repeat('a',32),'historical.png',100,500,300,true,true);
INSERT INTO content.moderation_audit(account_id,board,target_id,action) VALUES(42,'oldfile',8811001,'spoiler');
INSERT INTO poll_private.polls(id,title,description,vote_count,published,catalogue_ordinal)
VALUES(8811001,'Operator poll','Retain operator configuration',3,true,1);
INSERT INTO poll_private.options(poll_id,id,ordinal,caption,score) VALUES(8811001,1,1,'Retain me',3);
DO $$ DECLARE actor bigint; BEGIN
    INSERT INTO staff_identity.accounts(role,flags,allow_boards,deny_boards)
    VALUES('admin',ARRAY['capcode','capcodename'],ARRAY['all'],ARRAY[]::text[]) RETURNING id INTO actor;
    INSERT INTO staff_identity.credentials(id,account_id,credential)
    VALUES(convert_to('owned-attachment-upgrade-key','UTF8'),actor,'{}');
    INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id)
    VALUES(decode(repeat('11',32),'hex'),decode(repeat('22',32),'hex'),actor,convert_to('owned-attachment-upgrade-key','UTF8'));
END $$;
SQL
"${auth[@]}" <<'SQL'
SELECT staff_identity.issue_source_post_authority(decode(repeat('33',32),'hex'),decode(repeat('11',32),'hex'),decode(repeat('22',32),'hex'),900,false,
    8811002,'oldfile',8811001,'Prepared text','','Pending text proof','2026-01-01Z',true,10000,NULL,NULL,'capcode_admin_hl','!ozOtJW9BFA',true,true);
SQL
"${migrator[@]}" <<'SQL'
CREATE VIEW public.owned_attachment_rows AS
    SELECT 'boards' AS relation,to_jsonb(b) AS value FROM content.boards b
    UNION ALL SELECT 'posts',to_jsonb(p) FROM content.posts p
    UNION ALL SELECT 'threads',to_jsonb(t) FROM content.threads t
    UNION ALL SELECT 'media',to_jsonb(m) FROM content.post_media m
    UNION ALL SELECT 'assets',to_jsonb(a) FROM media.assets a
    UNION ALL SELECT 'audit',to_jsonb(a) FROM content.moderation_audit a
    UNION ALL SELECT 'deletion',to_jsonb(d) FROM post_secrets.deletion d
    UNION ALL SELECT 'peers',to_jsonb(o) FROM post_secrets.op_peers o
    UNION ALL SELECT 'polls',to_jsonb(p) FROM poll_private.polls p
    UNION ALL SELECT 'options',to_jsonb(o) FROM poll_private.options o;
CREATE TABLE public.owned_attachment_rows_before AS TABLE public.owned_attachment_rows;
CREATE TABLE public.owned_attachment_intents_before AS SELECT to_jsonb(i) AS value FROM post_secrets.staff_post_intents i;
CREATE TABLE public.owned_attachment_policies_before AS SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy;
CREATE TABLE public.owned_attachment_functions_before AS SELECT p.oid,p.oid::regprocedure::text AS signature,p.proowner,p.proacl::text AS acl,p.prosecdef,p.proconfig,p.prosrc
    FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
    WHERE n.nspname IN ('content','media','post_secrets','staff_identity');
CREATE TABLE public.owned_attachment_tables_before AS SELECT c.oid,c.relowner,c.relacl::text AS acl FROM pg_class c
    JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname IN ('content','media','post_secrets','staff_identity','poll_private');
CREATE TABLE public.owned_attachment_columns_before AS SELECT a.attrelid,a.attnum,a.attname,a.attacl::text AS acl FROM pg_attribute a
    WHERE a.attrelid IN(SELECT oid FROM public.owned_attachment_tables_before);
BEGIN;
\i migrations/0110_staff_post_attachments.sql
ROLLBACK;
DO $$ BEGIN
    IF to_regclass('post_secrets.staff_attachment_handoffs') IS NOT NULL
       OR EXISTS(TABLE public.owned_attachment_rows_before EXCEPT TABLE public.owned_attachment_rows)
       OR EXISTS(TABLE public.owned_attachment_rows EXCEPT TABLE public.owned_attachment_rows_before)
       OR EXISTS(SELECT to_jsonb(i) FROM post_secrets.staff_post_intents i EXCEPT TABLE public.owned_attachment_intents_before)
       OR EXISTS(TABLE public.owned_attachment_intents_before EXCEPT SELECT to_jsonb(i) FROM post_secrets.staff_post_intents i)
       OR EXISTS(SELECT p.oid,p.oid::regprocedure::text,p.proowner,p.proacl::text,p.prosecdef,p.proconfig,p.prosrc FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname IN ('content','media','post_secrets','staff_identity') EXCEPT TABLE public.owned_attachment_functions_before)
       OR EXISTS(TABLE public.owned_attachment_functions_before EXCEPT SELECT p.oid,p.oid::regprocedure::text,p.proowner,p.proacl::text,p.prosecdef,p.proconfig,p.prosrc FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname IN ('content','media','post_secrets','staff_identity'))
       OR EXISTS(SELECT oid,relowner,relacl::text FROM pg_class WHERE oid IN(SELECT oid FROM public.owned_attachment_tables_before) EXCEPT TABLE public.owned_attachment_tables_before)
       OR EXISTS(SELECT attrelid,attnum,attname,attacl::text FROM pg_attribute WHERE attrelid IN(SELECT oid FROM public.owned_attachment_tables_before) EXCEPT TABLE public.owned_attachment_columns_before) THEN
        RAISE EXCEPTION 'Rolled-back attachment migration changed rows, proofs, functions or grants';
    END IF;
END $$;
SQL
"${migrator[@]}" --single-transaction -f migrations/0110_staff_post_attachments.sql
"${migrator[@]}" <<'SQL'
DO $$ BEGIN
    IF EXISTS(TABLE public.owned_attachment_rows_before EXCEPT TABLE public.owned_attachment_rows)
       OR EXISTS(TABLE public.owned_attachment_rows EXCEPT TABLE public.owned_attachment_rows_before)
       OR EXISTS(SELECT to_jsonb(i)-ARRAY['attachment_job','attachment_capability_hash','attachment_spoiler'] FROM post_secrets.staff_post_intents i EXCEPT TABLE public.owned_attachment_intents_before)
       OR EXISTS(TABLE public.owned_attachment_intents_before EXCEPT SELECT to_jsonb(i)-ARRAY['attachment_job','attachment_capability_hash','attachment_spoiler'] FROM post_secrets.staff_post_intents i)
       OR EXISTS(SELECT 1 FROM post_secrets.staff_post_intents WHERE attachment_job IS NOT NULL OR attachment_capability_hash IS NOT NULL OR attachment_spoiler IS NOT NULL)
       OR EXISTS(SELECT 1 FROM post_secrets.staff_attachment_handoffs)
       OR EXISTS(SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy EXCEPT TABLE public.owned_attachment_policies_before)
       OR EXISTS(TABLE public.owned_attachment_policies_before EXCEPT SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy)
       OR EXISTS(SELECT oid,relowner,relacl::text FROM pg_class WHERE oid IN(SELECT oid FROM public.owned_attachment_tables_before) EXCEPT TABLE public.owned_attachment_tables_before)
       OR EXISTS(TABLE public.owned_attachment_tables_before EXCEPT SELECT oid,relowner,relacl::text FROM pg_class WHERE oid IN(SELECT oid FROM public.owned_attachment_tables_before)) THEN
        RAISE EXCEPTION 'Attachment upgrade changed historical rows, pending text proof, table grants or policies';
    END IF;
    -- All retained functions keep their exact owner, ACL and definition except
    -- the renamed consumer, its invoker trigger and the scoped filename grant.
    IF EXISTS(SELECT 1 FROM public.owned_attachment_functions_before old
        LEFT JOIN pg_proc p ON p.oid=old.oid
        WHERE old.signature NOT IN (
            'content.consume_staff_post_authority(bytea,bigint,text,bigint,text,text,text,timestamp with time zone)',
            'content.apply_staff_capcode()', 'content.attachment_upload_filename(text,text)')
          AND (p.oid IS NULL OR p.proowner<>old.proowner OR p.proacl::text IS DISTINCT FROM old.acl
            OR p.prosecdef<>old.prosecdef OR p.proconfig IS DISTINCT FROM old.proconfig OR p.prosrc<>old.prosrc))
       OR NOT EXISTS(SELECT 1 FROM public.owned_attachment_functions_before old JOIN pg_proc p USING(oid)
          WHERE old.signature='content.consume_staff_post_authority(bytea,bigint,text,bigint,text,text,text,timestamp with time zone)'
            AND p.proname='consume_staff_post_authority_without_attachment' AND p.proowner=old.proowner
            AND p.prosecdef=old.prosecdef AND p.proconfig IS NOT DISTINCT FROM old.proconfig AND p.prosrc=old.prosrc)
       OR EXISTS(SELECT 1 FROM public.owned_attachment_columns_before old
          LEFT JOIN pg_attribute a ON a.attrelid=old.attrelid AND a.attnum=old.attnum
          WHERE NOT ((old.attrelid='content.boards'::regclass AND old.attname IN ('staff_only','upload_board','comment_spoiler_cleanup'))
              OR (old.attrelid='media.jobs'::regclass AND old.attname='input_bytes'))
            AND (a.attnum IS NULL OR a.attname<>old.attname OR a.attacl::text IS DISTINCT FROM old.acl)) THEN
        RAISE EXCEPTION 'Attachment upgrade changed an unrelated function or column grant';
    END IF;
END $$;
SQL
"${staff[@]}" <<'SQL'
BEGIN;
SELECT content.lock_posting_actor(decode(repeat('44',32),'hex'),false);
SELECT set_config('board.posting_actor',repeat('44',32),true);
SELECT set_config('board.staff_post_ticket',repeat('33',32),true),set_config('board.post_trip','!ozOtJW9BFA',true),
    set_config('board.staff_raw_name_nonempty','true',true);
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(8811002,'oldfile',8811001,'Prepared text','','Pending text proof','2026-01-01Z');
ROLLBACK;
SQL
"${migrator[@]}" <<'SQL'
DO $$ BEGIN
    IF NOT EXISTS(SELECT 1 FROM post_secrets.staff_post_intents WHERE token_hash=decode(repeat('33',32),'hex'))
       OR EXISTS(SELECT 1 FROM content.posts WHERE id=8811002)
       OR EXISTS(SELECT 1 FROM content.moderation_audit WHERE board='oldfile' AND action='staff-post')
       OR EXISTS(SELECT 1 FROM post_secrets.staff_attachment_handoffs) THEN
        RAISE EXCEPTION 'Rolled-back text consumption lost its proof or retained partial state';
    END IF;
END $$;
SQL
"${staff[@]}" <<'SQL'
BEGIN;
SELECT content.lock_posting_actor(decode(repeat('44',32),'hex'),false);
SELECT set_config('board.posting_actor',repeat('44',32),true);
SELECT set_config('board.staff_post_ticket',repeat('33',32),true),set_config('board.post_trip','!ozOtJW9BFA',true),
    set_config('board.staff_raw_name_nonempty','true',true);
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(8811002,'oldfile',8811001,'Prepared text','','Pending text proof','2026-01-01Z');
DO $$ BEGIN
    BEGIN
        INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
        VALUES(8811003,'oldfile',8811001,'Prepared text','','Pending text proof','2026-01-01Z');
        RAISE EXCEPTION 'Consumed text proof replayed' USING ERRCODE='ZX001';
    EXCEPTION WHEN SQLSTATE '28000' THEN NULL; END;
END $$;
COMMIT;
SQL
"${migrator[@]}" <<'SQL'
DO $$ BEGIN
    IF NOT EXISTS(SELECT 1 FROM content.posts WHERE id=8811002 AND capcode='admin_highlight'
          AND poster_id='Admin' AND trip='!ozOtJW9BFA' AND name='Prepared text' AND staff_authorized_limits)
       OR EXISTS(SELECT 1 FROM post_secrets.staff_post_intents)
       OR EXISTS(SELECT 1 FROM post_secrets.staff_attachment_handoffs)
       OR EXISTS(SELECT 1 FROM content.post_media WHERE post_id=8811002)
       OR (SELECT count(*) FROM content.moderation_audit WHERE board='oldfile' AND action='staff-post')<>1 THEN
        RAISE NOTICE 'Synthetic retained-proof diagnostics: %', (
            SELECT jsonb_build_object(
                'post', (SELECT jsonb_build_object('id',id,'capcode',capcode,'poster_id',poster_id,
                    'trip',trip,'name',name,'authorized_limits',staff_authorized_limits)
                    FROM content.posts WHERE id=8811002),
                'remaining_intents', (SELECT count(*) FROM post_secrets.staff_post_intents),
                'remaining_handoffs', (SELECT count(*) FROM post_secrets.staff_attachment_handoffs),
                'text_post_attachments', (SELECT count(*) FROM content.post_media WHERE post_id=8811002),
                'staff_post_audits', (SELECT count(*) FROM content.moderation_audit WHERE board='oldfile' AND action='staff-post'))
        );
        RAISE EXCEPTION 'Retained text proof failed single-use compatibility or left an attachment';
    END IF;
END $$;
SQL
# An owner-side receipt without its matching post cannot survive the transaction.
"${migrator[@]}" <<'SQL'
BEGIN;
SET LOCAL ROLE board_staff_post_owner;
DO $$ BEGIN
    BEGIN
        INSERT INTO post_secrets.staff_attachment_handoffs(post_id,board,thread_id,job_id,capability_hash,
            spoiler,authorized_limits,account_id,session_hash,idle_seconds,expires_at,transaction_id)
        VALUES(8811004,'oldfile',8811001,repeat('b',32),decode(repeat('bb',32),'hex'),false,true,42,
            decode(repeat('11',32),'hex'),900,clock_timestamp()+interval '1 minute',txid_current());
        SET CONSTRAINTS ALL IMMEDIATE;
        RAISE EXCEPTION 'Orphan attachment handoff survived its deferred check' USING ERRCODE='ZX001';
    EXCEPTION WHEN SQLSTATE '28000' THEN NULL; END;
    IF EXISTS(SELECT 1 FROM post_secrets.staff_attachment_handoffs) THEN
        RAISE EXCEPTION 'Rejected orphan left a persistent handoff';
    END IF;
END $$;
COMMIT;
SQL
# 0111 must change only two capability-scoped EXECUTE grants. Snapshot the
# entire retained function definitions, ACL entries, tables, columns and rows.
"${migrator[@]}" <<'SQL'
CREATE TABLE public.owned_upload_functions_before AS
    SELECT p.oid,to_jsonb(p)-'proacl' AS definition FROM pg_proc p
    JOIN pg_namespace n ON n.oid=p.pronamespace
    WHERE n.nspname IN ('content','media','media_intake','post_secrets','staff_identity');
CREATE TABLE public.owned_upload_acl_before AS
    SELECT p.oid,a.* FROM pg_proc p JOIN public.owned_upload_functions_before old ON old.oid=p.oid
    CROSS JOIN LATERAL aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a;
CREATE TABLE public.owned_upload_tables_before AS
    SELECT c.oid,to_jsonb(c) AS definition FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
    WHERE n.nspname IN ('content','media','media_intake','post_secrets','staff_identity');
CREATE TABLE public.owned_upload_columns_before AS
    SELECT a.attrelid,a.attnum,to_jsonb(a) AS definition FROM pg_attribute a
    JOIN public.owned_upload_tables_before old ON old.oid=a.attrelid;
CREATE TABLE public.owned_upload_rows_before AS TABLE public.owned_attachment_rows;
BEGIN;
\i migrations/0111_staff_upload_controls.sql
ROLLBACK;
DO $$ BEGIN
    IF EXISTS(SELECT p.oid,a.* FROM pg_proc p JOIN public.owned_upload_functions_before old ON old.oid=p.oid
        CROSS JOIN LATERAL aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a
        EXCEPT TABLE public.owned_upload_acl_before)
       OR EXISTS(TABLE public.owned_upload_acl_before EXCEPT
        SELECT p.oid,a.* FROM pg_proc p JOIN public.owned_upload_functions_before old ON old.oid=p.oid
        CROSS JOIN LATERAL aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a) THEN
        RAISE EXCEPTION 'Rolled-back upload controls changed grants';
    END IF;
END $$;
SQL
"${migrator[@]}" --single-transaction -f migrations/0111_staff_upload_controls.sql
"${migrator[@]}" <<'SQL'
DO $$ BEGIN
    IF EXISTS(SELECT p.oid,to_jsonb(p)-'proacl' FROM pg_proc p
        JOIN pg_namespace n ON n.oid=p.pronamespace
        WHERE n.nspname IN ('content','media','media_intake','post_secrets','staff_identity')
        EXCEPT TABLE public.owned_upload_functions_before)
       OR EXISTS(TABLE public.owned_upload_functions_before EXCEPT
        SELECT p.oid,to_jsonb(p)-'proacl' FROM pg_proc p)
       OR EXISTS(SELECT c.oid,to_jsonb(c) FROM pg_class c JOIN public.owned_upload_tables_before old ON old.oid=c.oid
        EXCEPT TABLE public.owned_upload_tables_before)
       OR EXISTS(SELECT a.attrelid,a.attnum,to_jsonb(a) FROM pg_attribute a
        JOIN public.owned_upload_tables_before old ON old.oid=a.attrelid EXCEPT TABLE public.owned_upload_columns_before)
       OR EXISTS(TABLE public.owned_upload_rows_before EXCEPT TABLE public.owned_attachment_rows)
       OR EXISTS(TABLE public.owned_attachment_rows EXCEPT TABLE public.owned_upload_rows_before)
       OR EXISTS(TABLE public.owned_upload_acl_before EXCEPT
        SELECT p.oid,a.* FROM pg_proc p JOIN public.owned_upload_functions_before old ON old.oid=p.oid
        CROSS JOIN LATERAL aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a) THEN
        RAISE EXCEPTION 'Upload controls changed retained definitions, tables, rows or grants';
    END IF;
    IF (SELECT count(*) FROM (
        SELECT p.oid,a.* FROM pg_proc p JOIN public.owned_upload_functions_before old ON old.oid=p.oid
        CROSS JOIN LATERAL aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a
        EXCEPT TABLE public.owned_upload_acl_before) added)<>2
       OR EXISTS(SELECT 1 FROM (
        SELECT p.oid,a.* FROM pg_proc p JOIN public.owned_upload_functions_before old ON old.oid=p.oid
        CROSS JOIN LATERAL aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a
        EXCEPT TABLE public.owned_upload_acl_before) added
        WHERE oid NOT IN ('content.check_attachment_upload(text,text)'::regprocedure,
            'content.cancel_attachment_upload(text,text)'::regprocedure)
          OR grantor<>(SELECT oid FROM pg_roles WHERE rolname='board_attachment_owner')
          OR grantee<>(SELECT oid FROM pg_roles WHERE rolname='board_staff')
          OR privilege_type<>'EXECUTE' OR is_grantable) THEN
        RAISE EXCEPTION 'Upload controls added more than the two reviewed staff grants';
    END IF;
END $$;
SQL
# Historical compatibility is checked at 0110 and 0111 above. Bring the upgraded
# database to the current schema before evaluating the application's contract.
for migration in migrations/*.sql; do
    [[ $migration > migrations/0111_staff_upload_controls.sql ]] || continue
    "${migrator[@]}" --single-transaction -f "$migration"
done
# Run the exact runtime readiness query under both restricted pools.
ready_sql=$(python3 - <<'PY'
from pathlib import Path
s = Path('apps/staff/src/auth.rs').read_text()
print(s.split('pub(crate) const STAFF_ATTACHMENT_READY_SQL: &str = r#"', 1)[1].split('"#;', 1)[0])
PY
)
for role in board_auth board_staff; do
    [[ $("${psql[@]}" -At -U "$role" -d staff_attachment_upgrade -c "$ready_sql") = t ]] || {
        echo "Attachment readiness rejected the populated upgrade for $role" >&2; exit 1;
    }
done
# Readiness must notice broken permissions and a disabled deferred safeguard.
for alteration in \
    'GRANT SELECT ON post_secrets.staff_attachment_handoffs TO board_public;' \
    'REVOKE EXECUTE ON FUNCTION content.check_attachment_upload(text,text) FROM board_staff;' \
    'REVOKE EXECUTE ON FUNCTION content.cancel_attachment_upload(text,text) FROM board_staff;' \
    'REVOKE EXECUTE ON FUNCTION content.check_attachment_upload(text,text) FROM board_public;' \
    'GRANT EXECUTE ON FUNCTION content.cancel_attachment_upload(text,text) TO board_auth;' \
    'GRANT EXECUTE ON FUNCTION content.check_attachment_upload(text,text) TO PUBLIC;' \
    'GRANT EXECUTE ON FUNCTION content.cancel_attachment_upload(text,text) TO board_staff WITH GRANT OPTION;' \
    'ALTER FUNCTION content.check_attachment_upload(text,text) SET search_path=public;' \
    'GRANT SELECT ON media_intake.handles TO board_staff;' \
    'ALTER TABLE post_secrets.staff_attachment_handoffs DISABLE TRIGGER reject_orphan_staff_attachment;'; do
    runuser -u postgres -- "${psql[@]}" -d staff_attachment_upgrade <<SQL
BEGIN;
$alteration
SET LOCAL ROLE board_staff;
SELECT ($ready_sql) AS attachment_ready \gset
\if :attachment_ready
    \echo 'Attachment readiness accepted a broken private contract'
    \quit 1
\endif
ROLLBACK;
SQL
done
# Fresh initialization must produce the same private contract.
for migration in migrations/*.sql; do
    "${psql[@]}" -U board_migrator -d staff_attachment_fresh --single-transaction -f "$migration"
done
for role in board_auth board_staff; do
    [[ $("${psql[@]}" -At -U "$role" -d staff_attachment_fresh -c "$ready_sql") = t ]] || {
        echo "Attachment readiness rejected fresh initialization for $role" >&2; exit 1;
    }
done
echo 'Staff attachment migration passed: fresh and populated catalogs, retained history and grants, migration rollback, pending text proof rollback and single use, and fail-closed readiness.'
