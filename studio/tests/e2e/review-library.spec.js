// review_library.js (#848) — driven against tests/e2e/review-library.html.
// See tests/e2e/README.md for what this style of test covers.
import { test, expect } from '@playwright/test';

const KEY = '4096:00ff';
const KILLS = [[100, 1, 'K98'], [200, 3, 'K98'], [300, 5, 'BAR']];
const highlight = (extra = {}) => ({ player_index: 3, target_player: 'm00cat', source_key: KEY, kills: KILLS, ...extra });

async function gotoHarness(page) {
  await page.goto('/tests/e2e/review-library.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

/** Marks saved earlier, as a review with no project left them. */
async function saveEarlierMark(page) {
  await page.evaluate((h) => window.__addDemo({ path: 'C:/demos/first.dem', streaks: [h] }), highlight());
  await page.evaluate(async () => {
    window.__demos[0].streaks[0].curation = 'Keep';
    window.__demos[0].streaks[0].notes = 'the triple';
    window.__sync();
    await window.__flush();
  });
  await page.evaluate(() => window.__demos.splice(0));
}

test.describe('review_library', () => {
  test('a mark made with no project is saved under the demo contents', async ({ page }) => {
    await gotoHarness(page);
    await saveEarlierMark(page);
    const lib = await page.evaluate(() => window.__library);
    expect(lib[KEY].name).toBe('first.dem');
    expect(lib[KEY].highlights['3#100#300']).toMatchObject({ curation: 'Keep', notes: 'the triple' });
  });

  test('with no project, a copy a scan adds gets the marks without a question', async ({ page }) => {
    await gotoHarness(page);
    await saveEarlierMark(page);
    await page.evaluate((h) => window.__addDemo({ path: 'E:/elsewhere/renamed.dem', streaks: [h] }), highlight());
    await expect(page.locator('#themed-confirm-modal')).toBeHidden();
    const mark = await page.evaluate(() => window.__demos[0].streaks[0].curation);
    expect(mark).toBe('Keep');
    await expect(page.locator('#toast-container')).toContainText('Brought in your saved Review marks: 1 highlight in 1 demo.');
  });

  test('in an open project it asks, names the demo, and brings them in on yes', async ({ page }) => {
    await gotoHarness(page);
    await saveEarlierMark(page);
    await page.evaluate((h) => { window.__project = true; window.__addDemo({ path: 'C:/project/match.dem', streaks: [h] }); }, highlight());
    await expect(page.locator('#themed-confirm-modal')).toBeVisible();
    await expect(page.locator('#themed-confirm-message')).toHaveText(
      '1 of these demos has Review marks you saved before (1 highlight). Bring them in?',
    );
    await expect(page.locator('#themed-confirm-details')).toContainText('match.dem');
    await page.locator('#themed-confirm-ok-btn').click();
    await expect.poll(() => page.evaluate(() => window.__demos[0].streaks[0].notes)).toBe('the triple');
  });

  test('Not now leaves the project as it was', async ({ page }) => {
    await gotoHarness(page);
    await saveEarlierMark(page);
    await page.evaluate((h) => { window.__project = true; window.__addDemo({ path: 'C:/project/match.dem', streaks: [h] }); }, highlight());
    await page.locator('#themed-confirm-cancel-btn').click();
    await expect(page.locator('#themed-confirm-modal')).toBeHidden();
    expect(await page.evaluate(() => window.__demos[0].streaks[0].curation)).toBeUndefined();
    expect(await page.evaluate(() => window.__brought.length)).toBe(0);
  });
});
