// An older project takes the one snapshot of Configuration taken at the
// first start of a build with project settings (#1085, decision 6), not
// Configuration as it is when the project opens. Boots the real app shell
// (index.html + main.js) against the mocked IPC, like app-boot.spec.js.
import { test, expect } from '@playwright/test';

const GIB = 1024 ** 3;

test.beforeEach(async ({ page }) => {
  await page.addInitScript((gib) => {
    window.__mockInvokeHandlers = {
      get_settings: () => ({
        resolution_width: 1920,
        resolution_height: 1080,
        capture_fps: 120,
        target_drives: ['E:/caps'],
        render_export_dirs: ['F:/clips'],
        pc_id: 'pc-a',
      }),
      calculate_export_pool_space: () => 10 * gib,
      system_memory_bytes: () => 16 * gib,
      // project_settings_snapshot.rs: the first snapshot offered is kept.
      keep_project_settings_snapshot: ({ snapshot }) => {
        const taken = !window.__kept;
        if (taken) window.__kept = snapshot;
        return { snapshot: window.__kept, taken };
      },
      'plugin:dialog|open': () => 'D:\\projects\\old.dodstudio',
      // A project saved before #1085: no `settings` block.
      load_project_session: () => JSON.stringify({ version: '0.12.0', demos: [] }),
      files_exist: ({ paths }) => paths.map(() => true),
      locate_missing_demos: () => [],
    };
  }, GIB);
});

/** Types `value` into a Configuration number field, as the user would. */
async function setField(page, selector, value) {
  await page.evaluate(({ selector, value }) => {
    const el = document.querySelector(selector);
    el.value = String(value);
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
  }, { selector, value });
}

const strip = (page) => page.locator('#capture-summary-strip');

test('an older project opened after Configuration changed captures with the snapshot\'s settings', async ({ page }) => {
  await page.goto('/index.html');
  await expect.poll(() => page.evaluate(() => window.__kept?.settings?.picture?.width)).toBe(1920);
  await expect(strip(page)).toContainText('1920×1080');

  // Configuration set up for another movie.
  await setField(page, '#config-res-width', 2560);
  await setField(page, '#config-res-height', 1440);
  await expect(strip(page)).toContainText('2560×1440');

  await page.evaluate(() => document.querySelector('#load-project-btn').click());
  await expect(page.locator('#session-file-indicator')).toHaveText('old.dodstudio');
  // The strip redraws on a Configuration change; #1364 also redraws it when
  // a project opens.
  await page.evaluate(() => document.querySelector('#export-config-panel')
    .dispatchEvent(new Event('change', { bubbles: true })));
  await expect(strip(page)).toContainText('1920×1080');
  await expect(strip(page)).not.toContainText('2560');
  // Configuration itself keeps the app's new values.
  await expect(page.locator('#config-res-width')).toHaveValue('2560');

  // The snapshot was offered once, and its folders stay out of its block.
  const offered = await page.evaluate(() => window.__mockInvocations
    .filter((c) => c.cmd === 'keep_project_settings_snapshot').map((c) => c.args.snapshot));
  expect(offered).toHaveLength(1);
  expect(offered[0].folders).toEqual({ destinations: ['E:/caps'], exports: ['F:/clips'] });
  expect(offered[0].settings.folders).toBeUndefined();
});
