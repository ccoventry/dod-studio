// Render Studio frontend behavior, driven against tests/e2e/render-studio.html
// (a minimal harness — see the comment at the top of that file and
// tests/e2e/README.md for what this suite covers and what it doesn't).
import { test, expect } from '@playwright/test';

/** A RenderJobView-shaped fixture with sane defaults, overridable per test. */
function job(overrides = {}) {
  return {
    id: '0',
    name: 'demo1-chain_01_b0-obs',
    stream: 'all',
    frames: 100,
    date: '2026-08-29 10:00 AM',
    status: 'Queued',
    speed: '',
    progress: 0,
    error_log: null,
    settings_summary: 'ProRes @ 300fps',
    output_path: '',
    output_size_bytes: null,
    take_folder: 'C:\\captures\\demo1\\chain_01_b0\\take0000',
    codec_id: 'prores',
    custom_codec_args: '',
    skip_available: false,
    ...overrides,
  };
}

async function gotoHarness(page) {
  await page.goto('/tests/e2e/render-studio.html');
  await page.waitForFunction(() => window.__renderPaneReady === true);
}

async function emitSnapshot(page, jobs) {
  await page.evaluate((j) => window.__mockEmit('render_jobs_snapshot', j), jobs);
}

function invocationsFor(page, cmd) {
  return page.evaluate(
    (c) => window.__mockInvocations.filter((i) => i.cmd === c).map((i) => i.args),
    cmd,
  );
}

test.describe('empty and basic rendering', () => {
  test('shows the empty-state row before any snapshot arrives', async ({ page }) => {
    await gotoHarness(page);
    await expect(page.locator('#render-jobs-tbody td.table-empty')).toBeVisible();
  });

  test('a snapshot renders one row per job with the right content', async ({ page }) => {
    await gotoHarness(page);
    await emitSnapshot(page, [
      job({ id: '0', name: 'clip-a', status: 'Rendering', progress: 42 }),
      job({ id: '1', name: 'clip-b', status: 'Finished', progress: 100 }),
    ]);

    const rows = page.locator('#render-jobs-tbody tr[data-job-id]');
    await expect(rows).toHaveCount(2);
    await expect(page.locator('tr[data-job-id="0"] .rj-name')).toHaveText('clip-a');
    await expect(page.locator('tr[data-job-id="0"] .rj-status')).toHaveText('Rendering');
    await expect(page.locator('tr[data-job-id="0"] .rj-progress-fill')).toHaveAttribute('style', /width:\s*42%/);
    await expect(page.locator('tr[data-job-id="1"] .rj-status')).toHaveText('Finished');
  });

  test('the empty-state row is replaced once jobs exist, and comes back once they are cleared', async ({ page }) => {
    await gotoHarness(page);
    await emitSnapshot(page, [job()]);
    await expect(page.locator('#render-jobs-tbody td.table-empty')).toHaveCount(0);
    await emitSnapshot(page, []);
    await expect(page.locator('#render-jobs-tbody td.table-empty')).toBeVisible();
  });
});

test.describe('issue #80 — row/button identity survives unrelated snapshot churn', () => {
  test('the Cancel button DOM node is not replaced by a progress-only update', async ({ page }) => {
    await gotoHarness(page);
    await emitSnapshot(page, [job({ id: '0', status: 'Rendering', progress: 10 })]);

    await page.evaluate(() => {
      document.querySelector('tr[data-job-id="0"] .render-job-cancel-btn').dataset.testMarker = 'original';
    });

    await emitSnapshot(page, [job({ id: '0', status: 'Rendering', progress: 55, speed: '31 fps (1.0x)' })]);

    const marker = await page.evaluate(
      () => document.querySelector('tr[data-job-id="0"] .render-job-cancel-btn')?.dataset.testMarker,
    );
    expect(marker).toBe('original');
    await expect(page.locator('tr[data-job-id="0"] .rj-progress-fill')).toHaveAttribute('style', /width:\s*55%/);
  });

  test('the row node itself is not replaced across repeated snapshots', async ({ page }) => {
    await gotoHarness(page);
    await emitSnapshot(page, [job({ id: '0' })]);
    await page.evaluate(() => {
      document.querySelector('tr[data-job-id="0"]').dataset.testMarker = 'same-row';
    });
    for (let i = 0; i < 5; i++) {
      await emitSnapshot(page, [job({ id: '0', progress: i * 10 })]);
    }
    const marker = await page.evaluate(() => document.querySelector('tr[data-job-id="0"]')?.dataset.testMarker);
    expect(marker).toBe('same-row');
  });

  test('a status change that rebuilds the Actions cell still ends up wired — a real click reaches the backend', async ({ page }) => {
    await gotoHarness(page);
    await emitSnapshot(page, [job({ id: '0', status: 'Queued' })]);
    await emitSnapshot(page, [job({ id: '0', status: 'Rendering' })]);

    await page.locator('tr[data-job-id="0"] .render-job-cancel-btn').click();

    const calls = await invocationsFor(page, 'cancel_render_job');
    expect(calls).toEqual([{ jobId: '0' }]);
  });

  test('clicking Cancel on several rows in quick succession, against continuous snapshot churn, reaches the backend for every row', async ({ page }) => {
    await gotoHarness(page);
    const ids = ['0', '1', '2', '3', '4'];
    await emitSnapshot(page, ids.map((id) => job({ id, status: 'Rendering' })));

    await page.evaluate(() => {
      let tick = 0;
      window.__churnInterval = setInterval(() => {
        tick += 1;
        const ids = ['0', '1', '2', '3', '4'];
        window.__mockEmit(
          'render_jobs_snapshot',
          ids.map((id) => ({
            id, name: 'x', stream: 'all', frames: 1, date: '-', status: 'Rendering',
            speed: '', progress: tick % 100, error_log: null, settings_summary: 'ProRes @ 300fps',
            output_path: '', take_folder: 'C:\\x', codec_id: 'prores', skip_available: false,
          })),
        );
      }, 80);
    });

    for (const id of ids) {
      await page.locator(`tr[data-job-id="${id}"] .render-job-cancel-btn`).click();
      await page.waitForTimeout(25);
    }

    await page.evaluate(() => clearInterval(window.__churnInterval));

    const calledIds = (await invocationsFor(page, 'cancel_render_job')).map((a) => a.jobId).sort();
    expect(calledIds).toEqual(ids);
  });
});

test.describe('scan stages a batch, Start is a separate step', () => {
  test('clicking Scan calls queue_render_batch and populates the job table, without starting the batch', async ({ page }) => {
    await gotoHarness(page);
    await page.evaluate(() => {
      window.__testCaptureLocations = ['C:\\captures'];
      window.__mockInvokeHandlers['queue_render_batch'] = () => {
        window.__mockEmit('render_jobs_snapshot', [
          {
            id: '0', name: 'demo1-chain_01_b0-obs', stream: 'all', frames: 100, date: '-',
            status: 'Queued', speed: '', progress: 0, error_log: null,
            settings_summary: 'ProRes @ 300fps', output_path: '', take_folder: 'C:\\x',
            codec_id: 'prores', skip_available: true,
          },
        ]);
        return 1;
      };
    });

    await page.locator('#scan-render-btn').click();
    await expect(page.locator('tr[data-job-id="0"] .rj-status')).toHaveText('Queued');

    expect(await invocationsFor(page, 'queue_render_batch')).toHaveLength(1);
    expect(await invocationsFor(page, 'start_queued_render')).toHaveLength(0);
    await expect(page.locator('#start-render-btn')).toBeEnabled();
    await expect(page.locator('#scan-render-btn')).toBeDisabled();
  });

  test('clicking Start Render Batch calls start_queued_render with the ticked Queued rows (#70)', async ({ page }) => {
    await gotoHarness(page);
    await emitSnapshot(page, [job({ id: '0', status: 'Queued' })]);
    await expect(page.locator('#start-render-btn')).toBeEnabled();
    await expect(page.locator('#start-render-btn')).toHaveText('Start Render Batch (1)');

    await page.locator('#start-render-btn').click();

    const calls = await invocationsFor(page, 'start_queued_render');
    expect(calls).toEqual([{ jobIds: ['0'] }]);
  });

  test('once rendering starts, Scan and Start are both disabled again', async ({ page }) => {
    await gotoHarness(page);
    await emitSnapshot(page, [job({ id: '0', status: 'Queued' })]);
    await emitSnapshot(page, [job({ id: '0', status: 'Rendering', progress: 5 })]);

    await expect(page.locator('#scan-render-btn')).toBeDisabled();
    await expect(page.locator('#start-render-btn')).toBeDisabled();
    await expect(page.locator('#cancel-render-btn')).toBeEnabled();
  });
});

test.describe('Custom codec', () => {
  test('the args input is only visible while Custom is selected', async ({ page }) => {
    await gotoHarness(page);
    await expect(page.locator('#render-custom-codec-group')).toBeHidden();

    await page.selectOption('#render-codec-select', 'custom');
    await expect(page.locator('#render-custom-codec-group')).toBeVisible();

    await page.selectOption('#render-codec-select', 'prores');
    await expect(page.locator('#render-custom-codec-group')).toBeHidden();
  });

  test('Scan is refused when Custom is selected with no args entered', async ({ page }) => {
    await gotoHarness(page);
    await page.evaluate(() => { window.__testCaptureLocations = ['C:\\captures']; });
    await page.selectOption('#render-codec-select', 'custom');

    await page.locator('#scan-render-btn').click();

    expect(await invocationsFor(page, 'queue_render_batch')).toHaveLength(0);
  });

  test('Scan sends the typed args as custom_codec_args once Custom is filled in', async ({ page }) => {
    await gotoHarness(page);
    await page.evaluate(() => {
      window.__testCaptureLocations = ['C:\\captures'];
      window.__mockInvokeHandlers['queue_render_batch'] = () => 0;
    });
    await page.selectOption('#render-codec-select', 'custom');
    await page.locator('#render-custom-codec-input').fill('-c:v mpeg4 -q:v 3');

    await page.locator('#scan-render-btn').click();

    const calls = await invocationsFor(page, 'queue_render_batch');
    expect(calls).toHaveLength(1);
    expect(calls[0].payload.codec).toBe('custom');
    expect(calls[0].payload.custom_codec_args).toBe('-c:v mpeg4 -q:v 3');
  });
});

test.describe('File Size column', () => {
  test('shows a dash for Queued/Rendering — never a per-row estimate', async ({ page }) => {
    await gotoHarness(page);
    await emitSnapshot(page, [
      job({ id: '0', status: 'Queued', output_size_bytes: null }),
      job({ id: '1', status: 'Rendering', output_size_bytes: null }),
    ]);
    await expect(page.locator('tr[data-job-id="0"] .rj-file-size')).toHaveText('—');
    await expect(page.locator('tr[data-job-id="1"] .rj-file-size')).toHaveText('—');
  });

  test('shows the real size once Finished, in GB for a large file', async ({ page }) => {
    await gotoHarness(page);
    await emitSnapshot(page, [
      job({ id: '0', status: 'Finished', output_size_bytes: 2_500_000_000 }),
    ]);
    await expect(page.locator('tr[data-job-id="0"] .rj-file-size')).toHaveText('2.33 GB');
  });

  test('shows the real size in MB for a file under 1 GB', async ({ page }) => {
    await gotoHarness(page);
    await emitSnapshot(page, [
      job({ id: '0', status: 'Finished', output_size_bytes: 200_000_000 }),
    ]);
    await expect(page.locator('tr[data-job-id="0"] .rj-file-size')).toHaveText('191 MB');
  });

  test('a Cancelled/Error job also shows a dash, not a stale size from a prior run', async ({ page }) => {
    await gotoHarness(page);
    await emitSnapshot(page, [job({ id: '0', status: 'Cancelled', output_size_bytes: null })]);
    await expect(page.locator('tr[data-job-id="0"] .rj-file-size')).toHaveText('—');
  });
});

test.describe('Export Pool Free / Required (Over-estimated) footer refresh', () => {
  test('a snapshot with Queued/Rendering jobs calls get_render_required_estimate_gb and shows the result', async ({ page }) => {
    await gotoHarness(page);
    await page.evaluate(() => {
      window.__mockInvokeHandlers['get_render_required_estimate_gb'] = () => 42.5;
    });
    await emitSnapshot(page, [job({ id: '0', status: 'Queued' })]);
    await expect(page.locator('#render-footer-required-space')).toHaveText('Required (Over-estimated): 42.50 GB');
  });

  test('a progress-only update (same id:status) does not re-call get_render_required_estimate_gb', async ({ page }) => {
    await gotoHarness(page);
    await emitSnapshot(page, [job({ id: '0', status: 'Rendering', progress: 10 })]);
    const callsAfterFirst = (await invocationsFor(page, 'get_render_required_estimate_gb')).length;
    await emitSnapshot(page, [job({ id: '0', status: 'Rendering', progress: 55 })]);
    const callsAfterSecond = (await invocationsFor(page, 'get_render_required_estimate_gb')).length;
    expect(callsAfterSecond).toBe(callsAfterFirst);
  });

  test('a status change (Queued -> Rendering) does re-call get_render_required_estimate_gb', async ({ page }) => {
    await gotoHarness(page);
    await emitSnapshot(page, [job({ id: '0', status: 'Queued' })]);
    const callsAfterFirst = (await invocationsFor(page, 'get_render_required_estimate_gb')).length;
    await emitSnapshot(page, [job({ id: '0', status: 'Rendering', progress: 5 })]);
    const callsAfterSecond = (await invocationsFor(page, 'get_render_required_estimate_gb')).length;
    expect(callsAfterSecond).toBe(callsAfterFirst + 1);
  });

  test('a job finishing (still present, status changes) re-calls it; a job disappearing from the snapshot also re-calls it', async ({ page }) => {
    await gotoHarness(page);
    await emitSnapshot(page, [job({ id: '0', status: 'Rendering' }), job({ id: '1', status: 'Queued' })]);
    const callsBefore = (await invocationsFor(page, 'get_render_required_estimate_gb')).length;

    await emitSnapshot(page, [job({ id: '0', status: 'Finished' }), job({ id: '1', status: 'Queued' })]);
    const callsAfterFinish = (await invocationsFor(page, 'get_render_required_estimate_gb')).length;
    expect(callsAfterFinish).toBe(callsBefore + 1);

    // Simulates Remove: job '0' no longer appears in the snapshot at all.
    await emitSnapshot(page, [job({ id: '1', status: 'Queued' })]);
    const callsAfterRemove = (await invocationsFor(page, 'get_render_required_estimate_gb')).length;
    expect(callsAfterRemove).toBe(callsAfterFinish + 1);
  });
});

test.describe('Remove row / Reset All / Remove All', () => {
  test('Remove is shown for a Queued row, alongside Cancel', async ({ page }) => {
    await gotoHarness(page);
    await emitSnapshot(page, [job({ id: '0', status: 'Queued' })]);
    await expect(page.locator('tr[data-job-id="0"] .render-job-cancel-btn')).toBeVisible();
    await expect(page.locator('tr[data-job-id="0"] .render-job-remove-btn')).toBeVisible();
  });

  test('Remove is withheld for a Rendering row', async ({ page }) => {
    await gotoHarness(page);
    await emitSnapshot(page, [job({ id: '0', status: 'Rendering' })]);
    await expect(page.locator('tr[data-job-id="0"] .render-job-remove-btn')).toHaveCount(0);
  });

  test('Remove is shown alongside Reset for a terminal-state row', async ({ page }) => {
    await gotoHarness(page);
    await emitSnapshot(page, [job({ id: '0', status: 'Cancelled' })]);
    await expect(page.locator('tr[data-job-id="0"] .render-job-reset-btn')).toBeVisible();
    await expect(page.locator('tr[data-job-id="0"] .render-job-remove-btn')).toBeVisible();
  });

  test('clicking Remove calls remove_render_job with that job id', async ({ page }) => {
    await gotoHarness(page);
    await emitSnapshot(page, [job({ id: '0', status: 'Queued' })]);
    await page.locator('tr[data-job-id="0"] .render-job-remove-btn').click();
    const calls = await invocationsFor(page, 'remove_render_job');
    expect(calls).toEqual([{ jobId: '0' }]);
  });

  test('Retry and Remove start disabled, enable once a ticked eligible row exists', async ({ page }) => {
    await gotoHarness(page);
    await expect(page.locator('#retry-render-btn')).toBeDisabled();
    await expect(page.locator('#remove-render-btn')).toBeDisabled();

    await emitSnapshot(page, [job({ id: '0', status: 'Queued' })]);
    // Nothing to retry yet (Queued isn't Cancelled/Finished/Error) but it
    // isn't Rendering either, so Remove should already be live.
    await expect(page.locator('#retry-render-btn')).toBeDisabled();
    await expect(page.locator('#remove-render-btn')).toBeEnabled();

    await emitSnapshot(page, [job({ id: '0', status: 'Cancelled' })]);
    await expect(page.locator('#retry-render-btn')).toBeEnabled();
    await expect(page.locator('#remove-render-btn')).toBeEnabled();
  });

  test('Retry and Remove disable again once everything is Rendering', async ({ page }) => {
    await gotoHarness(page);
    await emitSnapshot(page, [job({ id: '0', status: 'Rendering' }), job({ id: '1', status: 'Rendering' })]);
    await expect(page.locator('#retry-render-btn')).toBeDisabled();
    await expect(page.locator('#remove-render-btn')).toBeDisabled();
  });

  test('clicking Retry calls reset_render_jobs with the ticked ids', async ({ page }) => {
    await gotoHarness(page);
    await emitSnapshot(page, [job({ id: '0', status: 'Error' })]);
    await page.locator('#retry-render-btn').click();
    const calls = await invocationsFor(page, 'reset_render_jobs');
    expect(calls).toEqual([{ jobIds: ['0'] }]);
  });

  test('clicking Remove calls remove_render_jobs with the ticked ids', async ({ page }) => {
    await gotoHarness(page);
    await emitSnapshot(page, [job({ id: '0', status: 'Finished' })]);
    await page.locator('#remove-render-btn').click();
    const calls = await invocationsFor(page, 'remove_render_jobs');
    expect(calls).toEqual([{ jobIds: ['0'] }]);
  });
});

test.describe('issue #70 — ticked rows and Clear Queue', () => {
  const rendered = [{ started_unix_ms: 1700000000000, stream: 'all', codec: 'prores', custom_codec_args: '', fps: 300, outcome: 'finished', output_path: 'E:/out/a.mov', output_size_bytes: 1024, error: null, output_exists: true }];

  test('a new take starts ticked, a rendered one unticked, and the count is exact', async ({ page }) => {
    await gotoHarness(page);
    await emitSnapshot(page, [
      job({ id: '0' }),
      job({ id: '1', take_folder: 'C:\\captures\\demo1\\chain_02_b0\\take0000', history: rendered }),
      job({ id: '2', take_folder: 'C:\\captures\\demo1\\chain_03_b0\\take0000' }),
    ]);
    await expect(page.locator('tr[data-job-id="0"] .render-job-select-cb')).toBeChecked();
    await expect(page.locator('tr[data-job-id="1"] .render-job-select-cb')).not.toBeChecked();
    await expect(page.locator('tr[data-job-id="2"] .render-job-select-cb')).toBeChecked();
    await expect(page.locator('#render-selection-count')).toHaveText('2 of 3 selected');
    await expect(page.locator('#start-render-btn')).toHaveText('Start Render Batch (2)');
  });

  test('Select all, none and unrendered, and the header tick', async ({ page }) => {
    await gotoHarness(page);
    await emitSnapshot(page, [
      job({ id: '0' }),
      job({ id: '1', take_folder: 'C:\\captures\\demo1\\chain_02_b0\\take0000', history: rendered }),
    ]);
    await page.locator('#render-select-all-btn').click();
    await expect(page.locator('#render-selection-count')).toHaveText('2 of 2 selected');
    await expect(page.locator('#render-select-all-cb')).toBeChecked();

    await page.locator('#render-select-none-btn').click();
    await expect(page.locator('#render-selection-count')).toHaveText('0 of 2 selected');
    await expect(page.locator('#start-render-btn')).toBeDisabled();
    await expect(page.locator('#start-render-btn')).toHaveText('Start Render Batch (0)');

    await page.locator('#render-select-unrendered-btn').click();
    await expect(page.locator('tr[data-job-id="0"] .render-job-select-cb')).toBeChecked();
    await expect(page.locator('tr[data-job-id="1"] .render-job-select-cb')).not.toBeChecked();
    await expect(page.locator('#render-select-all-cb')).toHaveJSProperty('indeterminate', true);

    await page.locator('#render-select-all-cb').click();
    await expect(page.locator('#render-selection-count')).toHaveText('2 of 2 selected');
  });

  test('a tick survives snapshots, and Start sends only the ticked Queued rows', async ({ page }) => {
    await gotoHarness(page);
    const rows = [job({ id: '0' }), job({ id: '1', take_folder: 'C:\\captures\\demo1\\chain_02_b0\\take0000' })];
    await emitSnapshot(page, rows);
    await page.locator('tr[data-job-id="1"] .render-job-select-cb').uncheck();
    await emitSnapshot(page, rows);
    await emitSnapshot(page, rows);
    await expect(page.locator('tr[data-job-id="1"] .render-job-select-cb')).not.toBeChecked();
    await expect(page.locator('#start-render-btn')).toHaveText('Start Render Batch (1)');

    await page.locator('#start-render-btn').click();
    expect(await invocationsFor(page, 'start_queued_render')).toEqual([{ jobIds: ['0'] }]);
  });

  test('Cancel sends the ticked rendering and queued rows', async ({ page }) => {
    await gotoHarness(page);
    await emitSnapshot(page, [
      job({ id: '0', status: 'Rendering' }),
      job({ id: '1', status: 'Queued', take_folder: 'C:\\captures\\demo1\\chain_02_b0\\take0000' }),
      job({ id: '2', status: 'Finished', take_folder: 'C:\\captures\\demo1\\chain_03_b0\\take0000' }),
    ]);
    await expect(page.locator('#cancel-render-btn')).toHaveText('Cancel (2)');
    await page.locator('#cancel-render-btn').click();
    expect(await invocationsFor(page, 'cancel_render_jobs')).toEqual([{ jobIds: ['0', '1'] }]);
  });

  test('Delete Take folders lists each ticked take once and skips a busy one', async ({ page }) => {
    const A = 'D:/captures/s1/chain_01_b0/take0000';
    const B = 'D:/captures/s1/chain_02_b0/take0000';
    await gotoHarness(page);
    await page.evaluate(() => {
      window.__mockInvokeHandlers.measure_take_folders = ({ takeFolders }) =>
        takeFolders.map((f) => ({ take_folder: f, take_key: 'k', size_bytes: 1024 }));
    });
    await emitSnapshot(page, [
      job({ id: '0', status: 'Finished', take_folder: A }),
      job({ id: '1', status: 'Finished', take_folder: A, stream: 'HUD ONLY' }),
      job({ id: '2', status: 'Rendering', take_folder: B }),
    ]);
    await expect(page.locator('#delete-takes-render-btn')).toHaveText('Delete Take folders (1)');
    await page.locator('#delete-takes-render-btn').click();
    await expect(page.locator('#themed-confirm-modal')).toBeVisible();
    expect(await invocationsFor(page, 'measure_take_folders')).toEqual([{ takeFolders: [A] }]);
    await page.locator('#themed-confirm-cancel-btn').click();
  });

  test('Clear Queue empties an idle list without asking', async ({ page }) => {
    await gotoHarness(page);
    await expect(page.locator('#clear-render-queue-btn')).toBeDisabled();
    await emitSnapshot(page, [job({ id: '0', status: 'Queued' }), job({ id: '1', status: 'Finished' })]);
    await page.locator('#clear-render-queue-btn').click();
    await expect(page.locator('#themed-confirm-modal')).toBeHidden();
    expect(await invocationsFor(page, 'clear_render_queue')).toEqual([undefined]);
  });

  test('Clear Queue asks first while a take renders, and Cancel keeps the list', async ({ page }) => {
    await gotoHarness(page);
    await emitSnapshot(page, [job({ id: '0', status: 'Rendering' }), job({ id: '1', status: 'Queued' })]);
    await page.locator('#clear-render-queue-btn').click();
    const modal = page.locator('#themed-confirm-modal');
    await expect(modal).toBeVisible();
    await expect(page.locator('#themed-confirm-message')).toHaveText('A take is rendering. Stop it and empty the list?');
    await page.locator('#themed-confirm-cancel-btn').click();
    expect(await invocationsFor(page, 'clear_render_queue')).toEqual([]);

    await page.locator('#clear-render-queue-btn').click();
    await page.locator('#themed-confirm-ok-btn').click();
    await expect.poll(() => invocationsFor(page, 'clear_render_queue')).toEqual([undefined]);
  });
});

test.describe('Skip (keep original) toggle', () => {
  test('the Skip checkbox only appears for an OBS-shaped, Queued job', async ({ page }) => {
    await gotoHarness(page);
    await emitSnapshot(page, [
      job({ id: '0', status: 'Queued', skip_available: true }),
      job({ id: '1', status: 'Queued', skip_available: false }),
      job({ id: '2', status: 'Rendering', skip_available: true }),
    ]);
    await expect(page.locator('tr[data-job-id="0"] .render-job-skip-checkbox')).toHaveCount(1);
    await expect(page.locator('tr[data-job-id="1"] .render-job-skip-checkbox')).toHaveCount(0);
    await expect(page.locator('tr[data-job-id="2"] .render-job-skip-checkbox')).toHaveCount(0);
  });

  test('checking Skip calls set_render_job_codec with source_copy', async ({ page }) => {
    await gotoHarness(page);
    await emitSnapshot(page, [job({ id: '0', status: 'Queued', skip_available: true, codec_id: 'prores' })]);
    await page.locator('tr[data-job-id="0"] .render-job-skip-checkbox').check();
    const calls = await invocationsFor(page, 'set_render_job_codec');
    expect(calls).toEqual([{ jobId: '0', codec: 'source_copy', customCodecArgs: '' }]);
  });

  test("unchecking Skip restores the job's own prior codec, not whatever the panel currently shows", async ({ page }) => {
    await gotoHarness(page);
    await emitSnapshot(page, [job({ id: '0', status: 'Queued', skip_available: true, codec_id: 'prores' })]);
    await page.locator('tr[data-job-id="0"] .render-job-skip-checkbox').check();
    await emitSnapshot(page, [job({ id: '0', status: 'Queued', skip_available: true, codec_id: 'source_copy' })]);
    await page.selectOption('#render-codec-select', 'dnxhr');
    await page.locator('tr[data-job-id="0"] .render-job-skip-checkbox').uncheck();

    const calls = await invocationsFor(page, 'set_render_job_codec');
    const last = calls[calls.length - 1];
    expect(last).toEqual({ jobId: '0', codec: 'prores', customCodecArgs: '' });
  });

  test('settings summary omits the fps suffix once Skip is confirmed', async ({ page }) => {
    await gotoHarness(page);
    await emitSnapshot(page, [
      job({ id: '0', status: 'Queued', skip_available: true, codec_id: 'source_copy', settings_summary: 'Skip (Keep Original)' }),
    ]);
    await expect(page.locator('tr[data-job-id="0"] .rj-settings')).toContainText('Skip (Keep Original)');
    await expect(page.locator('tr[data-job-id="0"] .rj-settings')).not.toContainText('fps');
  });
});

test.describe('issue #438 — render history per take', () => {
  test('a new take says New; a rendered one counts and lists its attempts', async ({ page }) => {
    await gotoHarness(page);
    await emitSnapshot(page, [
      job({ id: '0', history: [] }),
      job({
        id: '1',
        history: [
          { started_unix_ms: Date.UTC(2026, 8, 29, 10), stream: 'all', codec: 'prores', fps: 300, outcome: 'finished', output_path: 'D:/o/a.mov', output_size_bytes: 1048576, output_exists: true },
          { started_unix_ms: Date.UTC(2026, 8, 29, 11), stream: 'all', codec: 'h264', fps: 120, outcome: 'finished', output_path: 'D:/o/b.mp4', output_size_bytes: 1048576, output_exists: true },
        ],
      }),
    ]);
    await expect(page.locator('tr[data-job-id="0"] .rj-history')).toHaveText('New');
    const cell = page.locator('tr[data-job-id="1"] .rj-history');
    await expect(cell).toContainText('Rendered ×2, 2 MB');
    await expect(cell.locator('.rj-history-list')).toBeHidden();
    await cell.locator('.rj-history-toggle').click();
    await expect(cell.locator('.rj-history-list div')).toHaveCount(2);
    await expect(cell.locator('.rj-history-list div').first()).toContainText('h264 @ 120fps · b.mp4');

    // Still open after a snapshot that changes something else.
    await emitSnapshot(page, [job({ id: '0', history: [], progress: 5 }), job({
      id: '1',
      history: [
        { started_unix_ms: Date.UTC(2026, 8, 29, 10), stream: 'all', codec: 'prores', fps: 300, outcome: 'finished', output_path: 'D:/o/a.mov', output_size_bytes: 1048576, output_exists: true },
        { started_unix_ms: Date.UTC(2026, 8, 29, 11), stream: 'all', codec: 'h264', fps: 120, outcome: 'finished', output_path: 'D:/o/b.mp4', output_size_bytes: 1048576, output_exists: true },
        { started_unix_ms: Date.UTC(2026, 8, 29, 12), stream: 'all', codec: 'prores', fps: 300, outcome: 'interrupted', output_exists: false },
      ],
    })]);
    await expect(cell).toContainText('Interrupted');
    await expect(cell.locator('.rj-history-list')).toBeVisible();
  });
});

test.describe('issue #40 — sortable columns and the batch progress bar', () => {
  test('a header click sorts the rows, again reverses, a third time restores job order', async ({ page }) => {
    await gotoHarness(page);
    await emitSnapshot(page, [
      job({ id: '0', name: 'b-clip' }),
      job({ id: '1', name: 'a-clip' }),
      job({ id: '2', name: 'c-clip' }),
    ]);
    const names = () => page.locator('#render-jobs-tbody .rj-name').allTextContents();
    const header = page.locator('th[data-sort="name"]');

    await header.click();
    expect(await names()).toEqual(['a-clip', 'b-clip', 'c-clip']);
    await expect(header).toHaveText(/▲$/);
    await header.click();
    expect(await names()).toEqual(['c-clip', 'b-clip', 'a-clip']);
    await header.click();
    expect(await names()).toEqual(['b-clip', 'a-clip', 'c-clip']);
    await expect(header).toHaveText('Clip Name');
  });

  test('a sorted row keeps its own nodes when a snapshot moves it', async ({ page }) => {
    await gotoHarness(page);
    await emitSnapshot(page, [job({ id: '0', status: 'Queued' }), job({ id: '1', status: 'Rendering', progress: 5 })]);
    await page.locator('th[data-sort="status"]').click();
    await page.evaluate(() => { window.__row0 = document.querySelector('tr[data-job-id="0"]'); });
    await emitSnapshot(page, [job({ id: '0', status: 'Rendering', progress: 1 }), job({ id: '1', status: 'Finished', progress: 100 })]);
    expect(await page.evaluate(() => window.__row0 === document.querySelector('tr[data-job-id="0"]'))).toBe(true);
    expect(await page.locator('#render-jobs-tbody tr').first().getAttribute('data-job-id')).toBe('0');
  });

  test('the batch bar averages the batch and hides with no jobs', async ({ page }) => {
    await gotoHarness(page);
    await expect(page.locator('#render-batch-progress')).toBeHidden();
    await emitSnapshot(page, [job({ id: '0', status: 'Finished', progress: 100 }), job({ id: '1', status: 'Rendering', progress: 50 })]);
    await expect(page.locator('#render-batch-progress')).toBeVisible();
    await expect(page.locator('#render-batch-progress-label')).toHaveText('Batch 75%');
  });
});

test.describe('issue #644 — Delete Take', () => {
  const TAKE = 'D:/captures/session_1/chain_01_b0/take0000';

  test('is off while either half of the take is rendering, saying which; a queued take stays on', async ({ page }) => {
    await gotoHarness(page);
    await emitSnapshot(page, [
      job({ id: '0', status: 'Finished', take_folder: TAKE }),
      job({ id: '1', status: 'Rendering', take_folder: TAKE, stream: 'HUD ONLY' }),
    ]);
    const finishedBtn = page.locator('tr[data-job-id="0"] .render-job-delete-take-btn');
    const pairBtn = page.locator('tr[data-job-id="1"] .render-job-delete-take-btn');
    await expect(finishedBtn).toBeDisabled();
    await expect(finishedBtn).toHaveAttribute('title', 'Another stream of this take is rendering. Cancel it first.');
    await expect(pairBtn).toBeDisabled();
    await expect(pairBtn).toHaveAttribute('title', 'This job is rendering. Cancel it first.');

    await emitSnapshot(page, [
      job({ id: '0', status: 'Finished', take_folder: TAKE }),
      job({ id: '1', status: 'Queued', take_folder: TAKE, stream: 'HUD ONLY' }),
    ]);
    await expect(finishedBtn).toBeEnabled();
    await expect(pairBtn).toBeEnabled();
  });

  test('the confirm says a queued render leaves the queue', async ({ page }) => {
    await gotoHarness(page);
    await page.evaluate(() => {
      window.__mockInvokeHandlers.measure_take_folders = ({ takeFolders }) =>
        takeFolders.map((f) => ({ take_folder: f, take_key: 'k', size_bytes: 1024 }));
    });
    await emitSnapshot(page, [job({ id: '0', status: 'Queued', take_folder: TAKE })]);
    await page.locator('tr[data-job-id="0"] .render-job-delete-take-btn').click();
    await expect(page.locator('#themed-confirm-footer')).toHaveText(
      'Its queued render is removed from the queue. The space comes back when the Recycle Bin is emptied. The render history stays.',
    );
    await page.locator('#themed-confirm-cancel-btn').click();
  });

  test('confirms with the folder and its size, then sends the take once', async ({ page }) => {
    await gotoHarness(page);
    await page.evaluate((take) => {
      window.__mockInvokeHandlers.measure_take_folders = ({ takeFolders }) =>
        takeFolders.map((f) => ({ take_folder: f, take_key: 'session_1/chain_01_b0', size_bytes: 3 * 1024 * 1024 * 1024 }));
      window.__mockInvokeHandlers.delete_take_folders = ({ takeFolders }) => ({
        deleted: takeFolders.map((f) => ({ take_folder: f, take_key: 'k', size_bytes: 3 * 1024 * 1024 * 1024 })),
        failed: [],
      });
      window.__take = take;
    }, TAKE);
    await emitSnapshot(page, [
      job({ id: '0', status: 'Finished', take_folder: TAKE }),
      job({ id: '1', status: 'Finished', take_folder: TAKE, stream: 'HUD ONLY' }),
    ]);

    await page.locator('tr[data-job-id="0"] .render-job-delete-take-btn').click();
    const modal = page.locator('#themed-confirm-modal');
    await expect(modal).toBeVisible();
    await expect(page.locator('#themed-confirm-message')).toHaveText('Move this take (3.00 GB) to the Recycle Bin?');
    await expect(modal).toContainText(TAKE);
    // Every Delete Take is off while the dialog is open.
    await expect(page.locator('tr[data-job-id="1"] .render-job-delete-take-btn')).toBeDisabled();

    await page.locator('#themed-confirm-ok-btn').click();
    await expect.poll(() => invocationsFor(page, 'delete_take_folders')).toEqual([{ takeFolders: [TAKE] }]);
    expect(await invocationsFor(page, 'measure_take_folders')).toEqual([{ takeFolders: [TAKE] }]);
    await expect(page.locator('#toast-container')).toContainText('Take moved to the Recycle Bin (3.00 GB).');
    await expect(page.locator('#render-take-delete-progress')).toBeHidden();
  });

  test('cancelling the confirm deletes nothing', async ({ page }) => {
    await gotoHarness(page);
    await page.evaluate(() => {
      window.__mockInvokeHandlers.measure_take_folders = ({ takeFolders }) =>
        takeFolders.map((f) => ({ take_folder: f, take_key: 'k', size_bytes: 1024 }));
    });
    await emitSnapshot(page, [job({ id: '0', status: 'Cancelled', take_folder: TAKE })]);
    await page.locator('tr[data-job-id="0"] .render-job-delete-take-btn').click();
    await page.locator('#themed-confirm-cancel-btn').click();
    await expect(page.locator('tr[data-job-id="0"] .render-job-delete-take-btn')).toBeEnabled();
    expect(await invocationsFor(page, 'delete_take_folders')).toEqual([]);
  });
});
