// menu_scale_setting.js — Configuration > Paths' "Size the Game's Menus Like
// the Pre-Anniversary Engine" (#985).
import { test, expect } from '@playwright/test';

const POST = 'C:\\Steam\\steamapps\\common\\Half-Life - POST-Anniversary for Movies\\hl.exe';
const PATH = 'C:\\Steam\\steamapps\\common\\Half-Life - POST-Anniversary for Movies\\dod_addon\\resource\\TrackerScheme.res';

async function loadHarness(page, { applies = true, on = false, foreign = false, fail = false } = {}) {
  await page.addInitScript(([a, o, f, path, failSet]) => {
    window.__disk = { applies: a, on: o, foreign: f, path };
    window.__mockInvokeHandlers = {
      menu_scale_status: () => ({ ...window.__disk }),
      set_menu_scale: ({ on: wanted }) => {
        if (failSet) return Promise.reject('could not write');
        window.__disk.on = wanted;
        return { ...window.__disk };
      },
    };
  }, [applies, on, foreign, PATH, fail]);
  await page.goto('/tests/e2e/menu-scale.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

const row = (page) => page.locator('#config-menu-scale-row');
const box = (page) => page.locator('#config-menu-scale');
const invoked = (page, cmd) =>
  page.evaluate((c) => window.__mockInvocations.filter((i) => i.cmd === c), cmd);

test('an Anniversary install shows the box off; ticking writes the copy, unticking removes it', async ({ page }) => {
  await loadHarness(page);
  await page.evaluate((p) => window.__start({ hlPath: p, gameFolder: 'dod' }), POST);
  await expect(row(page)).toBeVisible();
  await expect(box(page)).not.toBeChecked();
  await expect(box(page)).toBeEnabled();
  await expect(box(page)).toHaveAttribute('title', /-addons/);

  await box(page).check();
  await expect.poll(async () => (await invoked(page, 'set_menu_scale')).map((i) => i.args))
    .toEqual([{ gamePath: POST, gameFolder: 'dod', on: true }]);
  await expect(box(page)).toBeChecked();
  await expect(box(page)).toBeEnabled();

  await box(page).uncheck();
  await expect.poll(async () => (await invoked(page, 'set_menu_scale')).length).toBe(2);
  expect((await invoked(page, 'set_menu_scale'))[1].args.on).toBe(false);
  await expect(box(page)).not.toBeChecked();
});

test('a pre-Anniversary install hides the row', async ({ page }) => {
  await loadHarness(page, { applies: false });
  await page.evaluate((p) => window.__start({ hlPath: p }), POST);
  await expect.poll(async () => (await invoked(page, 'menu_scale_status')).length).toBe(1);
  await expect(row(page)).toBeHidden();
});

test('no hl.exe set hides the row and asks the backend nothing', async ({ page }) => {
  await loadHarness(page);
  await page.evaluate(() => window.__start({ hlPath: '' }));
  await expect(row(page)).toBeHidden();
  expect(await invoked(page, 'menu_scale_status')).toEqual([]);
});

test('a scheme that is not ours greys the box out and says why', async ({ page }) => {
  await loadHarness(page, { foreign: true });
  await page.evaluate((p) => window.__start({ hlPath: p }), POST);
  await expect(row(page)).toBeVisible();
  await expect(box(page)).toBeDisabled();
  await expect(box(page)).toHaveAttribute('title', /isn't DoD Studio's/);
});

test('a failed write puts the box back to what is on disk', async ({ page }) => {
  await loadHarness(page, { fail: true });
  await page.evaluate((p) => window.__start({ hlPath: p }), POST);
  // click, not check: check() fails when the box doesn't stay ticked.
  await box(page).click();
  await expect.poll(async () => (await invoked(page, 'menu_scale_status')).length).toBe(2);
  await expect(box(page)).not.toBeChecked();
  await expect(box(page)).toBeEnabled();
});
