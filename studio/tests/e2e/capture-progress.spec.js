// The capture batch's footer bar (#952), driven against
// tests/e2e/capture-precheck.html (initCaptureUI with the Capture footer):
// one bar for the whole batch that never goes backwards across its steps,
// a sliding block over a step it can't measure, plain step lines, and
// "Done in" at the end.
import { test, expect } from '@playwright/test';

async function loadHarness(page) {
  await page.addInitScript(() => {
    window.__mockInvokeHandlers = {
      calculate_export_pool_space: () => 500e9,
      diagnose_capture_output_paths: () => [],
    };
  });
  await page.goto('/tests/e2e/capture-precheck.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

const emit = (page, payload) => page.evaluate((p) => window.__mockEmit('capture_status', p), payload);
const fillPct = (page) => page.evaluate(() => parseFloat(document.querySelector('#capture-progress-bar').style.width) || 0);
const slide = (page) => page.locator('#capture-progress-container .progress-bar-step');

const STEPS = [
  { stage: 'preparing', index: 0, total: 0, measured: false, status: 'Getting demos ready…' },
  { stage: 'reading_again', index: 0, total: 1, status: 'Reading anzio.dem again for its clip times (1 of 1)' },
  { stage: 'preparing', index: 0.4, total: 2, measured: true, status: 'Getting demos ready: 1 of 2 (anzio.dem, 40%)' },
  { stage: 'preparing', index: 2, total: 2, measured: true, status: 'Getting demos ready: 2 of 2' },
  { stage: 'starting', index: 0, total: 3, status: 'Starting the game for anzio.dem (demo 1 of 2)' },
  { stage: 'loading', index: 0, total: 3, status: 'Loading anzio.dem…' },
  { stage: 'capturing', index: 0, total: 3, status: 'Capturing clip 1 of 3 (anzio.dem)' },
  { stage: 'capturing', index: 1, total: 3, status: 'Capturing clip 2 of 3 (anzio.dem)', seconds_left: 30 },
  { stage: 'loading', index: 2, total: 3, status: 'Loading avalanche.dem…' },
  { stage: 'capturing', index: 2, total: 3, status: 'Capturing clip 3 of 3 (avalanche.dem)', seconds_left: 10 },
  { stage: 'checking', index: 0, total: 3, status: 'Checking the takes: 0 of 3' },
  { stage: 'checking', index: 3, total: 3, status: 'Checking the takes: 3 of 3' },
];

test('one bar for the whole batch: it never goes backwards across the steps', async ({ page }) => {
  await loadHarness(page);
  let shown = 0;
  for (const step of STEPS) {
    await emit(page, { running: true, ...step });
    await expect(page.locator('#batch-status')).toContainText(step.status);
    const pct = await fillPct(page);
    expect(pct, step.status).toBeGreaterThanOrEqual(shown);
    shown = pct;
  }
  expect(shown).toBe(100);
  await emit(page, { running: false, status: 'Complete', clips_captured: 3, clips_total: 3 });
  await expect(page.locator('#batch-status')).toHaveText(/^Done in \d+(\.\d)? s: 3 clips captured$/);
  expect(await fillPct(page)).toBe(100);
  await expect(slide(page)).toHaveCount(0);
});

test('a step it can\'t measure keeps the fill and slides a block over it', async ({ page }) => {
  await loadHarness(page);
  await emit(page, { running: true, stage: 'capturing', index: 1, total: 4, status: 'Capturing clip 2 of 4 (anzio.dem)' });
  await expect(slide(page)).toHaveCount(1);
  const fill = await fillPct(page);
  expect(fill).toBeCloseTo(32, 5);
  // The block covers the clip being recorded, from the fill on.
  const left = await slide(page).evaluate((el) => parseFloat(el.style.left));
  expect(left).toBeCloseTo(fill, 5);
  // The clock runs on the line.
  await expect(page.locator('#batch-status')).toHaveText(/Capturing clip 2 of 4 \(anzio\.dem\) · 0:0\d/);
  // Nothing measured inside the check of the takes' count: no block.
  await emit(page, { running: true, stage: 'checking', index: 2, total: 4, status: 'Checking the takes: 2 of 4' });
  await expect(slide(page)).toHaveCount(0);
});

test('a cancelled batch says so and empties the bar', async ({ page }) => {
  await loadHarness(page);
  await emit(page, { running: true, stage: 'capturing', index: 1, total: 4, status: 'Capturing clip 2 of 4' });
  await emit(page, { running: false, status: 'Cancelled', clips_captured: 1, clips_total: 4 });
  await expect(page.locator('#batch-status')).toHaveText('Cancelled');
  expect(await fillPct(page)).toBe(0);
  await expect(slide(page)).toHaveCount(0);
});
