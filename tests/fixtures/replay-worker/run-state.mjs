// Opt-in only. Use existing locked dependencies and keep build output outside
// this checkout, including when invoked on hosted Windows/macOS/Linux runners.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { resolve, relative, isAbsolute, join } from 'node:path';
import { fileURLToPath } from 'node:url';
const root = fileURLToPath(new URL('../../../', import.meta.url));
const target = resolve(process.env.REPLAY_PROBE_CARGO_TARGET_DIR || join(tmpdir(), '26chan-replay-worker-probe-target'));
const inside = relative(root, target);
assert(inside.startsWith('..') || isAbsolute(inside), 'Probe build output must be outside the checkout');
const result = spawnSync('cargo', ['test', '--manifest-path',
  fileURLToPath(new URL('./rust-check/Cargo.toml', import.meta.url)),
  '--locked', '--offline', '--target-dir', target], { cwd: root, stdio: 'inherit' });
if (result.error) throw result.error;
process.exit(result.status ?? 1);
