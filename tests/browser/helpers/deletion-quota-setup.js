import { initializeDeletionQuotaRun } from './deletion-quota-fixture.js';
export default function setup(config) {
  if (config.workers !== 1 || config.fullyParallel || config.projects.some(project => project.retries !== 0 || project.fullyParallel)) {
    throw new Error('Owned deletion quota browser runs require one serial worker and zero retries');
  }
  initializeDeletionQuotaRun();
}

