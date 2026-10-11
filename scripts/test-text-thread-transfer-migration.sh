#!/usr/bin/env bash
# Owned synthetic PG16 cluster only: populated upgrade, narrow SQL handoff,
# catalog drift rejection, and administrator dump/restore with changed OIDs.
# This does not qualify Rust formatting or HTTP authorization semantics.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-text-transfer.XXXXXXXX)
exec 3>&1
startup_attempted=0
cleanup() {
    status=$?
    trap - EXIT
    [[ $cluster =~ ^/tmp/board-text-transfer\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
    [[ $(readlink -f "$cluster") = "$cluster" ]] || exit 1
    if [[ $startup_attempted = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null 2>&1 || status=1
        if runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" status > /dev/null 2>&1; then
            server_status=0
        else
            server_status=$?
        fi
        if [[ $server_status != 3 ]]; then
            printf 'Transfer cluster stop is unconfirmed; private cluster and diagnostics preserved at %s.\n' "$cluster" >&3
            exit 1
        fi
    fi
    if [[ $status != 0 ]]; then
        printf 'Transfer qualification failed; stopped private cluster and diagnostics preserved at %s.\n' "$cluster" >&3
    else
        rm -rf -- "$cluster"
    fi
    exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
chown postgres:postgres "$cluster"
runuser -u postgres -- "$pg_bin/initdb" -D "$cluster/data" --auth=trust --encoding=UTF8 --no-locale > /dev/null
startup_attempted=1
runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" \
    -o "-c listen_addresses='' -c unix_socket_directories='$cluster' -c statement_timeout=30000 -c lock_timeout=5000" -w start > /dev/null
psql=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h "$cluster")
exec > "$cluster/qualification.log" 2>&1
trap 'printf "Transfer qualification failed at line %s (private diagnostics are not printed).\n" "$LINENO" >&3' ERR
runuser -u postgres -- "${psql[@]}" -d postgres -f - < deploy/roles.sql
runuser -u postgres -- "${psql[@]}" -d postgres -c 'ALTER ROLE board_staff LOGIN; ALTER ROLE board_auth LOGIN'
create_database() {
    runuser -u postgres -- "${psql[@]}" -d postgres -v database="$1" <<'SQL'
CREATE DATABASE :"database" OWNER board_migrator;
REVOKE ALL ON DATABASE :"database" FROM PUBLIC;
GRANT CONNECT ON DATABASE :"database" TO board_migrator,board_public,board_staff,board_auth;
SQL
}
create_database text_transfer_upgrade
migrator=("${psql[@]}" -U board_migrator -d text_transfer_upgrade)
admin=(runuser -u postgres -- "${psql[@]}" -d text_transfer_upgrade)
for migration in migrations/*.sql; do
    [[ $(basename "$migration") < 0131_ ]] || break
    "${migrator[@]}" --single-transaction -f - < "$migration"
done
"${admin[@]}" <<'SQL'
CREATE FUNCTION public.capture_rows() RETURNS TABLE(relation text,value jsonb)
LANGUAGE plpgsql AS $$ DECLARE r record; BEGIN
 FOR r IN SELECT n.nspname,c.relname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname NOT IN('public','pg_catalog','information_schema') AND n.nspname !~ '^pg_'
 AND c.relkind='r' LOOP
  RETURN QUERY EXECUTE format('SELECT %L,to_jsonb(r) FROM %I.%I r',r.nspname||'.'||r.relname,r.nspname,r.relname);
 END LOOP;
END $$;
REVOKE ALL ON FUNCTION public.capture_rows() FROM PUBLIC;
CREATE FUNCTION public.normalized_acl(object_kind "char",owner_id oid,permissions aclitem[]) RETURNS jsonb
LANGUAGE sql STABLE SET search_path=pg_catalog,pg_temp AS $$
 SELECT coalesce(jsonb_agg(jsonb_build_array(pg_get_userbyid(a.grantor),
  CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee)::text END,
  a.privilege_type,a.is_grantable) ORDER BY pg_get_userbyid(a.grantor),
  CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee)::text END,
  a.privilege_type,a.is_grantable),'[]'::jsonb)
 FROM aclexplode(coalesce(permissions,acldefault(object_kind,owner_id))) a
$$;
REVOKE ALL ON FUNCTION public.normalized_acl("char",oid,aclitem[]) FROM PUBLIC;
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,image_limit)
VALUES('xfersrc','Owned transfer source','Synthetic',1000,100,100,100,10,100),
 ('xferdst','Owned transfer destination','Synthetic',1000,100,100,100,10,100);
BEGIN;
SET LOCAL ROLE board_migrator;
SELECT set_config('board.posting_actor',repeat('11',32),true);
SELECT set_config('board.poster_fingerprint',repeat('12',32),true);
SELECT set_config('board.poster_epoch',repeat('13',32),true);
INSERT INTO content.threads(id,board) VALUES(13100001,'xfersrc'),(13100010,'xferdst');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
SELECT i,'xfersrc',13100001,'Owned name','Owned subject','Owned static transfer text.'
FROM unnest(ARRAY[13100001,13100002,13100003]) i;
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(13100010,'xferdst',13100010,'Unrelated name','Unrelated subject','Unrelated retained text.');
COMMIT;
BEGIN;
SELECT slug FROM content.boards WHERE slug='xfersrc' FOR UPDATE;
SET LOCAL ROLE board_public;
UPDATE content.posts SET deleted=true WHERE id=13100003;
COMMIT;
INSERT INTO staff_identity.accounts(id,role) OVERRIDING SYSTEM VALUE VALUES(13100001,'admin');
INSERT INTO staff_identity.credentials(id,account_id,credential) VALUES(decode('1310','hex'),13100001,'{}');
INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id)
VALUES(decode(repeat('88',32),'hex'),decode(repeat('99',32),'hex'),13100001,decode('1310','hex'));
DO $$ BEGIN
 IF (SELECT count(*) FROM content.posts WHERE id IN(13100001,13100002)
  AND (comment_format BETWEEN 104 AND 111 OR comment_format BETWEEN 120 AND 127))<>2
 OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=13100003 AND content_erased AND deleted AND comment='')
 THEN RAISE EXCEPTION 'Pre-upgrade synthetic fixtures are invalid'; END IF;
END $$;
CREATE TABLE public.before_rows AS SELECT * FROM public.capture_rows();
SQL
# Transactional migration rollback must preserve every preexisting row too.
{ printf 'BEGIN;\n'; cat migrations/0131_text_thread_transfer.sql; printf '\nROLLBACK;\n'; } | "${migrator[@]}"
"${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM information_schema.columns WHERE table_schema='content' AND table_name='posts' AND column_name='saved_presentation')
 OR to_regclass('post_secrets.thread_transfer_gate') IS NOT NULL
 OR EXISTS(TABLE public.before_rows EXCEPT ALL SELECT * FROM public.capture_rows())
 OR EXISTS(SELECT * FROM public.capture_rows() EXCEPT ALL TABLE public.before_rows)
 THEN RAISE EXCEPTION '0131 rollback changed schema or preexisting data'; END IF;
END $$;
SQL
"${migrator[@]}" --single-transaction -f - < migrations/0131_text_thread_transfer.sql
"${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(TABLE public.before_rows EXCEPT ALL
  SELECT relation,CASE WHEN relation='content.posts' THEN value-'saved_presentation'-'saved_presentation_search' ELSE value END
  FROM public.capture_rows() WHERE relation NOT LIKE 'post_secrets.thread_transfer_%')
 OR EXISTS(SELECT relation,CASE WHEN relation='content.posts' THEN value-'saved_presentation'-'saved_presentation_search' ELSE value END
  FROM public.capture_rows() WHERE relation NOT LIKE 'post_secrets.thread_transfer_%' EXCEPT ALL TABLE public.before_rows)
 OR EXISTS(SELECT 1 FROM content.posts WHERE saved_presentation IS NOT NULL OR saved_presentation_search IS NOT NULL)
 OR (SELECT count(*) FROM information_schema.columns WHERE table_schema='content' AND table_name='posts'
  AND column_name IN('saved_presentation','saved_presentation_search') AND is_nullable='YES' AND column_default IS NULL)<>2
 OR NOT EXISTS(SELECT 1 FROM information_schema.columns WHERE table_schema='content' AND table_name='posts'
  AND column_name='saved_presentation' AND data_type='bytea')
 OR NOT EXISTS(SELECT 1 FROM information_schema.columns WHERE table_schema='content' AND table_name='posts'
  AND column_name='saved_presentation_search' AND data_type='text')
 OR (SELECT count(*) FROM post_secrets.thread_transfer_gate WHERE singleton)<>1
 OR (SELECT count(*) FROM post_secrets.thread_transfer_catalog WHERE singleton AND octet_length(definition_hash)=32)<>1
 OR EXISTS(SELECT 1 FROM public.capture_rows() WHERE relation LIKE 'post_secrets.thread_transfer_%'
  AND relation NOT IN('post_secrets.thread_transfer_gate','post_secrets.thread_transfer_catalog'))
 THEN RAISE EXCEPTION '0131 rewrote preexisting data or initialized unexpected transfer state'; END IF;
END $$;
DROP TABLE public.before_rows;
SQL
cat > "$cluster/ready.sql" <<'SQL'
DO $$ BEGIN
 IF content.text_thread_transfer_ready() IS DISTINCT FROM true
 OR staff_identity.text_thread_transfer_ready() IS DISTINCT FROM true
 THEN RAISE EXCEPTION 'Transfer readiness positive control failed'; END IF;
END $$;
SQL
cat > "$cluster/not-ready.sql" <<'SQL'
DO $$ BEGIN
 IF content.text_thread_transfer_ready() IS DISTINCT FROM false
 OR staff_identity.text_thread_transfer_ready() IS DISTINCT FROM false
 THEN RAISE EXCEPTION 'Transfer readiness accepted catalog drift'; END IF;
END $$;
SQL
# Each drift gets its own positive control and transaction, including a second
# positive control after rollback. No drift is used to reseal the catalog hash.
cat > "$cluster/drifts.txt" <<'SQL'
CREATE POLICY owned_transfer_restrictive ON content.posts AS RESTRICTIVE FOR SELECT TO board_thread_transfer_owner USING (id=thread_id)
CREATE POLICY owned_transfer_private_restrictive ON post_secrets.deletion AS RESTRICTIVE FOR SELECT TO board_thread_transfer_owner USING (false)
GRANT EXECUTE ON FUNCTION content.consume_text_thread_transfer(bytea) TO board_public
ALTER FUNCTION content.consume_text_thread_transfer(bytea) OWNER TO board_migrator
ALTER FUNCTION content.consume_text_thread_transfer(bytea) SET search_path=public,pg_temp
ALTER TABLE content.posts ALTER COLUMN saved_presentation SET DEFAULT decode('465030310000000000','hex')
ALTER TABLE content.posts DROP CONSTRAINT posts_saved_presentation_check
ALTER TABLE content.posts DROP CONSTRAINT posts_erased_presentation
ALTER TABLE content.posts ADD CONSTRAINT owned_transfer_extra_check CHECK(id>0)
GRANT SELECT(saved_presentation) ON content.posts TO board_auth
GRANT SELECT ON post_secrets.thread_transfer_map TO board_staff
ALTER ROLE board_thread_transfer_owner LOGIN
ALTER ROLE board_thread_transfer_owner BYPASSRLS
SQL
cat > "$cluster/assert-state.sql" <<'SQL'
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='board_thread_transfer_owner'
  AND NOT (rolcanlogin OR rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls))
 OR EXISTS(SELECT 1 FROM pg_auth_members WHERE member='board_thread_transfer_owner'::regrole)
 OR EXISTS(SELECT 1 FROM pg_auth_members WHERE roleid='board_thread_transfer_owner'::regrole AND member<>'board_migrator'::regrole)
 OR EXISTS(SELECT 1 FROM pg_proc WHERE proowner='board_thread_transfer_owner'::regrole
  AND (proconfig IS NULL OR NOT ('search_path=pg_catalog, pg_temp'=ANY(proconfig))))
 OR EXISTS(SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
  WHERE n.nspname='post_secrets' AND c.relname LIKE 'thread_transfer_%' AND c.relkind='r'
  AND c.relowner<>'board_thread_transfer_owner'::regrole)
 THEN RAISE EXCEPTION 'Transfer owner/function hardening is invalid'; END IF;
 IF NOT EXISTS(SELECT 1 FROM content.posts WHERE id=13100003 AND deleted AND content_erased
  AND saved_presentation IS NULL AND saved_presentation_search IS NULL AND comment='')
 THEN RAISE EXCEPTION 'Preexisting canonical tombstone changed'; END IF;
END $$;
SQL
cat > "$cluster/fingerprint.sql" <<'SQL'
SELECT relation,md5(value::text) FROM public.capture_rows() ORDER BY relation,value::text;
SELECT n.nspname,p.proname,pg_get_function_identity_arguments(p.oid),pg_get_userbyid(p.proowner),p.proconfig::text,public.normalized_acl('f',p.proowner,p.proacl),md5(pg_get_functiondef(p.oid)) FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname NOT IN('public','pg_catalog','information_schema') AND n.nspname !~ '^pg_' ORDER BY 1,2,3;
SELECT t.tgrelid::regclass::text,t.tgname,t.tgenabled,pg_get_triggerdef(t.oid) FROM pg_trigger t WHERE NOT t.tgisinternal ORDER BY 1,2;
SELECT c.conrelid::regclass::text,c.conname,pg_get_constraintdef(c.oid,true) FROM pg_constraint c JOIN pg_namespace n ON n.oid=c.connamespace WHERE n.nspname NOT IN('public','pg_catalog','information_schema') AND n.nspname !~ '^pg_' ORDER BY 1,2;
SELECT table_schema,table_name,column_name,grantor,grantee,privilege_type,is_grantable FROM information_schema.column_privileges WHERE table_schema NOT IN('public','pg_catalog','information_schema') AND table_schema !~ '^pg_' ORDER BY 1,2,3,4,5,6,7;
SELECT n.nspname,pg_get_userbyid(n.nspowner),public.normalized_acl('n',n.nspowner,n.nspacl) FROM pg_namespace n WHERE n.nspname NOT IN('public','pg_catalog','information_schema') AND n.nspname !~ '^pg_' ORDER BY 1;
SELECT n.nspname,c.relname,c.relkind,pg_get_userbyid(c.relowner),public.normalized_acl(CASE WHEN c.relkind='S' THEN 's' ELSE 'r' END::"char",c.relowner,c.relacl),c.relrowsecurity,c.relforcerowsecurity,c.reloptions::text,CASE WHEN c.relkind='v' THEN pg_get_viewdef(c.oid,true) END FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname NOT IN('public','pg_catalog','information_schema') AND n.nspname !~ '^pg_' AND c.relkind IN('r','p','v','m','f','S') ORDER BY 1,2;
SELECT schemaname,tablename,policyname,permissive,roles::text,cmd,qual,with_check FROM pg_policies WHERE schemaname NOT IN('public','pg_catalog','information_schema') AND schemaname !~ '^pg_' ORDER BY 1,2,3;
SELECT table_schema,table_name,column_name,ordinal_position,is_nullable,data_type,udt_name,column_default FROM information_schema.columns WHERE table_schema NOT IN('public','pg_catalog','information_schema') AND table_schema !~ '^pg_' ORDER BY 1,2,4;
SQL

# Real board_auth login prepares and seals only synthetic static text. FP01:
# false provenance, one Text atom, big-endian UTF-8 byte length, UTF-8 text.
# This matches the finite codec layout; it is not a Rust formatter proof.
"${psql[@]}" -U board_auth -d text_transfer_upgrade <<'SQL'
SELECT encode(source_fingerprint,'hex') AS fingerprint,array_agg(source_post_id ORDER BY ordinal)::text AS old_ids,
 array_agg(destination_post_id ORDER BY ordinal)::text AS new_ids
FROM staff_identity.prepare_text_thread_transfer(decode(repeat('aa',32),'hex'),decode(repeat('88',32),'hex'),
 decode(repeat('99',32),'hex'),3600,'xfersrc',13100001,'xferdst',1::smallint)
GROUP BY source_fingerprint \gset
SELECT staff_identity.seal_text_thread_transfer(decode(repeat('aa',32),'hex'),decode(repeat('88',32),'hex'),
 decode(repeat('99',32),'hex'),decode(:'fingerprint','hex'),1::smallint,:'old_ids'::bigint[],:'new_ids'::bigint[],
 ARRAY['Owned static transfer text.','Owned static transfer text.'],
 array_fill(decode('46503031000000000100','hex')||int4send(octet_length('Owned static transfer text.'))||convert_to('Owned static transfer text.','UTF8'),ARRAY[2]),
 ARRAY['Owned static transfer text.','Owned static transfer text.']);
SQL
"${admin[@]}" -c 'CREATE TABLE public.sealed_rows AS SELECT * FROM public.capture_rows()'
"${psql[@]}" -U board_staff -d text_transfer_upgrade <<'SQL'
BEGIN;
SELECT * FROM content.consume_text_thread_transfer(decode(repeat('aa',32),'hex'));
ROLLBACK;
SQL
"${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(TABLE public.sealed_rows EXCEPT ALL SELECT * FROM public.capture_rows())
 OR EXISTS(SELECT * FROM public.capture_rows() EXCEPT ALL TABLE public.sealed_rows)
 THEN RAISE EXCEPTION 'Consumer rollback changed rows'; END IF;
END $$;
DROP TABLE public.sealed_rows;
SQL
"${psql[@]}" -U board_staff -d text_transfer_upgrade -c "SELECT * FROM content.consume_text_thread_transfer(decode(repeat('aa',32),'hex'))"
"${admin[@]}" <<'SQL'
DO $$ BEGIN
 IF (SELECT count(*) FROM content.posts WHERE board='xfersrc' AND content_erased AND deleted
  AND comment='' AND saved_presentation IS NULL AND saved_presentation_search IS NULL)<>3
 OR (SELECT count(*) FROM content.posts WHERE board='xferdst' AND saved_presentation=
  decode('46503031000000000100','hex')||int4send(octet_length('Owned static transfer text.'))||convert_to('Owned static transfer text.','UTF8')
  AND saved_presentation_search='Owned static transfer text.' AND comment='Owned static transfer text.'
  AND NOT deleted AND NOT content_erased)<>2
 OR (SELECT count(*) FROM post_secrets.thread_transfer_results)<>1
 OR (SELECT count(*) FROM post_secrets.thread_transfer_result_posts)<>2
 OR EXISTS(SELECT 1 FROM post_secrets.thread_transfer_intents)
 OR EXISTS(SELECT 1 FROM post_secrets.thread_transfer_active)
 OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=13100010 AND comment='Unrelated retained text.' AND NOT deleted)
 THEN RAISE EXCEPTION 'Narrow SQL handoff state is invalid'; END IF;
END $$;
SQL

for phase in live restored; do
    database=text_transfer_upgrade
    if [[ $phase = restored ]]; then
        runuser -u postgres -- "$pg_bin/pg_dump" -h "$cluster" -U postgres -d "$database" --format=custom > "$cluster/current.dump"
        create_database text_transfer_restore
        runuser -u postgres -- "$pg_bin/pg_restore" -h "$cluster" --dbname=text_transfer_restore --single-transaction --exit-on-error < "$cluster/current.dump"
        database=text_transfer_restore
    fi
    admin=(runuser -u postgres -- "${psql[@]}" -d "$database")
    printf 'Checking %s transfer readiness and retained state.\n' "$phase"
    "${admin[@]}" -f - < "$cluster/ready.sql"
    # Caller display settings must not affect stable catalog identities. LOCAL
    # settings and rollback restore each connection without changing the seal.
    { printf "BEGIN; SET LOCAL TIME ZONE 'UTC'; SET LOCAL DateStyle='ISO, YMD';\n"
      cat "$cluster/ready.sql"
      printf "SET LOCAL TIME ZONE 'Pacific/Auckland'; SET LOCAL DateStyle='SQL, DMY';\n"
      cat "$cluster/ready.sql"
      printf 'ROLLBACK;\n'
      cat "$cluster/ready.sql"
    } | "${admin[@]}"
    "${admin[@]}" -f - < "$cluster/assert-state.sql"
    "${psql[@]}" -U board_auth -d "$database" <<'SQL'
DO $$ BEGIN
 IF staff_identity.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Auth readiness failed'; END IF;
 BEGIN
  PERFORM content.consume_text_thread_transfer(decode(repeat('aa',32),'hex'));
  RAISE EXCEPTION 'Auth could invoke staff consumer' USING ERRCODE='ZX131';
 EXCEPTION WHEN insufficient_privilege THEN NULL;
 END;
 IF (SELECT count(*) FROM staff_identity.text_thread_transfer_result(NULL,decode(repeat('88',32),'hex'),
  decode(repeat('99',32),'hex'),3600,'xfersrc',13100001))<>2
 THEN RAISE EXCEPTION 'Auth could not reconcile durable transfer receipt'; END IF;
END $$;
SQL
    "${psql[@]}" -U board_staff -d "$database" <<'SQL'
DO $$ BEGIN
 IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Staff readiness failed'; END IF;
 BEGIN
  PERFORM staff_identity.prepare_text_thread_transfer(decode(repeat('ab',32),'hex'),
   decode(repeat('88',32),'hex'),decode(repeat('99',32),'hex'),3600,'xfersrc',13100001,'xferdst',1::smallint);
  RAISE EXCEPTION 'Staff could invoke auth preparation' USING ERRCODE='ZX131';
 EXCEPTION WHEN insufficient_privilege THEN NULL;
 END;
END $$;
SQL
    for role in board_public board_auth board_staff; do
        "${psql[@]}" -U "$role" -d "$database" <<'SQL'
BEGIN;
DO $$ DECLARE relation_name text; command text; BEGIN
 FOREACH relation_name IN ARRAY ARRAY['gate','intents','map','sealed_rows','active','results','result_posts','catalog'] LOOP
  FOREACH command IN ARRAY ARRAY[
   format('SELECT * FROM post_secrets.thread_transfer_%I',relation_name),
   format('DELETE FROM post_secrets.thread_transfer_%I',relation_name)] LOOP
   BEGIN
    EXECUTE command;
    RAISE EXCEPTION 'Runtime accessed private transfer state' USING ERRCODE='ZX131';
   EXCEPTION WHEN insufficient_privilege THEN NULL;
   END;
  END LOOP;
 END LOOP;
 BEGIN
  PERFORM post_secrets.transfer_catalog_hash();
  RAISE EXCEPTION 'Runtime invoked private transfer helper' USING ERRCODE='ZX131';
 EXCEPTION WHEN insufficient_privilege THEN NULL;
 END;
END $$;
ROLLBACK;
SQL
    done
    while IFS= read -r drift; do
        printf 'Checking %s isolated drift: %s\n' "$phase" "$drift"
        { printf 'BEGIN;\n'; cat "$cluster/ready.sql"; printf '%s;\n' "$drift"; cat "$cluster/not-ready.sql"; printf 'ROLLBACK;\n'; cat "$cluster/ready.sql"; } | "${admin[@]}"
    done < "$cluster/drifts.txt"
    # Replace only the exact trigger WHEN expression, retaining its function,
    # name and timing. A mere disabled-trigger test would miss this drift.
    { printf 'BEGIN;\n'; cat "$cluster/ready.sql"; cat <<'SQL'
DO $$ DECLARE definition text; BEGIN
 SELECT pg_get_triggerdef(oid) INTO STRICT definition FROM pg_trigger
 WHERE tgrelid='content.posts'::regclass AND tgname='stamp_comment_format';
 IF position(' WHEN ' IN definition)=0 THEN RAISE EXCEPTION 'Missing gated trigger control'; END IF;
 definition:=regexp_replace(definition,' WHEN .* EXECUTE FUNCTION ',' WHEN (true) EXECUTE FUNCTION ');
 DROP TRIGGER stamp_comment_format ON content.posts;
 EXECUTE definition;
END $$;
SQL
      cat "$cluster/not-ready.sql"; printf 'ROLLBACK;\n'; cat "$cluster/ready.sql"; } | "${admin[@]}"
    # Even the offline migration role cannot rehydrate a tombstone or mutate
    # the new FP fields while canonical erasure and presentation guards stand.
    "${admin[@]}" <<'SQL'
BEGIN;
SELECT slug FROM content.boards ORDER BY slug FOR UPDATE;
SET LOCAL ROLE board_migrator;
DO $$ DECLARE command text; BEGIN
 FOREACH command IN ARRAY ARRAY[
  'UPDATE content.posts SET deleted=false WHERE id=13100003',
  'UPDATE content.posts SET content_erased=false WHERE id=13100003',
  'UPDATE content.posts SET id=13100999 WHERE id=13100003',
  'UPDATE content.posts SET saved_presentation_search=''restored'' WHERE id=13100003',
  'UPDATE content.posts SET saved_presentation=decode(''465030310000000000'',''hex''),saved_presentation_search='''' WHERE id=13100003',
  'UPDATE content.posts SET comment=''changed'' WHERE board=''xferdst'' AND saved_presentation IS NOT NULL'
 ] LOOP
  BEGIN
   EXECUTE command;
   RAISE EXCEPTION 'Immutable erased or frozen content was changed' USING ERRCODE='ZX131';
  EXCEPTION WHEN check_violation OR SQLSTATE 'P131I' THEN NULL;
  END;
 END LOOP;
END $$;
ROLLBACK;
SQL
    # Erasure of an actually transferred FP row must clear both additive fields;
    # rollback keeps a live frozen row in the archive for the restore trial.
    "${admin[@]}" <<'SQL'
BEGIN;
SELECT slug FROM content.boards ORDER BY slug FOR UPDATE;
SET LOCAL ROLE board_staff;
UPDATE content.threads SET deleted=true WHERE board='xferdst'
 AND id=(SELECT thread_id FROM content.posts WHERE board='xferdst' AND saved_presentation IS NOT NULL LIMIT 1);
RESET ROLE;
DO $$ BEGIN
 IF (SELECT count(*) FROM content.posts p JOIN post_secrets.thread_transfer_result_posts r ON r.destination_post_id=p.id
  WHERE p.deleted AND p.content_erased AND p.saved_presentation IS NULL AND p.saved_presentation_search IS NULL
  AND p.comment='' AND p.name='' AND p.subject='' AND p.comment_format=0)<>2
 THEN RAISE EXCEPTION 'Fresh erasure retained a transferred presentation'; END IF;
END $$;
ROLLBACK;
SQL
    "${admin[@]}" -f - < "$cluster/ready.sql"
    "${admin[@]}" -Atc "SELECT 'post_secrets.thread_transfer_catalog'::regclass::oid" > "$cluster/$phase.oid"
    "${admin[@]}" -At -f - < "$cluster/fingerprint.sql" > "$cluster/$phase.fingerprint"
done
if cmp -s "$cluster/live.oid" "$cluster/restored.oid"; then
    printf 'Restore did not change the catalog relation OID; qualification is inconclusive.\n' >&3
    exit 1
fi
cmp -s "$cluster/live.fingerprint" "$cluster/restored.fingerprint" || { printf 'Transfer administrator restore fingerprint mismatch.\n' >&3; exit 1; }
printf '0131 populated upgrade, additive FP preservation, narrow SQL handoff, private role split, isolated readiness drift rejection and changed-OID administrator restore passed.\n' >&3
