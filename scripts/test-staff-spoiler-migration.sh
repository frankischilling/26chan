#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-staff-spoilers.XXXXXXXX)
[[ $cluster =~ ^/tmp/board-staff-spoilers\.[[:alnum:]]+$ && -d $cluster && ! -L $cluster ]] || exit 1
started=0
cleanup() {
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null
    fi
    [[ $cluster =~ ^/tmp/board-staff-spoilers\.[[:alnum:]]+$ && -d $cluster && ! -L $cluster ]] || exit 1
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
CREATE DATABASE staff_spoiler_upgrade OWNER board_migrator;
REVOKE ALL ON DATABASE staff_spoiler_upgrade FROM PUBLIC;
GRANT CONNECT ON DATABASE staff_spoiler_upgrade TO board_migrator,board_public,board_staff;
-- Runtime logins exist only in this private trust-authenticated fixture.
ALTER ROLE board_staff LOGIN;
SQL
migrator=("${psql[@]}" -U board_migrator -d staff_spoiler_upgrade)
for migration in migrations/*.sql; do
    [[ $migration != migrations/0078_staff_image_spoilers.sql ]] || break
    "${migrator[@]}" --single-transaction -f "$migration"
done
"${migrator[@]}" <<'SQL'
INSERT INTO content.boards(slug,title,description,comment_spoiler_cleanup,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('oldspoil','Owned spoiler upgrade','Synthetic',true,1000,100,100,100,10);
INSERT INTO content.threads(id,board,created_at,modified_at) VALUES(8807801,'oldspoil','2026-01-01Z','2026-01-02Z');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(8807801,'oldspoil',8807801,'Owned','SPOILER<>User literal','Owned body','2026-01-01Z'),
    (8807802,'oldspoil',8807801,'Owned','Owned spoiler','Owned reply','2026-01-01Z'),
    (8807803,'oldspoil',8807801,'Owned','Owned text','Owned text reply','2026-01-01Z');
INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES(8807801,'owned-upgrade-hash');
INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at)
VALUES(repeat('a',32),repeat('a',32),repeat('a',32),repeat('a',64),100,500,300,'approved',clock_timestamp()),
    (repeat('b',32),repeat('b',32),repeat('b',32),repeat('b',64),100,500,300,'approved',clock_timestamp());
INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler,file_deleted)
VALUES(8807801,repeat('a',32),repeat('a',32),'owned.png',100,500,300,false,false),
    (8807802,repeat('b',32),repeat('b',32),'owned-spoiler.png',100,500,300,true,true);
CREATE TABLE public.owned_spoiler_posts_before AS SELECT to_jsonb(p) AS value FROM content.posts p;
CREATE TABLE public.owned_spoiler_boards_before AS SELECT to_jsonb(b) AS value FROM content.boards b;
CREATE TABLE public.owned_spoiler_threads_before AS SELECT to_jsonb(t) AS value FROM content.threads t;
CREATE TABLE public.owned_spoiler_media_before AS SELECT to_jsonb(m) AS value FROM content.post_media m;
CREATE TABLE public.owned_spoiler_secrets_before AS SELECT to_jsonb(d) AS value FROM post_secrets.deletion d;
CREATE TABLE public.owned_spoiler_policies_before AS SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy;
CREATE TABLE public.owned_spoiler_functions_before AS SELECT oid,proowner,proacl::text AS acl,prosecdef,proconfig,prosrc FROM pg_proc
    WHERE oid IN ('content.insert_post_attachment(bigint,text,bigint,text,text,text,text,text,boolean,timestamptz)'::regprocedure,
        'content.consume_staff_post_authority(bytea,bigint,text,bigint,text,text,text,timestamptz)'::regprocedure,
        'content.delete_post_attachment(text,bigint)'::regprocedure);
SQL
"${migrator[@]}" --single-transaction -f migrations/0078_staff_image_spoilers.sql
"${migrator[@]}" <<'SQL'
DO $$ BEGIN
  IF EXISTS(SELECT to_jsonb(p)-'image_spoiler' FROM content.posts p EXCEPT SELECT value FROM public.owned_spoiler_posts_before)
     OR EXISTS(SELECT value FROM public.owned_spoiler_posts_before EXCEPT SELECT to_jsonb(p)-'image_spoiler' FROM content.posts p)
     OR EXISTS(SELECT to_jsonb(b) FROM content.boards b EXCEPT SELECT value FROM public.owned_spoiler_boards_before)
     OR EXISTS(SELECT value FROM public.owned_spoiler_boards_before EXCEPT SELECT to_jsonb(b) FROM content.boards b)
     OR EXISTS(SELECT to_jsonb(t) FROM content.threads t EXCEPT SELECT value FROM public.owned_spoiler_threads_before)
     OR EXISTS(SELECT value FROM public.owned_spoiler_threads_before EXCEPT SELECT to_jsonb(t) FROM content.threads t)
     OR EXISTS(SELECT to_jsonb(m) FROM content.post_media m EXCEPT SELECT value FROM public.owned_spoiler_media_before)
     OR EXISTS(SELECT value FROM public.owned_spoiler_media_before EXCEPT SELECT to_jsonb(m) FROM content.post_media m)
     OR EXISTS(SELECT to_jsonb(d) FROM post_secrets.deletion d EXCEPT SELECT value FROM public.owned_spoiler_secrets_before)
     OR EXISTS(SELECT value FROM public.owned_spoiler_secrets_before EXCEPT SELECT to_jsonb(d) FROM post_secrets.deletion d)
     OR EXISTS(SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy EXCEPT SELECT * FROM public.owned_spoiler_policies_before)
     OR EXISTS(SELECT * FROM public.owned_spoiler_policies_before EXCEPT SELECT oid,polrelid,polname,polroles,polqual::text,polwithcheck::text FROM pg_policy)
     OR EXISTS(SELECT oid,proowner,proacl::text,prosecdef,proconfig,prosrc FROM pg_proc WHERE oid IN (SELECT oid FROM public.owned_spoiler_functions_before) EXCEPT SELECT * FROM public.owned_spoiler_functions_before)
     OR EXISTS(SELECT * FROM public.owned_spoiler_functions_before EXCEPT SELECT oid,proowner,proacl::text,prosecdef,proconfig,prosrc FROM pg_proc WHERE oid IN (SELECT oid FROM public.owned_spoiler_functions_before))
     OR EXISTS(SELECT 1 FROM content.posts p LEFT JOIN content.post_media m ON m.post_id=p.id WHERE p.image_spoiler IS DISTINCT FROM coalesce(m.spoiler,false)) THEN
    RAISE EXCEPTION 'Spoiler upgrade changed historical values, privacy or existing functions';
  END IF;
END $$;
SQL
"${psql[@]}" -U board_public -d staff_spoiler_upgrade <<'SQL'
DO $$ BEGIN
  BEGIN PERFORM content.set_post_image_spoiler('oldspoil',8807801,true);
    RAISE EXCEPTION 'Public invoked staff spoiler action' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  BEGIN UPDATE content.posts SET image_spoiler=true WHERE false;
    RAISE EXCEPTION 'Public wrote spoiler metadata' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SQL
"${psql[@]}" -U board_staff -d staff_spoiler_upgrade <<'SQL'
DO $$ BEGIN
  IF NOT content.set_post_image_spoiler('oldspoil',8807801,true)
     OR content.set_post_image_spoiler('oldspoil',8807801,true)
     OR NOT content.set_post_image_spoiler('oldspoil',8807802,false)
     OR NOT content.set_post_image_spoiler('oldspoil',8807803,true)
     OR (SELECT subject FROM content.posts WHERE id=8807801)<>'SPOILER<>User literal'
     OR EXISTS(SELECT 1 FROM content.visible_post_media WHERE post_id=8807802 AND NOT file_deleted) THEN
    RAISE EXCEPTION 'Scoped spoiler action changed subject, repeated work or restored a file';
  END IF;
END $$;
SQL
cleanup
trap - EXIT
printf 'Staff spoiler populated upgrade passed: historical values, deleted files, existing functions and privacy preserved; scoped actions and unchanged requests qualified. Private cluster removed.\n'
