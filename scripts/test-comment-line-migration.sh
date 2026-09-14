#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root against the disposable development cluster.' >&2; exit 1; }
source .local/database.env
cluster=$(cat .local/cluster-path)
[[ $cluster =~ ^/tmp/board-postgres\.[[:alnum:]]+$ && -d $cluster ]] || exit 1
pg_bin=/usr/lib/postgresql/16/bin
admin=(runuser -u postgres -- "$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h /tmp -p 55432 -d postgres)
actual=$("${admin[@]}" -At -c 'SHOW data_directory')
[[ $actual = "$cluster" ]] || { echo 'Port 55432 belongs to a different cluster.' >&2; exit 1; }
upgrade_db="imageboard_lines_$(date +%s)_$RANDOM"
[[ $upgrade_db =~ ^imageboard_lines_[0-9]+_[0-9]+$ ]] || exit 1
created=0
cleanup() {
  if [[ $created = 1 ]]; then
    "${admin[@]}" -v upgrade_db="$upgrade_db" <<'SQL'
DROP DATABASE :"upgrade_db";
SQL
    created=0
  fi
}
trap cleanup EXIT
"${admin[@]}" -v upgrade_db="$upgrade_db" <<'SQL'
CREATE DATABASE :"upgrade_db" OWNER board_migrator TEMPLATE template0 ENCODING 'UTF8';
REVOKE ALL ON DATABASE :"upgrade_db" FROM PUBLIC;
GRANT CONNECT ON DATABASE :"upgrade_db" TO board_migrator,board_public;
SQL
created=1
upgrade_url="${MIGRATION_DATABASE_URL%/imageboard}/$upgrade_db"
db=("$pg_bin/psql" "$upgrade_url" -Xq -v ON_ERROR_STOP=1)
for migration in migrations/*.sql; do
  [[ $migration != migrations/0028_comment_line_rules.sql ]] || break
  "${db[@]}" --single-transaction -f "$migration"
done
"${db[@]}" <<'SQL'
-- Fixed active source defaults, including CATEGORY inheritance; no source runtime required.
CREATE TABLE public.owned_line_defaults(slug text PRIMARY KEY, max_lines integer, spoilers boolean);
INSERT INTO public.owned_line_defaults VALUES
('3',100,false),
('a',100,true),
('aco',70,false),
('adv',100,false),
('an',100,false),
('asp',100,false),
('b',50,false),
('bant',50,false),
('biz',100,false),
('c',100,false),
('cgl',100,false),
('ck',100,false),
('cm',100,false),
('co',100,true),
('d',70,false),
('diy',100,false),
('e',70,false),
('f',70,false),
('fa',100,false),
('fit',100,false),
('g',100,false),
('gd',100,false),
('gif',70,false),
('h',70,false),
('hc',70,false),
('his',100,false),
('hm',70,false),
('hr',70,false),
('i',70,false),
('ic',70,false),
('int',100,false),
('j',70,false),
('jp',100,true),
('k',100,false),
('lgbt',100,false),
('lit',100,true),
('m',100,true),
('mlp',100,true),
('mu',100,false),
('n',100,false),
('news',100,true),
('o',100,false),
('out',100,false),
('p',100,false),
('po',100,false),
('pol',70,false),
('pw',100,false),
('qa',100,false),
('qb',70,false),
('qst',100,true),
('r',70,false),
('r9k',70,true),
('s',70,false),
('s4s',70,true),
('sci',100,false),
('soc',70,false),
('sp',100,false),
('t',70,false),
('test',100,true),
('tg',100,true),
('toy',100,false),
('trash',70,false),
('trv',100,false),
('tv',100,true),
('u',70,true),
('v',100,true),
('vg',100,true),
('vip',100,true),
('vm',100,true),
('vmg',100,true),
('vp',100,true),
('vr',100,true),
('vrpg',100,true),
('vst',100,true),
('vt',100,true),
('w',100,false),
('wg',70,false),
('wsg',100,false),
('wsr',100,false),
('x',100,false),
('xs',100,false),
('y',70,false);
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
SELECT slug,'Line upgrade','Owned fixture',1000,100,100,10,10 FROM public.owned_line_defaults;
INSERT INTO content.threads(id,board,created_at,modified_at)
VALUES(7601,'test','2026-01-01Z','2026-01-02Z');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(7601,'test',7601,'Anonymous','Owned historical',E'a[spoiler]b[/spoiler]c\r\n\r\n\r\n\r\n','2026-01-01Z');
CREATE TABLE public.owned_thread_before AS SELECT * FROM content.threads WHERE id=7601;
SQL
"${db[@]}" --single-transaction -f migrations/0028_comment_line_rules.sql
"${db[@]}" <<'SQL'
DO $$ BEGIN
  IF (SELECT count(*) FROM content.boards) <> 82
    OR EXISTS(SELECT 1 FROM content.boards b JOIN public.owned_line_defaults e USING(slug)
      WHERE b.comment_max_lines IS DISTINCT FROM e.max_lines OR b.comment_spoiler_cleanup IS DISTINCT FROM e.spoilers)
    OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=7601 AND comment=E'a[spoiler]b[/spoiler]c\r\n\r\n\r\n\r\n' AND created_at='2026-01-01Z')
    OR EXISTS(SELECT * FROM content.threads WHERE id=7601 EXCEPT SELECT * FROM public.owned_thread_before)
  THEN RAISE EXCEPTION 'Historical content/clocks or active source line defaults changed'; END IF;
END $$;
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,worksafe)
VALUES('newws','New fixture','Owned fixture',1000,100,100,10,10,true),
      ('newnws','New fixture','Owned fixture',1000,100,100,10,10,false);
DO $$ BEGIN
  IF EXISTS(SELECT 1 FROM content.boards WHERE slug IN ('newws','newnws') AND (comment_max_lines <> 70 OR comment_spoiler_cleanup))
  THEN RAISE EXCEPTION 'Future boards inferred source category from worksafe'; END IF;
  BEGIN
    UPDATE content.boards SET comment_max_lines=-1 WHERE slug='newws';
    RAISE EXCEPTION 'Negative line limit admitted' USING ERRCODE='ZX001';
  EXCEPTION WHEN check_violation THEN NULL; END;
  BEGIN
    UPDATE content.boards SET comment_max_lines=16001 WHERE slug='newws';
    RAISE EXCEPTION 'Unbounded line limit admitted' USING ERRCODE='ZX001';
  EXCEPTION WHEN check_violation THEN NULL; END;
END $$;
UPDATE content.boards SET comment_max_lines=0,comment_spoiler_cleanup=true WHERE slug='newws';
UPDATE content.boards SET comment_max_lines=16000 WHERE slug='newnws';
SQL
public_url="${TEST_PUBLIC_DATABASE_URL%/imageboard}/$upgrade_db"
"$pg_bin/psql" "$public_url" -Xq -v ON_ERROR_STOP=1 <<'SQL'
DO $$ BEGIN
  IF current_user <> 'board_public' OR NOT EXISTS(SELECT 1 FROM content.boards WHERE slug='newws' AND comment_max_lines=0 AND comment_spoiler_cleanup)
  THEN RAISE EXCEPTION 'Public login cannot read configured policy'; END IF;
  BEGIN
    UPDATE content.boards SET comment_max_lines=100,comment_spoiler_cleanup=false WHERE slug='newws';
    RAISE EXCEPTION 'Public changed source line policy' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  BEGIN
    UPDATE content.posts SET comment='rewritten' WHERE id=7601;
    RAISE EXCEPTION 'Public rewrote historical comments' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SQL
cleanup
echo 'Comment line upgrade passed: all 82 source defaults, historical text/clocks, future defaults, bounded operator policy and actual public read-only privileges. Disposable database removed.'
