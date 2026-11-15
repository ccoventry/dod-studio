// Nothing pulls the rug during a batch (#1085): the real app shell, with a
// batch's capture_status reports sent by hand.
import { test, expect } from './fixtures.js';

const GIB = 1024 ** 3;
const HELD_TITLE = 'A capture batch is running. Wait for it to finish, or cancel it.';

test.beforeEach(async ({ page }) => {
  await page.addInitScript((gib) => {
    window.__mockInvokeHandlers = {
      get_settings: () => ({ setup_dismissed: true, pinned_folders: [], target_drives: [], render_export_dirs: [], recent_projects: ['D:/p/monday.dodstudio'] }),
      calculate_export_pool_space: () => 10 * gib,
      system_memory_bytes: () => 16 * gib,
      files_exist: ({ paths }) => paths.map(() => true),
      load_project_session: () => JSON.stringify({ version: '0.12.0', demos: [] }),
    };
  }, GIB);
  await page.goto('/index.html');
  await expect(page.locator('#load-project-btn')).toBeAttached();
  await page.waitForFunction(() => (window.__mockEventListeners.capture_status || []).length >= 2);
});

const batch = (page, running) => page.evaluate((r) => window.__mockEmit('capture_status', { running: r }), running);
const held = (page, selector) => page.locator(selector).evaluate((el) => ({
  grey: el.classList.contains('batch-held'), title: el.title,
}));

test('a running batch greys Open, New and Clear with the reason, and its end gives them back', async ({ page }) => {
  const ownTitle = await page.locator('#load-project-btn').getAttribute('title');
  await batch(page, true);
  for (const selector of ['#load-project-btn', '#new-session-btn', '#rescan-recent-btn', '#clear-all-btn', '#clear-untracked-btn', '#clear-selected-btn']) {
    expect(await held(page, selector)).toEqual({ grey: true, title: HELD_TITLE });
  }
  await batch(page, false);
  expect(await held(page, '#load-project-btn')).toEqual({ grey: false, title: ownTitle });
});

test('Ctrl+O and Ctrl+N during a batch do nothing but say why', async ({ page }) => {
  await batch(page, true);
  await page.keyboard.press('Control+o');
  await expect(page.locator('#toast-container')).toContainText('this project stays open until it ends');
  await page.keyboard.press('Control+n');
  await expect(page.locator('#toast-container')).not.toContainText('Started a new session');
  const opened = await page.evaluate(() => window.__mockInvocations.filter((c) => c.cmd === 'plugin:dialog|open'));
  expect(opened).toEqual([]);
});

test('Open Recent during a batch greys its rows and opens nothing', async ({ page }) => {
  await batch(page, true);
  await page.locator('#file-menu-btn').click();
  await page.locator('#recent-projects-menu').hover();
  const row = page.locator('.recent-project-item');
  await expect(row).toHaveClass(/batch-held/);
  await expect(row).toHaveAttribute('title', HELD_TITLE);
  await row.click();
  await expect(page.locator('#toast-container')).toContainText('this project stays open until it ends');
  const loads = await page.evaluate(() => window.__mockInvocations.filter((c) => c.cmd === 'load_project_session'));
  expect(loads).toEqual([]);
});

test('after the batch, Open Recent opens again', async ({ page }) => {
  await batch(page, true);
  await batch(page, false);
  await page.locator('#file-menu-btn').click();
  await page.locator('#recent-projects-menu').hover();
  await page.locator('.recent-project-item').click();
  await expect(page.locator('#session-file-indicator')).toHaveText('monday.dodstudio');
});

test('Restart waits for the batch, on the update window and the menu bar', async ({ page }) => {
  await page.evaluate(() => window.__mockEmit('update_ready', null));
  await expect(page.locator('#update-chip')).toHaveText('Restart to update');
  await batch(page, true);
  await expect(page.locator('#update-chip')).toHaveText('Restart after the batch');
  await page.locator('#update-chip').click();
  const restart = page.locator('#restart-to-apply-btn');
  await expect(restart).toBeDisabled();
  await expect(restart).toHaveText('Restart after the batch');
  await expect(restart).toHaveAttribute('title', /capture batch is running/);
  await batch(page, false);
  await expect(restart).toBeEnabled();
  await expect(restart).toHaveText('Restart to Apply');
  await expect(page.locator('#update-chip')).toHaveText('Restart to update');
});

test('Clear Selected during a batch removes nothing and says why', async ({ page }) => {
  await batch(page, true);
  await page.evaluate(() => {
    const btn = document.querySelector('#clear-selected-btn');
    btn.disabled = false;
    btn.click();
  });
  await expect(page.locator('#toast-container')).toContainText('this project stays open until it ends');
  await expect(page.locator('#toast-container')).not.toContainText('No demos');
});

test('a Restart that reaches its handler during a batch is still refused', async ({ page }) => {
  await page.evaluate(() => window.__mockEmit('update_ready', null));
  await batch(page, true);
  await page.evaluate(() => {
    const btn = document.querySelector('#restart-to-apply-btn');
    btn.disabled = false;
    btn.click();
  });
  await expect(page.locator('#toast-container')).toContainText('this project stays open until it ends');
  const restarts = await page.evaluate(() => window.__mockInvocations.filter((c) => c.cmd === 'restart_app'));
  expect(restarts).toEqual([]);
});

test('a Download & Install that reaches its handler during a batch is still refused', async ({ page }) => {
  await batch(page, true);
  await page.evaluate(() => {
    const btn = document.querySelector('#download-install-update-btn');
    btn.disabled = false;
    btn.click();
  });
  await expect(page.locator('#toast-container')).toContainText('this project stays open until it ends');
  await expect(page.locator('#update-status-text')).not.toContainText('Downloading');
});

test('Download & Install waits for the batch: on Windows the installer closes Studio', async ({ page }) => {
  await page.evaluate(() => {
    const btn = document.querySelector('#download-install-update-btn');
    btn.style.display = 'inline-block';
  });
  await batch(page, true);
  const download = page.locator('#download-install-update-btn');
  await expect(download).toBeDisabled();
  await expect(download).toHaveAttribute('title', /closes DoD Studio/);
  await batch(page, false);
  await expect(download).toBeEnabled();
});
