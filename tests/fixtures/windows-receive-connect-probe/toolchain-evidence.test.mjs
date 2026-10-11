// Static contracts run everywhere. Runtime controls invoke the actual production
// PowerShell functions when pwsh is available; they do not run the hosted probe.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
const source = readFileSync(new URL('./run-hosted.ps1', import.meta.url), 'utf8');
const observer = source.slice(source.indexOf('function Save-EnvironmentObservation'), source.indexOf('function Export-FixtureEvidence'));

test('environment evidence is a fixed schema without raw text or environment', () => {
  const schema = source.match(/environment_evidence=\[ordered\]@\{ ([^}]+) \}/)?.[1];
  assert.ok(schema);
  assert.deepEqual([...schema.matchAll(/(?:^|; )([a-z_]+)=/g)].map(m => m[1]), ['checkpoint', 'wait_result', 'root_exited', 'root_exit', 'active_processes', 'observation_failed']);
  assert.doesNotMatch(observer, /\$_|\.Message|\.Path|\.Id\b|\.Handle\b|Get-Content|GetEnvironmentVariable|Write-Output|Set-Content|Save-Status/);
  assert.match(observer, /root_exited=\[bool\]\$Child\.HasExited/);
  assert.match(observer, /root_exit=\[int\]\$Child\.ExitCode/);
  assert.match(observer, /active_processes=\[uint32\]\$Child\.ActiveProcessCount/);
  assert.deepEqual([...source.matchAll(/environment_evidence\.checkpoint='([^']+)'/g)].map(m => m[1]), ['wait', 'exit-check', 'launch', 'read-export', 'apply-export', 'resolve-tools', 'complete']);
});

test('environment observation retains the original wait, rejection and cleanup contracts', () => {
  assert.equal((observer.match(/\.WaitForExit\(20000\)/g) || []).length, 1);
  assert.match(observer, /if \(-not \$finished\) \{ throw 'MSVC environment failed\.' \}/);
  assert.match(observer, /if \(\$Child\.ExitCode -ne 0\) \{ throw 'MSVC environment failed\.' \}/);
  assert.match(observer, /finally \{ Save-EnvironmentObservation -Child \$Child \}/);
  assert.doesNotMatch(observer, /\.Kill\(|\.Dispose\(|failure_class\s*=|qualified\s*=|forced_cleanup\s*=|Start-Sleep|while\s*\(/);
  assert.ok(source.indexOf('Wait-ToolchainEnvironment -Child $environment') < source.indexOf('$child.Kill($true)'));
  assert.match(source, /\$status\.failure_class=\$status\.phase \+ '-error'/);
  assert.match(source, /-not \$status\.forced_cleanup -and \$null -eq \$status\.failure_class/);
});

const probe = spawnSync('pwsh', ['-NoLogo', '-NoProfile', '-Command', 'exit 0'], { encoding: 'utf8', timeout: 15000 });
const unavailable = process.platform !== 'win32' && probe.error?.code === 'ENOENT';
test('actual PowerShell evidence controls cover deadlines, survivors, errors and success', { skip: unavailable ? 'pwsh unavailable; static contracts only on this host' : false }, () => {
  assert.ifError(probe.error);
  assert.equal(probe.status, 0, probe.stderr);
  const result = spawnSync('pwsh', ['-NoLogo', '-NoProfile', '-File', fileURLToPath(new URL('./toolchain-evidence.test.ps1', import.meta.url))], { encoding: 'utf8', timeout: 15000 });
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /Toolchain evidence controls passed \(7\)\./);
});
