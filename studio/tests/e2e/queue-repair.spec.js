// queue_repair.js — a Master Queue demo damaged partway (#15) gets a
// "Damaged · Repair" button; clicking it repairs the demo without selecting
// the row, with a progress bar in its place until it ends.
import { test, expect } from '@playwright/test';

const row = (page, name) => page.locator('#master-demo-table-body tr', { hasText: name });

test.beforeEach(async ({ page }) => {
  await page.goto('/tests/e2e/queue-repair.html');
  await page.waitForFunction(() => window.__harnessReady === true);
});

test('only a damaged demo gets the button, which says where it breaks on hover', async ({ page }) => {
  const btn = row(page, 'broken.dem').locator('.damaged-repair-btn');
  await expect(btn).toHaveText('Damaged · Repair');
  await expect(btn).toHaveAttribute('title', /damaged at 8:27 on the playback clock/);
  await expect(row(page, 'fine.dem').locator('.damaged-repair-btn')).toHaveCount(0);
});

test('Repair runs without selecting the row, and shows its bar until it ends', async ({ page }) => {
  const r = row(page, 'broken.dem');
  await r.locator('.damaged-repair-btn').click();
  await expect(page.locator('#result')).toHaveText('repair broken.dem');
  await expect(r.locator('.repair-progress .split-progress-text')).toHaveText('Reading the recording… 40%');
  await expect(r.locator('.damaged-repair-btn')).toBeHidden();
  await page.evaluate(() => window.__finishRowRepair());
  await expect(r.locator('.repair-progress')).toHaveCount(0);
  await expect(r.locator('.damaged-repair-btn')).toBeVisible();
});

test('a damaged demo with more than one map greys out Split and offers Repair & Split', async ({ page }) => {
  const r = row(page, 'both.dem');
  const split = r.locator('.multimap-split-btn');
  await expect(split).toBeDisabled();
  await expect(split).toHaveAttribute('title', 'Repair it first: the damage stops a split. Repair & Split does both.');
  await expect(r.locator('.damaged-repair-btn')).toHaveText(['Damaged · Repair', 'Repair & Split']);
  await r.locator('.damaged-repair-btn', { hasText: 'Repair & Split' }).click();
  await expect(page.locator('#result')).toHaveText('repair-split both.dem');
  await expect(r.locator('.repair-progress .split-progress-text')).toHaveText('Splitting the repaired demo: Writing dod_anzio… 20%');
  await expect(r.locator('.damaged-repair-btn')).toHaveCount(2);
  await expect(r.locator('.damaged-repair-btn').first()).toBeHidden();
  await page.evaluate(() => window.__finishRowRepair());
  await expect(r.locator('.repair-progress')).toHaveCount(0);
});

test('once the repair was split, the original offers neither Repair nor Split', async ({ page }) => {
  await page.evaluate(() => {
    window.__splitStatus = { split: true, files: ['both_repaired_split1_anzio.dem', 'both_repaired_split2_pandemic_aim.dem'] };
  });
  // A redraw asks again; the harness draws once, so draw it again.
  await page.evaluate(async () => {
    const { renderMasterList } = await import('/src/master_pane.js');
    renderMasterList([{ name: 'both.dem', path: 'C:/demos/again/both.dem', streaks: [], signon_maps: ['dod_anzio', 'dod_pandemic_aim'],
      damage: { read_up_to_secs: 1480.2, frame_index: 1, byte_offset: 1, bytes_unread: 1, reason: 'x' } }], null, () => {});
  });
  const r = row(page, 'both.dem');
  await expect(r.locator('.damaged-repair-hint')).toBeHidden();
  await expect(r.locator('.multimap-split-btn')).toBeHidden();
  await expect(r.locator('.multimap-split-note')).toBeVisible();
});

test('a demo damaged where the repair finds no break offers no Repair and leaves its Split on', async ({ page }) => {
  const r = row(page, 'inside.dem');
  await expect(r.locator('.damaged-repair-hint')).toBeHidden();
  const split = r.locator('.multimap-split-btn');
  await expect(split).toBeEnabled();
  await expect(split).not.toHaveAttribute('title', /Repair it first/);
  await split.click();
  await expect(page.locator('#result')).toHaveText('split');
  // A demo the repair can fix keeps both, on the same queue.
  await expect(row(page, 'both.dem').locator('.multimap-split-btn')).toBeDisabled();
  await expect(row(page, 'both.dem').locator('.damaged-repair-btn').first()).toBeVisible();
});

test('a Repair that finds nothing to cut out takes Repair away and gives Split back at once', async ({ page }) => {
  await page.goto('/tests/e2e/queue-repair.html?real');
  await page.waitForFunction(() => window.__harnessReady === true);
  // Damaged, the repair's own check said yes (a file changed since, say),
  // and the repair itself then finds the frames whole.
  await page.evaluate(() => { window.__repairResult = { verdict: 'not_damaged', holes: [], reasons: [] }; });
  const r = row(page, 'both.dem');
  await expect(r.locator('.multimap-split-btn')).toBeDisabled();
  await r.locator('.damaged-repair-btn', { hasText: 'Damaged · Repair' }).click();
  await expect(page.locator('#toast-container')).toContainText('Not damaged');
  await expect(r.locator('.damaged-repair-hint')).toBeHidden();
  await expect(r.locator('.multimap-split-btn')).toBeEnabled();
});
