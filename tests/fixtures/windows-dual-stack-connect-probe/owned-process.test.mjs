// Static regression checks only. Native job accounting is tested by the .ps1.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
const read = name => readFileSync(new URL(name, import.meta.url), 'utf8');
const fixture = read('owned-process-fixture.cjs');
const runner = read('owned-process.test.ps1');
const helper = read('owned-process.cs');
const accounting = read('owned-process-accounting.ps1');

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
  assert.match(runner, /-not \$before\.RolesQualified\(\$true\)/);
  assert.match(runner, /-not \$after\.RolesQualified\(\$false\)/);
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
  assert.match(helper, /DescendantInJob = !descendantExited && ContainsProcess\(candidate\)/);
  assert.match(helper, /Marshal.SizeOf<Accounting>\(\) == 48/);
  assert.match(helper, /Marshal.OffsetOf<Accounting>\("ActiveProcesses"\).ToInt64\(\) == 40/);
  assert.ok(runner.indexOf("Write-OwnedState -Stage 'before-parent-release'") < runner.indexOf('if ($before.RootExited'));
  assert.ok(runner.indexOf("Write-OwnedState -Stage 'after-parent-exit'") < runner.indexOf('if (-not $after.RootExited'));
  assert.match(runner, /-not \$before.RootInJob/);
  assert.match(runner, /-not \$before.DescendantInJob/);
  assert.match(runner, /-not \$after.DescendantInJob/);
});

test('ownership diagnostics expose only fixed enums, booleans and job counts', () => {
  const start = runner.indexOf('function Write-OwnedState');
  const diagnostic = runner.slice(start, runner.indexOf('try {', start));
  const keys = [...diagnostic.matchAll(/(?:^|[;\n])\s*([a-z_]+)=/g)].map(match => match[1]);
  assert.deepEqual(keys, ['type', 'schema', 'stage', 'root_exited', 'root_in_job', 'root_membership_observed', 'root_listed', 'descendant_exited', 'descendant_in_job', 'accounting_layout_valid', 'active_processes', 'total_processes', 'terminated_processes', 'process_list_complete', 'process_counts_consistent', 'enumerated_processes', 'root_role_count', 'descendant_role_count', 'console_host_role_count', 'unknown_role_count', 'unavailable_role_count']);
  assert.doesNotMatch(diagnostic, /\.(Id|Handle|Path|Message)|\$_/);
  assert.match(diagnostic, /ValidateSet\('before-parent-release','after-parent-exit','after-job-termination'\)/);
});


test('exact job enumeration is fixed-capacity, complete and race-checked', () => {
  assert.match(helper, /const int capacity = 16/);
  assert.match(helper, /QueryJobProcessIds\(job, 3, buffer/);
  assert.match(helper, /assigned != listed \|\| listed > capacity/);
  assert.match(helper, /!seen.Add\(id\)/);
  assert.match(helper, /listed == snapshot.ActiveProcesses && after.ActiveProcesses == snapshot.ActiveProcesses/);
  assert.match(helper, /after.TotalProcesses == snapshot.TotalProcesses && after.TerminatedProcesses == snapshot.TerminatedProcesses/);
  assert.match(helper, /finally \{ Marshal.FreeHGlobal\(buffer\); \}/);
});

test('console-host classification requires a live exact-job member and full OS system image path', () => {
  assert.match(helper, /OpenProcess\(0x00101000, false/);
  assert.match(helper, /IsProcessInJob\(member, job, out belongs\)/);
  assert.match(helper, /WaitForSingleObject\(member, 0\) != 258/);
  assert.match(helper, /GetSystemDirectoryW\(system/);
  assert.match(helper, /QueryFullProcessImageNameW\(member, 0, image/);
  assert.match(helper, /String.Equals\(image, Path.Combine\(systemDirectory, "conhost.exe"\), StringComparison.OrdinalIgnoreCase\)/);
  assert.doesNotMatch(helper, /GetFileName|EndsWith|StartsWith/);
  assert.match(helper, /finally \{ Check\(CloseHandle\(member\)\); \}/);
});

test('role guards reject unknown or unavailable members and require all roles gone after kill', () => {
  assert.match(helper, /ProcessListComplete && ProcessCountsConsistent && EnumeratedProcesses <= 16/);
  assert.match(helper, /UnknownRoleCount == 0 && UnavailableRoleCount == 0/);
  assert.match(helper, /RootRoleCount == \(rootExpected \? 1u : 0u\) && DescendantRoleCount == 1/);
  assert.match(helper, /EnumeratedProcesses == RootRoleCount \+ DescendantRoleCount \+ ConsoleHostRoleCount/);
  assert.match(runner, /\$stopped.ActiveProcesses -ne 0 -or \$stopped.EnumeratedProcesses -ne 0/);
});

test('hosted classifier tests cover path impostors and incomplete synthetic role evidence', () => {
  for (const value of ['conhost.exe.evil', 'evilconhost.exe', 'sub\\conhost.exe', 'C:\\fake\\conhost.exe', '..\\System32\\conhost.exe']) assert.ok(runner.includes(value));
  for (const value of ['ProcessListComplete', 'ProcessCountsConsistent', 'UnknownRoleCount', 'UnavailableRoleCount', 'RootRoleCount', 'DescendantRoleCount', 'ConsoleHostRoleCount', 'ActiveProcesses', 'EnumeratedProcesses']) assert.ok(runner.includes(`'${value}'`));
  assert.match(runner, /Over-capacity role evidence admitted/);
});


test('native diagnostic explicitly preserves IPv4-only fixture and restores the process setting', () => {
  const launcher = read('run-hosted.ps1');
  const save = launcher.indexOf("$previousMediaProfile=[Environment]::GetEnvironmentVariable('VISUAL_FIXTURE_MEDIA_PROFILE','Process')");
  const set = launcher.indexOf("[Environment]::SetEnvironmentVariable('VISUAL_FIXTURE_MEDIA_PROFILE','ipv4-only','Process')");
  const spawn = launcher.indexOf("$fixture=[DualStackOwnedProcess]::Start($fixtureExe");
  const restore = launcher.indexOf("[Environment]::SetEnvironmentVariable('VISUAL_FIXTURE_MEDIA_PROFILE',$previousMediaProfile,'Process')");
  assert.ok(save >= 0 && set > save && spawn > set && restore > spawn);
  assert.match(launcher.slice(spawn, restore), /} finally {/);
  assert.match(launcher, /\$listeners.Count -ne 1 -or \$listeners\[0\].LocalAddress -ne '127\.0\.0\.1'/);
});

test('exited-root accounting waits only for its exact retained identity and keeps it unavailable', () => {
  assert.match(helper, /if \(id == process.Id\) \{\s+snapshot.RootListed = true;/);
  assert.match(helper, /if \(!snapshot.RootExited && snapshot.RootInJob\) snapshot.RootRoleCount\+\+;\s+else snapshot.UnavailableRoleCount\+\+;/);
  for (const guard of ['RootExited', 'RootListed', 'AccountingLayoutValid', 'DescendantInJob', 'ProcessListComplete', 'ProcessCountsConsistent']) assert.ok(accounting.includes(`$state.${guard}`));
  assert.match(accounting, /\$state.UnknownRoleCount -eq 0 -and \$state.UnavailableRoleCount -eq 1/);
  assert.match(accounting, /\$state.EnumeratedProcesses -le 16/);
  assert.match(accounting, /\$state.EnumeratedProcesses -eq \(1 \+ \$state.DescendantRoleCount \+ \$state.ConsoleHostRoleCount\)/);
  assert.doesNotMatch(accounting, /RolesQualified|\.Kill|Get-Process|Remove-Item/);
});

test('settlement shares the original five-second exit window and never replaces final assertions', () => {
  const release = runner.indexOf("New-Item -ItemType File -Path (Join-Path $directory 'release-parent')");
  const snapshot = runner.indexOf('$after=Wait-OwnedRootAccounting');
  assert.equal((runner.slice(release, snapshot).match(/\$watch.Restart\(\)/g) ?? []).length, 1);
  assert.match(runner, /\$after=Wait-OwnedRootAccounting -ReadSnapshot \{ \$child.InspectOwnership\(\$descendant\) \} -ElapsedMilliseconds \{ \$watch.ElapsedMilliseconds \}/);
  assert.match(accounting, /\(& \$ElapsedMilliseconds\) -lt 5000/);
  assert.match(accounting, /if \(\(& \$ElapsedMilliseconds\) -ge 5000\) \{ return \$state \}/);
  assert.ok(runner.indexOf('if (-not $child.HasExited -or $child.ExitCode -ne 0)', release) < snapshot);
  assert.ok(runner.indexOf('-not $after.RolesQualified($false)', snapshot) > snapshot);
});

test('hosted deterministic controls cover delayed accounting, deadline expiry and unrelated roles', () => {
  assert.match(runner, /\$sequence.Enqueue\(\$pending\); \$sequence.Enqueue\(\$pending\); \$sequence.Enqueue\(\$settled\)/);
  assert.match(runner, /\$clock.pauses -ne 2/);
  assert.match(runner, /\$clock.elapsed -ne 5000/);
  assert.match(runner, /\$expired.RolesQualified\(\$false\)/);
  assert.match(runner, /Unrelated or unverified role was retried/);
  assert.match(runner, /Pending root accounting was admitted/);
  assert.match(runner, /'RootExited','RootListed','AccountingLayoutValid','DescendantInJob','ProcessListComplete','ProcessCountsConsistent','UnknownRoleCount','UnavailableRoleCount'/);
});

test('the first snapshot is rejected before reading and after completion when the original deadline expires', () => {
  assert.match(accounting, /if \(\(& \$ElapsedMilliseconds\) -ge 5000\) \{ throw 'Root accounting observation deadline\.' \}\s+\$state = & \$ReadSnapshot\s+if \(\(& \$ElapsedMilliseconds\) -ge 5000\) \{ throw 'Root accounting observation deadline\.' \}/);
  assert.match(runner, /\$clock=@\{ elapsed=5000; reads=0 \}/);
  assert.match(runner, /\$clock.reads -ne 0/);
  assert.match(runner, /foreach \(\$finishedAt in @\(5000,5001\)\)/);
  assert.match(runner, /\$clock.elapsed=\$finishedAt; return \$settled/);
  assert.match(runner, /A late initial snapshot was admitted/);
});
