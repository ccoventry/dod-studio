// overviews_pane.js — the Overviews page (#371).
//
// Pins the install and map pickers, that a picked map is drawn, that a click
// colours or hides an area and the edits are saved, labels, and what Save
// hands the backend: the drawing at the game's 1024x768 with the format and
// place chosen.
import { test, expect } from '@playwright/test';

const square = (x, y, size) => [[x, y], [x + size, y], [x + size, y + size], [x, y + size]];

/** native::overview::scene::Scene, as serde sends it. */
const SCENE = {
  map: 'dod_test',
  width: 1024,
  height: 768,
  transform: { zoom: 1.5, origin: [0, 0, 0], rotated: false, height: 0 },
  areas: [
    { id: 0, indoor: false, colour: [94, 94, 85], anchor: [10, 20], cells: 400 },
    { id: 1, indoor: true, colour: [146, 155, 247], anchor: [30, 40], cells: 400 },
  ],
  faces: [
    { points: square(100, 100, 400), z: 0, area: 0, stairs: false, face: 1 },
    { points: square(550, 100, 300), z: 10, area: 1, stairs: false, face: 2 },
  ],
  water: [],
  cap_zones: [[square(150, 150, 60)]],
  flags: [{ name: 'Plaza', at: [300, 300], world: [5, 6, 7] }],
  allies: [0, 1, 2, 3].map((i) => ({ name: 'Allies', at: [120 + i * 12, 450], world: [0, 0, 0] })),
  axis: [],
  palette: [[94, 94, 85], [146, 155, 247], [125, 29, 55], [255, 255, 255]],
  background: [0, 255, 0],
  void: [16, 17, 14],
};

const INSTALLS = [
  { name: 'Half-Life', path: 'C:/games/Half-Life' },
  { name: 'Half-Life - PRE-Anniversary for Movies', path: 'C:/games/PRE' },
];

const MAPS = [
  { name: 'dod_anzio', bsp: 'x', has_overview: true, has_ours: false, has_edits: false },
  { name: 'dod_test', bsp: 'y', has_overview: false, has_ours: true, has_edits: true },
];

async function loadHarness(page, { edits = null, scene = SCENE } = {}) {
  await page.addInitScript(({ installs, maps, scene, edits }) => {
    window.__mockInvokeHandlers = {
      overview_installs: () => installs,
      overview_maps: () => maps,
      overview_scene: (a) => ({ ...scene, map: a.map }),
      overview_load_edits: () => edits,
      overview_save_edits: () => null,
      overview_reset_edits: () => null,
      overview_export: (args) => ({
        written: [`C:/games/Half-Life/dod/overviews/${args.request.map}.${args.request.format}`],
        backed_up: [],
      }),
      overview_export_hd: (bytes) => {
        window.__hdBytes = bytes.length;
        return 'C:/games/Half-Life/dod_addon/overviews/dod_test_hd.tga';
      },
    };
  }, { installs: INSTALLS, maps: MAPS, scene, edits });
  await page.goto('/tests/e2e/overviews.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

const calls = (page, cmd) => page.evaluate((c) => window.__mockInvocations.filter((i) => i.cmd === c), cmd);

/** Clicks an image pixel on the canvas, whatever size it is shown at. */
async function clickPixel(page, x, y) {
  const box = await page.locator('#ov-canvas').boundingBox();
  await page.mouse.click(box.x + (x / 1024) * box.width, box.y + (y / 768) * box.height);
}

test('lists the installs, picks the stock one, and badges the maps', async ({ page }) => {
  await loadHarness(page);
  await expect(page.locator('#ov-install-select')).toHaveValue('C:/games/Half-Life');
  const rows = page.locator('.ov-map-row');
  await expect(rows).toHaveCount(2);
  await expect(rows.nth(0)).toContainText('has one');
  await expect(rows.nth(1)).toContainText('edited');
  await expect(rows.nth(1)).toContainText('saved');

  await page.fill('#ov-map-filter', 'anz');
  await expect(rows).toHaveCount(1);
});

test('picking a map draws it and fills the side panels', async ({ page }) => {
  await loadHarness(page);
  await page.locator('.ov-map-row', { hasText: 'dod_test' }).click();
  await expect(page.locator('#ov-map-title')).toHaveText('dod_test');
  await expect(page.locator('#ov-canvas')).toBeVisible();
  await expect(page.locator('#ov-empty')).toBeHidden();
  await expect(page.locator('#ov-flag-names input')).toHaveValue('Plaza');
  await expect(page.locator('#ov-palette .ov-swatch')).toHaveCount(4);
  await expect(page.locator('#footer-overviews-summary')).toHaveText('dod_test: 2 areas, 2 floor pieces');

  // The outdoor floor is drawn in its colour where it is.
  const pixel = await page.evaluate(() => {
    const c = document.querySelector('#ov-canvas');
    const s = c.width / 1024;
    return Array.from(c.getContext('2d').getImageData(Math.round(400 * s), Math.round(400 * s), 1, 1).data);
  });
  expect(pixel.slice(0, 3)).toEqual([94, 94, 85]);
});

test('a click colours an area and the edit is saved', async ({ page }) => {
  await loadHarness(page);
  await page.locator('.ov-map-row', { hasText: 'dod_test' }).click();
  await page.locator('#ov-palette .ov-swatch').nth(2).click();
  await clickPixel(page, 700, 250);
  await expect.poll(async () => (await calls(page, 'overview_save_edits')).length).toBeGreaterThan(0);
  const saved = (await calls(page, 'overview_save_edits')).at(-1).args;
  expect(saved.map).toBe('dod_test');
  expect(saved.edits.areas).toEqual([{ at: [30, 40], hidden: false, colour: [125, 29, 55] }]);

  // Undo takes it back.
  await page.click('#ov-undo-btn');
  await expect.poll(async () => (await calls(page, 'overview_save_edits')).at(-1).args.edits.areas).toEqual([]);
});

test('hide area hides it, and a second click shows it again', async ({ page }) => {
  await loadHarness(page);
  await page.locator('.ov-map-row', { hasText: 'dod_test' }).click();
  await page.click('.ov-mode[data-mode="hide"]');
  await clickPixel(page, 700, 250);
  await expect.poll(async () => (await calls(page, 'overview_save_edits')).at(-1)?.args.edits.areas[0]?.hidden).toBe(true);
  await clickPixel(page, 700, 250);
  await expect.poll(async () => (await calls(page, 'overview_save_edits')).at(-1).args.edits.areas).toEqual([]);
});

test('add label puts one where clicked, and it can be renamed and deleted', async ({ page }) => {
  await loadHarness(page);
  await page.locator('.ov-map-row', { hasText: 'dod_test' }).click();
  await page.click('.ov-mode[data-mode="label"]');
  await clickPixel(page, 300, 200);
  const input = page.locator('#ov-labels input[type="text"]');
  await expect(input).toHaveCount(1);
  await input.fill('Church');
  await input.press('Enter');
  await expect.poll(async () => (await calls(page, 'overview_save_edits')).at(-1)?.args.edits.labels[0]?.text).toBe('Church');
  await page.click('#ov-labels button');
  await expect(page.locator('#ov-labels input[type="text"]')).toHaveCount(0);
});

test('saved edits come back when the map is opened', async ({ page }) => {
  await loadHarness(page, {
    edits: { version: 1, show: { flags: false }, format: 'bmp', target: 'addon', labels: [], areas: [], faces: [], flagNames: [] },
  });
  await page.locator('.ov-map-row', { hasText: 'dod_test' }).click();
  await expect(page.locator('#ov-show input[data-show="flags"]')).not.toBeChecked();
  await expect(page.locator('#ov-format')).toHaveValue('bmp');
});

test('save hands the backend the 1024x768 drawing and format, for dod/overviews', async ({ page }) => {
  await loadHarness(page);
  await page.locator('.ov-map-row', { hasText: 'dod_test' }).click();
  await page.selectOption('#ov-format', 'bmp');
  await page.click('#ov-save-btn');
  await expect(page.locator('#ov-save-status')).toHaveText('Saved the overview in dod/overviews, and the high-quality copy and your edits in dod_addon/overviews.');
  // The full paths are there on hover, not on the page.
  await expect(page.locator('#ov-save-status')).toHaveAttribute('title', /dod\/overviews\/dod_test\.bmp/);
  const request = (await calls(page, 'overview_export')).at(-1).args.request;
  expect(request).toMatchObject({ map: 'dod_test', install: 'C:/games/Half-Life', format: 'bmp', target: 'game', width: 1024, height: 768 });
  expect(request.transform).toEqual(SCENE.transform);
  // The edits go with it, for dod_addon/overviews/<map>.dodstudio.json.
  expect(request.edits).toMatchObject({ format: 'bmp' });
  expect((await calls(page, 'overview_load_edits')).at(-1).args).toEqual({ map: 'dod_test', install: 'C:/games/Half-Life' });
  // 1024 * 768 * 4 bytes, base64.
  expect(request.rgba.length).toBe(Math.ceil((1024 * 768 * 4) / 3) * 4);
  // And the high-quality copy, raw: 4096 * 3072 * 4 bytes.
  await expect(page.locator('#ov-save-status')).toHaveAttribute('title', /dod_test_hd\.tga/);
  expect(await page.evaluate(() => window.__hdBytes)).toBe(4096 * 3072 * 4);
});

test('the high-quality copy can be left out', async ({ page }) => {
  await loadHarness(page);
  await page.locator('.ov-map-row', { hasText: 'dod_test' }).click();
  await page.uncheck('#ov-hd');
  await page.click('#ov-save-btn');
  await expect(page.locator('#ov-save-status')).toHaveText('Saved the overview in dod/overviews, and your edits in dod_addon/overviews.');
  expect(await calls(page, 'overview_export_hd')).toHaveLength(0);
});

test('Up and Down move through the map list and open each map', async ({ page }) => {
  await loadHarness(page);
  await expect(page.locator('.ov-map-row')).toHaveCount(2);
  await page.focus('#ov-map-filter');
  await page.keyboard.press('ArrowDown');
  await expect(page.locator('.ov-map-row.active')).toHaveText(/dod_anzio/);
  await expect.poll(async () => (await calls(page, 'overview_scene')).at(-1)?.args.map).toBe('dod_anzio');
  await page.keyboard.press('ArrowDown');
  await expect.poll(async () => (await calls(page, 'overview_scene')).at(-1)?.args.map).toBe('dod_test');
  await page.keyboard.press('ArrowUp');
  await expect.poll(async () => (await calls(page, 'overview_scene')).at(-1)?.args.map).toBe('dod_anzio');
});

test('the theme changes how the floors are drawn, and stays chosen for the next map', async ({ page }) => {
  await loadHarness(page);
  await page.locator('.ov-map-row', { hasText: 'dod_test' }).click();
  const pixel = () => page.evaluate(() => {
    const c = document.querySelector('#ov-canvas');
    const s = c.width / 1024;
    return Array.from(c.getContext('2d').getImageData(Math.round(400 * s), Math.round(400 * s), 1, 1).data).slice(0, 3);
  });
  expect(await pixel()).toEqual([94, 94, 85]);
  await page.selectOption('#ov-theme', 'grey');
  const grey = await pixel();
  expect(grey[0]).toBe(grey[1]);
  expect(grey).not.toEqual([94, 94, 85]);
  // One choice for every map: another map opens in it, and so does this
  // page next time.
  await page.locator('.ov-map-row', { hasText: 'dod_anzio' }).click();
  await expect(page.locator('#ov-theme')).toHaveValue('grey');
  expect(await page.evaluate(() => localStorage.getItem('overviews.theme'))).toBe('grey');
  await page.reload();
  await page.locator('.ov-map-row', { hasText: 'dod_test' }).click();
  await expect(page.locator('#ov-theme')).toHaveValue('grey');
});

test('the classic theme draws the paper map: a dark ruler frame round pale floors', async ({ page }) => {
  await loadHarness(page);
  await page.locator('.ov-map-row', { hasText: 'dod_test' }).click();
  await page.selectOption('#ov-theme', 'classic');
  const at = (x, y) => page.evaluate(([x, y]) => {
    const c = document.querySelector('#ov-canvas');
    const s = c.width / 1024;
    return Array.from(c.getContext('2d').getImageData(Math.round(x * s), Math.round(y * s), 1, 1).data).slice(0, 3);
  }, [x, y]);
  const corner = await at(3, 3);
  expect(Math.max(...corner)).toBeLessThan(40);
  const floor = await at(400, 400);
  expect(Math.min(...floor)).toBeGreaterThan(180);
});

test('water covers the floors below it but not a bridge above it', async ({ page }) => {
  // One sheet of water at height 5 over both floors: the one at 0 is under
  // it, the one at 10 crosses over it.
  const scene = { ...SCENE, water: [{ points: square(50, 50, 900), z: 5 }] };
  await loadHarness(page, { scene });
  await page.locator('.ov-map-row', { hasText: 'dod_test' }).click();
  const at = (x, y) => page.evaluate(([x, y]) => {
    const c = document.querySelector('#ov-canvas');
    const s = c.width / 1024;
    return Array.from(c.getContext('2d').getImageData(Math.round(x * s), Math.round(y * s), 1, 1).data).slice(0, 3);
  }, [x, y]);
  expect(await at(400, 400)).toEqual([64, 208, 213]);
  expect(await at(700, 300)).toEqual([146, 155, 247]);
});

test('a floor that breaks gets a dotted outline, and it can be turned off', async ({ page }) => {
  const scene = { ...SCENE, breakable_edges: [[[200, 600], [400, 600]]] };
  await loadHarness(page, { scene });
  await page.locator('.ov-map-row', { hasText: 'dod_test' }).click();
  // Along the line: black dashes with white between them.
  const row = () => page.evaluate(() => {
    const c = document.querySelector('#ov-canvas');
    const s = c.width / 1024;
    const d = c.getContext('2d').getImageData(Math.round(200 * s), Math.round(600 * s) - 2, Math.round(200 * s), 5).data;
    const out = [];
    for (let i = 0; i < d.length; i += 4) out.push([d[i], d[i + 1], d[i + 2]]);
    return out;
  });
  const black = (p) => p.every((v) => v < 60);
  const white = (p) => p.every((v) => v > 200);
  const on = await row();
  expect(on.some(black)).toBe(true);
  expect(on.some(white)).toBe(true);
  await page.locator('input[data-show="breakables"]').uncheck();
  const off = await row();
  expect(off.some(black) || off.some(white)).toBe(false);
});

test('a map clicked while another is building is highlighted at once and wins', async ({ page }) => {
  await loadHarness(page);
  await page.evaluate(() => {
    const build = window.__mockInvokeHandlers.overview_scene;
    window.__mockInvokeHandlers.overview_scene = (a) => new Promise((done) => setTimeout(() => done(build(a)), 400));
  });
  await page.locator('.ov-map-row', { hasText: 'dod_anzio' }).click();
  await page.locator('.ov-map-row', { hasText: 'dod_test' }).click();
  await expect(page.locator('.ov-map-row.active')).toHaveText(/dod_test/);
  await expect(page.locator('#ov-map-title')).toHaveText('dod_test');
  // The last map's drawing is gone while the new one builds, with a bar.
  await expect(page.locator('#ov-canvas')).toBeHidden();
  await expect(page.locator('#ov-building')).toBeVisible();
  await page.evaluate(() => window.__mockEmit('overview_progress', { map: 'dod_test', fraction: 0.5 }));
  await expect(page.locator('#ov-building-text')).toContainText('50%');
  await expect(page.locator('#ov-building-fill')).toHaveAttribute('style', /width: 50%/);
  // Progress for a map no longer asked for is ignored.
  await page.evaluate(() => window.__mockEmit('overview_progress', { map: 'dod_anzio', fraction: 0.9 }));
  await expect(page.locator('#ov-building-text')).toContainText('50%');
  await expect(page.locator('#ov-canvas')).toBeVisible();
  await expect(page.locator('#ov-building')).toBeHidden();
  await expect(page.locator('.ov-map-row.active')).toHaveText(/dod_test/);
});

test('a slope you slide on is outlined, not filled, and can be turned off', async ({ page }) => {
  const scene = { ...SCENE, slope_edges: [[[600, 600], [800, 600]]] };
  await loadHarness(page, { scene });
  await page.locator('.ov-map-row', { hasText: 'dod_test' }).click();
  const dark = () => page.evaluate(() => {
    const c = document.querySelector('#ov-canvas');
    const s = c.width / 1024;
    const d = c.getContext('2d').getImageData(Math.round(650 * s), Math.round(600 * s) - 2, Math.round(100 * s), 5).data;
    // Darker than the green round it (a thin line at a small canvas size
    // blends with it).
    for (let i = 0; i < d.length; i += 4) if (d[i + 1] < 160) return true;
    return false;
  });
  expect(await dark()).toBe(true);
  await page.locator('input[data-show="slopes"]').uncheck();
  expect(await dark()).toBe(false);
});

test('edits that fit nothing on the map any more are kept aside, and the page says so', async ({ page }) => {
  await loadHarness(page, {
    edits: { version: 1, areas: [{ at: [99999, 99999], colour: [1, 2, 3] }], labels: [], faces: [], flagNames: [] },
  });
  await page.locator('.ov-map-row', { hasText: 'dod_test' }).click();
  await expect(page.locator('#ov-save-status')).toContainText('1 area colour');
  // Kept with the rest, to try again next time.
  await page.selectOption('#ov-theme', 'grey');
  await page.locator('#ov-show input[data-show="water"]').uncheck();
  await expect.poll(async () => (await calls(page, 'overview_save_edits')).at(-1)?.args.edits.aside.areas).toEqual([{ at: [99999, 99999], colour: [1, 2, 3] }]);
});
