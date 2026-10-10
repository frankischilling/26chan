import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { release, arch } from 'node:os';
import { armName, binding, demand, sha256, runnerIdentity, PLAYWRIGHT_VERSION, BROWSER_VERSION,
  TEST_COUNT, featureLists } from './windows-port-contrast-core.mjs';
import { contrastDirectory, verifiedFixture, checkedFile } from '../tests/helpers/windows-port-contrast.js';

function identity(test) {
  return JSON.stringify([path.relative(process.cwd(), test.location.file).replaceAll('\\', '/'), test.titlePath()]);
}
export default class PortContrastReporter {
  constructor() { this.valid = true; this.expected = []; this.started = []; this.ended = new Set(); this.workers = new Set(); this.failed = 0; this.skipped = 0; this.retries = 0; }
  guard(fn) { try { fn(); } catch { this.valid = false; } }
  onBegin(config, suite) {
    this.guard(() => {
      demand(config.workers === 1 && config.fullyParallel === false && config.shard?.current === 7 && config.shard?.total === 8);
      demand(config.projects.length === 1 && config.projects[0].retries === 0 && config.projects[0].repeatEach === 1 && config.projects[0].timeout === 30000);
      demand(config.maxFailures === 0);
      const tests = suite.allTests();
      demand(tests.length === TEST_COUNT);
      demand(tests.every(test => test.expectedStatus === 'passed' && test.timeout === 30000 && !test.annotations.some(a => ['skip', 'fail', 'slow', 'fixme'].includes(a.type))));
      this.expected = tests.map(identity);
      demand(new Set(this.expected).size === TEST_COUNT);
    });
  }
  onTestBegin(test, result) {
    this.guard(() => { demand(this.started.length < TEST_COUNT && result.retry === 0); this.started.push(identity(test)); });
  }
  onTestEnd(test, result) {
    this.guard(() => {
      const id = identity(test);
      demand(!this.ended.has(id) && this.expected.includes(id));
      this.ended.add(id);
      demand(Number.isInteger(result.workerIndex) && result.workerIndex >= 0 && result.workerIndex < TEST_COUNT);
      this.workers.add(result.workerIndex);
      this.retries += result.retry;
      if (result.status === 'skipped') this.skipped++;
      else if (result.status !== 'passed') this.failed++;
      demand(['passed', 'failed', 'timedOut'].includes(result.status));
    });
  }
  onError() { this.valid = false; }
  onEnd(result) {
    this.guard(() => {
      const env = process.env;
      demand(this.valid && this.started.length === TEST_COUNT && this.ended.size === TEST_COUNT && this.skipped === 0 && this.retries === 0);
      demand(JSON.stringify(this.expected) === JSON.stringify(this.started));
      demand(result.status === (this.failed ? 'failed' : 'passed'));
      const arm = armName(env.WINDOWS_PORT_ARM), run = binding(env), directory = contrastDirectory(env);
      const expected = featureLists(arm);
      const browsers = [];
      for (const worker of this.workers) {
        const file = path.join(directory, `browser-${worker}.json`);
        checkedFile(file, 4096);
        const proof = JSON.parse(readFileSync(file, 'utf8'));
        demand(proof.schema === 1 && JSON.stringify(proof.binding) === JSON.stringify(run) && proof.arm === arm && proof.worker === worker);
        demand(proof.browser_sha256 === env.WINDOWS_PORT_BROWSER_SHA256 && proof.browser_version === BROWSER_VERSION);
        demand(proof.command_line_verified === true && proof.feature_enabled === (arm === 'enabled') && proof.feature_disabled === (arm === 'disabled'));
        demand(proof.enabled_count === expected.enabled.length && proof.disabled_count === expected.disabled.length);
        demand(proof.non_target_features_preserved === true && proof.field_trial_config_disabled === true && proof.socket_option_success === 'unobserved');
        browsers.push({ worker, command_line_verified: true, feature_enabled: proof.feature_enabled,
          feature_disabled: proof.feature_disabled, enabled_count: proof.enabled_count, disabled_count: proof.disabled_count,
          non_target_features_preserved: true, field_trial_config_disabled: true });
      }
      const { manifest } = verifiedFixture(env);
      const summary = { schema: 1, binding: run, arm, complete: true, fixture_verified: true,
        fixture_sha256: manifest.fixture_sha256, fixture_bytes: manifest.fixture_bytes,
        browser_sha256: env.WINDOWS_PORT_BROWSER_SHA256, playwright: PLAYWRIGHT_VERSION, browser_version: BROWSER_VERSION,
        runner: runnerIdentity(env, release(), arch(), process.version), discovery_verified: true,
        order_sha256: sha256(JSON.stringify(this.expected)), test_count: TEST_COUNT,
        executed: this.ended.size, failed: this.failed, skipped: this.skipped, retries: this.retries,
        browser_count: this.workers.size, proof_count: this.workers.size, browsers, all_command_lines_verified: true,
        feature_enabled: arm === 'enabled', feature_disabled: arm === 'disabled', non_target_features_preserved: true,
        socket_option_success: 'unobserved' };
      const body = JSON.stringify(summary);
      demand(Buffer.byteLength(body) <= 65536);
      writeFileSync(path.join(directory, 'evidence.json'), body, { flag: 'wx' });
    });
    if (!this.valid) process.stdout.write('Port contrast proof is incomplete; pair qualification must fail.\n');
  }
}
