// running_game_guard.js — the ask before reusing a game started with other
// launch settings (#666).
import { test, expect } from '@playwright/test';

const PRE = 'C:\\Games\\Half-Life - PRE-Anniversary for Movies\\hl.exe';
const POST = 'C:\\Games\\Half-Life - POST-Anniversary for Movies\\hl.exe';

const MISMATCH = {
  state: 'mismatch',
  pid: 4242,
  running: { install: 'Half-Life - PRE-Anniversary for Movies', exe: PRE, width: 1920, height: 1080 },
  wanted: { install: 'Half-Life - POST-Anniversary for Movies', exe: POST, width: 3440, height: 1440 },
  differs: ['install', 'resolution'],
};

async function loadHarness(page, check, { closeFails = false } = {}) {
  await page.addInitScript(([c, fails]) => {
    window.__mockInvokeHandlers = {
      check_running_game: () => c,
      close_running_game: () => (fails ? Promise.reject('still running') : undefined),
    };
  }, [check, closeFails]);
  await page.goto('/tests/e2e/running-game.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

const invoked = (page, cmd) =>
  page.evaluate((c) => window.__mockInvocations.filter((i) => i.cmd === c), cmd);

test('another install and resolution are named, and Cancel closes nothing', async ({ page }) => {
  await loadHarness(page, MISMATCH);
  page.evaluate(() => window.__guard({ game_path: 'C:/x/hl.exe', resolution_width: 3440, resolution_height: 1440 }));
  await expect(page.locator('#themed-confirm-title')).toHaveText('Day of Defeat is running with other settings');
  await expect(page.locator('#themed-confirm-message')).toHaveText(
    'Day of Defeat is running from "Half-Life - PRE-Anniversary for Movies" at 1920×1080; '
    + 'DoD Studio is set to "Half-Life - POST-Anniversary for Movies" at 3440×1440. '
    + 'The game only takes these when it starts. Close it and start again?',
  );
  await expect(page.locator('#themed-confirm-details')).toContainText(PRE);
  await expect(page.locator('#themed-confirm-details')).toContainText(POST);
  await expect(page.locator('#themed-confirm-ok-btn')).toHaveText('Close it and start again');
  await page.click('#themed-confirm-cancel-btn');
  await expect(page.locator('#result')).toHaveText('false');
  expect(await invoked(page, 'close_running_game')).toEqual([]);
  const [check] = await invoked(page, 'check_running_game');
  expect(check.args.request).toEqual({ game_path: 'C:/x/hl.exe', resolution_width: 3440, resolution_height: 1440 });
});

test('yes closes the running game by pid and lets the launch go on', async ({ page }) => {
  await loadHarness(page, { ...MISMATCH, differs: ['resolution'] });
  page.evaluate(() => window.__guard(null));
  await expect(page.locator('#themed-confirm-message')).toContainText('running at 1920×1080; DoD Studio is set to 3440×1440.');
  await expect(page.locator('#themed-confirm-details')).toBeHidden();
  await page.click('#themed-confirm-ok-btn');
  await expect(page.locator('#result')).toHaveText('true');
  const closes = await invoked(page, 'close_running_game');
  expect(closes.map((c) => c.args)).toEqual([{ pid: 4242 }]);
  await expect(page.locator('#launch-btn')).toBeEnabled();
});

test('a game that will not close stops the launch', async ({ page }) => {
  await loadHarness(page, MISMATCH, { closeFails: true });
  page.evaluate(() => window.__guard(null));
  await page.click('#themed-confirm-ok-btn');
  await expect(page.locator('#result')).toHaveText('false');
});

test('a matching game, no game, or a failed check asks nothing', async ({ page }) => {
  for (const check of [{ state: 'match', pid: 7 }, { state: 'none' }]) {
    await loadHarness(page, check);
    await page.evaluate(() => window.__guard(null));
    await expect(page.locator('#result')).toHaveText('true');
    await expect(page.locator('#themed-confirm-modal')).toBeHidden();
  }
  await page.addInitScript(() => {
    window.__mockInvokeHandlers = { check_running_game: () => Promise.reject('boom') };
  });
  await page.goto('/tests/e2e/running-game.html');
  await page.waitForFunction(() => window.__harnessReady === true);
  await page.evaluate(() => window.__guard(null));
  await expect(page.locator('#result')).toHaveText('true');
});
