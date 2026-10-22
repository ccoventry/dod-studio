// The Master Queue's Group by dropdown (#685, #823) — driven against
// tests/e2e/match-groups.html. See tests/e2e/README.md.
//
// Pins that the queue keeps its own order until a grouping is picked, then
// shows each group under a header line (a match first half first), that a
// grouping never changes which demos count as shown, that the choice is
// remembered, and that the demos' match info never reaches the project file.
import { test, expect } from '@playwright/test';

// native::match_groups::Grouping, as serde sends it.
const D = (name) => `C:\\demos\\${name}`;
const GROUPING = {
  matches: [
    { number: 1, map: 'dod_anzio', tags: ['dicE', 'gskiLL'], halves: [
      { half: 1, demos: [D('h1_hltv.dem'), D('h1_pov.dem')], reason: 'EndOfHalfChat', detail: 'chat: gh 3, gg 0' },
      { half: 2, demos: [D('h2_pov.dem')], reason: 'EndOfHalfChat', detail: 'chat: gh 0, gg 2' },
    ] },
  ],
  demos: [
    { path: D('h2_pov.dem'), match_number: 1, half: 2, half_reason: 'EndOfHalfChat', half_detail: 'chat: gh 0, gg 2' },
    { path: D('h1_hltv.dem'), match_number: 1, half: 1, half_reason: 'EndOfHalfChat', half_detail: 'chat: gh 3, gg 0' },
    { path: D('h1_pov.dem'), match_number: 1, half: 1, half_reason: 'EndOfHalfChat', half_detail: 'chat: gh 3, gg 0' },
  ],
};
const QUEUE_ORDER = ['h2_pov.dem', 'other.dem', 'h1_hltv.dem', 'h1_pov.dem'];

async function gotoHarness(page) {
  await page.addInitScript((grouping) => {
    window.__mockInvokeHandlers = window.__mockInvokeHandlers || {};
    window.__mockInvokeHandlers.queue_match_groups = () => grouping;
  }, GROUPING);
  await page.goto('/tests/e2e/match-groups.html');
  await page.waitForFunction(() => window.__harnessReady === true);
  await page.evaluate(() => window.__regroup());
}

const rowTexts = (page) => page.locator('#master-demo-table-body tr').evaluateAll(
  (rows) => rows.map((r) => (r.classList.contains('queue-match-header') ? `# ${r.textContent}` : r.querySelector('td:nth-child(2)')?.textContent)),
);

test('the queue keeps its own order until a grouping is picked', async ({ page }) => {
  await gotoHarness(page);
  await expect(page.locator('#master-group-by-select')).toHaveValue('none');
  expect(await rowTexts(page)).toEqual(QUEUE_ORDER);
});

test('grouped by match, each match sits under its header line, first half first', async ({ page }) => {
  await gotoHarness(page);
  await page.selectOption('#master-group-by-select', 'match');
  await expect(page.locator('.queue-match-header')).toHaveCount(2);
  expect(await rowTexts(page)).toEqual([
    '# Match 1 · dod_anzio · dicE vs gskiLL · h1: 2 demos, h2: 1 demo',
    'h1_hltv.dem', 'h1_pov.dem', 'h2_pov.dem',
    '# Not in a match: these demos could not be read',
    'other.dem',
  ]);
  await expect(page.locator('.queue-match-header').first().locator('td')).toHaveAttribute('title', /What decided the half: chat/);
  await page.selectOption('#master-group-by-select', 'none');
  expect(await rowTexts(page)).toEqual(QUEUE_ORDER);
});

test('picking Match reads the demos the grouping has not placed', async ({ page }) => {
  await gotoHarness(page);
  await page.selectOption('#master-group-by-select', 'match');
  const asked = await page.evaluate(() => window.__mockInvocations.filter((c) => c.cmd === 'queue_match_groups').map((c) => c.args.parseMissing));
  expect(asked).toEqual([false, true]);
});

test('grouped by player, map or demo type, each header carries its counts', async ({ page }) => {
  await gotoHarness(page);
  await page.selectOption('#master-group-by-select', 'player');
  expect(await rowTexts(page)).toEqual([
    '# Brain · 1 demo, 2 highlights', 'h2_pov.dem',
    '# m00cat · 2 demos, 1 highlight', 'other.dem', 'h1_pov.dem',
    '# HLTV demos · 1 demo, 3 highlights', 'h1_hltv.dem',
  ]);
  await page.selectOption('#master-group-by-select', 'map');
  expect(await rowTexts(page)).toEqual([
    '# dod_anzio · 3 demos, 5 highlights', 'h2_pov.dem', 'h1_hltv.dem', 'h1_pov.dem',
    '# dod_avalanche · 1 demo, 1 highlight', 'other.dem',
  ]);
  await page.selectOption('#master-group-by-select', 'type');
  expect(await rowTexts(page)).toEqual([
    '# POV demos · 3 demos, 3 highlights', 'h2_pov.dem', 'other.dem', 'h1_pov.dem',
    '# HLTV demos · 1 demo, 3 highlights', 'h1_hltv.dem',
  ]);
});

test('a grouping never changes which demos count as shown', async ({ page }) => {
  await gotoHarness(page);
  await page.fill('#demo-search-input', 'pov');
  await page.dispatchEvent('#demo-search-input', 'input');
  const before = await page.evaluate(() => window.__visible());
  for (const by of ['match', 'player', 'map', 'folder', 'date', 'type']) {
    await page.selectOption('#master-group-by-select', by);
    expect(await page.evaluate(() => window.__visible())).toEqual(before);
    const shownRows = await page.locator('#master-demo-table-body tr:not(.queue-match-header)').count();
    expect(shownRows).toBe(before.length);
  }
});

test('the choice is remembered', async ({ page }) => {
  await gotoHarness(page);
  await page.selectOption('#master-group-by-select', 'map');
  await page.reload();
  await page.waitForFunction(() => window.__harnessReady === true);
  await expect(page.locator('#master-group-by-select')).toHaveValue('map');
  await expect(page.locator('.queue-match-header')).toHaveCount(2);
});

test('remembered as Match, a regroup reads the uncached demos too', async ({ page }) => {
  await page.addInitScript(() => {
    try { localStorage.setItem('dodstudio.queueGroupBy', 'match'); } catch { /* storage unavailable */ }
  });
  await gotoHarness(page);
  const asked = await page.evaluate(() => window.__mockInvocations.filter((c) => c.cmd === 'queue_match_groups').map((c) => c.args.parseMissing));
  expect(asked).toEqual([true]);
});

test('the match info stays out of the saved project', async ({ page }) => {
  await gotoHarness(page);
  const saved = await page.evaluate(() => JSON.parse(JSON.stringify(window.__demos)));
  expect(saved.every((d) => !('match_info' in d))).toBe(true);
  expect(await page.evaluate(() => window.__demos[0].match_info.half)).toBe(2);
});
