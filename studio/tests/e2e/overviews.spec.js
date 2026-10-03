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
      // Per map, as the backend keeps them; `edits` for a map not saved yet.
      overview_load_edits: (a) => window.__savedEdits?.[a.map] ?? edits,
      overview_save_edits: (a) => {
        (window.__savedEdits ||= {})[a.map] = a.edits;
        return null;
      },
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
  // In this theme's colours; hiding is for every theme.
  expect(saved.edits.colours.colours.areas).toEqual([{ at: [30, 40], colour: [125, 29, 55] }]);
  expect(saved.edits.areas).toEqual([]);

  // Undo takes it back.
  await page.click('#ov-undo-btn');
  await expect.poll(async () => (await calls(page, 'overview_save_edits')).at(-1).args.edits.colours.colours?.areas ?? []).toEqual([]);
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

test('the theme changes how the floors are drawn, each map keeps its own, and a new map opens in the last one', async ({ page }) => {
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
  await expect.poll(() => page.evaluate(() => window.__savedEdits?.dod_test?.theme)).toBe('grey');
  // A map with no theme of its own opens in the last one picked...
  await page.locator('.ov-map-row', { hasText: 'dod_anzio' }).click();
  await expect(page.locator('#ov-theme')).toHaveValue('grey');
  await page.selectOption('#ov-theme', 'classic');
  await expect.poll(() => page.evaluate(() => window.__savedEdits?.dod_anzio?.theme)).toBe('classic');
  // ...and one that has a theme keeps it.
  await page.locator('.ov-map-row', { hasText: 'dod_test' }).click();
  await expect(page.locator('#ov-theme')).toHaveValue('grey');
  await page.locator('.ov-map-row', { hasText: 'dod_anzio' }).click();
  await expect(page.locator('#ov-theme')).toHaveValue('classic');
  // The last one picked is remembered for next time.
  expect(await page.evaluate(() => localStorage.getItem('overviews.theme'))).toBe('classic');
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
  await expect.poll(async () => (await calls(page, 'overview_save_edits')).at(-1)?.args.edits.aside.colours.colours.areas).toEqual([{ at: [99999, 99999], colour: [1, 2, 3] }]);
});

test('each theme keeps its own colours', async ({ page }) => {
  await loadHarness(page);
  await page.locator('.ov-map-row', { hasText: 'dod_test' }).click();
  const pixel = () => page.evaluate(() => {
    const c = document.querySelector('#ov-canvas');
    const s = c.width / 1024;
    return Array.from(c.getContext('2d').getImageData(Math.round(700 * s), Math.round(250 * s), 1, 1).data).slice(0, 3);
  });
  await page.locator('#ov-palette .ov-swatch').nth(2).click();
  await clickPixel(page, 700, 250);
  // Off the map, so the hover highlight isn't over the pixel read.
  await page.mouse.move(0, 0);
  await expect.poll(pixel).toEqual([125, 29, 55]);
  // Flat grey has its own colours: none yet.
  await page.selectOption('#ov-theme', 'grey');
  await expect.poll(pixel).not.toEqual([125, 29, 55]);
  // Back to Colour-coded: still painted.
  await page.selectOption('#ov-theme', 'colours');
  await expect.poll(pixel).toEqual([125, 29, 55]);
});

test('a flag name can be dragged, and the reset button puts it back', async ({ page }) => {
  await loadHarness(page);
  await page.locator('.ov-map-row', { hasText: 'dod_test' }).click();
  const box = await page.locator('#ov-canvas').boundingBox();
  const at = (x, y) => [box.x + (x / 1024) * box.width, box.y + (y / 768) * box.height];
  // Plaza's name sits under the flag (300, 300) by default.
  await page.mouse.move(...at(300, 342));
  await page.mouse.down();
  await page.mouse.move(...at(450, 200), { steps: 4 });
  await page.mouse.up();
  await expect.poll(async () => (await calls(page, 'overview_save_edits')).at(-1)?.args.edits.flagNames[0]?.offset).toBeTruthy();
  const reset = page.locator('#ov-flag-names .ov-flag-reset');
  await expect(reset).toBeEnabled();
  await reset.click();
  await expect.poll(async () => (await calls(page, 'overview_save_edits')).at(-1).args.edits.flagNames).toEqual([]);
  await expect(reset).toBeDisabled();
});

test("the game's flag icons can be previewed, sized for a screen height, and are never saved", async ({ page }) => {
  await loadHarness(page);
  await page.evaluate(() => {
    window.__mockInvokeHandlers.overview_screen_height = () => 720;
    window.__mockInvokeHandlers.overview_flag_icons = async () => {
      const { toWorld } = await import('/src/overview_draw.js');
      const [x, y] = toWorld({ zoom: 1.5, origin: [0, 0, 0], rotated: false, height: 0 }, 600, 650);
      const red = new Uint8Array(32 * 32 * 4).map((_, i) => [255, 0, 0, 255][i % 4]);
      return [{ world: [x, y, 0], width: 32, height: 32, rgba: btoa(String.fromCharCode(...red)) }];
    };
  });
  await page.locator('.ov-map-row', { hasText: 'dod_test' }).click();
  const pixel = (x, y) => page.evaluate(([x, y]) => {
    const c = document.querySelector('#ov-canvas');
    const s = c.width / 1024;
    return Array.from(c.getContext('2d').getImageData(Math.round(x * s), Math.round(y * s), 1, 1).data).slice(0, 3);
  }, [x, y]);
  expect(await pixel(600, 650)).not.toEqual([255, 0, 0]);
  await page.check('#ov-flag-icons');
  await expect.poll(() => pixel(600, 650)).toEqual([255, 0, 0]);
  // At 720p a 32 px icon is 32 * 768 / 450 = 55 image pixels across: 25 out
  // from its middle is still icon; at 2160p (18 across) it isn't.
  await page.selectOption('#ov-flag-screen', '720');
  await expect.poll(() => pixel(625, 650)).toEqual([255, 0, 0]);
  await page.selectOption('#ov-flag-screen', '2160');
  await expect.poll(() => pixel(625, 650)).not.toEqual([255, 0, 0]);
  // Saving draws without them.
  await page.click('#ov-save-btn');
  await expect(page.locator('#ov-save-status')).toContainText('Saved');
});

test('scrolling zooms around the pointer, clicks still land where they point, and 100% shows it all', async ({ page }) => {
  await loadHarness(page);
  await page.locator('.ov-map-row', { hasText: 'dod_test' }).click();
  const box = await page.locator('#ov-canvas').boundingBox();
  // Over the indoor area (550-850 x 100-400 in image pixels).
  const [mx, my] = [box.x + (600 / 1024) * box.width, box.y + (150 / 768) * box.height];
  await page.mouse.move(mx, my);
  for (let i = 0; i < 4; i++) await page.mouse.wheel(0, -400);
  await expect(page.locator('#ov-zoom-fit')).not.toHaveText('100%');
  // What is under the pointer stayed: colouring there colours the indoor area.
  await page.locator('#ov-palette .ov-swatch').nth(2).click();
  await page.mouse.click(mx, my);
  await expect.poll(async () => (await calls(page, 'overview_save_edits')).at(-1)?.args.edits.colours.colours.areas[0]?.at).toEqual([30, 40]);
  // Zoomed in, the canvas shows that area's colour edge to edge (pointer
  // off the map, so no hover highlight).
  await page.mouse.move(0, 0);
  await expect.poll(() => page.evaluate(() => {
    const c = document.querySelector('#ov-canvas');
    return Array.from(c.getContext('2d').getImageData(Math.round(c.width * 0.5), Math.round(c.height * 0.5), 1, 1).data).slice(0, 3);
  })).toEqual([125, 29, 55]);
  // Dragging with the right button moves about, and colours nothing.
  const before = (await calls(page, 'overview_save_edits')).length;
  const moved = () => page.evaluate(() => {
    const c = document.querySelector('#ov-canvas');
    return Array.from(c.getContext('2d').getImageData(Math.round(c.width * 0.5), Math.round(c.height * 0.5), 1, 1).data).slice(0, 3);
  });
  await page.mouse.move(mx, my);
  await page.mouse.down({ button: 'right' });
  await page.mouse.move(mx + box.width * 0.45, my, { steps: 5 });
  await page.mouse.up({ button: 'right' });
  await expect.poll(moved).not.toEqual([125, 29, 55]);
  expect((await calls(page, 'overview_save_edits')).length).toBe(before);
  await page.click('#ov-zoom-fit');
  await expect(page.locator('#ov-zoom-fit')).toHaveText('100%');
  await expect(page.locator('#ov-zoom-out')).toBeDisabled();
});

test('spawn protection fills its floor and draws the line where you walk in, in the colours picked', async ({ page }) => {
  // An Axis zone over the outdoor floor (100-500) and beyond it: its edge
  // at x 300 crosses the floor; the part past y 500 is off the floor.
  const scene = {
    ...SCENE,
    spawn_zones: [{
      team: 'axis',
      edges: [[[300, 150], [300, 700]]],
      polygons: [[[300, 150], [480, 150], [480, 700], [300, 700]]],
    }],
  };
  await loadHarness(page, { scene });
  await page.locator('.ov-map-row', { hasText: 'dod_test' }).click();
  const at = (x, y) => page.evaluate(([x, y]) => {
    const c = document.querySelector('#ov-canvas');
    const s = c.width / 1024;
    return Array.from(c.getContext('2d').getImageData(Math.round(x * s), Math.round(y * s), 1, 1).data).slice(0, 3);
  }, [x, y]);
  // Tinted red inside on the floor; nothing past the floor's edge.
  const inside = await at(400, 300);
  expect(inside[0]).toBeGreaterThan(inside[1] + 20);
  expect(await at(400, 650)).toEqual([0, 255, 0]);
  // The line in the team colour across the floor.
  await expect.poll(async () => (await at(300, 300))[0]).toBeGreaterThan(180);
  // Hazard stripes, in colours picked.
  await page.selectOption('#ov-sp-line', 'hazard');
  await page.locator('#ov-sp-stripe1').evaluate((el) => { el.value = '#0000ff'; el.dispatchEvent(new Event('change')); });
  const column = () => page.evaluate(() => {
    const c = document.querySelector('#ov-canvas');
    const s = c.width / 1024;
    const d = c.getContext('2d').getImageData(Math.round(300 * s) - 1, Math.round(150 * s), 3, Math.round(300 * s)).data;
    for (let i = 0; i < d.length; i += 4) if (d[i + 2] > 200 && d[i] < 60) return true;
    return false;
  });
  await expect.poll(column).toBe(true);
  await expect.poll(async () => (await calls(page, 'overview_save_edits')).at(-1)?.args.edits.spawnProtection).toMatchObject({ line: 'hazard', stripe1: '#0000ff' });
  // Off: no tint.
  await page.locator('input[data-show="spawnProtection"]').uncheck();
  await expect.poll(() => at(400, 300)).toEqual([94, 94, 85]);
});

test('a spawn name can be dragged and put back, and the cursor shows what a click does', async ({ page }) => {
  await loadHarness(page);
  await page.locator('.ov-map-row', { hasText: 'dod_test' }).click();
  const box = await page.locator('#ov-canvas').boundingBox();
  const at = (x, y) => [box.x + (x / 1024) * box.width, box.y + (y / 768) * box.height];
  const cursor = () => page.locator('#ov-canvas').evaluate((c) => c.style.cursor);
  // Colour area mode: the bucket.
  await page.mouse.move(...at(200, 200));
  await expect.poll(cursor).toContain('svg');
  // Over the Allies spawn name (under the four spawns at 120-156, 450): a hand.
  const spawnName = at(138, 463);
  await page.mouse.move(...spawnName);
  await expect.poll(cursor).toBe('grab');
  await page.mouse.down();
  await page.mouse.move(...at(300, 600), { steps: 4 });
  await page.mouse.up();
  await expect.poll(async () => (await calls(page, 'overview_save_edits')).at(-1)?.args.edits.spawnNames[0]?.team).toBe('Allies');
  await expect(page.locator('#ov-spawn-reset')).toBeEnabled();
  await page.click('#ov-spawn-reset');
  await expect.poll(async () => (await calls(page, 'overview_save_edits')).at(-1).args.edits.spawnNames).toEqual([]);
  // Add label mode: a text cursor.
  await page.click('.ov-add-label');
  await page.mouse.move(...at(500, 500));
  await expect.poll(cursor).toBe('text');
});

test('hovering shows what a click would paint, and Show areas outlines them all', async ({ page }) => {
  await loadHarness(page);
  await page.locator('.ov-map-row', { hasText: 'dod_test' }).click();
  const box = await page.locator('#ov-canvas').boundingBox();
  const at = (x, y) => [box.x + (x / 1024) * box.width, box.y + (y / 768) * box.height];
  const pixel = (x, y) => page.evaluate(([x, y]) => {
    const c = document.querySelector('#ov-canvas');
    const s = c.width / 1024;
    return Array.from(c.getContext('2d').getImageData(Math.round(x * s), Math.round(y * s), 1, 1).data).slice(0, 3);
  }, [x, y]);
  // The highlight is on unless turned off.
  await expect(page.locator('#ov-hover-preview')).toBeChecked();
  // The indoor area is lighter while hovered, and back after.
  const plain = await pixel(700, 250);
  await page.mouse.move(...at(700, 250));
  await expect.poll(async () => (await pixel(700, 250))[0]).toBeGreaterThan(plain[0]);
  await page.mouse.move(...at(1000, 700));
  await expect.poll(() => pixel(700, 250)).toEqual(plain);
  // Show areas draws outlines (the canvas changes), and unticking takes
  // them away again.
  const sum = () => page.evaluate(() => {
    const c = document.querySelector('#ov-canvas');
    const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data;
    let total = 0;
    for (let i = 0; i < d.length; i += 4) total += d[i];
    return total;
  });
  const before = await sum();
  await page.check('#ov-show-areas');
  await expect.poll(sum).toBeLessThan(before);
  // With Colour piece picked it outlines the pieces instead, and says so.
  const areasSum = await sum();
  await page.click('.ov-mode[data-mode="face"]');
  await expect(page.locator('#ov-preview')).toContainText('Show pieces');
  await page.click('.ov-mode[data-mode="area"]');
  await expect(page.locator('#ov-preview')).toContainText('Show areas');
  await expect.poll(sum).toBe(areasSum);
  await page.uncheck('#ov-show-areas');
  await expect.poll(sum).toBe(before);
  // With the highlight turned off, hovering changes nothing.
  await page.uncheck('#ov-hover-preview');
  await page.mouse.move(...at(700, 250));
  await page.waitForTimeout(100);
  expect(await pixel(700, 250)).toEqual(plain);
  await page.mouse.move(...at(1000, 700));
  await page.check('#ov-hover-preview');
  // Nothing of this is in the saved image.
  await page.click('#ov-save-btn');
  const rgba = (await calls(page, 'overview_export')).at(-1).args.request.rgba;
  const bytes = await page.evaluate((b) => {
    const s = atob(b);
    const i = (250 * 1024 + 700) * 4;
    return [s.charCodeAt(i), s.charCodeAt(i + 1), s.charCodeAt(i + 2)];
  }, rgba);
  expect(bytes).toEqual([146, 155, 247]);
});

test('colouring an area keeps pieces coloured on their own, and Shift paints over them', async ({ page }) => {
  await loadHarness(page);
  await page.locator('.ov-map-row', { hasText: 'dod_test' }).click();
  const theme = async () => (await calls(page, 'overview_save_edits')).at(-1)?.args.edits.colours.colours;
  // A piece coloured on its own (face 2, the indoor square).
  await page.click('.ov-mode[data-mode="face"]');
  await page.locator('#ov-palette .ov-swatch').nth(2).click();
  await clickPixel(page, 700, 250);
  await expect.poll(async () => (await theme())?.faces).toEqual([{ face: 2, colour: [125, 29, 55] }]);
  // A plain area click colours the area and keeps the piece's colour.
  await page.click('.ov-mode[data-mode="area"]');
  await page.locator('#ov-palette .ov-swatch').nth(1).click();
  await clickPixel(page, 700, 250);
  await expect.poll(async () => (await theme())?.areas?.length).toBe(1);
  expect((await theme()).faces).toHaveLength(1);
  // Shift-click paints over it.
  const box = await page.locator('#ov-canvas').boundingBox();
  await page.keyboard.down('Shift');
  await page.mouse.click(box.x + (700 / 1024) * box.width, box.y + (250 / 768) * box.height);
  await page.keyboard.up('Shift');
  await expect.poll(async () => (await theme())?.faces).toEqual([]);
  // Undo brings the piece's colour back.
  await page.click('#ov-undo-btn');
  await expect.poll(async () => (await theme())?.faces).toHaveLength(1);
});

test("the colour swatches are the theme's own", async ({ page }) => {
  await loadHarness(page);
  await page.locator('.ov-map-row', { hasText: 'dod_test' }).click();
  const swatches = () => page.locator('#ov-palette .ov-swatch').evaluateAll((els) => els.map((e) => e.title));
  // Colour-coded: the scene's palette.
  expect(await swatches()).toEqual(['#5e5e55', '#929bf7', '#7d1d37', '#ffffff']);
  // Flat grey: its greys, and nothing from Colour-coded.
  await page.selectOption('#ov-theme', 'grey');
  const grey = await swatches();
  expect(grey).not.toContain('#929bf7');
  expect(grey[0]).toBe('#767674');
  // Classic: its paper first.
  await page.selectOption('#ov-theme', 'classic');
  expect((await swatches())[0]).toBe('#f0eee7');
});
