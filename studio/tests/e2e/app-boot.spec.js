// Boots the real app shell (index.html + main.js) against the mocked IPC and
// checks the wiring main.js does at startup: settings land in the modules that
// own them, and the app-wide listeners are in place. Every other spec drives
// one pane in its own harness, so this is the one that notices a main.js
// wiring slip (#683).
import { test, expect } from '@playwright/test';

const GIB = 1024 ** 3;

test.beforeEach(async ({ page }) => {
  await page.addInitScript((gib) => {
    window.__mockInvokeHandlers = {
      get_settings: () => ({
        pinned_folders: ['D:/demos'],
        target_drives: ['E:/caps'],
        render_export_dirs: ['F:/clips'],
        analyzer_explorer_width: 300,
      }),
      calculate_export_pool_space: () => 10 * gib,
      system_memory_bytes: () => 16 * gib,
    };
  }, GIB);
});

test('boots and wires settings into the folder pools', async ({ page }) => {
  const errors = [];
  page.on('pageerror', (err) => errors.push(err.message));
  await page.goto('/index.html');

  await expect(page.locator('#export-pool-free-indicator')).toContainText('10.0');
  await expect(page.locator('#config-scan-workers-hint')).toContainText('16');
  const values = (sel) => page.locator(`${sel} input`).evaluateAll((els) => els.map((e) => e.value));
  await expect.poll(() => values('#target-drive-list')).toEqual(['E:/caps']);
  await expect.poll(() => values('#render-export-dir-list')).toEqual(['F:/clips']);

  // A typed folder is added on Add and saved with the pinned folder kept.
  await page.evaluate(() => { document.querySelector('#drive-path-input').value = 'G:/more'; });
  await page.evaluate(() => document.querySelector('#add-drive-btn').click());
  await expect.poll(() => values('#target-drive-list')).toEqual(['E:/caps', 'G:/more']);
  const saved = await page.evaluate(() => window.__mockInvocations
    .map((c) => JSON.stringify(c.args ?? null))
    .filter((a) => a.includes('G:/more') && a.includes('D:/demos')).length);
  expect(saved).toBeGreaterThan(0);

  // Panes whose boot IPC isn't mocked throw on the undefined it resolves to;
  // a wiring slip in main.js shows up as one of these instead.
  expect(errors.filter((m) => /before initialization|is not defined|is not a function/.test(m))).toEqual([]);
});

test('"Capture Output Free" reads the drives again after finished takes are deleted (#644)', async ({ page }) => {
  await page.goto('/index.html');
  await expect(page.locator('#export-pool-free-indicator')).toContainText('10.0');
  await page.evaluate((gib) => {
    window.__mockInvokeHandlers.calculate_export_pool_space = () => 25 * gib;
    window.__mockEmit('take_cleanup_done', { reason: 'capture', method: 'delete', deleted: [], failed: [], short: null });
  }, GIB);
  await expect(page.locator('#export-pool-free-indicator')).toContainText('25.0');
});

test("+ Add Folder's folder shows in the Explorer's Recent list straight away", async ({ page }) => {
  await page.addInitScript(() => {
    Object.assign(window.__mockInvokeHandlers, {
      'plugin:dialog|open': () => 'H:/added',
      scan_directory: () => ({ demos: [], unchanged: 0, copies: [], cancelled: false }),
      queue_match_groups: () => ({ demos: [], matches: [] }),
      // The Explorer's Local tier; unmocked, the quick links can't redraw.
      scan_demo_folders: () => [],
    });
  });
  await page.goto('/index.html');
  await expect(page.locator('#export-pool-free-indicator')).toContainText('10.0');
  const quickLinks = page.locator('#analyzer-quick-links');
  await expect(quickLinks.locator('.quicklink-row[data-path="D:/demos"]')).toHaveCount(1);
  await expect(quickLinks.locator('.quicklink-row[data-path="H:/added"]')).toHaveCount(0);

  await page.evaluate(() => document.querySelector('#add-folder-btn').click());
  await expect(quickLinks.locator('.quicklink-row[data-path="H:/added"]')).toHaveCount(1);
  await expect(quickLinks).toContainText('Recent');
});

test('swallows reload and session shortcuts and the browser context menu', async ({ page }) => {
  await page.goto('/index.html');
  await expect(page.locator('#export-pool-free-indicator')).toContainText('10.0');
  const prevented = await page.evaluate(() => {
    const fire = (type, init) => {
      const ev = new KeyboardEvent(type, { cancelable: true, bubbles: true, ...init });
      window.dispatchEvent(ev);
      return ev.defaultPrevented;
    };
    const menu = new MouseEvent('contextmenu', { cancelable: true, bubbles: true });
    window.dispatchEvent(menu);
    return {
      f5: fire('keydown', { key: 'F5' }),
      ctrlR: fire('keydown', { key: 'r', ctrlKey: true }),
      ctrlW: fire('keydown', { key: 'w', ctrlKey: true }),
      plainR: fire('keydown', { key: 'r' }),
      noKey: fire('keydown', {}),
      menu: menu.defaultPrevented,
    };
  });
  expect(prevented).toEqual({ f5: true, ctrlR: true, ctrlW: true, plainR: false, noKey: false, menu: true });
});
