// Boots the real app shell (index.html + main.js) against the mocked IPC and
// checks the wiring main.js does at startup: settings land in the modules that
// own them, and the app-wide listeners are in place. Every other spec drives
// one pane in its own harness, so this is the one that notices a main.js
// wiring slip (#683).
import { test, expect } from './fixtures.js';

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

  // Any uncaught error already fails the test (fixtures.js); this names the
  // wiring slips main.js can make, for a clearer message.
  expect(errors.filter((m) => /before initialization|is not defined|is not a function/.test(m))).toEqual([]);
});

test('a fresh install shows no red fields before anything is picked to capture (#25)', async ({ page }) => {
  await page.goto('/index.html');
  await expect(page.locator('#export-pool-free-indicator')).toContainText('10.0');
  await expect(page.locator('#tab-path-routing .field-invalid')).toHaveCount(0);
  await expect(page.locator('#hl-install-choice')).toHaveCount(1);
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

test('"After a clip is finished" sits in the Destinations tab, since it acts before captures too (#1094)', async ({ page }) => {
  await page.goto('/index.html');
  for (const id of ['#config-take-after-finish', '#config-take-delete-when-needed', '#config-take-after-finish-hint']) {
    await expect(page.locator(`#tab-drive-overrides ${id}`)).toHaveCount(1);
    await expect(page.locator(`#tab-render-output ${id}`)).toHaveCount(0);
  }
});

test('"Keep free on the Windows drive" loads into Destinations and saves a change (#1430)', async ({ page }) => {
  await page.addInitScript(() => {
    const loaded = window.__mockInvokeHandlers.get_settings();
    window.__mockInvokeHandlers.get_settings = () => ({ ...loaded, windows_drive_keep_free_gb: 80 });
  });
  await page.goto('/index.html');
  const field = page.locator('#tab-drive-overrides #config-windows-drive-keep-free');
  await expect(field).toHaveValue('80');
  // The tab is hidden until opened, so the change is made in the page.
  await field.evaluate((el) => {
    el.value = '30';
    el.dispatchEvent(new Event('change'));
  });
  await expect.poll(() => page.evaluate(() => {
    const saves = window.__mockInvocations.filter((c) => c.cmd === 'save_settings');
    return saves.length ? saves[saves.length - 1].args.settings.windows_drive_keep_free_gb : null;
  })).toBe(30);
});

test('a Windows drive below "Keep free on the Windows drive" shows its real free space and names the setting (#1430)', async ({ page }) => {
  await page.addInitScript((gib) => {
    const loaded = window.__mockInvokeHandlers.get_settings();
    Object.assign(window.__mockInvokeHandlers, {
      get_settings: () => ({ ...loaded, target_drives: ['C:/caps'], windows_drive_keep_free_gb: 50 }),
      calculate_export_pool_space: () => 2 * gib,
      capture_pool_room: () => ({ free_bytes: 2 * gib, usable_bytes: 0 }),
    });
  }, GIB);
  await page.goto('/index.html');
  await expect(page.locator('#config-windows-drive-keep-free')).toHaveValue('50');
  await expect(page.locator('#export-pool-free-indicator')).toContainText('2.0');
  const guard = await page.evaluate(async () => {
    document.querySelector('#hl-path-input').value = 'C:/Games/Half-Life/hl.exe';
    document.querySelector('#hlae-path-input').value = 'C:/HLAE/HLAE.exe';
    const m = await import('/src/capture_pane.js');
    return m.refreshLaunchGuard({
      targetDrives: ['C:/caps'],
      currentScannedDemos: [{ streaks: [{ selected: true, start_tick: 1000, end_tick: 2000 }] }],
    });
  });
  expect(guard.blocked).toBe(true);
  const banner = page.locator('#disk-space-warning-banner');
  await expect(banner).toContainText('Keep free on the Windows drive');
  await expect(banner).toContainText('2.00 GB, below the 50 GB');
  await expect(banner).toContainText('add a Capture Output folder on another drive');
  await expect(banner).not.toContainText('have any free space');
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

test("Studio's first sub-tab is Highlights, and every queue filter sits in one Filters row", async ({ page }) => {
  await page.goto('/index.html');
  await expect(page.locator('#export-pool-free-indicator')).toContainText('10.0');
  await expect(page.locator('.capture-detail-subtab-btn[data-capture-subtab="highlights"]')).toHaveText('Highlights');

  // #127: every demo filter sits in one row. Min Kills is gone (#1070).
  const row = page.locator('#queue-filter-row');
  await expect(row).toContainText('Filters');
  for (const id of ['demo-search-input', 'master-player-filter', 'master-kills-filter', 'master-owner-only-cb', 'master-tracked-only-cb']) {
    await expect(row.locator(`#${id}`)).toHaveCount(1);
  }
  // Tracked only: an empty queue has nothing tracked, so it boots dimmed with a reason.
  await expect(row.locator('#master-tracked-only-cb')).toBeDisabled();
  await expect(row.locator('.queue-filter-tracked-only')).toHaveText('Tracked only');
  await expect(row.locator('.queue-filter-tracked-only')).toHaveAttribute('title', /No demo is tracked yet/);
  await expect(page.locator('#input-min-kills')).toHaveCount(0);
});

test('the boot watchdog hears the boot finish and shows nothing', async ({ page }) => {
  await page.goto('/index.html');
  await expect.poll(() => page.evaluate(() => window.dodStudioBoot?.state())).toBe('started');
  await expect(page.locator('#boot-watchdog-banner')).toHaveCount(0);
});

test('a boot whose scripts never arrive reloads once, then says to restart', async ({ page }) => {
  await page.addInitScript(() => { window.__dodStudioBootDeadlineMs = 1500; });
  // One of main.js's imports never answers, as in the tauri dev window that
  // loaded 76 of the page's scripts after a hard reset.
  await page.route('**/src/players_ui.js*', () => {});
  // The page's load event never comes while a module hangs.
  await page.goto('/index.html', { waitUntil: 'commit' });
  const banner = page.locator('#boot-watchdog-banner');
  await expect(banner).toContainText('even after a reload', { timeout: 15000 });
  expect(await page.evaluate(() => window.dodStudioBoot.state())).toBe('stuck');
  expect(await page.evaluate(() => sessionStorage.getItem('dodstudio.bootReloaded'))).toBe('1');
});
