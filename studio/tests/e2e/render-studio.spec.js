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

  test('clicking Start Render Batch calls start_queued_render with no payload of its own', async ({ page }) => {
    await gotoHarness(page);
    await emitSnapshot(page, [job({ id: '0', status: 'Queued' })]);
    await expect(page.locator('#start-render-btn')).toBeEnabled();

    await page.locator('#start-render-btn').click();

    const calls = await invocationsFor(page, 'start_queued_render');
    expect(calls).toEqual([undefined]);
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

  test('Reset All and Remove All start disabled, enable once an eligible row exists', async ({ page }) => {
    await gotoHarness(page);
    await expect(page.locator('#reset-all-render-btn')).toBeDisabled();
    await expect(page.locator('#remove-all-render-btn')).toBeDisabled();

    await emitSnapshot(page, [job({ id: '0', status: 'Queued' })]);
    // Nothing resettable yet (Queued isn't Cancelled/Finished/Error) but it
    // isn't Rendering either, so Remove All should already be live.
    await expect(page.locator('#reset-all-render-btn')).toBeDisabled();
    await expect(page.locator('#remove-all-render-btn')).toBeEnabled();

    await emitSnapshot(page, [job({ id: '0', status: 'Cancelled' })]);
    await expect(page.locator('#reset-all-render-btn')).toBeEnabled();
    await expect(page.locator('#remove-all-render-btn')).toBeEnabled();
  });

  test('Reset All and Remove All disable again once everything is Rendering', async ({ page }) => {
    await gotoHarness(page);
    await emitSnapshot(page, [job({ id: '0', status: 'Rendering' }), job({ id: '1', status: 'Rendering' })]);
    await expect(page.locator('#reset-all-render-btn')).toBeDisabled();
    await expect(page.locator('#remove-all-render-btn')).toBeDisabled();
  });

  test('clicking Reset All calls reset_all_render_jobs with no payload', async ({ page }) => {
    await gotoHarness(page);
    await emitSnapshot(page, [job({ id: '0', status: 'Error' })]);
    await page.locator('#reset-all-render-btn').click();
    const calls = await invocationsFor(page, 'reset_all_render_jobs');
    expect(calls).toEqual([undefined]);
  });

  test('clicking Remove All calls remove_non_rendering_render_jobs with no payload', async ({ page }) => {
    await gotoHarness(page);
    await emitSnapshot(page, [job({ id: '0', status: 'Finished' })]);
    await page.locator('#remove-all-render-btn').click();
    const calls = await invocationsFor(page, 'remove_non_rendering_render_jobs');
    expect(calls).toEqual([undefined]);
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
    expect(calls).toEqual([{ jobId: '0', codec: 'source_copy' }]);
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
    expect(last).toEqual({ jobId: '0', codec: 'prores' });
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
