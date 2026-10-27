// What's New (#1013), whats_new.js, in the real app shell: this build's
// notes from the bundled file, the other versions from the backend's
// release_history, the update window's pending notes, and the first-start
// toast.
import { test, expect } from '@playwright/test';

const BUNDLED = [
  'See assets below to download.',
  '',
  "## What's new",
  '',
  '### Capture',
  '- **Seek Between Clips:** a batch jumps between clips.',
  '',
  '## Fixed',
  '',
  '- `ds_hud_deathmsg` keeps its size. (#710)',
  '',
  '<details><summary>All 2 pull requests</summary>',
  '',
  '- CI: cache from dev only (#346)',
  '- Docs: tidy (#649)',
  '',
  '</details>',
  '',
  '**Full diff:** https://github.com/ccoventry/dod-studio/compare/v0.1.4...abc',
].join('\n');

const RELEASES = [
  { tag: 'experimental-latest', name: 'DoD Studio 0.2.1-10 (experimental)', prerelease: true, published_at: '2026-10-26T05:10:11Z', body: '## New\n\n- An experimental thing (#1)' },
  { tag: 'v0.2.0', name: 'DoD Studio v0.2.0', prerelease: false, published_at: '2026-10-22T07:55:25Z', body: '## New\n\n- Review highlights (#623)' },
  { tag: 'v0.1.4', name: 'DoD Studio v0.1.4', prerelease: false, published_at: '2026-09-22T08:56:59Z', body: 'See assets below to download.' },
];

function boot(page, { autoCheck = true, seen = null, bundled = BUNDLED } = {}) {
  return page.addInitScript(({ autoCheck, seen, releases }) => {
    try {
      if (seen) localStorage.setItem('dodstudio.whatsNew.seenVersion', seen);
      else localStorage.removeItem('dodstudio.whatsNew.seenVersion');
    } catch { /* no storage */ }
    window.__mockInvokeHandlers = {
      get_settings: () => ({ pinned_folders: [], target_drives: ['E:/caps'], auto_check_updates: autoCheck }),
      calculate_export_pool_space: () => 10 * 1024 ** 3,
      system_memory_bytes: () => 16 * 1024 ** 3,
      release_history: ({ offline }) => (offline
        ? { releases: releases.slice(1, 2), from_cache: true, error: null }
        : { releases, from_cache: false, error: null }),
    };
  }, { autoCheck, seen, releases: RELEASES }).then(() => page.route('**/release_notes.md', (route) => (bundled
    ? route.fulfill({ status: 200, contentType: 'text/markdown', body: bundled })
    : route.fulfill({ status: 404, body: '' }))));
}

const historyCalls = (page) => page.evaluate(() => window.__mockInvocations
  .filter((c) => c.cmd === 'release_history').map((c) => c.args.offline));

async function openFromHelp(page) {
  await page.goto('/index.html');
  await expect(page.locator('#export-pool-free-indicator')).toContainText('10.0');
  await page.evaluate(() => document.querySelector('#whats-new-menu-btn').click());
  await expect(page.locator('#whats-new-modal')).toBeVisible();
}

test("Help > What's New shows this build's notes, folded PR list, and the other stable versions", async ({ page }) => {
  await boot(page);
  await openFromHelp(page);
  const current = page.locator('#whats-new-current');
  await expect(page.locator('#whats-new-current-title')).toHaveText('This version: v0.0.0-e2e');
  await expect(current.locator('h4').first()).toHaveText("What's new");
  await expect(current.locator('strong')).toHaveText('Seek Between Clips:');
  await expect(current.locator('code')).toHaveText('ds_hud_deathmsg');
  await expect(current).not.toContainText('See assets');
  await expect(current).not.toContainText('Full diff');
  const more = current.locator('details.release-notes-more');
  await expect(more.locator('summary')).toHaveText('Everything else: All 2 pull requests');
  await expect(more).not.toHaveAttribute('open', '');

  const others = page.locator('#whats-new-others .whats-new-version');
  await expect(others.locator('.whats-new-version-name')).toHaveText(['v0.2.0', 'v0.1.4']);
  await expect(others.nth(1)).toContainText('No notes were written for this version.');
  expect(await historyCalls(page)).toEqual([false]);
  await expect(page.locator('#whats-new-fetch-btn')).toBeHidden();

  // A second open in the same session asks GitHub no more.
  await page.evaluate(() => document.querySelector('#whats-new-close-btn').click());
  await page.evaluate(() => document.querySelector('#whats-new-menu-btn').click());
  await expect(others).toHaveCount(2);
  expect(await historyCalls(page)).toEqual([false]);
});

test('with update checks off it shows the saved copy, and reads GitHub only on the button', async ({ page }) => {
  await boot(page, { autoCheck: false, bundled: null });
  await openFromHelp(page);
  await expect(page.locator('#whats-new-status')).toHaveText('Update checks are off, so this is the release list saved last time.');
  await expect(page.locator('#whats-new-current')).toContainText("This build doesn't carry its own notes");
  await expect(page.locator('#whats-new-others .whats-new-version')).toHaveCount(1);
  expect(await historyCalls(page)).toEqual([true]);

  const fetchBtn = page.locator('#whats-new-fetch-btn');
  await expect(fetchBtn).toBeVisible();
  await fetchBtn.click();
  await expect(page.locator('#whats-new-others .whats-new-version')).toHaveCount(2);
  expect(await historyCalls(page)).toEqual([true, false]);
  await expect(fetchBtn).toBeHidden();
  await expect(page.locator('#whats-new-status')).toBeHidden();
});

test('a failed GitHub read says so and keeps the saved copy', async ({ page }) => {
  await boot(page);
  await page.addInitScript(() => {
    window.__mockInvokeHandlers.release_history = () => ({
      releases: [{ tag: 'v0.2.0', name: 'DoD Studio v0.2.0', prerelease: false, published_at: null, body: '## New\n- A (#1)' }],
      from_cache: true,
      error: 'https://api.github.com/...: timed out',
    });
  });
  await openFromHelp(page);
  await expect(page.locator('#whats-new-status')).toContainText("Couldn't read GitHub's release list, so this is the copy saved last time.");
  await expect(page.locator('#whats-new-others .whats-new-version')).toHaveCount(1);
  await expect(page.locator('#whats-new-fetch-btn')).toBeVisible();
});

test("the first start after an update offers What's New in a toast", async ({ page }) => {
  await boot(page, { seen: '0.0.0-older' });
  await page.goto('/index.html');
  const toast = page.locator('#toast-container .toast', { hasText: 'DoD Studio updated to v0.0.0-e2e.' });
  await expect(toast).toBeVisible();
  await toast.locator('button').click();
  await expect(page.locator('#whats-new-modal')).toBeVisible();
  expect(await page.evaluate(() => localStorage.getItem('dodstudio.whatsNew.seenVersion'))).toBe('0.0.0-e2e');
});

test("About's button opens it, and the update window shows a pending update's notes", async ({ page }) => {
  await boot(page);
  await page.goto('/index.html');
  await expect(page.locator('#export-pool-free-indicator')).toContainText('10.0');
  await expect(page.locator('#toast-container .toast', { hasText: 'updated to' })).toHaveCount(0);
  await page.evaluate(() => document.querySelector('#about-menu-btn').click());
  await page.locator('#about-whats-new-btn').click();
  await expect(page.locator('#about-modal')).toBeHidden();
  await expect(page.locator('#whats-new-modal')).toBeVisible();

  await page.evaluate(async () => {
    const { showPendingNotes } = await import('/src/whats_new.js');
    showPendingNotes('See assets below to download.\n\n## Fixed\n\n- Previews open again (#999)');
  });
  await expect(page.locator('#update-modal-notes')).not.toHaveAttribute('hidden', '');
  await expect(page.locator('#update-modal-notes-body li')).toHaveText('Previews open again (#999)');
  await page.evaluate(async () => (await import('/src/whats_new.js')).showPendingNotes(null));
  await expect(page.locator('#update-modal-notes')).toHaveAttribute('hidden', '');
});
