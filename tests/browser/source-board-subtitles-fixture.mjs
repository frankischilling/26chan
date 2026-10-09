import { randomBytes } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import assert from 'node:assert/strict';

export function subtitleDatabaseEnvironment(environment = process.env) {
  let database;
  try {
    database = new URL(environment.MIGRATION_DATABASE_URL);
    if (environment.APP_ENV !== 'development' || environment.VISUAL_FIXTURE_SERVER === '1'
      || !['postgres:', 'postgresql:'].includes(database.protocol) || database.hostname !== '127.0.0.1'
      || database.username !== 'board_migrator' || !/^\/[a-zA-Z0-9_]{1,63}$/.test(database.pathname)
      || database.search || database.hash) throw Error();
  } catch { throw Error('Subtitle fixtures require a loopback development migration database'); }
  return { PATH: environment.PATH, SystemRoot: environment.SystemRoot,
    PGHOST: database.hostname, PGPORT: database.port || '5432', PGDATABASE: database.pathname.slice(1),
    PGUSER: 'board_migrator', PGPASSWORD: decodeURIComponent(database.password),
    PGSSLMODE: 'disable', PGCONNECT_TIMEOUT: '5' };
}
function sql(env, variables, input) {
  const result = spawnSync('psql', ['-XqAt', '-v', 'ON_ERROR_STOP=1',
    ...Object.entries(variables).flatMap(([key, value]) => ['-v', `${key}=${value}`])],
  { env, input, encoding: 'utf8', timeout: 15000, maxBuffer: 262144 });
  // Do not forward database diagnostics, credentials or unrelated records.
  if (result.error || result.status !== 0) throw Error('Owned subtitle fixture database command failed');
  return result.stdout.trim();
}
export function subtitlePaths(board) {
  return [
    ['index', `/${board.slug}/`], ['thread', `/${board.slug}/thread/${board.live}`],
    ['archived-thread', `/${board.slug}/thread/${board.archived}`],
    ['catalog', `/${board.slug}/catalog`], ['archive-index', `/${board.slug}/archive`],
  ];
}
export async function withOwnedSubtitles(callback) {
  const env = subtitleDatabaseEnvironment();
  const marker = `OwnedSubtitle${randomBytes(16).toString('hex')}`;
  const title = 'Owned <img src=x onerror=bad()>';
  const description = `${marker} <script>window.subtitleInjected=1</script> & description`;
  const boards = [
    { kind: 'none', worksafe: true, textOnly: false },
    { kind: 'fiction', worksafe: false, textOnly: false },
    { kind: 'worksafe_gif', worksafe: false, textOnly: false },
    { kind: 'fiction', worksafe: true, textOnly: true },
  ].map(row => ({ ...row, slug: `st${randomBytes(4).toString('hex')}`, title, description }));
  const variables = { marker, title, description,
    ...Object.fromEntries(boards.map((board, index) => [`slug${index}`, board.slug])) };
  const failures = []; let result;
  try {
    sql(env, variables, `BEGIN; SET LOCAL lock_timeout='5s'; SET LOCAL statement_timeout='10s';
      INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,archive_retention_seconds)
        VALUES(:'slug0',:'title',:'description',2000,300,250,100,10,259200);
      INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,archive_retention_seconds,board_subtitle,worksafe,text_only)
        VALUES(:'slug1',:'title',:'description',2000,300,250,100,10,259200,'fiction',false,false),
          (:'slug2',:'title',:'description',2000,300,250,100,10,259200,'worksafe_gif',false,false),
          (:'slug3',:'title',:'description',2000,300,250,100,10,259200,'fiction',true,true);
      WITH inserted AS (
        INSERT INTO content.threads(board,closed,archived_at,archive_expires_at)
        SELECT b.slug,a.archived,CASE WHEN a.archived THEN clock_timestamp()-interval '1 minute' END,
          CASE WHEN a.archived THEN clock_timestamp()+interval '1 hour' END
        FROM content.boards b CROSS JOIN (VALUES(false),(true)) a(archived)
        WHERE b.slug IN (:'slug0',:'slug1',:'slug2',:'slug3') AND b.title=:'title' AND b.description=:'description'
        RETURNING id,board
      ) INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
        SELECT id,board,id,'Anonymous','Owned subtitle thread',:'marker' FROM inserted;
      COMMIT;`);
    const persisted = JSON.parse(sql(env, variables, `SELECT json_agg(json_build_object(
      'slug',b.slug,'kind',b.board_subtitle,'worksafe',b.worksafe,'textOnly',b.text_only,
      'live', (SELECT id::text FROM content.threads WHERE board=b.slug AND archived_at IS NULL),
      'archived',(SELECT id::text FROM content.threads WHERE board=b.slug AND archived_at IS NOT NULL)) ORDER BY b.slug)
      FROM content.boards b WHERE b.slug IN (:'slug0',:'slug1',:'slug2',:'slug3') AND b.title=:'title' AND b.description=:'description';`));
    assert.equal(persisted.length, boards.length);
    for (const board of boards) {
      const saved = persisted.find(row => row.slug === board.slug);
      assert.deepEqual({ kind: saved.kind, worksafe: saved.worksafe, textOnly: saved.textOnly },
        { kind: board.kind, worksafe: board.worksafe, textOnly: board.textOnly });
      assert.match(saved.live, /^[1-9][0-9]*$/); assert.match(saved.archived, /^[1-9][0-9]*$/);
      Object.assign(board, saved);
    }
    result = await callback({ boards, marker });
  } catch (error) { failures.push(error); }
  finally {
    try {
      // Random slugs plus exact title/description prove ownership even if seed
      // acknowledgement was lost. Never rewrite or delete existing board data.
      sql(env, variables, `BEGIN; SET LOCAL lock_timeout='5s'; SET LOCAL statement_timeout='10s';
        CREATE TEMP TABLE owned_subtitle_boards ON COMMIT DROP AS
          SELECT slug FROM content.boards WHERE slug IN (:'slug0',:'slug1',:'slug2',:'slug3')
            AND title=:'title' AND description=:'description';
        DELETE FROM content.posts WHERE board IN (SELECT slug FROM owned_subtitle_boards);
        DELETE FROM content.threads WHERE board IN (SELECT slug FROM owned_subtitle_boards);
        DELETE FROM content.boards WHERE slug IN (SELECT slug FROM owned_subtitle_boards);
        COMMIT;`);
    } catch (error) { failures.push(error); }
  }
  if (failures.length === 1) throw failures[0];
  if (failures.length > 1) throw new AggregateError(failures, 'Subtitle page checks and fixture cleanup failed');
  return result;
}

export function canonicalSubtitleBoards() {
  const rows = JSON.parse(sql(subtitleDatabaseEnvironment(), {}, `SELECT json_agg(json_build_object(
    'slug',slug,'title',title,'description',description,'kind',board_subtitle,
    'worksafe',worksafe,'textOnly',text_only) ORDER BY slug)
    FROM content.boards WHERE slug IN ('b','trash','gif','g');`));
  assert.deepEqual(rows?.map(row => [row.slug, row.kind]),
    [['b', 'fiction'], ['g', 'none'], ['gif', 'worksafe_gif'], ['trash', 'fiction']],
    'The real source boards must have the migrated policy');
  return rows;
}
