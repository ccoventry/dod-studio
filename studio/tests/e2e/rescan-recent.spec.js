// Rescan Recent Sessions (#938), driven against tests/e2e/rescan-recent.html.
import { test, expect } from '@playwright/test';

const old = (path) => ({ path, name: path.split('/').pop(), file_key: '1-abc', streaks: [] });
const project = (...demos) => JSON.stringify({ version: '0.12.0', demos });

async function gotoHarness(page, { files, recent, toParse = 1 }) {
  await page.goto('/tests/e2e/rescan-recent.html');
  await page.waitForFunction(() => window.__harnessReady === true);
  await page.evaluate(({ f, r, t }) => {
    window.__files = f;
    window.__recent = r;
    window.__toParse = t;
  }, { f: files, r: recent, t: toParse });
}

const TWO = {
  files: {
    'D:/p/wsod25.json': project(old('D:/d/1.dem'), old('D:/d/2.dem')),
    'D:/p/monday.json': project(old('D:/d/3.dem')),
  },
  recent: ['D:/p/wsod25.json', 'D:/p/gone.json', 'D:/p/monday.json'],
};

async function openFromMenu(page) {
  await page.locator('#file-menu-btn').click();
  await page.locator('#rescan-recent-btn').click();
  await expect(page.locator('#rescan-recent-modal')).toBeVisible();
}

const files = (page) => page.evaluate(() => window.__files);
const rows = (page) => page.locator('.rescan-recent-row');

test('lists each recent session with its outdated demos and time, and a missing file as skipped', async ({ page }) => {
  await gotoHarness(page, TWO);
  await openFromMenu(page);
  await expect(rows(page)).toHaveCount(3);
  await expect(rows(page).nth(0)).toContainText('wsod25.json');
  await expect(rows(page).nth(0)).toContainText('2 demos · About 6 s: 2 need a full read.');
  await expect(rows(page).nth(1)).toContainText('gone.json');
  await expect(rows(page).nth(1)).toContainText('file not found, skipped');
  await expect(rows(page).nth(2)).toContainText('1 demo · About 3 s');
  await expect(page.locator('#rescan-recent-total')).toHaveText('Total: about 9 s.');
  // A short run has no per-session skip.
  await expect(page.locator('.rescan-recent-skip')).toHaveCount(0);

  await page.locator('#rescan-recent-cancel-btn').click();
  await expect(page.locator('#rescan-recent-modal')).toBeHidden();
  expect(Object.keys(await files(page))).toEqual(['D:/p/wsod25.json', 'D:/p/monday.json']);
});

test('Rescan all backs up, rescans and saves each session, with one bar and a line naming the step', async ({ page }) => {
  await gotoHarness(page, TWO);
  await page.evaluate(() => { window.__hold = true; });
  await openFromMenu(page);
  await page.locator('#rescan-recent-start-btn').click();

  await expect(page.locator('#rescan-recent-step')).toHaveText('wsod25.json (1 of 2): rescanning 2 demos…');
  await expect(page.locator('#rescan-recent-start-btn')).toBeHidden();
  await expect(page.locator('#rescan-recent-cancel-btn')).toHaveText('Cancel');
  const before = await files(page);
  expect(before['D:/p/wsod25.before-rescan-' + (await page.evaluate(() => {
    const d = new Date(); const p = (n) => String(n).padStart(2, '0');
    return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
  })) + '.json']).toBe(TWO.files['D:/p/wsod25.json']);

  await page.evaluate(() => window.__release());
  await expect(page.locator('#rescan-recent-step')).toHaveText('monday.json (2 of 2): rescanning 1 demo…');
  // wsod25 (weight 3) done, monday (weight 2) 5% in: 62%.
  await expect(page.locator('#rescan-recent-progress .progress-bar-fill')).toHaveAttribute('style', /width: 62%/);
  await page.evaluate(() => window.__release());
  await expect(page.locator('#rescan-recent-modal')).toBeHidden();

  expect(await page.evaluate(() => window.__scans)).toEqual([['D:/d/1.dem', 'D:/d/2.dem'], ['D:/d/3.dem']]);
  const after = await files(page);
  const backups = Object.keys(after).filter((p) => p.includes('.before-rescan-'));
  expect(backups).toHaveLength(2);
  const saved = JSON.parse(after['D:/p/wsod25.json']);
  expect(saved.demos.map((d) => d.highlights_version)).toEqual([1, 1]);
  expect(JSON.parse(after['D:/p/monday.json']).demos[0].no_camera_data).toBe(false);

  const logs = await page.evaluate(() => window.__mockInvocations
    .filter((c) => c.cmd === 'log_frontend_event').map((c) => c.args.message));
  expect(logs.filter((l) => l.startsWith('[rescan] '))).toEqual([
    '[rescan] D:/p/gone.json: file not found, skipped',
    expect.stringMatching(/^\[rescan\] wsod25\.json: 2 demo\(s\) rescanned in \d+\.\d s and saved \(backup: wsod25\.before-rescan-.*\.json\)$/),
    expect.stringMatching(/^\[rescan\] monday\.json: 1 demo\(s\) rescanned in /),
  ]);
  // Nothing was open before, so the queue is left blank, with no session.
  await expect(page.locator('#session-file-indicator')).toHaveText('No session loaded');
  await expect(page.locator('#toast-container')).toContainText('Rescanned 2 of 2 sessions.');
});

test('the session that was open is reopened when it finishes, without its own rescan offer', async ({ page }) => {
  await gotoHarness(page, TWO);
  await page.evaluate(() => window.__session.loadProjectFile('D:/p/monday.json'));
  expect(await page.evaluate(() => window.__rescanOffers)).toBe(1);
  await openFromMenu(page);
  await page.locator('#rescan-recent-start-btn').click();
  await expect(page.locator('#rescan-recent-modal')).toBeHidden();
  await expect(page.locator('#session-file-indicator')).toHaveText('monday.json');
  expect(await page.evaluate(() => window.__getDemos().map((d) => d.highlights_version))).toEqual([1]);
  expect(await page.evaluate(() => window.__rescanOffers)).toBe(1);
});

test('unsaved changes are asked about first, and Cancel there rescans nothing', async ({ page }) => {
  await gotoHarness(page, TWO);
  await page.evaluate(() => window.__session.loadProjectFile('D:/p/monday.json'));
  await page.evaluate(() => window.__session.markProjectDirty());
  await openFromMenu(page);
  await page.locator('#rescan-recent-start-btn').click();
  await expect(page.locator('#unsaved-changes-modal')).toBeVisible();
  await expect(page.locator('#rescan-recent-modal')).toBeHidden();
  await page.locator('#unsaved-changes-cancel-btn').click();
  expect(await page.evaluate(() => window.__scans)).toEqual([]);
  expect(Object.keys(await files(page))).toHaveLength(2);
});

test('Cancel stops between sessions: the one under way is saved, the rest are left', async ({ page }) => {
  await gotoHarness(page, TWO);
  await page.evaluate(() => { window.__hold = true; });
  await openFromMenu(page);
  await page.locator('#rescan-recent-start-btn').click();
  await expect(page.locator('#rescan-recent-step')).toContainText('wsod25.json (1 of 2)');
  await page.locator('#rescan-recent-cancel-btn').click();
  await expect(page.locator('#rescan-recent-step')).toHaveText('wsod25.json (1 of 2): stopping after this session…');
  await expect(page.locator('#rescan-recent-cancel-btn')).toBeDisabled();
  await page.evaluate(() => window.__release());
  await expect(page.locator('#rescan-recent-modal')).toBeHidden();
  expect(await page.evaluate(() => window.__scans)).toHaveLength(1);
  const after = await files(page);
  expect(JSON.parse(after['D:/p/wsod25.json']).demos[0].highlights_version).toBe(1);
  expect(after['D:/p/monday.json']).toBe(TWO.files['D:/p/monday.json']);
  await expect(page.locator('#toast-container')).toContainText('Rescanned 1 of 2 sessions.');
});

test('a session swapped for another mid-scan (Ctrl+O) is not saved, and the end asks nothing', async ({ page }) => {
  const other = project(old('D:/d/9.dem'));
  await gotoHarness(page, { ...TWO, files: { ...TWO.files, 'D:/p/other.json': other } });
  await page.evaluate(() => { window.__hold = true; });
  await openFromMenu(page);
  await page.locator('#rescan-recent-start-btn').click();
  await expect(page.locator('#rescan-recent-step')).toContainText('wsod25.json (1 of 2): rescanning');
  await page.evaluate(() => window.__release());
  await expect(page.locator('#rescan-recent-step')).toContainText('monday.json (2 of 2): rescanning');
  // The user opens another project while monday's scan runs.
  await page.evaluate(() => window.__session.openProject('D:/p/other.json'));
  await page.evaluate(() => window.__release());
  await expect(page.locator('#rescan-recent-modal')).toBeHidden();
  await expect(page.locator('#unsaved-changes-modal')).toBeHidden();
  const after = await files(page);
  expect(after['D:/p/other.json']).toBe(other);
  expect(after['D:/p/monday.json']).toBe(TWO.files['D:/p/monday.json']);
  await expect(page.locator('#session-file-indicator')).toHaveText('No session loaded');
  await expect(page.locator('#toast-container')).toContainText('Rescanned 1 of 2 sessions; 1 failed');
});

test('a long run lets each session be skipped for now', async ({ page }) => {
  await gotoHarness(page, { ...TWO, toParse: 20 });
  await openFromMenu(page);
  await expect(page.locator('#rescan-recent-total')).toContainText('Total: about 3 min.');
  await expect(page.locator('#rescan-recent-total')).toContainText('Skip for now');
  const skips = page.locator('.rescan-recent-skip');
  await expect(skips).toHaveCount(2);
  await skips.nth(0).click();
  await expect(skips.nth(0)).toHaveText('Skipped');
  await expect(rows(page).nth(0)).toHaveClass(/skipped/);
  await expect(page.locator('#rescan-recent-total')).toContainText('Total: about 1 min.');
  await page.locator('#rescan-recent-start-btn').click();
  await expect(page.locator('#rescan-recent-modal')).toBeHidden();
  expect(await page.evaluate(() => window.__scans)).toEqual([['D:/d/3.dem']]);
  const after = await files(page);
  expect(after['D:/p/wsod25.json']).toBe(TWO.files['D:/p/wsod25.json']);
});

test('every session up to date: the menu item says so, and the start-up check stays silent', async ({ page }) => {
  await gotoHarness(page, { files: { 'D:/p/a.json': project() }, recent: ['D:/p/a.json'] });
  await page.locator('#file-menu-btn').click();
  await page.locator('#rescan-recent-btn').click();
  await expect(page.locator('#toast-container')).toContainText('Every recent session is up to date.');
  await expect(page.locator('#rescan-recent-modal')).toBeHidden();
});

test('the first start after an update offers it once, with two sessions to rescan', async ({ page }) => {
  await gotoHarness(page, TWO);
  const shown = page.evaluate(() => window.__rescan.offerAfterUpdate());
  await expect(page.locator('#rescan-recent-modal')).toBeVisible();
  await expect(rows(page)).toHaveCount(3);
  await page.locator('#rescan-recent-cancel-btn').click();
  expect(await shown).toBe(null);
  // Same version: not asked again.
  expect(await page.evaluate(() => window.__rescan.offerAfterUpdate())).toBe(null);
  await expect(page.locator('#rescan-recent-modal')).toBeHidden();
  expect(await page.evaluate(() => window.__mockInvocations
    .filter((c) => c.cmd === 'load_project_session').length)).toBe(2);
});
