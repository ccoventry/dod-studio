// The Master Queue's Group by match (#685) — driven against
// tests/e2e/match-groups.html. See tests/e2e/README.md.
//
// Pins that the queue keeps its own order until Group by match is ticked,
// then shows each match under a header line, first half first, and that
// the demos' match info never reaches the project file.
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

test('the queue keeps its own order until Group by match is ticked', async ({ page }) => {
  await gotoHarness(page);
  expect(await rowTexts(page)).toEqual(['h2_pov.dem', 'other.dem', 'h1_hltv.dem', 'h1_pov.dem']);
});

test('grouped, each match sits under its header line, first half first', async ({ page }) => {
  await gotoHarness(page);
  await page.check('#master-group-by-match-cb');
  await expect(page.locator('.queue-match-header')).toHaveCount(2);
  expect(await rowTexts(page)).toEqual([
    '# Match 1 · dod_anzio · dicE vs gskiLL · h1: 2 demos, h2: 1 demo',
    'h1_hltv.dem', 'h1_pov.dem', 'h2_pov.dem',
    '# Not in a match: these demos could not be read',
    'other.dem',
  ]);
  await expect(page.locator('.queue-match-header').first().locator('td')).toHaveAttribute('title', /What decided the half: chat/);
  await page.uncheck('#master-group-by-match-cb');
  expect(await rowTexts(page)).toEqual(['h2_pov.dem', 'other.dem', 'h1_hltv.dem', 'h1_pov.dem']);
});

test('ticking it reads the demos the grouping has not placed', async ({ page }) => {
  await gotoHarness(page);
  await page.check('#master-group-by-match-cb');
  const asked = await page.evaluate(() => window.__mockInvocations.filter((c) => c.cmd === 'queue_match_groups').map((c) => c.args.parseMissing));
  expect(asked).toEqual([false, true]);
});

test('the match info stays out of the saved project', async ({ page }) => {
  await gotoHarness(page);
  const saved = await page.evaluate(() => JSON.parse(JSON.stringify(window.__demos)));
  expect(saved.every((d) => !('match_info' in d))).toBe(true);
  expect(await page.evaluate(() => window.__demos[0].match_info.half)).toBe(2);
});
