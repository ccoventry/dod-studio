// Each Capture Output row shows whether its folder is usable (#25), on the
// real app shell with the diagnose command mocked.
import { test, expect } from '@playwright/test';

test('each Capture Output row shows its own status and rechecks on a change', async ({ page }) => {
  await page.addInitScript(() => {
    window.__mockInvokeHandlers = {
      get_settings: () => ({ target_drives: ['E:/caps', 'Q:/gone', 'E:/new'] }),
      calculate_export_pool_space: () => 10 * 1024 ** 3,
      diagnose_capture_output_paths: (args) => args.paths.map((path) => ({
        path,
        status: path === 'E:/caps' ? 'ok' : 'not_found',
        usable: !path.startsWith('Q:'),
      })),
    };
  });
  await page.goto('/index.html');
  const chips = () => page.locator('#target-drive-list .output-row-status');
  await expect(chips()).toHaveText(['Found', 'Drive not found', 'Will be created']);
  // Adding a folder checks it too.
  await page.evaluate(() => { document.querySelector('#drive-path-input').value = 'Q:/other'; });
  await page.evaluate(() => document.querySelector('#add-drive-btn').click());
  await expect(chips()).toHaveText(['Found', 'Drive not found', 'Will be created', 'Drive not found']);
});
