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

  test('Split sends the ticked maps and lists what was written', async ({ page }) => {
    await gotoHarness(page);
    await findDemos(page);
    const three = demo(page, 'three.dem');
    await expect(three.locator('tbody tr').nth(2).locator('.split-keep')).not.toBeChecked();
    await three.locator('.split-keep').nth(0).uncheck();
    await three.locator('.split-go').click();
    await expect(three.locator('.split-result')).toContainText('Wrote 1 demo:');
    await expect(three.locator('.split-result')).toContainText('three_part2.dem (20:00, 5.0 MB)');
    const call = await page.evaluate(() => window.__mockInvocations.find((c) => c.cmd === 'split_demo_maps').args);
    expect(call).toEqual({ path: 'C:\\demos\\sub\\three.dem', keep: [1] });
  });

  test('nothing ticked asks for a map instead of calling the backend', async ({ page }) => {
    await gotoHarness(page);
    await findDemos(page);
    const two = demo(page, 'two.dem');
    await expect(two.locator('.split-keep').nth(1)).not.toBeChecked();
    await two.locator('.split-keep').nth(0).uncheck();
    await two.locator('.split-go').click();
    await expect(two.locator('.split-result')).toHaveText('Tick at least one map.');
    expect(await page.evaluate(() => window.__mockInvocations.some((c) => c.cmd === 'split_demo_maps'))).toBe(false);
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
});
