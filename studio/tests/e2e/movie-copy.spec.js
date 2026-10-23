// movie_copy_ui.js — Setup's Make a movie copy dialog (#939), against
// tests/e2e/movie-copy.html with mocked IPC.
//
// Pins: the default name and its checks, the size against the free space
// with and without demos, the controls locked while copying with a bar by
// bytes and the file, Cancel asking first, and Done picking the copy.
import { test, expect } from '@playwright/test';

async function openDialog(page) {
  await page.goto('/tests/e2e/movie-copy.html');
  await page.waitForFunction(() => window.__harnessReady === true);
  await page.evaluate(() => window.__copy.open());
  await expect(page.locator('#movie-copy-modal')).toBeVisible();
  await expect(page.locator('#movie-copy-name')).toHaveValue('dod_movies');
}

const invocations = (page, cmd) =>
  page.evaluate((c) => window.__mockInvocations.filter((i) => i.cmd === c), cmd);

test('suggests dod_movies, checks the name, and weighs the size against the free space', async ({ page }) => {
  await openDialog(page);
  await expect(page.locator('#movie-copy-size')).toHaveText('4422 files, 15 GB to copy. Demos: 424 demos, 29 GB.');
  await expect(page.locator('#movie-copy-space')).toHaveText('50 GB free on this drive.');
  await expect(page.locator('#movie-copy-go')).toBeEnabled();

  await page.locator('#movie-copy-name').fill('dod movies');
  await expect(page.locator('#movie-copy-name-problem')).toHaveText('Letters, digits and _ only.');
  await expect(page.locator('#movie-copy-go')).toBeDisabled();
  await page.locator('#movie-copy-name').fill('Valve');
  await expect(page.locator('#movie-copy-name-problem')).toContainText('already a valve');
  await page.locator('#movie-copy-name').fill('dod_movies');
  await expect(page.locator('#movie-copy-name-problem')).toBeHidden();

  // With the demos it's 44 GB, which still fits 50 GB with 1 GB spare.
  await page.locator('#movie-copy-demos').check();
  await expect(page.locator('#movie-copy-size')).toHaveText('4846 files, 44 GB to copy.');
  await expect(page.locator('#movie-copy-go')).toBeEnabled();
});

test("refuses a copy that doesn't fit", async ({ page }) => {
  await page.goto('/tests/e2e/movie-copy.html');
  await page.waitForFunction(() => window.__harnessReady === true);
  await page.evaluate(() => { window.__plan.free_bytes = 40 * 1024 ** 3; window.__copy.open(); });
  await expect(page.locator('#movie-copy-go')).toBeEnabled();
  await page.locator('#movie-copy-demos').check();
  await expect(page.locator('#movie-copy-space')).toContainText('Not enough space: the copy needs 44 GB and this drive has 40 GB free');
  await expect(page.locator('#movie-copy-go')).toBeDisabled();
});

test('copies with a bar by bytes, locked while it runs, and picks the copy when done', async ({ page }) => {
  await openDialog(page);
  await page.locator('#movie-copy-go').click();
  await expect(page.locator('#movie-copy-go')).toBeDisabled();
  await expect(page.locator('#movie-copy-close')).toBeDisabled();
  await expect(page.locator('#movie-copy-name')).toBeDisabled();
  await expect(page.locator('#movie-copy-demos')).toBeDisabled();
  await expect(page.locator('#movie-copy-cancel')).toBeVisible();
  expect((await invocations(page, 'start_movie_copy'))[0].args).toEqual({
    gamePath: 'C:\\Games\\Half-Life\\hl.exe', name: 'dod_movies', includeDemos: false,
  });

  await page.evaluate(() => window.__mockEmit('movie_copy_progress', {
    done_bytes: 4 * 1024 ** 3, total_bytes: 16 * 1024 ** 3, done_files: 9, total_files: 40, file: 'maps\\dod_anzio.bsp',
  }));
  await expect(page.locator('#movie-copy-status')).toHaveText('Copied 4.0 GB of 16 GB: maps\\dod_anzio.bsp');
  await expect(page.locator('#movie-copy-progress .progress-bar-fill')).toHaveAttribute('style', /width: 25%/);

  await page.evaluate(() => window.__finishCopy({ state: 'done', folder: 'dod_movies', target: 'x', files: 40, bytes: 1 }));
  await expect(page.locator('#movie-copy-status')).toHaveText('Done: dod_movies is ready and picked as the game folder.');
  expect(await page.evaluate(() => window.__calls)).toContainEqual(['onDone', 'dod_movies']);
  await expect(page.locator('#movie-copy-go')).toBeHidden();
  await page.locator('#movie-copy-close').click();
  await expect(page.locator('#movie-copy-modal')).toBeHidden();
});

test('Cancel asks first, and only a yes stops the copy', async ({ page }) => {
  await openDialog(page);
  await page.locator('#movie-copy-go').click();
  await page.locator('#movie-copy-cancel').click();
  await expect(page.locator('#themed-confirm-message')).toContainText('What it has copied so far is deleted');
  await page.locator('#themed-confirm-cancel-btn').click();
  expect(await invocations(page, 'cancel_movie_copy')).toHaveLength(0);
  await expect(page.locator('#movie-copy-cancel')).toBeEnabled();

  await page.locator('#movie-copy-cancel').click();
  await page.locator('#themed-confirm-ok-btn').click();
  expect(await invocations(page, 'cancel_movie_copy')).toHaveLength(1);
  await expect(page.locator('#movie-copy-cancel')).toBeDisabled();
  await expect(page.locator('#movie-copy-status')).toContainText('Stopping and deleting');

  await page.evaluate(() => window.__finishCopy({ state: 'cancelled' }));
  await expect(page.locator('#movie-copy-status')).toHaveText('Stopped. The half-made copy was deleted.');
  await expect(page.locator('#movie-copy-go')).toBeEnabled();
  await expect(page.locator('#movie-copy-close')).toBeEnabled();
  expect(await page.evaluate(() => window.__calls)).toEqual([]);
});

test('a failed copy says why and can be tried again', async ({ page }) => {
  await openDialog(page);
  await page.locator('#movie-copy-go').click();
  await page.evaluate(() => window.__finishCopy(new Error("couldn't write maps/x.bsp: disk full; what it had copied was removed")));
  await expect(page.locator('#movie-copy-status')).toHaveText("The copy didn't finish: couldn't write maps/x.bsp: disk full; what it had copied was removed.");
  await expect(page.locator('#movie-copy-go')).toBeEnabled();
});
