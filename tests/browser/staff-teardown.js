import teardownDeletionQuota from './helpers/deletion-quota-teardown.js';
import teardownStaffIntake from './staff-intake-teardown.js';

export default async function teardown(config) {
  const failures = [];
  // Each cleanup owns a different resource. A failure must not skip the other.
  for (const cleanup of [teardownDeletionQuota, teardownStaffIntake]) {
    try { await cleanup(config); } catch (error) { failures.push(error); }
  }
  if (failures.length === 1) throw failures[0];
  if (failures.length > 1) throw new AggregateError(failures, 'Staff quota and intake teardown both failed');
}
