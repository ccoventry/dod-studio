// Draggable splits (#47): Capture Studio's queue / details panes and the Demo
// Analyzer's Demos panel / report. Drives the real app shell against mocked IPC.
import { test, expect } from '@playwright/test';

test.use({ viewport: { width: 1600, height: 900 } });

async function boot(page, savedSettings = {}) {
  await page.addInitScript((saved) => {
    window.__mockInvokeHandlers = {
      get_settings: () => ({ pinned_folders: [], ...saved }),
      scan_demo_folders: () => [],
    };
  }, savedSettings);
  await page.goto('/index.html');
  await expect(page.locator('#capture-split-handle')).toBeVisible();
}

const width = (page, sel) => page.locator(sel).evaluate((el) => el.getBoundingClientRect().width);
const height = (page, sel) => page.locator(sel).evaluate((el) => el.getBoundingClientRect().height);

async function drag(page, sel, dx, dy) {
  const box = await page.locator(sel).boundingBox();
  const x = box.x + box.width / 2;
  const y = box.y + box.height / 2;
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x + dx / 2, y + dy / 2);
  await page.mouse.move(x + dx, y + dy);
  await page.mouse.up();
}

const lastSavedSettings = (page) => page.evaluate(() => {
  const saves = window.__mockInvocations.filter((c) => c.cmd === 'save_settings');
  return saves.length ? saves[saves.length - 1].args.settings : null;
});

test('Capture Studio starts with the queue pane at about 40% of the width', async ({ page }) => {
  await boot(page);
  const total = await width(page, '#main-content-split');
  const left = await width(page, '#pane-workspace');
  expect(left / total).toBeGreaterThan(0.39);
  expect(left / total).toBeLessThan(0.41);
});

test('dragging the Capture Studio handle resizes the panes and saves the split', async ({ page }) => {
  await boot(page);
  const before = await width(page, '#pane-workspace');
  await drag(page, '#capture-split-handle', 200, 0);
  const after = await width(page, '#pane-workspace');
  expect(after).toBeGreaterThan(before + 190);
  await expect.poll(async () => (await lastSavedSettings(page))?.capture_split_left_permille).toBeGreaterThan(450);
});

test('the Capture Studio split stops at the minimum widths', async ({ page }) => {
  await boot(page);
  await drag(page, '#capture-split-handle', -2000, 0);
  expect(await width(page, '#pane-workspace')).toBeGreaterThanOrEqual(379);
  await drag(page, '#capture-split-handle', 3000, 0);
  expect(await width(page, '#pane-details-config')).toBeGreaterThanOrEqual(519);
});

test('a saved Capture Studio split is restored on start', async ({ page }) => {
  await boot(page, { capture_split_left_permille: 550 });
  const total = await width(page, '#main-content-split');
  const left = await width(page, '#pane-workspace');
  expect(Math.abs(left / total - 0.55)).toBeLessThan(0.01);
});

test('double-clicking the Capture Studio handle goes back to the default split', async ({ page }) => {
  await boot(page, { capture_split_left_permille: 600 });
  await page.locator('#capture-split-handle').dblclick();
  const total = await width(page, '#main-content-split');
  const left = await width(page, '#pane-workspace');
  expect(Math.abs(left / total - 0.4)).toBeLessThan(0.01);
  await expect.poll(async () => (await lastSavedSettings(page))?.capture_split_left_permille).toBeNull();
});

test('the Capture Studio handle goes away with the queue pane on the Render and Configuration sub-tabs', async ({ page }) => {
  await boot(page);
  await page.locator('.capture-detail-subtab-btn[data-capture-subtab="configuration"]').click();
  await expect(page.locator('#capture-split-handle')).toBeHidden();
  await page.locator('.capture-detail-subtab-btn[data-capture-subtab="highlights"]').click();
  await expect(page.locator('#capture-split-handle')).toBeVisible();
});

test('dragging the Demo Analyzer handle resizes the Demos panel and saves its height', async ({ page }) => {
  await boot(page);
  await page.locator('[data-nav="demo-analyzer"]').click();
  await expect(page.locator('#analyzer-demos-resize-handle')).toBeVisible();
  const before = await height(page, '.analyzer-demos-panel');
  await drag(page, '#analyzer-demos-resize-handle', 0, 120);
  const after = await height(page, '.analyzer-demos-panel');
  expect(after).toBeGreaterThan(before + 110);
  await expect.poll(async () => (await lastSavedSettings(page))?.analyzer_demos_height_px).toBeGreaterThan(before + 110);
});

test('a saved Demos panel height is restored, and double-click resets it', async ({ page }) => {
  await boot(page, { analyzer_demos_height_px: 400 });
  await page.locator('[data-nav="demo-analyzer"]').click();
  expect(Math.round(await height(page, '.analyzer-demos-panel'))).toBe(400);
  await page.locator('#analyzer-demos-resize-handle').dblclick();
  expect(await height(page, '.analyzer-demos-panel')).toBeLessThanOrEqual(260);
  await expect.poll(async () => (await lastSavedSettings(page))?.analyzer_demos_height_px).toBeNull();
});
