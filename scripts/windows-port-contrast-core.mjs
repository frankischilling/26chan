// Fixed, diagnostic-only Chromium contrast. Never include rejected inputs in errors.
import { createHash } from 'node:crypto';

export const FEATURE = 'TcpPortRandomizationWin';
export const PLAYWRIGHT_VERSION = '1.62.0';
export const BROWSER_VERSION = '151.0.7922.34';
export const BROWSER_REVISION = '1234';
export const CORE_BUNDLE_SHA256 = '3258d1cf334c6afc95f22aa9c292436cb976b391e0437f1359c83b84f0cb9d66';
export const TEST_COUNT = 148;
export const DEFAULT_DISABLED = Object.freeze([
  'AvoidUnnecessaryBeforeUnloadCheckSync', 'BoundaryEventDispatchTracksNodeRemoval',
  'DestroyProfileOnBrowserClose', 'DialMediaRouteProvider', 'GlobalMediaControls',
  'HttpsUpgrades', 'LensOverlay', 'MediaRouter', 'PaintHolding',
  'ThirdPartyStoragePartitioning', 'BlockOriginHeaderModificationOnRedirect',
  'Translate', 'AutoDeElevate', 'OptimizationHints', 'msForceBrowserSignIn',
  'msEdgeUpdateLaunchServicesPreferredVersion',
]);
export const DEFAULT_ENABLED = Object.freeze(['CDPScreenshotNewSurface']);
export const sha256 = value => createHash('sha256').update(value).digest('hex');
export const demand = condition => { if (!condition) throw new Error('Port contrast evidence rejected'); };
const hash = value => typeof value === 'string' && /^[a-f0-9]{64}$/.test(value);
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);

export function armName(value) { demand(value === 'enabled' || value === 'disabled'); return value; }
export function contrastEnabled(env = process.env) {
  const value = env.WINDOWS_PORT_CONTRAST;
  if (value === undefined) return false;
  demand(value === '1');
  return true;
}
export function binding(env = process.env) {
  demand(/^[a-f0-9]{40}$/.test(env.CANDIDATE_SHA ?? ''));
  demand(/^[1-9][0-9]{0,19}$/.test(env.GITHUB_RUN_ID ?? ''));
  demand(env.GITHUB_RUN_ATTEMPT === '1');
  return { head: env.CANDIDATE_SHA, run: env.GITHUB_RUN_ID, attempt: 1 };
}
// Both are official Windows 2025 runner-image families. Preserve the exact
// value in evidence: this is an allowlist, never an alias/canonicalization.
export const RUNNER_IMAGE_FAMILIES = Object.freeze(['win25', 'win25-vs2026']);
export const RUNNER_DIAGNOSTIC_BYTES = 512;
const boundedValue = (value, maximum, pattern) => typeof value === 'string' &&
  value.length <= maximum && pattern.test(value) ? value : null;
export function runnerIdentityDiagnostic(env, release, arch, nodeVersion) {
  // Only these six already-retained identity fields can enter this record.
  // Unknown image names, missing values, paths and malformed strings become null.
  const identity = {
    image_os: RUNNER_IMAGE_FAMILIES.includes(env.ImageOS) ? env.ImageOS : null,
    image_version: boundedValue(env.ImageVersion, 22, /^[0-9]{8}\.[0-9]{1,6}\.[0-9]{1,6}$/),
    windows_build: boundedValue(env.WINDOWS_PORT_RUNNER_BUILD, 15, /^[0-9]{1,6}\.[0-9]{1,8}$/),
    release: boundedValue(release, 11, /^10\.0\.[0-9]{1,6}$/),
    arch: ['x64', 'arm64', 'ia32'].includes(arch) ? arch : null,
    node: boundedValue(nodeVersion, 12, /^v[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3}$/),
  };
  return { schema: 1, stage: 'runner', identity };
}
export function runnerIdentity(env, release, arch, nodeVersion, report = () => {}) {
  const record = runnerIdentityDiagnostic(env, release, arch, nodeVersion);
  const identity = record.identity;
  if (identity.image_os === null || identity.image_version === null ||
      identity.windows_build === null || !/^26100\.[0-9]{1,8}$/.test(identity.windows_build) ||
      identity.release !== '10.0.26100' || identity.arch !== 'x64' || identity.node !== 'v24.14.0') {
    const message = `Port contrast runner identity rejected: ${JSON.stringify(record)}`;
    demand(Buffer.byteLength(message) <= RUNNER_DIAGNOSTIC_BYTES);
    try { report(message); } catch {}
    throw new Error('Port contrast runner identity rejected');
  }
  return identity;
}
export function checkEnvironment(env, platform) {
  demand(platform === 'win32' && contrastEnabled(env));
  binding(env); armName(env.WINDOWS_PORT_ARM);
  demand(env.THEME_SHARD === '7' && env.WINDOWS_VISUAL_RESOURCE_DIAGNOSTICS === '1' && env.WINDOWS_VISUAL_NETLOG === '1');
  for (const key of Object.keys(env)) {
    const normalized = key.replace(/^npm_(?:config|package_config)_/i, '').toUpperCase();
    // Hosted Windows images set this inventory marker. The pinned Playwright
    // bundle never reads it; only Selenium REMOTE_* variables affect launch.
    // Keep every other Selenium/debug/launch override rejected.
    if (normalized === 'SELENIUM_JAR_PATH') continue;
    if (/^(?:PW_|PWDEBUG|PWTEST_|PLAYWRIGHT_|SELENIUM_)/.test(normalized) || ['NODE_OPTIONS', 'DEBUG', 'DEBUG_FILE', 'WINDOWS_BROWSER_CONTROL', 'WINDOWS_BROWSER_CONTROL_OWNED'].includes(normalized)) {
      demand(env[key] === undefined || env[key] === '');
    }
  }
}
const PREFLIGHT_STAGES = Object.freeze(['environment', 'binding', 'runner', 'fixture', 'browser', 'output']);
export function contrastPreflight(checks, report = () => {}) {
  const values = {};
  for (const stage of PREFLIGHT_STAGES) {
    try {
      demand(typeof checks?.[stage] === 'function');
      values[stage] = checks[stage]();
    } catch {
      // Emit one owned enum only. Never inspect or retain the exception, inputs,
      // environment values, executable/profile paths or browser arguments.
      try { report(`Port contrast preflight rejected: ${stage}.`); } catch {}
      throw new Error('Port contrast preflight unavailable');
    }
  }
  return values;
}

export function featureLists(arm) {
  armName(arm);
  return {
    enabled: [...DEFAULT_ENABLED, ...(arm === 'enabled' ? [FEATURE] : [])],
    disabled: [...DEFAULT_DISABLED, ...(arm === 'disabled' ? [FEATURE] : [])],
  };
}
export function contrastLaunchOptions(options, arm) {
  armName(arm);
  demand(options && typeof options === 'object' && Object.keys(options).length === 0);
  const kind = arm === 'enabled' ? 'enable' : 'disable';
  const original = arm === 'enabled' ? DEFAULT_ENABLED : DEFAULT_DISABLED;
  // Playwright filters the entire final argument list, including caller args.
  // Ignore only the changed ORIGINAL switch. The replacement must differ.
  return { ...options,
    ignoreDefaultArgs: [`--${kind}-features=${original.join(',')}`],
    args: [`--${kind}-features=${[...original, FEATURE].join(',')}`],
  };
}
export function verifyCommandLine(args, arm, version) {
  demand(version === BROWSER_VERSION && Array.isArray(args) && args.length > 10 && args.length <= 128);
  demand(args.every(arg => typeof arg === 'string' && Buffer.byteLength(arg) <= 8192));
  const lists = {};
  for (const kind of ['enable', 'disable']) {
    const prefix = `--${kind}-features=`;
    const switches = args.filter(arg => arg.startsWith(`--${kind}-features`));
    demand(switches.length === 1 && switches[0].startsWith(prefix));
    const features = switches[0].slice(prefix.length).split(',');
    demand(features.every(name => /^[A-Za-z][A-Za-z0-9]*$/.test(name)));
    demand(new Set(features).size === features.length);
    lists[`${kind}d`] = features.sort();
  }
  const expected = featureLists(arm);
  demand(same(lists.enabled, expected.enabled.sort()) && same(lists.disabled, expected.disabled.sort()));
  demand(args.includes('--disable-field-trial-config') && args.includes('--headless') && args.includes('--enable-automation'));
  demand(!args.some(arg => /^--(?:force-fieldtrials|force-fieldtrial-params|variations|enable-field-trial-config|proxy-server|proxy-pac-url|host-resolver-rules)(?:=|$)/.test(arg)));
  return { browser_version: version, command_line_verified: true,
    feature_enabled: arm === 'enabled', feature_disabled: arm === 'disabled',
    enabled_count: lists.enabled.length, disabled_count: lists.disabled.length,
    non_target_features_preserved: true, field_trial_config_disabled: true,
    socket_option_success: 'unobserved' };
}
export function validateManifest(manifest, identity) {
  demand(manifest && manifest.schema === 1 && same(manifest.binding, identity));
  demand(hash(manifest.fixture_sha256) && Number.isSafeInteger(manifest.fixture_bytes) && manifest.fixture_bytes > 0 && manifest.fixture_bytes <= 536870912);
  demand(manifest.rust === '1.94.0' && manifest.fixture === 'visual-fixtures.exe');
  return manifest;
}
export function qualifyPair(enabled, disabled, expectedBinding) {
  demand(expectedBinding?.attempt === 1 && /^[a-f0-9]{40}$/.test(expectedBinding.head ?? '') && /^[1-9][0-9]{0,19}$/.test(expectedBinding.run ?? ''));
  for (const [arm, evidence] of [['enabled', enabled], ['disabled', disabled]]) {
    demand(evidence?.schema === 1 && evidence.arm === arm && same(evidence.binding, expectedBinding));
    demand(evidence.complete === true && evidence.fixture_verified === true && evidence.discovery_verified === true);
    demand(hash(evidence.fixture_sha256) && hash(evidence.browser_sha256) && hash(evidence.order_sha256));
    demand(Number.isSafeInteger(evidence.fixture_bytes) && evidence.fixture_bytes > 0 && evidence.fixture_bytes <= 536870912);
    demand(evidence.playwright === PLAYWRIGHT_VERSION && evidence.browser_version === BROWSER_VERSION);
    demand(evidence.test_count === TEST_COUNT && evidence.executed === TEST_COUNT && evidence.skipped === 0 && evidence.retries === 0);
    demand(Number.isInteger(evidence.failed) && evidence.failed >= 0 && evidence.failed <= TEST_COUNT);
    demand(Number.isInteger(evidence.browser_count) && evidence.browser_count > 0 && evidence.browser_count <= TEST_COUNT);
    demand(evidence.proof_count === evidence.browser_count && evidence.all_command_lines_verified === true);
    demand(Array.isArray(evidence.browsers) && evidence.browsers.length === evidence.browser_count);
    demand(new Set(evidence.browsers.map(proof => proof.worker)).size === evidence.browser_count);
    const lists = featureLists(arm);
    for (const proof of evidence.browsers) {
      demand(Number.isInteger(proof.worker) && proof.worker >= 0 && proof.worker < TEST_COUNT);
      demand(proof.command_line_verified === true && proof.feature_enabled === (arm === 'enabled') && proof.feature_disabled === (arm === 'disabled'));
      demand(proof.enabled_count === lists.enabled.length && proof.disabled_count === lists.disabled.length);
      demand(proof.non_target_features_preserved === true && proof.field_trial_config_disabled === true);
    }
    demand(evidence.feature_enabled === (arm === 'enabled') && evidence.feature_disabled === (arm === 'disabled'));
    demand(evidence.non_target_features_preserved === true && evidence.socket_option_success === 'unobserved');
    demand(evidence.original?.kind === 'exit' && Number.isInteger(evidence.original.code) && evidence.original.code >= 0 && evidence.original.code <= 255);
    demand((evidence.original.code === 0) === (evidence.failed === 0));
    const log = evidence.netlog;
    demand(log?.schema === 1 && log.event_chains_preserved === true);
    demand(Number.isInteger(log.files) && log.files >= 0 && log.files <= 4);
    demand(Number.isInteger(log.command_line_redactions) && log.command_line_redactions >= 0 && log.command_line_redactions <= log.files);
    demand(log.status === (log.files ? 'complete' : 'absent') && (evidence.failed === 0 || log.files > 0));
    demand(Array.isArray(log.captures) && log.captures.length === log.files);
    demand(log.captures.filter(capture => capture.command_line_redacted === true).length === log.command_line_redactions);
    for (const [index, capture] of log.captures.entries()) {
      demand(capture.ordinal === index + 1 && Number.isSafeInteger(capture.bytes) && capture.bytes > 0 && capture.bytes <= 12582912 && hash(capture.sha256));
      demand(typeof capture.command_line_redacted === 'boolean');
    }
    demand(evidence.runner && typeof evidence.runner === 'object');
    const r = evidence.runner;
    const normalizedRunner = runnerIdentity({ ImageOS: r.image_os, ImageVersion: r.image_version, WINDOWS_PORT_RUNNER_BUILD: r.windows_build }, r.release, r.arch, r.node);
    demand(same(r, normalizedRunner));
  }
  for (const key of ['binding', 'runner', 'fixture_sha256', 'fixture_bytes', 'browser_sha256', 'order_sha256', 'test_count', 'playwright', 'browser_version']) {
    demand(same(enabled[key], disabled[key]));
  }
  return { schema: 1, binding: expectedBinding, qualified: true, enabled_exit: enabled.original.code,
    disabled_exit: disabled.original.code,
    interpretation: enabled.failed === 0 && disabled.failed === 0 ? 'both-passed-inconclusive' : 'matched-observation-only',
    production_fix_established: false, os_root_cause_established: false, socket_option_success: 'unobserved' };
}
