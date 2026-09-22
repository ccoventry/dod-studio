// Frontend e2e config — drives tests/e2e/render-studio.html (a minimal
// harness, not the real app shell) in headless Chromium, with Tauri's IPC
// mocked out (vite.config.e2e.js). See tests/e2e/README.md for what this
// covers and, more importantly, what it doesn't.
import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests/e2e',
  testMatch: '**/*.spec.js',
  fullyParallel: true,
  reporter: 'list',
  use: {
    baseURL: 'http://localhost:5183',
    trace: 'retain-on-failure',
  },
  webServer: {
    command: 'npm run dev:e2e',
    url: 'http://localhost:5183',
    reuseExistingServer: !process.env.CI,
  },
});
