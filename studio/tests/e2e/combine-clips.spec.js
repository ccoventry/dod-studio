// combine_clips.js — Render Studio's Combine Clips window (#107).
//
// Pins the list (filled from the finished renders, reorder, remove, add
// files without duplicates), what the plan line says for each way of
// joining, and what Combine sends: the clips in order and the path saved to.
import { test, expect } from '@playwright/test';

const A = 'C:\\renders\\anzio_krod_4k.mp4';
const B = 'C:\\renders\\harrington_krod_3k.mp4';
const C = 'C:\\clips\\extra.mov';

async function loadHarness(page, { finished = [A, B], streamCopy = true } = {}) {
  await page.addInitScript(({ finished, streamCopy, picked }) => {
    window.__finished = finished;
    window.__mockInvokeHandlers = {
      combine_plan: ({ clips }) => ({
        stream_copy: streamCopy,
        total_secs: clips.length * 7,
        clips: clips.map((path) => ({ path, width: 1920, height: 1080, fps: '60' })),
      }),
      combine_clips: () => new Promise((resolve, reject) => { window.__finishCombine = { resolve, reject }; }),
      combine_cancel: () => { window.__finishCombine?.reject('cancelled'); },
      'plugin:dialog|open': () => picked,
      'plugin:dialog|save': (args) => { window.__saveDefault = args?.options?.defaultPath; return 'C:\\renders\\reel.mp4'; },
    };
  }, { finished, streamCopy, picked: [C, A] });
  await page.goto('/tests/e2e/combine-clips.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

const names = (page) => page.locator('#combine-list .combine-name');

test('opens with the finished renders, and the plan says they join as they are', async ({ page }) => {
  await loadHarness(page);
  await page.click('#combine-open-btn');
  await expect(page.locator('#combine-modal')).toBeVisible();
  await expect(names(page)).toHaveText(['anzio_krod_4k.mp4', 'harrington_krod_3k.mp4']);
  await expect(page.locator('#combine-plan')).toContainText('joined as they are');
  await expect(page.locator('#combine-plan')).toContainText('0:14 in all');
  await expect(page.locator('#combine-start-btn')).toBeEnabled();
});

test('reorder, remove, and add files without duplicates', async ({ page }) => {
  await loadHarness(page);
  await page.click('#combine-open-btn');
  await page.locator('#combine-list li').nth(1).getByTitle('Move up').click();
  await expect(names(page)).toHaveText(['harrington_krod_3k.mp4', 'anzio_krod_4k.mp4']);
  await page.click('#combine-add-files-btn');
  // A was already in the list.
  await expect(names(page)).toHaveText(['harrington_krod_3k.mp4', 'anzio_krod_4k.mp4', 'extra.mov']);
  await page.locator('#combine-list li').nth(0).getByTitle('Remove from the list').click();
  await page.locator('#combine-list li').nth(0).getByTitle('Remove from the list').click();
  await expect(names(page)).toHaveText(['extra.mov']);
  await expect(page.locator('#combine-plan')).toHaveText('Add at least two clips.');
  await expect(page.locator('#combine-start-btn')).toBeDisabled();
});

test('mixed clips say they are re-encoded at the first clip\'s size', async ({ page }) => {
  await loadHarness(page, { streamCopy: false });
  await page.click('#combine-open-btn');
  await expect(page.locator('#combine-plan')).toContainText('re-encoded to MP4 at 1920×1080, 60 fps');
});

test('Combine saves where chosen, sends the clips in order, shows progress, and can be cancelled', async ({ page }) => {
  await loadHarness(page);
  await page.click('#combine-open-btn');
  await page.click('#combine-start-btn');
  await expect(page.locator('#combine-cancel-btn')).toBeEnabled();
  await expect(page.locator('#combine-progress')).not.toHaveAttribute('hidden');
  const sent = await page.evaluate(() => window.__mockInvocations.find((c) => c.cmd === 'combine_clips').args);
  expect(sent).toEqual({ clips: [A, B], output: 'C:\\renders\\reel.mp4', ffmpegPath: 'C:/ffmpeg/ffmpeg.exe' });
  // Joined as they are, so the suggested name keeps the first clip's container.
  expect(await page.evaluate(() => window.__saveDefault)).toBe('C:\\renders\\anzio_krod_4k_combined.mp4');
  await page.evaluate(() => window.__mockEmit('combine_progress', { fraction: 0.5 }));
  await expect(page.locator('#combine-progress-fill')).toHaveAttribute('style', /width: 50%/);
  // Editing is locked while it runs.
  await expect(page.locator('#combine-add-files-btn')).toBeDisabled();

  await page.click('#combine-cancel-btn');
  await expect(page.locator('#combine-cancel-btn')).toBeDisabled();
  await expect(page.locator('#combine-progress')).toHaveAttribute('hidden');
  await expect(page.locator('#combine-add-files-btn')).toBeEnabled();
});
