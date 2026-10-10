import { randomBytes } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import assert from 'node:assert/strict';

function databaseEnvironment() {
  if (process.env.APP_ENV !== 'development' || process.env.VISUAL_FIXTURE_SERVER === '1') {
    throw new Error('Poll fixtures require the real development server');
  }
  let database;
  try {
    database = new URL(process.env.MIGRATION_DATABASE_URL);
    if (!['postgres:', 'postgresql:'].includes(database.protocol)
        || database.hostname !== '127.0.0.1' || database.username !== 'board_migrator'
        || !/^\/[a-zA-Z0-9_]{1,63}$/.test(database.pathname) || database.search || database.hash) throw new Error();
  } catch { throw new Error('Poll fixtures require a loopback migration database'); }
  return {
    PATH: process.env.PATH, SystemRoot: process.env.SystemRoot,
    PGHOST: database.hostname, PGPORT: database.port || '5432',
    PGDATABASE: database.pathname.slice(1), PGUSER: 'board_migrator',
    PGPASSWORD: decodeURIComponent(database.password), PGSSLMODE: 'disable', PGCONNECT_TIMEOUT: '5',
  };
}

function sql(env, variables, input) {
  const result = spawnSync('psql', ['-XqAt', '-v', 'ON_ERROR_STOP=1',
    ...Object.entries(variables).flatMap(([name, value]) => ['-v', `${name}=${value}`])], {
    input, env, encoding: 'utf8', timeout: 15_000, maxBuffer: 262_144,
  });
  // Database diagnostics can contain credentials or unrelated operator data.
  if (result.error || result.status !== 0) throw new Error('Owned poll fixture database command failed');
  return result.stdout.trim();
}

export async function withOwnedPolls(callback) {
  const env = databaseEnvironment();
  const marker = `OwnedPollBrowser${randomBytes(16).toString('hex')}`;
  const base = 10_000_000_000 + randomBytes(5).readUIntBE(0, 5) * 4;
  const fixture = {
    first: String(base + 1), second: String(base), hidden: String(base + 2),
    title: `${marker} <img src="/poll-browser-unexpected" onerror="window.pollInjected=1"> & ' " `.padEnd(512, 'T'),
    description: '<svg onload="window.pollInjected=1"> & description\n'.padEnd(16384, 'D'),
    captions: ['<script>window.pollInjected=1</script> & first '.padEnd(1024, 'C'), 'Owned second option', 'Owned missing score'],
  };
  const variables = { ...fixture, captions: undefined, marker, first_caption: fixture.captions[0] };
  delete variables.captions;
  const failures = [];
  let value;
  try {
    // Commit all rows together, using only free catalogue slots. The advisory
    // lock matches the HTTP/store poll fixtures while this transaction runs.
    const lowId = sql(env, variables, `BEGIN;
      SET LOCAL lock_timeout='5s'; SET LOCAL statement_timeout='10s';
      SELECT pg_advisory_xact_lock(2250109);
      CREATE TEMP TABLE owned_poll_slots ON COMMIT DROP AS
        SELECT n, row_number() OVER (ORDER BY n) AS position FROM generate_series(1,200) n
        WHERE NOT EXISTS (SELECT 1 FROM poll_private.polls WHERE catalogue_ordinal=n)
        ORDER BY n LIMIT 2;
      DO $$ BEGIN IF (SELECT count(*) FROM owned_poll_slots) <> 2 THEN
        RAISE EXCEPTION 'Two free poll catalogue slots required'; END IF; END $$;
      CREATE TEMP TABLE owned_poll_low_id ON COMMIT DROP AS
        SELECT n FROM generate_series(1,9) n
        WHERE NOT EXISTS (SELECT 1 FROM poll_private.polls WHERE id=n) ORDER BY n LIMIT 1;
      DO $$ BEGIN IF NOT EXISTS (SELECT 1 FROM owned_poll_low_id) THEN
        RAISE EXCEPTION 'One unused single-digit poll ID required'; END IF; END $$;
      INSERT INTO poll_private.polls(id,title,description,vote_count,published,catalogue_ordinal)
        VALUES(:'first'::bigint,:'title',:'description',6,true,(SELECT n FROM owned_poll_slots WHERE position=1)),
          (:'second'::bigint,:'marker' || ' second','',0,true,(SELECT n FROM owned_poll_slots WHERE position=2)),
          (:'hidden'::bigint,:'marker' || ' private','',0,false,NULL);
      INSERT INTO poll_private.options(poll_id,id,ordinal,caption,score) VALUES
        (:'first'::bigint,30,1,:'first_caption',2),
        (:'first'::bigint,20,2,'Owned second option',3),
        (:'first'::bigint,10,3,'Owned missing score',NULL);
      INSERT INTO poll_private.polls(id,title,description,vote_count,published)
        SELECT n,:'marker' || ' low-ID','',0,true FROM owned_poll_low_id;
      SELECT n FROM owned_poll_low_id;
      COMMIT;`);
    if (!/^[1-9]$/.test(lowId)) throw new Error('Owned single-digit poll fixture receipt is missing');
    fixture.lowId = lowId;
    value = await callback(fixture);
    const saved = JSON.parse(sql(env, variables, `SELECT json_build_object(
      'vote_count', vote_count, 'scores', (SELECT json_agg(score ORDER BY ordinal)
        FROM poll_private.options WHERE poll_id=:'first'::bigint))
      FROM poll_private.polls WHERE id=:'first'::bigint AND title=:'title';`));
    assert.deepEqual(saved, { vote_count: 6, scores: [2, 3, null] }, 'Browsing must preserve supplied poll results');
  } catch (error) { failures.push(error); }
  finally {
    // Cleanup also runs after an uncertain seed acknowledgement. Both the ID
    // and this run's random marker must match; existing rows remain untouched.
    try {
      sql(env, variables, `BEGIN;
        SET LOCAL lock_timeout='5s'; SET LOCAL statement_timeout='10s';
        SELECT pg_advisory_xact_lock(2250109);
        DELETE FROM poll_private.polls WHERE
          (id=:'first'::bigint AND title=:'title') OR
          (id=:'second'::bigint AND title=:'marker' || ' second') OR
          (id=:'hidden'::bigint AND title=:'marker' || ' private') OR
          (id BETWEEN 1 AND 9 AND title=:'marker' || ' low-ID');
        COMMIT;`);
    } catch (error) { failures.push(error); }
  }
  if (failures.length === 1) throw failures[0];
  if (failures.length > 1) throw new AggregateError(failures, 'Poll browser checks and fixture cleanup failed');
  return value;
}

export async function withOwnedVotingPoll(callback) {
  const env = databaseEnvironment(), marker = `OwnedPollVoteBrowser${randomBytes(16).toString('hex')}`;
  const poll = String(30_000_000_000 + randomBytes(5).readUIntBE(0, 5));
  const fixture = {
    poll,
    title: `${marker} <img src="/poll-vote-unexpected" onerror="window.pollInjected=1"> & '.`.padEnd(512, 'T'),
    description: 'Owned voting description <svg onload="window.pollInjected=1"> & text\n'.padEnd(16384, 'D'),
    captions: ['Owned <script>window.pollInjected=1</script> & first '.padEnd(1024, 'C'), 'Owned second choice'],
  };
  const variables = { poll, title: fixture.title, description: fixture.description, caption: fixture.captions[0] };
  const failures = [];
  let value;
  try {
    sql(env, variables, `BEGIN;
      SET LOCAL lock_timeout='5s'; SET LOCAL statement_timeout='10s';
      SELECT pg_advisory_xact_lock(2250109);
      INSERT INTO poll_private.polls(id,title,description,vote_count,published,accepting_votes)
        VALUES(:'poll'::bigint,:'title',:'description',6,true,true);
      INSERT INTO poll_private.options(poll_id,id,ordinal,caption,score)
        VALUES(:'poll'::bigint,41,1,:'caption',2),
              (:'poll'::bigint,17,2,'Owned second choice',4);
      COMMIT;`);
    fixture.snapshot = () => JSON.parse(sql(env, variables, `SELECT json_build_object(
      'votes',vote_count,'newVotes',new_vote_count,'open',accepting_votes,
      'receipts',(SELECT count(*) FROM poll_private.votes WHERE poll_id=:'poll'::bigint),
      'scores',(SELECT json_agg(score ORDER BY ordinal) FROM poll_private.options WHERE poll_id=:'poll'::bigint))
      FROM poll_private.polls WHERE id=:'poll'::bigint AND title=:'title';`));
    fixture.close = () => {
      const result = sql(env, variables, `UPDATE poll_private.polls SET accepting_votes=false
        WHERE id=:'poll'::bigint AND title=:'title' AND published AND accepting_votes
        RETURNING id=:'poll'::bigint;`);
      assert.equal(result, 't', 'Only the owned, open poll can be closed');
    };
    value = await callback(fixture);
    assert.deepEqual(fixture.snapshot(), { votes: 8, newVotes: 2, open: false, receipts: 2, scores: [3, 5] });
  } catch (error) { failures.push(error); }
  finally {
    try {
      sql(env, variables, `BEGIN;
        SET LOCAL lock_timeout='5s'; SET LOCAL statement_timeout='10s';
        SELECT pg_advisory_xact_lock(2250109);
        DELETE FROM poll_private.polls WHERE id=:'poll'::bigint AND title=:'title';
        COMMIT;`);
    } catch (error) { failures.push(error); }
  }
  if (failures.length === 1) throw failures[0];
  if (failures.length > 1) throw new AggregateError(failures, 'Poll voting checks and owned fixture cleanup failed');
  return value;
}
