// Project files are `.dodstudio` (#528), driven against the File menu harness
// in tests/e2e/recent-projects.html (the real project_session.js, Tauri's IPC
// and its dialog plugin mocked).
import { test, expect } from '@playwright/test';

async function gotoHarness(page) {
  await page.goto('/tests/e2e/recent-projects.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

const calls = (page, cmd) => page.evaluate((c) =>
  window.__mockInvocations.filter((call) => call.cmd === c).map((call) => call.args), cmd);

test("a new project's first save defaults to a .dodstudio file", async ({ page }) => {
  await gotoHarness(page);
  await page.evaluate(() => {
    window.__mockInvokeHandlers.default_projects_dir = () => 'D:\\projects';
    window.__mockInvokeHandlers['plugin:dialog|save'] = () => 'D:\\projects\\monday.dodstudio';
    window.__markDirty();
  });
  await page.locator('#file-menu-btn').click();
  await page.locator('#save-project-btn').click();
  await expect(page.locator('#session-file-indicator')).toHaveText('monday.dodstudio');

  const [dialog] = await calls(page, 'plugin:dialog|save');
  expect(dialog.options.defaultPath).toBe('D:\\projects\\dod_project.dodstudio');
  expect(dialog.options.filters.map((f) => f.extensions)).toEqual([['dodstudio'], ['json']]);
  expect((await calls(page, 'save_project_session')).map((a) => a.path)).toEqual(['D:\\projects\\monday.dodstudio']);
});

test('a project opened from .json keeps saving to that .json, with no dialog', async ({ page }) => {
  await gotoHarness(page);
  await page.evaluate(() => window.__recent.load(['D:/projects/old.json']));
  await page.locator('#file-menu-btn').click();
  await page.locator('#recent-projects-menu').hover();
  await page.locator('.recent-project-item').click();
  await expect(page.locator('#session-file-indicator')).toHaveText('old.json');

  await page.evaluate(() => window.__markDirty());
  await page.locator('#file-menu-btn').click();
  await page.locator('#save-project-btn').click();
  await expect(page.locator('#session-file-indicator')).toHaveText('old.json');
  expect(await calls(page, 'plugin:dialog|save')).toEqual([]);
  expect((await calls(page, 'save_project_session')).map((a) => a.path)).toEqual(['D:/projects/old.json']);
});

test('Load Project lists .dodstudio and .json projects together', async ({ page }) => {
  await gotoHarness(page);
  await page.evaluate(() => {
    window.__mockInvokeHandlers['plugin:dialog|open'] = () => 'D:\\projects\\WSOD25.dodstudio';
  });
  await page.locator('#file-menu-btn').click();
  await page.locator('#load-project-btn').click();
  await expect(page.locator('#session-file-indicator')).toHaveText('WSOD25.dodstudio');

  const [dialog] = await calls(page, 'plugin:dialog|open');
  expect(dialog.options.filters).toHaveLength(1);
  expect(dialog.options.filters[0].extensions).toEqual(['dodstudio', 'json']);
  expect((await calls(page, 'load_project_session')).map((a) => a.path)).toEqual(['D:\\projects\\WSOD25.dodstudio']);
});
