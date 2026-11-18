// The New UI switch (#1085, new_ui.js) in the real app shell: off by default,
// with no File › Project Settings… and no way onto the page; on, the page
// opens as built; a flip in Configuration shows or hides it at once and is
// saved.
import { test, expect } from '@playwright/test';

const GIB = 1024 ** 3;

async function boot(page, extra = {}) {
  await page.addInitScript(({ gib, extra }) => {
    window.__mockInvokeHandlers = {
      get_settings: () => ({
        setup_dismissed: true,
        target_drives: ['E:/caps'],
        render_export_dirs: ['F:/clips'],
        resolution_width: 1920,
        resolution_height: 1080,
        pc_id: 'pc-test',
        ...extra,
      }),
      calculate_export_pool_space: () => 10 * gib,
      system_memory_bytes: () => 16 * gib,
      'plugin:dialog|open': () => 'D:\\projects\\movie.dodstudio',
      load_project_session: () => JSON.stringify({ version: '0.12.0', schema: 3, demos: [], settings: {} }),
      files_exist: ({ paths }) => paths.map(() => true),
      locate_missing_demos: () => [],
    };
  }, { gib: GIB, extra });
  await page.goto('/index.html');
  await expect(page.locator('#export-pool-free-indicator')).toContainText('10.0');
}

const menuItem = (page) => page.locator('#project-settings-menu-btn');
const pageRoot = (page) => page.locator('#project-settings-page');
const savedNewUi = (page) => page.evaluate(() => window.__mockInvocations
  .filter((c) => c.cmd === 'save_settings').map((c) => c.args.settings.new_ui));

/** Flips Configuration's box, as a click would. */
async function flip(page, on) {
  await page.evaluate((value) => {
    const box = document.querySelector('#config-new-ui');
    box.checked = value;
    box.dispatchEvent(new Event('change', { bubbles: true }));
  }, on);
}

test('off by default: no Project Settings in the File menu, and the page is never made', async ({ page }) => {
  await boot(page);
  await expect(page.locator('#tab-pipeline #config-new-ui')).not.toBeChecked();
  await page.locator('#file-menu-btn').click();
  await expect(page.locator('#save-project-btn')).toBeVisible();
  await expect(menuItem(page)).toHaveCount(0);
  await page.keyboard.press('Escape');

  // Opening a project and New Session go ahead with nothing to leave.
  await page.evaluate(() => document.querySelector('#load-project-btn').click());
  await expect(page.locator('#session-file-indicator')).toHaveText('movie.dodstudio');
  await page.evaluate(() => document.querySelector('#new-session-btn').click());
  await expect(page.locator('#session-file-indicator')).not.toHaveText('movie.dodstudio');
  await expect(pageRoot(page)).toHaveCount(0);
});

test('on in the settings file: File › Project Settings… opens the page', async ({ page }) => {
  await boot(page, { new_ui: true });
  await expect(page.locator('#config-new-ui')).toBeChecked();
  await page.locator('#file-menu-btn').click();
  await menuItem(page).click();
  await expect(pageRoot(page)).toBeVisible();
});

test('a flip in Configuration shows or hides the page at once, and is saved', async ({ page }) => {
  await boot(page);
  await flip(page, true);
  await expect(menuItem(page)).toHaveCount(1);
  await expect.poll(() => savedNewUi(page)).toContain(true);

  await page.locator('#file-menu-btn').click();
  await menuItem(page).click();
  await expect(pageRoot(page)).toBeVisible();

  // Turned off (here from code, as the page covers Configuration): the
  // page closes and the menu item goes.
  await flip(page, false);
  await expect(pageRoot(page)).toBeHidden();
  await expect(menuItem(page)).toHaveCount(0);
  expect((await savedNewUi(page)).at(-1)).toBe(false);
});
