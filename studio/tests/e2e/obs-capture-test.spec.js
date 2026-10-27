// The OBS capture test (#646), driven against tests/e2e/obs-capture-test.html.
// A test clip is a normal batch: the mocked start_capture_batch plays its
// capture_status lines and the obs_test_takes_verified the backend sends for
// a test's session, and obs_test_analyze answers from window.__verdicts.
import { test, expect } from '@playwright/test';

async function gotoHarness(page, { verdicts = {}, gameOpen = false, holdAnalyze = false } = {}) {
  await page.addInitScript(([v, open, hold]) => {
    window.__verdicts = v;
    window.__mockInvokeHandlers = {
      obs_test_connection: () => ({ connected: true, obs_version: '33.0.0', websocket_version: '5', current_profile: '[DoD-Studio]', current_scene: '[DoD-Studio]', canvas: '2560x1440', output: '2560x1440', fps: 120, record_directory: 'D:\\captures', missing_requests: [], warnings: [] }),
      check_engine_processes: () => open,
      steam_state: () => 'ready',
      obs_test_clip: () => ({ demo: 'C:\\Studio\\obs-test\\dodstudio_benchmark_anzio.dem', streak: { start_index: 1, end_index: 2, kills: [] }, pre_roll: 5.5, post_roll: 5.5 }),
      start_capture_batch: ({ payload }) => {
        window.__lastRate = payload.obs_capture_fps;
        const take = `D:\\captures\\${payload.session_id}\\dodstudio_benchmark_anzio_b0`;
        setTimeout(() => window.__mockEmit('capture_status', { running: true, stage: 'starting', status: 'Starting the game…' }), 10);
        setTimeout(() => window.__mockEmit('capture_status', { running: false, status: 'Batch complete' }), 30);
        setTimeout(() => window.__mockEmit('obs_test_takes_verified', { session_id: payload.session_id, blocks: [{ take_folder: take, captured: true }] }), 40);
      },
      obs_test_analyze: () => {
        const verdict = window.__verdicts[window.__lastRate] || { level: 'steady', cause: 'none', skipped_share: 0, missed_share: 0, repeated_share: 0.002, counters_read: true, sound: 'ok' };
        const game = { frames: 5000, avg_fps: 287.6, low1_fps: 240.4, low01_fps: 190.1, max_ms: 6.1, slower_than_obs: 0 };
        const result = { recording: {}, file: { audio_peak_db: -3.2 }, verdict, game };
        if (!hold) return result;
        return new Promise((resolve) => { window.__finishAnalyze = () => resolve(result); });
      },
    };
  }, [verdicts, gameOpen, holdAnalyze]);
  await page.goto('/tests/e2e/obs-capture-test.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

const invocations = (page, cmd) => page.evaluate(
  (c) => window.__mockInvocations.filter((i) => i.cmd === c).map((i) => i.args),
  cmd,
);

const notSteady = { level: 'not_steady', cause: 'encoder', skipped_share: 0.62, missed_share: 0, repeated_share: 0, counters_read: true, sound: 'ok' };

test('one test clip runs as an OBS batch at the current settings, shows its steps, and is deleted', async ({ page }) => {
  await gotoHarness(page, { holdAnalyze: true });
  await page.click('#obs-capture-test-btn');

  // Reading the file is measured: the bar follows the frames read.
  await expect.poll(() => page.evaluate(() => typeof window.__finishAnalyze)).toBe('function');
  await page.evaluate(() => window.__mockEmit('obs_test_progress', { done: 1200, total: 2400 }));
  await expect(page.locator('#obs-test-step')).toHaveText('Reading the clip: frame 1,200 of 2,400');
  await expect(page.locator('#obs-test-bar-fill')).toHaveAttribute('style', /width:\s*50%/);
  await expect(page.locator('#obs-capture-test-btn')).toBeDisabled();
  await expect(page.locator('#obs-test-stop-btn')).toHaveJSProperty('hidden', false);
  await page.evaluate(() => window.__finishAnalyze());

  await expect(page.locator('#obs-test-result')).toContainText('144 fps at OBS Quality High: Sustainable.');
  await expect(page.locator('#obs-test-result')).toContainText('loudest moment -3.2 dB');
  await expect(page.locator('#obs-test-result')).toContainText('The game itself: 288 fps average, 1% low 240 fps');
  await expect(page.locator('#obs-test-rows tr td').nth(5)).toHaveText('288 (240)');
  await expect(page.locator('#obs-test-result')).toContainText('Nothing to change');
  await expect(page.locator('#obs-test-rows tr')).toHaveCount(1);
  await expect(page.locator('#obs-capture-test-btn')).toBeEnabled();

  const [batch] = await invocations(page, 'start_capture_batch');
  expect(batch.payload).toMatchObject({
    capture_mode: 'obs',
    obs_capture_fps: 144,
    obs_recording_quality: 'high',
    pre_roll_seconds: 5.5,
    post_roll_seconds: 5.5,
    capture_directories: ['D:\\captures'],
  });
  expect(batch.payload.streaks).toHaveLength(1);
  // The hook times the game's frames over exactly the clip.
  expect(batch.payload.custom_commands).toEqual([
    { command: 'ds_debug_record_frame_times 1', relation: 'Before', offset_seconds: 5.5 },
    { command: 'ds_debug_record_frame_times 0', relation: 'After', offset_seconds: 5.5 },
  ]);
  expect((await invocations(page, 'obs_test_analyze'))[0].obsFps).toBe(144);
  expect(batch.payload.session_id).toMatch(/^obs_test_\d{8}_\d{6}_144fps$/);
  expect(await invocations(page, 'obs_test_clip')).toEqual([{ seconds: 20, demo: null }]);
  expect(await invocations(page, 'obs_test_watch_start')).toEqual([{ host: '127.0.0.1', port: 4455, password: 'pw' }]);
  const [deleted] = await invocations(page, 'obs_test_delete_take');
  expect(deleted.takeFolder).toBe(`D:\\captures\\${batch.payload.session_id}\\dodstudio_benchmark_anzio_b0`);
});

test('the ladder climbs until a rate clearly fails, reports the highest steady one and can apply it', async ({ page }) => {
  await gotoHarness(page, { verdicts: { 165: notSteady } });
  await page.click('#obs-fps-ladder-btn');
  await expect(page.locator('#obs-test-result')).toContainText('Highest steady rate at OBS Quality High: 144 fps.', { timeout: 10000 });
  await expect(page.locator('#obs-test-result')).toContainText("165 fps: Not sustainable, OBS's encoder couldn't keep up.");
  const rates = (await invocations(page, 'start_capture_batch')).map((a) => a.payload.obs_capture_fps);
  expect(rates).toEqual([60, 120, 144, 165]);
  await expect(page.locator('#obs-test-rows tr')).toHaveCount(4);
  // OBS goes back to the user's rate afterwards.
  await expect.poll(async () => (await invocations(page, 'obs_test_connection')).length).toBeGreaterThan(1);
  const last = (await invocations(page, 'obs_test_connection')).pop();
  expect(last.obsCaptureFps).toBe(144);
  // The field already says 144: nothing to apply.
  await expect(page.locator('#obs-test-apply-btn')).toHaveJSProperty('hidden', true);
});

test('"Use N fps" sets OBS Capture FPS to the highest steady rate', async ({ page }) => {
  await gotoHarness(page, { verdicts: { 144: notSteady } });
  await page.check('#obs-test-long');
  await page.click('#obs-fps-ladder-btn');
  await expect(page.locator('#obs-test-apply-btn')).toHaveText('Use 120 fps', { timeout: 10000 });
  expect((await invocations(page, 'obs_test_clip'))[0]).toEqual({ seconds: 60, demo: null });
  await page.click('#obs-test-apply-btn');
  await expect(page.locator('#config-obs-capture-fps')).toHaveValue('120');
  await expect(page.locator('#obs-test-result')).toContainText('OBS Capture FPS is now 120.');
  expect(await page.evaluate(() => window.__persisted)).toBe(1);
});

test('an open game stops the test before any batch', async ({ page }) => {
  await gotoHarness(page, { gameOpen: true });
  await page.click('#obs-capture-test-btn');
  await expect(page.locator('#obs-test-result')).toHaveText('Close the game first: the test starts its own.');
  expect(await invocations(page, 'start_capture_batch')).toEqual([]);
  await expect(page.locator('#obs-capture-test-btn')).toBeEnabled();
});

test('"Use another demo…" tests on the picked demo until the bundled one is put back', async ({ page }) => {
  await gotoHarness(page);
  await page.evaluate(() => { window.__mockInvokeHandlers['plugin:dialog|open'] = () => 'D:/demos/match.dem'; });
  await page.click('#obs-test-demo-pick-btn');
  await expect(page.locator('#obs-test-demo-name')).toHaveText('match.dem');
  await page.click('#obs-capture-test-btn');
  await expect(page.locator('#obs-test-result')).toContainText('Sustainable');
  expect((await invocations(page, 'obs_test_clip'))[0]).toEqual({ seconds: 20, demo: 'D:/demos/match.dem' });
  await page.click('#obs-test-demo-reset-btn');
  await expect(page.locator('#obs-test-demo-name')).toHaveText('the one that comes with DoD Studio');
  await expect(page.locator('#obs-test-demo-reset-btn')).toHaveJSProperty('hidden', true);
});
