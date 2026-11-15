// master_pane.js's missing-demo row (#21) — driven against
// tests/e2e/master-pane.html. See tests/e2e/README.md.
import { test, expect } from './fixtures.js';

async function gotoHarness(page) {
  await page.goto('/tests/e2e/master-pane.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

const row = (page, name) => page.locator('#master-demo-table-body tr', { hasText: name });

test.describe('master_pane missing demos', () => {
  test('a demo left as missing with a found copy offers it, with the path on hover', async ({ page }) => {
    await gotoHarness(page);
    const useBtn = row(page, 'moved.dem').locator('.use-found-copy-btn');
    await expect(useBtn).toHaveText('Use found copy');
    await expect(useBtn).toHaveAttribute('title', /C:\\demos\\sub\\moved\.dem/);
    await expect(row(page, 'moved.dem').locator('.locate-demo-btn')).toBeVisible();
  });

  test('clicking Use found copy hands that demo over without selecting the row', async ({ page }) => {
    await gotoHarness(page);
    await row(page, 'moved.dem').locator('.use-found-copy-btn').click();
    await expect(page.locator('#result')).toHaveText('use moved.dem');
  });

  test('a missing demo with nothing found, or a present one, has no Use found copy', async ({ page }) => {
    await gotoHarness(page);
    await expect(row(page, 'gone.dem').locator('.use-found-copy-btn')).toHaveCount(0);
    await expect(row(page, 'gone.dem').locator('.locate-demo-btn')).toBeVisible();
    await expect(row(page, 'here.dem').locator('.use-found-copy-btn')).toHaveCount(0);
    await expect(row(page, 'here.dem').locator('.locate-demo-btn')).toHaveCount(0);
  });
});

test.describe('master_pane multi-map demos (#217)', () => {
  test('a demo with two maps gets a Split button that names them on hover', async ({ page }) => {
    await gotoHarness(page);
    const split = row(page, 'here.dem').locator('.multimap-split-btn');
    await expect(split).toHaveText('2 maps · Split');
    await expect(split).toHaveAttribute('title', /recorded 2 maps \(dod_lennon2, dod_lennon2\)/);
  });

  test('clicking it hands the demo over without selecting the row', async ({ page }) => {
    await gotoHarness(page);
    await row(page, 'here.dem').locator('.multimap-split-btn').click();
    await expect(page.locator('#result')).toHaveText('split here.dem');
    await expect(row(page, 'here.dem')).not.toHaveClass(/table-row-selected/);
  });

  test('while it splits, the row shows a progress bar in place of the button', async ({ page }) => {
    await gotoHarness(page);
    const r = row(page, 'here.dem');
    await r.locator('.multimap-split-btn').click();
    await expect(r.locator('.multimap-split-progress .split-progress-text')).toHaveText('Reading the demo… 30%');
    await expect(r.locator('.multimap-split-btn')).toBeHidden();
    await page.evaluate(() => window.__finishRowSplit());
    await expect(r.locator('.multimap-split-progress')).toHaveCount(0);
    await expect(r.locator('.multimap-split-btn')).toBeVisible();
  });

  test('a demo whose maps were already split shows a quiet note naming the files, not the button', async ({ page }) => {
    await page.addInitScript(() => {
      window.__mockInvokeHandlers = {
        demo_split_status: () => ({ split: true, files: ['here_split1_lennon2.dem', 'here_split2_lennon2.dem'] }),
      };
    });
    await gotoHarness(page);
    const r = row(page, 'here.dem');
    const note = r.locator('.multimap-split-note');
    await expect(note).toHaveText('Already split');
    await expect(note).toHaveAttribute('title', /already split next to it: here_split1_lennon2\.dem, here_split2_lennon2\.dem/);
    await expect(r.locator('.multimap-split-btn')).toBeHidden();
    expect(await page.evaluate(() => window.__mockInvocations.find((c) => c.cmd === 'demo_split_status')?.args))
      .toEqual({ path: 'C:\\demos\\here.dem', maps: ['dod_lennon2', 'dod_lennon2'], minSeconds: 60 });
  });

  test('split pieces deleted later bring the Split button back on the next redraw', async ({ page }) => {
    await page.addInitScript(() => {
      window.__splitFiles = ['here_split1_lennon2.dem', 'here_split2_lennon2.dem'];
      window.__mockInvokeHandlers = {
        demo_split_status: () => ({ split: window.__splitFiles.length === 2, files: window.__splitFiles }),
      };
    });
    await gotoHarness(page);
    const r = row(page, 'here.dem');
    await expect(r.locator('.multimap-split-note')).toBeVisible();
    await page.evaluate(() => { window.__splitFiles = []; });
    // An answer is reused for RECHECK_AFTER_MS (3 s), then asked again.
    await page.waitForTimeout(3200);
    await page.evaluate(() => window.__rerender());
    await expect(row(page, 'here.dem').locator('.multimap-split-btn')).toBeVisible();
    await expect(row(page, 'here.dem').locator('.multimap-split-note')).toBeHidden();
  });

  test('a demo not split yet keeps the button, which says only the first map is used', async ({ page }) => {
    await page.addInitScript(() => {
      window.__mockInvokeHandlers = { demo_split_status: () => ({ split: false, files: [] }) };
    });
    await gotoHarness(page);
    const split = row(page, 'here.dem').locator('.multimap-split-btn');
    await expect(split).toBeVisible();
    await expect(split).toHaveAttribute('title', /use the first map only/);
    await expect(row(page, 'here.dem').locator('.multimap-split-note')).toBeHidden();
  });

  test('a one-map demo, or one saved before the map list existed, has none', async ({ page }) => {
    await gotoHarness(page);
    await expect(row(page, 'moved.dem').locator('.multimap-hint')).toHaveCount(0);
    await expect(row(page, 'gone.dem').locator('.multimap-hint')).toHaveCount(0);
  });
});

test.describe('master_pane Status and Review counts', () => {
  const cells = (page) => row(page, 'here.dem').locator('td.queue-counts-cell');

  test('each column is one cell of slash-separated counts, spelled out on hover', async ({ page }) => {
    await gotoHarness(page);
    await expect(cells(page)).toHaveText(['1 / 1 / 0', '2 / 1 / 0 / 1']);
    await expect(cells(page).nth(0)).toHaveAttribute('title', '1 Pending, 1 Captured, 0 Rendered');
    await expect(cells(page).nth(1)).toHaveAttribute('title', '2 not reviewed, 1 Keep, 0 Maybe, 1 Skip');
  });

  test('a count is in its colour token (the harness loads no stylesheet), and grey at zero', async ({ page }) => {
    await gotoHarness(page);
    const numbers = cells(page).nth(0).locator('span:not(.count-slash)');
    await expect(numbers.nth(0)).toHaveAttribute('style', /color: var\(--warn-orange\)/);
    await expect(numbers.nth(2)).toHaveAttribute('style', /color: var\(--text-dim\)/);
  });

  test('the headers carry the matching legend letters', async ({ page }) => {
    await gotoHarness(page);
    await expect(page.locator('#status-count-legend')).toHaveText('P / C / R');
    await expect(page.locator('#review-count-legend')).toHaveText('– / K / M / S');
  });
});

test.describe('master_pane search clear button (#529)', () => {
  const search = (page) => page.locator('#demo-search-input');
  const x = (page) => page.locator('.clearable-x');
  const rows = (page) => page.locator('#master-demo-table-body tr');

  test('the × shows only while the box has text', async ({ page }) => {
    await gotoHarness(page);
    await expect(x(page)).toBeHidden();
    await search(page).fill('moved');
    await expect(x(page)).toBeVisible();
    await search(page).fill('');
    await expect(x(page)).toBeHidden();
  });

  test('clicking it clears the text, re-filters and keeps focus in the box', async ({ page }) => {
    await gotoHarness(page);
    await search(page).fill('moved');
    await expect(rows(page)).toHaveCount(1);
    await x(page).click();
    await expect(search(page)).toHaveValue('');
    await expect(rows(page)).toHaveCount(3);
    await expect(search(page)).toBeFocused();
    await expect(x(page)).toBeHidden();
  });

  test('Esc in the box does the same', async ({ page }) => {
    await gotoHarness(page);
    await search(page).fill('gone');
    await expect(rows(page)).toHaveCount(1);
    await search(page).press('Escape');
    await expect(search(page)).toHaveValue('');
    await expect(rows(page)).toHaveCount(3);
  });
});

test.describe('master_pane totals row', () => {
  test('adds up each column under the header, for the demos listed', async ({ page }) => {
    await gotoHarness(page);
    const cells = page.locator('#master-totals-row td');
    // One cell per column of the real table: checkbox, Demo File, Highlights,
    // Selected, Status, Review, Actions.
    await expect(cells).toHaveCount(7);
    await expect(cells.nth(1)).toHaveText('Total: 3 demos');
    await expect(cells.nth(2)).toHaveText('4');
    await expect(cells.nth(3)).toHaveText('0');
    await expect(cells.nth(4)).toHaveText('1 / 1 / 0');
    await expect(cells.nth(4)).toHaveAttribute('title', '1 Pending, 1 Captured, 0 Rendered');
    await expect(cells.nth(5)).toHaveText('2 / 1 / 0 / 1');
  });

  test('follows the search: totals of what is listed, out of the queue', async ({ page }) => {
    await gotoHarness(page);
    await page.fill('#demo-search-input', 'here');
    await page.evaluate(() => document.querySelector('#demo-search-input').dispatchEvent(new Event('input')));
    await expect(page.locator('#master-totals-row td').nth(1)).toHaveText('Total: 1 of 3 demos');
  });
});

