import { randomBytes } from 'node:crypto';
import { spawn, spawnSync } from 'node:child_process';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';

function databaseEnvironment() {
  let database;
  try {
    database = new URL(process.env.MIGRATION_DATABASE_URL);
    if (process.env.APP_ENV !== 'development' || process.env.VISUAL_FIXTURE_SERVER === '1'
      || !['postgres:', 'postgresql:'].includes(database.protocol) || database.hostname !== '127.0.0.1'
      || database.username !== 'board_migrator' || !/^\/[a-zA-Z0-9_]{1,63}$/.test(database.pathname)
      || database.search || database.hash) throw Error();
  } catch { throw new Error('Blotter fixtures require a loopback development migration database'); }
  return { PATH: process.env.PATH, SystemRoot: process.env.SystemRoot,
    PGHOST: database.hostname, PGPORT: database.port || '5432', PGDATABASE: database.pathname.slice(1),
    PGUSER: 'board_migrator', PGPASSWORD: decodeURIComponent(database.password), PGSSLMODE: 'disable', PGCONNECT_TIMEOUT: '5' };
}
function sql(env, variables, input) {
  const result = spawnSync('psql', ['-XqAt', '-v', 'ON_ERROR_STOP=1',
    ...Object.entries(variables).flatMap(([key, value]) => ['-v', `${key}=${value}`])],
  { env, input, encoding: 'utf8', timeout: 15000, maxBuffer: 262144 });
  if (result.error || result.status !== 0) throw new Error('Owned blotter fixture database command failed');
  return result.stdout.trim();
}
async function lock(env) {
  const child = spawn('psql', ['-XqAt', '-v', 'ON_ERROR_STOP=1'], { env, stdio: ['pipe', 'pipe', 'ignore'] });
  let failed = false, releasing = false, released;
  child.stdin.on('error', () => { failed = true; child.kill(); });
  child.on('error', () => { failed = true; });
  const exited = () => child.exitCode !== null || child.signalCode !== null;
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => { child.kill(); reject(Error('Blotter fixture lock timed out')); }, 15000);
    let output = '';
    child.once('error', () => { clearTimeout(timer); reject(Error('Blotter fixture lock failed')); });
    child.once('exit', () => { clearTimeout(timer); reject(Error('Blotter fixture lock exited')); });
    child.stdout.on('data', chunk => { output += chunk; if (output.includes('blotter-ready')) { clearTimeout(timer); resolve(); } });
    child.stdin.write("SET statement_timeout='10s'; SELECT pg_advisory_lock(2250118); SELECT 'blotter-ready';\n");
  });
  child.on('exit', () => { if (!releasing) failed = true; });
  return {
    check() { if (failed || exited()) throw Error('Blotter fixture lost its database lock'); },
    release() {
      if (released) return released;
      releasing = true;
      released = new Promise((resolve, reject) => {
        if (exited() || failed) { if (!exited()) child.kill(); reject(Error('Blotter fixture lock exited unexpectedly')); return; }
        const timer = setTimeout(() => { child.kill('SIGKILL'); reject(Error('Blotter fixture unlock timed out')); }, 5000);
        child.once('exit', code => { clearTimeout(timer); code === 0 && !failed ? resolve() : reject(Error('Blotter fixture unlock failed')); });
        child.stdin.end('SELECT pg_advisory_unlock(2250118);\n\\q\n');
      });
      return released;
    },
  };
}
export async function withOwnedBlotter(callback) {
  const env = databaseEnvironment(), lease = await lock(env);
  const marker = `OwnedBlotter${randomBytes(16).toString('hex')}`;
  const slug = `bt${randomBytes(4).toString('hex')}`, disabled = `bx${randomBytes(4).toString('hex')}`;
  const variables = { marker, slug, disabled };
  const errors = []; let result, directory;
  try {
    directory = await mkdtemp(path.join(tmpdir(), 'owned-blotter-'));
    lease.check();
    // Empty-state qualification needs an empty isolated table. Never hide or
    // remove operator data to manufacture that state.
    if (sql(env, {}, 'SELECT count(*) FROM blotter_private.messages;') !== '0') {
      throw Error('Blotter browser qualification requires an empty isolated development blotter');
    }
    sql(env, variables, `BEGIN; SET LOCAL statement_timeout='10s';
      INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,show_blotter) VALUES
        (:'slug',:'marker',:'marker',2000,300,250,100,10,true),(:'disabled',:'marker',:'marker',2000,300,250,100,10,false); COMMIT;`);
    let timestamp = Math.floor(Date.now() / 1000), sequence = 0;
    const publish = async text => {
      lease.check();
      const content = `${marker} ${++sequence} ${text}`;
      const file = path.join(directory, 'message.json');
      await writeFile(file, JSON.stringify({ version: 1, published_at: ++timestamp, content }), { mode: 0o600 });
      const executable = path.resolve(process.env.CARGO_TARGET_DIR || 'target', `debug/board-blotter${process.platform === 'win32' ? '.exe' : ''}`);
      const command = spawnSync(executable, ['publish', file], { encoding: 'utf8', timeout: 15000, maxBuffer: 32768,
        env: { PATH: process.env.PATH, SystemRoot: process.env.SystemRoot, MIGRATION_DATABASE_URL: process.env.MIGRATION_DATABASE_URL } });
      if (command.error || command.status !== 0) throw Error('Owned blotter publication failed; build board-store --bin board-blotter first');
      const id = command.stdout.match(/^Published and verified local announcement ([1-9][0-9]*).\s*$/)?.[1];
      if (!id) throw Error('Owned blotter publication receipt missing');
      return { id, content, timestamp };
    };
    result = await callback({ slug, disabled, marker, publish });
  } catch (error) { errors.push(error); }
  finally {
    try { sql(env, variables, `BEGIN; SET LOCAL statement_timeout='10s';
      DELETE FROM blotter_private.messages WHERE starts_with(content,:'marker' || ' ');
      DELETE FROM content.boards WHERE slug IN (:'slug',:'disabled') AND title=:'marker' AND description=:'marker'; COMMIT;`); }
    catch (error) { errors.push(error); }
    try { if (directory) await rm(directory, { recursive: true, force: true }); } catch (error) { errors.push(error); }
    try { await lease.release(); } catch (error) { errors.push(error); }
  }
  if (errors.length) throw new AggregateError(errors, 'Owned blotter fixture failed');
  return result;
}
