// The Project settings page (#1085, build plan step 6) in the real app shell:
// File > Project Settings opens it, an edit waits for OK or Apply, Apply
// puts it into the project (unsaved) without touching Configuration, Cancel
// drops it, and leaving with pending edits asks Apply, Discard or Stay.
import { test, expect } from '@playwright/test';

const GIB = 1024 ** 3;

test.beforeEach(async ({ page }) => {
  await page.addInitScript((gib) => {
    window.__mockInvokeHandlers = {
      get_settings: () => ({
        // No pinned folders and Setup dismissed: no start-up pop-up over the page.
        setup_dismissed: true,
        target_drives: ['E:/caps'],
        render_export_dirs: ['F:/clips'],
        resolution_width: 1920,
        resolution_height: 1080,
        capture_fps: 120,
        pc_id: 'pc-test',
      }),
      calculate_export_pool_space: () => 10 * gib,
      system_memory_bytes: () => 16 * gib,
    };
  }, GIB);
  await page.goto('/index.html');
  await expect(page.locator('#export-pool-free-indicator')).toContainText('10.0');
});

const pageRoot = (page) => page.locator('#project-settings-page');
const chip = (page, group, value) => pageRoot(page).locator(`[data-chips="${group}"] [data-value="${value}"]`);
const footer = (page, action) => pageRoot(page).locator(`.ps-footer [data-action="${action}"]`);

async function openPage(page) {
  await page.locator('#file-menu-btn').click();
  await page.locator('#project-settings-menu-btn').click();
  await expect(pageRoot(page)).toBeVisible();
}

test('File > Project Settings opens the page on the Project tab, Apply greyed', async ({ page }) => {
  await openPage(page);
  await expect(pageRoot(page).locator('.ps-tab-on')).toHaveText('Project');
  await expect(pageRoot(page).locator('.ps-tab:disabled')).toHaveCount(2);
  await expect(chip(page, 'size', '1080p')).toHaveAttribute('aria-pressed', 'true');
  await expect(chip(page, 'rate', '120')).toHaveAttribute('aria-pressed', 'true');
  await expect(footer(page, 'apply')).toBeDisabled();
  await expect(footer(page, 'ok')).toBeEnabled();
  await expect(pageRoot(page).locator('[data-row="target_drives"]')).toContainText('E:/caps');
});

test('Apply puts an edit into the project, unsaved, and leaves Configuration alone', async ({ page }) => {
  await openPage(page);
  await chip(page, 'size', '1440p').click();
  await expect(footer(page, 'apply')).toBeEnabled();
  await footer(page, 'apply').click();
  await expect(footer(page, 'apply')).toBeDisabled();
  await expect(page.locator('#session-file-indicator')).toContainText('unsaved');
  await footer(page, 'ok').click();
  await expect(pageRoot(page)).toBeHidden();
  await expect(page.locator('#config-res-width')).toHaveValue('1920');
  await expect(page.locator('#capture-summary-strip')).toContainText('2560');
});

test('Cancel drops what changed since the last Apply', async ({ page }) => {
  await openPage(page);
  await chip(page, 'size', '1440p').click();
  await footer(page, 'apply').click();
  await chip(page, 'size', '4k').click();
  await footer(page, 'cancel').click();
  await expect(pageRoot(page)).toBeHidden();
  await openPage(page);
  await expect(chip(page, 'size', '1440p')).toHaveAttribute('aria-pressed', 'true');
});

test('OK with nothing changed leaves the project saved', async ({ page }) => {
  await openPage(page);
  await footer(page, 'ok').click();
  await expect(pageRoot(page)).toBeHidden();
  await expect(page.locator('#session-file-indicator')).not.toContainText('unsaved');
});

test('a value that cannot be used greys Apply and OK and says why', async ({ page }) => {
  await openPage(page);
  await chip(page, 'size', 'custom').click();
  await pageRoot(page).locator('[data-field="resolution_width"]').fill('');
  await expect(footer(page, 'apply')).toBeDisabled();
  await expect(footer(page, 'ok')).toBeDisabled();
  await expect(pageRoot(page).locator('[data-row="resolution_width"] .ps-problem')).toBeVisible();
  await pageRoot(page).locator('[data-field="resolution_width"]').fill('2560');
  await expect(footer(page, 'apply')).toBeEnabled();
});

test('leaving by the menu bar with pending edits asks Apply, Discard or Stay', async ({ page }) => {
  await openPage(page);
  await chip(page, 'rate', '300').click();
  await page.locator('.nav-tab-btn[data-nav="overviews"]').click();
  const prompt = pageRoot(page).locator('.ps-leave');
  await expect(prompt).toBeVisible();
  await prompt.getByRole('button', { name: 'Stay' }).click();
  await expect(prompt).toHaveCount(0);
  await expect(pageRoot(page)).toBeVisible();
  await expect(chip(page, 'rate', '300')).toHaveAttribute('aria-pressed', 'true');

  await page.locator('.nav-tab-btn[data-nav="overviews"]').click();
  await pageRoot(page).locator('.ps-leave').getByRole('button', { name: 'Discard' }).click();
  await expect(pageRoot(page)).toBeHidden();
  await expect(page.locator('.nav-tab-btn[data-nav="overviews"]')).toHaveClass(/active/);
  await expect(page.locator('#session-file-indicator')).not.toContainText('unsaved');
});

test('Apply in the leave prompt keeps the edit, then goes where was clicked', async ({ page }) => {
  await openPage(page);
  await chip(page, 'capture_mode', 'direct_to_video').click();
  await page.locator('.nav-tab-btn[data-nav="blender"]').click();
  await pageRoot(page).locator('.ps-leave').getByRole('button', { name: 'Apply' }).click();
  await expect(pageRoot(page)).toBeHidden();
  await expect(page.locator('.nav-tab-btn[data-nav="blender"]')).toHaveClass(/active/);
  await expect(page.locator('#session-file-indicator')).toContainText('unsaved');
  await openPage(page);
  await expect(chip(page, 'capture_mode', 'direct_to_video')).toHaveAttribute('aria-pressed', 'true');
});

test("Ctrl+N doesn't start a new session while the page is open", async ({ page }) => {
  await openPage(page);
  const clicks = await page.evaluate(() => {
    let count = 0;
    document.querySelector('#new-session-btn').addEventListener('click', () => { count += 1; });
    document.querySelector('#project-settings-page').dispatchEvent(new KeyboardEvent('keydown', { key: 'n', ctrlKey: true, bubbles: true, cancelable: true }));
    return count;
  });
  expect(clicks).toBe(0);
  await footer(page, 'cancel').click();
  const after = await page.evaluate(() => {
    let count = 0;
    document.querySelector('#new-session-btn').addEventListener('click', () => { count += 1; });
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'n', ctrlKey: true, bubbles: true, cancelable: true }));
    return count;
  });
  expect(after).toBe(1);
});

test('AGR shows its own positions a second, and has nothing to finish', async ({ page }) => {
  await openPage(page);
  await chip(page, 'capture_mode', 'agr').click();
  await expect(pageRoot(page).locator('[data-row="agr_fps"]')).toContainText('AGR positions a second');
  await expect(pageRoot(page).locator('[data-chips="finish"]')).toHaveCount(0);
});
