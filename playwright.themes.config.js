import { defineConfig } from '@playwright/test';
import base from './playwright.archive-visual.config.js';
import { contrastEnabled } from './scripts/windows-port-contrast-core.mjs';
import { contrastServerCommand } from './tests/helpers/windows-port-contrast.js';

export default defineConfig({
  ...base,
  testDir: './tests/themes',
  ...(contrastEnabled() ? { webServer: { ...base.webServer, command: contrastServerCommand(base.webServer.command) } } : {}),
  ...(process.platform === 'win32' && process.env.WINDOWS_VISUAL_RESOURCE_DIAGNOSTICS === '1'
    ? { reporter: [[process.env.CI ? 'dot' : 'list'], ['./scripts/windows-visual-resource-reporter.mjs'],
      ...(contrastEnabled() ? [['./scripts/windows-port-contrast-reporter.mjs']] : [])] }
    : {}),
});
