// Draggable splits beyond Capture Studio and the Analyzer (#46): the Overviews
// side panels and the Render / Auditor table heights. Drives the real app shell
// against mocked IPC, like layout-splits.spec.js.
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

const size = (page, sel, dim) => page.locator(sel).evaluate((el, d) => el.getBoundingClientRect()[d], dim);

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

// Each split: where to go, the handle, the pane it resizes, the settings key,
// which way a positive drag moves the pane's size, and the size the saved value
// should give back.
const SPLITS = [
  { name: 'Overviews Maps panel', nav: 'overviews', handle: '#ov-maps-resize-handle', pane: '#ov-maps-panel', dim: 'width', field: 'overviews_maps_width_px', dx: 80, dy: 0, sign: 1, saved: 330, defaultMax: 252 },
  { name: 'Overviews Edit panel', nav: 'overviews', handle: '#ov-edit-resize-handle', pane: '#ov-edit-panel', dim: 'width', field: 'overviews_edit_width_px', dx: -80, dy: 0, sign: 1, saved: 330, defaultMax: 252 },
  { name: 'Render queue table', nav: 'workspace', sub: 'render', handle: '#render-jobs-resize-handle', pane: '#render-jobs-wrap', dim: 'height', field: 'render_table_height_px', dx: 0, dy: 150, sign: 1, saved: 500, defaultMax: 460 },
  { name: 'Auditor results table', nav: 'demo-auditor', handle: '#audit-results-resize-handle', pane: '#audit-results-wrap', dim: 'height', field: 'auditor_table_height_px', dx: 0, dy: 150, sign: 1, saved: 500, defaultMax: 460 },
];

async function open(page, s) {
  await page.locator(`[data-nav="${s.nav}"]`).click();
  if (s.sub) await page.locator(`.capture-detail-subtab-btn[data-capture-subtab="${s.sub}"]`).click();
  await expect(page.locator(s.handle)).toBeVisible();
}

for (const s of SPLITS) {
  test(`dragging the ${s.name} handle resizes it and saves the size`, async ({ page }) => {
    await boot(page);
    await open(page, s);
    const before = await size(page, s.pane, s.dim);
    await drag(page, s.handle, s.dx, s.dy);
    const after = await size(page, s.pane, s.dim);
    expect(after).toBeGreaterThan(before + 70);
    await expect.poll(async () => (await lastSavedSettings(page))?.[s.field]).toBeGreaterThan(before + 70);
  });

  test(`a saved ${s.name} size is restored, and double-click resets it`, async ({ page }) => {
    await boot(page, { [s.field]: s.saved });
    await open(page, s);
    expect(Math.round(await size(page, s.pane, s.dim))).toBe(s.saved);
    await page.locator(s.handle).dblclick();
    expect(await size(page, s.pane, s.dim)).toBeLessThan(s.saved);
    await expect.poll(async () => (await lastSavedSettings(page))?.[s.field]).toBeNull();
  });
}

test('an Overviews side panel stops short of the other panel and the map', async ({ page }) => {
  await boot(page);
  await open(page, SPLITS[0]);
  await drag(page, '#ov-maps-resize-handle', 3000, 0);
  expect(await size(page, '#ov-maps-panel', 'width')).toBeLessThan(1600 - 250 - 300 - 20);
  expect(await size(page, '.ov-main', 'width')).toBeGreaterThanOrEqual(299);
  await drag(page, '#ov-maps-resize-handle', -3000, 0);
  expect(await size(page, '#ov-maps-panel', 'width')).toBeGreaterThanOrEqual(179);
});

test('a table never drags below its minimum height', async ({ page }) => {
  await boot(page);
  await open(page, SPLITS[3]);
  await drag(page, '#audit-results-resize-handle', 0, -2000);
  expect(await size(page, '#audit-results-wrap', 'height')).toBeGreaterThanOrEqual(119);
});
