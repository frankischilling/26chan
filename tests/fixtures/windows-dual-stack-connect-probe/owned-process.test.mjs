// Static regression checks only. Native job accounting is tested by the .ps1.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
const read = name => readFileSync(new URL(name, import.meta.url), 'utf8');
const fixture = read('owned-process-fixture.cjs');
const runner = read('owned-process.test.ps1');
const helper = read('owned-process.cs');

test('Windows survivor opts out of libuv parent-exit termination', () => {
  assert.match(fixture, /detached:\s*true/);
  assert.match(fixture, /stdio:\s*'ignore'/);
  assert.match(fixture, /child\.unref\(\)/);
  assert.match(fixture, /process\.platform !== 'win32'/);
});

test('child runs and parent checks its exact acknowledgement before release', () => {
  assert.match(fixture, /publish\('child-ready\.json', \{ schema: 1, pid: process\.pid \}\)/);
  assert.match(fixture, /ready\.pid !== child\.pid/);
  assert.match(fixture, /publish\('parent-ready\.json', \{ schema: 1, child_ready: true, pid: child\.pid \}\)/);
  assert.match(fixture, /acknowledged && fs\.existsSync\('release-parent'\)/);
  assert.match(fixture, /fs\.renameSync/);
});

test('membership uses retained process handle and this exact owned job', () => {
  assert.match(helper, /IsProcessInJob\(candidate\.Handle, job, out member\)/);
  assert.doesNotMatch(helper, /IsProcessInJob\([^\n]*IntPtr\.Zero/);
  assert.match(runner, /\$null=\$descendant\.Handle/);
  const before = runner.indexOf('$child.ContainsProcess($descendant)');
  const release = runner.indexOf("New-Item -ItemType File -Path (Join-Path $directory 'release-parent')");
  const after = runner.lastIndexOf('$child.ContainsProcess($descendant)');
  assert.ok(before >= 0 && release > before && after > release);
  assert.match(runner, /\$child\.ActiveProcessCount -ne 2/);
  assert.match(runner, /\$child\.ActiveProcessCount -ne 1/);
});

test('post-root live descendant blocks completion and job kill must terminate it', () => {
  assert.match(runner, /\$descendant\.HasExited -or -not \$child\.ContainsProcess/);
  assert.match(runner, /\$child\.TreeExited -or \$child\.WaitForExit\(100\)/);
  assert.match(runner, /\$child\.Kill\(\)/);
  assert.match(runner, /\$descendant\.WaitForExit\(5000\)/);
  assert.match(runner, /\$descendant\.ExitCode -ne 1/);
  assert.doesNotMatch(runner, /Stop-Process|taskkill|\$descendant\.Kill/);
});

test('acknowledgement and survivor lifetime remain bounded', () => {
  assert.match(fixture, /Date\.now\(\) \+ 10000/);
  assert.match(fixture, /setTimeout\(\(\) => process\.exit\(0\), 30000\)/);
  assert.equal((runner.match(/ElapsedMilliseconds -lt 5000/g) || []).length, 2);
});

test('failed acknowledgement still reaches unconditional job and handle disposal', () => {
  assert.match(runner, /finally \{ \$child\.Dispose\(\) \}/);
  assert.match(runner, /finally \{\s+if \(\$null -ne \$descendant\) \{ \$descendant\.Dispose\(\) \}/);
  assert.match(helper, /JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE/);
});
