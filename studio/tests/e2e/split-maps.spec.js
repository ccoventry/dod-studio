// The Demo Auditor's tabs and Split Maps tab (#624) -- driven against
// tests/e2e/split-maps.html. See tests/e2e/README.md.
import { test, expect } from '@playwright/test';

async function gotoHarness(page) {
  await page.goto('/tests/e2e/split-maps.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

const demo = (page, name) => page.locator('.split-demo', { hasText: name });

async function findDemos(page) {
  await page.locator('#audit-target-folder-input').fill('C:\\demos');
  await page.locator('.auditor-tab-btn[data-auditor-tab="split"]').click();
  await page.locator('#split-find-btn').click();
  await expect(page.locator('#split-status')).toHaveText('2 demos with more than one map.');
}

test.describe('Demo Auditor tabs', () => {
  test('one panel shows at a time', async ({ page }) => {
    await gotoHarness(page);
    await expect(page.locator('#dup-content')).toBeVisible();
    await expect(page.locator('#split-find-btn')).toBeHidden();
    await page.locator('.auditor-tab-btn[data-auditor-tab="split"]').click();
    await expect(page.locator('#dup-content')).toBeHidden();
    await expect(page.locator('#split-find-btn')).toBeVisible();
    await expect(page.locator('.auditor-tab-btn[data-auditor-tab="split"]')).toHaveClass(/active/);
  });
});

test.describe('Split Maps', () => {
  test('finding needs a folder', async ({ page }) => {
    await gotoHarness(page);
    await page.locator('.auditor-tab-btn[data-auditor-tab="split"]').click();
    await expect(page.locator('#split-find-btn')).toBeDisabled();
    await page.locator('#audit-target-folder-input').fill('C:\\demos');
    await expect(page.locator('#split-find-btn')).toBeEnabled();
  });

  test('lists each multi-map demo with its maps and how long each is', async ({ page }) => {
    await gotoHarness(page);
    await findDemos(page);
    const three = demo(page, 'three.dem');
    await expect(three.locator('tbody tr')).toHaveCount(3);
    await expect(three.locator('tbody tr').nth(1)).toContainText('dod_avalanche');
    // Only the length: the start is no clock viewdemo ever shows.
    await expect(three.locator('tbody tr').nth(1).locator('.split-map-when')).toHaveText('20:00');
    const args = await page.evaluate(() => window.__mockInvocations.find((c) => c.cmd === 'find_multi_map_demos_cmd').args);
    expect(args).toEqual({ folder: 'C:\\demos', recursive: true });
  });

  test('a map under a minute starts unticked; the rest stay ticked', async ({ page }) => {
    await gotoHarness(page);
    await findDemos(page);
    const three = demo(page, 'three.dem');
    await expect(three.locator('tbody tr').nth(2).locator('.split-map-when')).toHaveText('0:20');
    await expect(three.locator('.split-keep').nth(0)).toBeChecked();
    await expect(three.locator('.split-keep').nth(1)).toBeChecked();
    await expect(three.locator('.split-keep').nth(2)).not.toBeChecked();
  });

  test('a map a split already wrote says so and starts unticked; the footer counts it', async ({ page }) => {
    await gotoHarness(page);
    await findDemos(page);
    const two = demo(page, 'two.dem');
    await expect(two.locator('.split-done-chip')).toHaveText('Already split');
    await expect(two.locator('tbody tr').nth(0).locator('.split-map-done')).toHaveText('two_dod_lennon2_1.dem');
    await expect(two.locator('.split-keep').nth(0)).not.toBeChecked();
    await expect(demo(page, 'three.dem').locator('.split-done-chip')).toHaveCount(0);
    await expect(page.locator('#footer-audit-summary')).toHaveText('Multi-map demos: 2 | Already split: 1');
    // Splitting three.dem marks it split too.
    const three = demo(page, 'three.dem');
    await three.locator('.split-go').click();
    await expect(three.locator('.split-result')).toContainText('Wrote 2 demos');
    await expect(three.locator('tbody tr').nth(0).locator('.split-map-done')).toHaveText('three_part1.dem');
    await expect(page.locator('#footer-audit-summary')).toHaveText('Multi-map demos: 2 | Already split: 2');
  });

  test('Split sends the ticked maps and shows what was written in their rows', async ({ page }) => {
    await gotoHarness(page);
    await findDemos(page);
    const three = demo(page, 'three.dem');
    await expect(three.locator('tbody tr').nth(2).locator('.split-keep')).not.toBeChecked();
    await three.locator('.split-keep').nth(0).uncheck();
    await three.locator('.split-go').click();
    await expect(three.locator('.split-result')).toHaveText('Wrote 1 demo.');
    const row = three.locator('tbody tr').nth(1);
    await expect(row.locator('.split-map-status')).toHaveText('Finished');
    await expect(row.locator('.split-map-done')).toHaveText('three_part2.dem');
    await expect(row.locator('.split-map-size')).toHaveText('5.0 MB');
    // No list under the button: Show in folder is in the map's row.
    await expect(three.locator('.split-result .split-reveal')).toHaveCount(0);
    await row.locator('.split-reveal').click();
    const reveal = await page.evaluate(() => window.__mockInvocations.find((c) => c.cmd === 'reveal_in_explorer').args);
    expect(reveal).toEqual({ path: 'C:\\demos\\sub\\three_part2.dem' });
    // An unticked map's row stays as it was.
    await expect(three.locator('tbody tr').nth(0).locator('.split-map-status')).toHaveText('');
    const call = await page.evaluate(() => window.__mockInvocations.find((c) => c.cmd === 'split_demo_maps').args);
    expect(call).toEqual({ path: 'C:\\demos\\sub\\three.dem', keep: [1] });
  });

  test('a map split earlier has Show in folder for its file', async ({ page }) => {
    await gotoHarness(page);
    await findDemos(page);
    const two = demo(page, 'two.dem');
    await two.locator('tbody tr').nth(0).locator('.split-reveal').click();
    const reveal = await page.evaluate(() => window.__mockInvocations.find((c) => c.cmd === 'reveal_in_explorer').args);
    expect(reveal).toEqual({ path: 'C:\\demos\\two_dod_lennon2_1.dem' });
    await expect(two.locator('tbody tr').nth(1).locator('.split-reveal')).toHaveCount(0);
  });

  test('each ticked map shows its own status while the split runs', async ({ page }) => {
    await gotoHarness(page);
    await findDemos(page);
    await page.evaluate(() => { window.__holdSplit = true; });
    const three = demo(page, 'three.dem');
    const status = (k) => three.locator('tbody tr').nth(k).locator('.split-map-status');
    const emit = (p) => page.evaluate((p) => window.__mockEmit('split_progress', {
      path: 'C:\\demos\\sub\\three.dem',
      progress: { fraction: 0.5, map: 'm', part: 1, parts: 2, done: 0, file: null, error: null, ...p },
    }), p);
    await three.locator('.split-go').click();
    // Shows it's working the moment it's clicked.
    await expect(status(0)).toHaveText('Waiting');
    await expect(status(1)).toHaveText('Waiting');
    await expect(status(2)).toHaveText('');
    await emit({ stage: 'reading', index: null, map_fraction: 0.4, fraction: 0.16 });
    await expect(status(0)).toHaveText('Reading the demo… 40%');
    await expect(status(1)).toHaveText('Reading the demo… 40%');
    await expect(three.locator('.split-progress-text')).toHaveText('Reading the demo… 16%');
    await emit({ stage: 'writing', index: 1, map_fraction: 0 });
    await emit({ stage: 'checking', index: 0, map_fraction: 0.75 });
    await expect(status(0)).toHaveText('Checking… 50%');
    await expect(status(1)).toHaveText('Writing…');
    await expect(three.locator('tbody tr').nth(0).locator('.split-map-bar .progress-bar-fill')).toHaveAttribute('style', /width: 75%/);
    await emit({ stage: 'finished', index: 0, map_fraction: 1, done: 1, fraction: 0.7,
      file: { index: 0, path: 'C:\\demos\\sub\\three_part1.dem', map: 'm', seconds: 300, size_bytes: 2e6 } });
    await expect(status(0)).toHaveText('Finished');
    await expect(three.locator('tbody tr').nth(0).locator('.split-map-size')).toHaveText('2.0 MB');
    await expect(three.locator('tbody tr').nth(0).locator('.split-reveal')).toBeVisible();
    await expect(status(1)).toHaveText('Writing…');
    await expect(three.locator('.split-progress-text')).toHaveText('Writing and checking 2 maps: 1 done… 70%');
    await page.evaluate(() => window.__releaseSplit('failed'));
    await expect(status(1)).toHaveText('Failed: map 2 (m) read back as 2 map(s)');
    await expect(three.locator('.split-result')).toHaveText('Wrote 1 demo, 1 failed.');
  });

  test('Split is greyed out while no map is ticked', async ({ page }) => {
    await gotoHarness(page);
    await findDemos(page);
    const three = demo(page, 'three.dem');
    await expect(three.locator('.split-go')).toBeEnabled();
    await three.locator('.split-keep').nth(0).uncheck();
    await three.locator('.split-keep').nth(1).uncheck();
    await expect(three.locator('.split-go')).toBeDisabled();
    await expect(three.locator('.split-go')).toHaveAttribute('title', 'Tick at least one map to split.');
    await three.locator('.split-keep').nth(2).check();
    await expect(three.locator('.split-go')).toBeEnabled();
    // two.dem: map 1 already split and map 2 short, so nothing starts ticked.
    await expect(demo(page, 'two.dem').locator('.split-go')).toBeDisabled();
  });

  test('Find waits while a split runs, so the list keeps its progress and result', async ({ page }) => {
    await gotoHarness(page);
    await findDemos(page);
    await page.evaluate(() => { window.__holdSplit = true; });
    const three = demo(page, 'three.dem');
    await three.locator('.split-go').click();
    await expect(page.locator('#split-find-btn')).toBeDisabled();
    await page.evaluate(() => window.__releaseSplit());
    await expect(three.locator('.split-result')).toContainText('Wrote');
    await expect(page.locator('#split-find-btn')).toBeEnabled();
  });

  test('a running split locks its ticks and has its own Cancel', async ({ page }) => {
    await gotoHarness(page);
    await findDemos(page);
    await page.evaluate(() => { window.__holdSplit = true; });
    const three = demo(page, 'three.dem');
    await expect(three.locator('.split-cancel')).toBeHidden();
    await three.locator('.split-go').click();
    await expect(three.locator('.split-keep').nth(0)).toBeDisabled();
    await expect(three.locator('.split-cancel')).toBeVisible();
    await three.locator('.split-cancel').click();
    await expect(three.locator('.split-cancel')).toBeDisabled();
    await expect(three.locator('.split-cancel-note')).toHaveText('Cancelling after this step…');
    const args = await page.evaluate(() => window.__mockInvocations.find((c) => c.cmd === 'cancel_demo_split').args);
    expect(args).toEqual({ path: 'C:\\demos\\sub\\three.dem' });
    await page.evaluate(() => window.__releaseSplit('cancelled'));
    await expect(three.locator('.split-result')).toHaveText('Wrote 1 demo, 1 cancelled.');
    await expect(three.locator('tbody tr').nth(0).locator('.split-map-status')).toHaveText('Finished');
    await expect(three.locator('tbody tr').nth(1).locator('.split-map-status')).toHaveText('Cancelled');
    await expect(three.locator('.split-cancel')).toBeHidden();
    await expect(three.locator('.split-keep').nth(0)).toBeEnabled();
  });

  async function findHeld(page) {
    await gotoHarness(page);
    await page.evaluate(() => { window.__holdSegments = true; });
    await page.locator('#audit-target-folder-input').fill('C:\\demos');
    await page.locator('.auditor-tab-btn[data-auditor-tab="split"]').click();
    await page.locator('#split-find-btn').click();
  }

  test('a demo is listed once its lengths are read, ready to use; the bar says which step', async ({ page }) => {
    await findHeld(page);
    await expect(page.locator('#split-status')).toHaveText('Reading map lengths: 1 of 2 demos (two.dem)');
    await expect(page.locator('#split-progress')).not.toHaveAttribute('hidden');
    await expect(page.locator('.split-demo')).toHaveCount(0);
    await expect(page.locator('#split-find-btn')).toBeDisabled();
    await page.evaluate(() => window.__releaseSegments());
    const two = demo(page, 'two.dem');
    // Ready as it shows: ticks final (nothing to split on this one yet).
    await expect(two.locator('.split-keep').nth(0)).toBeEnabled();
    await expect(two.locator('.split-keep').nth(1)).not.toBeChecked();
    await expect(two.locator('.split-map-when').nth(0)).toHaveText('21:28');
    await expect(page.locator('#split-status')).toHaveText('Reading map lengths: 2 of 2 demos (three.dem)');
    await page.evaluate(() => window.__releaseSegments());
    await expect(page.locator('#split-status')).toHaveText('2 demos with more than one map.');
    await expect(page.locator('#split-progress')).toHaveAttribute('hidden');
    await expect(page.locator('#split-find-btn')).toBeEnabled();
  });

  test('Cancel while lengths are read keeps the demos already listed', async ({ page }) => {
    await findHeld(page);
    await expect(page.locator('#split-status')).toHaveText('Reading map lengths: 1 of 2 demos (two.dem)');
    await page.locator('#split-cancel-btn').click();
    await page.evaluate(() => window.__releaseSegments());
    await expect(page.locator('#split-status')).toHaveText('Cancelled: 1 of 2 demos with more than one map listed.');
    await expect(page.locator('.split-demo')).toHaveCount(1);
    expect(await page.evaluate(() => window.__mockInvocations.some((c) => c.cmd === 'cancel_multi_map_scan'))).toBe(false);
  });
});
