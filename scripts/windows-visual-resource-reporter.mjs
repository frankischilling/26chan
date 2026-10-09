import { createFixtureLifecycleCollector, saveFixtureLifecycleEvidence } from './windows-fixture-lifecycle.mjs';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { fileURLToPath } from 'node:url';

const execute = promisify(execFile);
const script = fileURLToPath(new URL('./windows-visual-resources.ps1', import.meta.url));

// Capture fixed aggregate counters while the suite is active. Failure output
// excludes command errors, paths, endpoints and all browser/application data.
export default class WindowsVisualResourceReporter {
  constructor() {
    this.lifecycle = createFixtureLifecycleCollector();
    this.outputDir = null;
    this.completed = 0;
    this.pending = Promise.resolve();
  }

  onBegin(config) {
    if (config?.projects?.length === 1) this.outputDir = config.projects[0].outputDir;
    this.capture('suite-running');
  }

  onStdOut(chunk, test, result) { this.lifecycle.push(chunk, test, result); }

  onTestEnd(_test, result) {
    this.completed += 1;
    if (result.status === 'failed' || result.status === 'timedOut') {
      this.capture('test-failure');
    } else if (this.completed % 64 === 0) {
      this.capture('suite-running');
    }
  }

  capture(phase) {
    if (process.platform !== 'win32' || process.env.WINDOWS_VISUAL_RESOURCE_DIAGNOSTICS !== '1') return;
    this.pending = this.pending.then(async () => {
      try {
        const { stdout } = await execute('pwsh', ['-NoProfile', '-NonInteractive', '-File', script, '-Phase', phase], {
          timeout: 15_000, maxBuffer: 32_768, windowsHide: true,
        });
        const snapshot = JSON.parse(stdout);
        // The trusted PowerShell helper emits only its fixed aggregate schema.
        if (snapshot.phase !== phase || !/^\d{4}-\d{2}-\d{2}T/.test(snapshot.utc)) throw new Error();
        process.stdout.write(`${JSON.stringify(snapshot)}\n`);
      } catch {
        process.stdout.write(`Windows visual aggregates unavailable (${phase}).\n`);
      }
    });
  }

  async onEnd() {
    await this.pending;
    if (process.platform !== 'win32' || process.env.WINDOWS_VISUAL_RESOURCE_DIAGNOSTICS !== '1') return;
    try {
      // Playwright owns the output root and cleanup, just as for screenshots.
      if (!this.outputDir) throw new Error();
      await saveFixtureLifecycleEvidence(this.outputDir, this.lifecycle.finish());
    } catch { process.stdout.write('Synthetic fixture lifecycle evidence unavailable.\n'); }
  }
}
