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
  const before = runner.indexOf('$child.InspectOwnership($descendant)');
  const release = runner.indexOf("New-Item -ItemType File -Path (Join-Path $directory 'release-parent')");
  const after = runner.lastIndexOf('$child.InspectOwnership($descendant)');
  assert.ok(before >= 0 && release > before && after > release);
  assert.match(runner, /\$before\.ActiveProcesses -ne 2/);
  assert.match(runner, /\$after\.ActiveProcesses -ne 1/);
});

test('post-root live descendant blocks completion and job kill must terminate it', () => {
  assert.match(runner, /\$after\.DescendantExited -or -not \$after\.DescendantInJob/);
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


test('ownership snapshots precede both hard assertions and preserve exact membership', () => {
  assert.match(helper, /RootMembershipObserved = !rootExited/);
  assert.match(helper, /RootInJob = !rootExited && ContainsProcess\(process\)/);
  assert.match(helper, /DescendantInJob = ContainsProcess\(candidate\)/);
  assert.match(helper, /Marshal.SizeOf<Accounting>\(\) == 48/);
  assert.match(helper, /Marshal.OffsetOf<Accounting>\("ActiveProcesses"\).ToInt64\(\) == 40/);
  assert.ok(runner.indexOf("Write-OwnedState -Stage 'before-parent-release'") < runner.indexOf('if ($before.RootExited'));
  assert.ok(runner.indexOf("Write-OwnedState -Stage 'after-parent-exit'") < runner.indexOf('if (-not $after.RootExited'));
  assert.match(runner, /-not \$before.RootInJob/);
  assert.match(runner, /-not \$before.DescendantInJob/);
  assert.match(runner, /-not \$after.DescendantInJob/);
});

test('ownership diagnostics expose only fixed enums, booleans and job counts', () => {
  const diagnostic = runner.slice(runner.indexOf('function Write-OwnedState'), runner.indexOf('try {'));
  const keys = [...diagnostic.matchAll(/(?:^|[;\n])\s*([a-z_]+)=/g)].map(match => match[1]);
  assert.deepEqual(keys, ['type', 'schema', 'stage', 'root_exited', 'root_in_job', 'root_membership_observed', 'descendant_exited', 'descendant_in_job', 'accounting_layout_valid', 'active_processes', 'total_processes', 'terminated_processes']);
  assert.doesNotMatch(diagnostic, /\.(Id|Handle|Path|Message)|\$_/);
  assert.match(diagnostic, /ValidateSet\('before-parent-release','after-parent-exit'\)/);
});
