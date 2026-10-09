// Finish clips after a capture batch (#440), driven against
// tests/e2e/finish-clips.html — see that file and tests/e2e/README.md.
import { test, expect } from '@playwright/test';

const BLOCK_A = 'D:\\cap\\s1\\demo1_b0';
const BLOCK_B = 'D:\\cap\\s1\\demo1_b1';

/** A `capture_takes_verified` payload: two renderable takes, one that isn't. */
function verified(overrides = {}) {
  return {
    session_id: 's1',
    outcome: 'complete',
    capture_fps: 240,
    blocks: [
      { take_folder: BLOCK_A, captured: true, renderable: true },
      { take_folder: BLOCK_B, captured: true, renderable: true },
      { take_folder: 'D:\\cap\\s1\\demo1_b2', captured: false, renderable: false },
    ],
    ...overrides,
  };
}

function job(id, block, status, extra = {}) {
  return {
    id, name: `clip-${id}`, stream: 'all', frames: 100, date: '', status,
    speed: '', progress: status === 'Finished' ? 100 : 0, error_log: null,
    settings_summary: 'ProRes @ 240fps', output_path: '', output_size_bytes: null,
    take_folder: `${block}\\take0000`, codec_id: 'prores', custom_codec_args: '',
    skip_available: false, ...extra,
  };
}

async function gotoHarness(page) {
  await page.addInitScript(() => {
    window.__mockInvokeHandlers = {
      queue_render_batch: (args) => args.payload.render_directories.length,
      get_export_pool_free_gb: () => 100,
      get_render_required_estimate_gb: () => 0,
    };
  });
  await page.goto('/tests/e2e/finish-clips.html');
  await page.waitForFunction(() => window.__finishReady === true);
}

const emit = (page, name, payload) => page.evaluate(([n, p]) => window.__mockEmit(n, p), [name, payload]);
const invocations = (page, cmd) => page.evaluate(
  (c) => window.__mockInvocations.filter((i) => i.cmd === c).map((i) => i.args),
  cmd,
);

test('does nothing while "When a batch finishes" is left on the Render tab', async ({ page }) => {
  await gotoHarness(page);
  await page.evaluate((v) => window.__requestFinish(v, 'frame_sequence'), verified());
  await page.waitForTimeout(100);
  expect(await invocations(page, 'queue_render_batch')).toEqual([]);
  await expect(page.locator('#batch-status')).toHaveText('Status: Waiting...');
});

test('queues and starts the batch\'s own takes, then reports progress and the end', async ({ page }) => {
  await gotoHarness(page);
  await page.selectOption('#config-finish-clips', 'finish');
  await page.evaluate((v) => window.__requestFinish(v, 'frame_sequence'), verified());

  await expect.poll(() => invocations(page, 'start_queued_render')).toHaveLength(1);
  const [queued] = await invocations(page, 'queue_render_batch');
  expect(queued.payload).toMatchObject({
    render_directories: [BLOCK_A, BLOCK_B],
    codec: 'prores',
    custom_codec_args: '',
    fps: 240, // the batch's capture rate, not the Render tab's 300
    export_directories: ['E:\\exports'],
    max_concurrent_renders: 2,
  });
  await expect(page.locator('#toast-container')).toContainText('Finishing 2 clips.');

  await emit(page, 'render_jobs_snapshot', [job('0', BLOCK_A, 'Finished'), job('1', BLOCK_B, 'Rendering')]);
  await expect(page.locator('#batch-status')).toHaveText('Finishing clips: 1 of 2, 1 in progress');
  await expect(page.locator('#capture-progress-bar')).toHaveAttribute('style', /width:\s*50%/);

  const final = [
    job('0', BLOCK_A, 'Finished', { output_path: 'E:\\exports\\clip-0.mov' }),
    job('1', BLOCK_B, 'Finished'),
  ];
  await emit(page, 'render_jobs_snapshot', final);
  await emit(page, 'render_batch_finished', {});
  await expect(page.locator('#batch-status')).toHaveText('2 clips ready.');
  await expect(page.locator('#toast-container')).toContainText('2 clips ready.');
  // The Render tab's own end-of-batch toast would say the same thing twice.
  await expect(page.locator('#toast-container')).not.toContainText('Render batch completed');

  await page.locator('#toast-container button', { hasText: 'Open export folder' }).click();
  expect(await invocations(page, 'reveal_in_explorer')).toEqual([{ path: 'E:\\exports\\clip-0.mov' }]);
});

test('an OBS batch is kept as captured by default', async ({ page }) => {
  await gotoHarness(page);
  await page.selectOption('#config-finish-clips', 'finish');
  await page.evaluate((v) => window.__requestFinish(v, 'obs'), verified());
  await expect.poll(() => invocations(page, 'queue_render_batch')).toHaveLength(1);
  const [queued] = await invocations(page, 'queue_render_batch');
  expect(queued.payload.codec).toBe('source_copy');
});

test('waits for a busy Render tab instead of replacing its batch', async ({ page }) => {
  await gotoHarness(page);
  await page.selectOption('#config-finish-clips', 'finish');
  await emit(page, 'render_jobs_snapshot', [job('0', 'D:\\older', 'Rendering')]);
  await page.evaluate((v) => window.__requestFinish(v, 'frame_sequence'), verified());

  await expect(page.locator('#batch-status')).toContainText('waiting for the Render tab');
  expect(await invocations(page, 'queue_render_batch')).toEqual([]);

  await emit(page, 'render_jobs_snapshot', [job('0', 'D:\\older', 'Finished')]);
  await expect.poll(() => invocations(page, 'queue_render_batch')).toHaveLength(1);
});

test('a cancelled batch is not finished', async ({ page }) => {
  await gotoHarness(page);
  await page.selectOption('#config-finish-clips', 'finish');
  await page.evaluate((v) => window.__requestFinish(v, 'frame_sequence'), verified({ outcome: 'cancelled' }));
  await expect(page.locator('#toast-container')).toContainText('was cancelled');
  expect(await invocations(page, 'queue_render_batch')).toEqual([]);
});

test('changing a finish setting saves settings', async ({ page }) => {
  await gotoHarness(page);
  await page.selectOption('#config-finish-codec-frames', 'prores');
  expect(await page.evaluate(() => window.__testSettingsChangeCount)).toBe(1);
});
