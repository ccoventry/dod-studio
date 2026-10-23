// install_picker.js — Configuration > Paths' Half-Life Executable as a list
// of the installs found on this PC (#931).
import { test, expect } from '@playwright/test';

const COMMON = 'C:\\Steam\\steamapps\\common';
const install = (name, build) => ({ name, path: `${COMMON}\\${name}`, hl_exe: `${COMMON}\\${name}\\hl.exe`, build });
const STOCK = install('Half-Life', 'anniversary');
const POST = install('Half-Life - POST-Anniversary for Movies', 'anniversary');
const PRE = install('Half-Life - PRE-Anniversary for Movies', 'pre_anniversary');

async function loadHarness(page, { installs = [STOCK, POST, PRE], folders = ['dod'], running = { state: 'none' } } = {}) {
  await page.addInitScript(([i, f, r]) => {
    window.__mockInvokeHandlers = {
      find_installs: () => i,
      list_game_folders: () => f,
      game_folder_language_file: () => null,
      check_running_game: () => r,
      close_running_game: () => undefined,
    };
  }, [installs, folders, running]);
  await page.goto('/tests/e2e/install-picker.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

const box = (page) => page.locator('#hl-install-choice');
const options = (page) => page.locator('.search-select-option');
const invoked = (page, cmd) =>
  page.evaluate((c) => window.__mockInvocations.filter((i) => i.cmd === c), cmd);

test('lists each install with its build, and a pick sets hl.exe and the Game Folder', async ({ page }) => {
  await loadHarness(page, { folders: ['dod', 'dod_movies'] });
  await page.evaluate((p) => window.__start({ hlPath: p, gameFolder: 'dod_movies' }), PRE.hl_exe);
  await expect(box(page)).toHaveValue('Half-Life - PRE-Anniversary for Movies (pre-Anniversary)');
  await box(page).click();
  await expect(options(page)).toHaveText([
    /^Half-Life \(25th Anniversary\)Hide$/,
    /^Half-Life - POST-Anniversary for Movies \(25th Anniversary\)Hide$/,
    /^Half-Life - PRE-Anniversary for Movies \(pre-Anniversary\)Hide$/,
  ]);
  const before = await page.evaluate(() => window.__persisted);
  await options(page).nth(1).click();
  await expect.poll(() => page.evaluate(() => window.__hlPath())).toBe(POST.hl_exe);
  // POST has a dod_movies too, so the Game Folder stays on it.
  expect(await page.evaluate(() => window.__gameFolder())).toBe('dod_movies');
  expect(await page.evaluate(() => window.__persisted)).toBeGreaterThan(before);
  await expect(box(page)).toHaveValue('Half-Life - POST-Anniversary for Movies (25th Anniversary)');
});

test('the Game Folder falls back to dod when the new install has no folder of that name', async ({ page }) => {
  await loadHarness(page, { folders: ['dod'] });
  await page.evaluate((p) => window.__start({ hlPath: p, gameFolder: 'dod_movies' }), PRE.hl_exe);
  await box(page).click();
  await options(page).nth(1).click();
  await expect.poll(() => page.evaluate(() => window.__hlPath())).toBe(POST.hl_exe);
  expect(await page.evaluate(() => window.__gameFolder())).toBe('dod');
});

test('Hide takes an install off the list and Show hidden installs brings it back', async ({ page }) => {
  await loadHarness(page);
  await page.evaluate((p) => window.__start({ hlPath: p }), PRE.hl_exe);
  await box(page).click();
  await options(page).first().locator('.search-select-action').click();
  // The list stays open, the stock install gone, a row to show it at the end.
  await expect(options(page)).toHaveCount(3);
  await expect(options(page).last()).toHaveText('Show hidden installs (1)');
  expect(await page.evaluate(() => window.__hidden())).toEqual([STOCK.path]);
  expect(await page.evaluate(() => window.__persisted)).toBe(1);

  await options(page).last().click();
  await expect(options(page).first()).toHaveText(/^Half-Life \(25th Anniversary\) · hiddenShow$/);
  await options(page).first().locator('.search-select-action').click();
  expect(await page.evaluate(() => window.__hidden())).toEqual([]);
  // Hiding never switched anything.
  expect(await page.evaluate(() => window.__hlPath())).toBe(PRE.hl_exe);
});

test('the list is the short one again when it is opened after Show hidden installs', async ({ page }) => {
  await loadHarness(page);
  await page.evaluate(([p, h]) => window.__start({ hlPath: p, hidden: [h] }), [PRE.hl_exe, STOCK.path]);
  await box(page).click();
  await options(page).last().click();
  await expect(options(page)).toHaveCount(4);
  await expect(options(page).first()).toHaveText(/· hiddenShow$/);
  // Closed and opened again: hidden ones are out of the list again.
  await page.locator('#after').click();
  await expect(page.locator('.search-select-panel')).toBeHidden();
  await box(page).click();
  await expect(options(page)).toHaveCount(3);
  await expect(options(page).last()).toHaveText('Show hidden installs (1)');
  // Picking a hidden install while they show keeps it picked and listed.
  await options(page).last().click();
  await options(page).first().click();
  await expect.poll(() => page.evaluate(() => window.__hlPath())).toBe(STOCK.hl_exe);
  await expect(box(page)).toHaveValue('Half-Life (25th Anniversary) · hidden');
});

test('a browsed hl.exe that is no install shows as its own row', async ({ page }) => {
  await loadHarness(page);
  await page.evaluate(() => window.__start({ hlPath: 'D:\\Games\\HL copy\\hl.exe' }));
  await expect(box(page)).toHaveValue('Other: HL copy\\hl.exe');
});

test('while the game runs, the switch asks to close it, and Cancel keeps the install', async ({ page }) => {
  const running = {
    state: 'mismatch',
    pid: 4242,
    running: { install: PRE.name, exe: PRE.hl_exe, width: 1920, height: 1080 },
    wanted: { install: POST.name, exe: POST.hl_exe, width: 1920, height: 1080 },
    differs: ['install'],
  };
  await loadHarness(page, { running });
  await page.evaluate((p) => window.__start({ hlPath: p }), PRE.hl_exe);
  await box(page).click();
  await options(page).nth(1).click();
  await expect(page.locator('#themed-confirm-ok-btn')).toHaveText('Close it and switch');
  await expect(page.locator('#themed-confirm-message')).toContainText(`running from "${PRE.name}"`);
  await page.click('#themed-confirm-cancel-btn');
  await expect(box(page)).toHaveValue('Half-Life - PRE-Anniversary for Movies (pre-Anniversary)');
  expect(await page.evaluate(() => window.__hlPath())).toBe(PRE.hl_exe);
  const [check] = await invoked(page, 'check_running_game');
  expect(check.args.request).toEqual({ game_path: POST.hl_exe });

  // Yes closes it by pid and switches.
  await box(page).click();
  await options(page).nth(1).click();
  await page.click('#themed-confirm-ok-btn');
  await expect.poll(() => page.evaluate(() => window.__hlPath())).toBe(POST.hl_exe);
  expect((await invoked(page, 'close_running_game')).map((c) => c.args)).toEqual([{ pid: 4242 }]);
});

test('with one install and no hl.exe set, it fills itself in', async ({ page }) => {
  await loadHarness(page, { installs: [PRE] });
  expect(await page.evaluate(() => window.__prefill())).toBe(true);
  expect(await page.evaluate(() => window.__hlPath())).toBe(PRE.hl_exe);
  // Not with two.
  await loadHarness(page, { installs: [PRE, POST] });
  expect(await page.evaluate(() => window.__prefill())).toBe(false);
  expect(await page.evaluate(() => window.__hlPath())).toBe('');
});
