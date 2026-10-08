import { expect } from '@playwright/test';
import { randomBytes } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import path from 'node:path';

export const ownedDeletionMarker = () => `OwnedBrowser${randomBytes(16).toString('hex')}`;

// Ownership is a marker written by this case, never text fetched from an
// arbitrary thread. The helper changes one post's age or removes that exact
// owned thread; imported board policy stays untouched.
export function deletionFixture(command, board, id, marker, field = 'subject') {
  const executable = path.resolve(process.env.CARGO_TARGET_DIR || 'target', `debug/examples/deletion-fixture${process.platform === 'win32' ? '.exe' : ''}`);
  const result = spawnSync(executable, [command, board, String(id), field, marker], {
    encoding: 'utf8', timeout: 15_000,
    env: { MIGRATION_DATABASE_URL: process.env.MIGRATION_DATABASE_URL, PATH: process.env.PATH, SystemRoot: process.env.SystemRoot },
  });
  expect(result.error, 'Owned deletion fixture helper must launch').toBeUndefined();
  expect(result.status, 'Owned deletion fixture helper must succeed').toBe(0);
}

export function cleanupDeletionFixtures(entries) {
  // Attempt every owned cleanup even if one fails, then report every failure.
  const failures = [];
  for (const { board, id, marker, field } of entries) {
    try { deletionFixture('cleanup', board, id, marker, field); } catch (error) { failures.push(error); }
  }
  if (failures.length) throw new AggregateError(failures, 'Owned deletion fixture cleanup failed');
}
