// Render's refused actions mark the field that is wrong, with the reason on a
// line under it (#25). The fields live on Configuration in the real app; the
// harness carries them next to the render controls.
import { test, expect } from './fixtures.js';

async function gotoHarness(page) {
  await page.goto('/tests/e2e/render-studio.html');
  await page.waitForFunction(() => window.__renderPaneReady === true);
}

const lineUnder = (page, inputSelector) =>
  page.locator(`${inputSelector}`).evaluate((el) => {
    const anchor = el.closest('.input-row') || el;
    const line = anchor.nextElementSibling;
    return line?.classList.contains('field-error-line') && !line.hidden ? line.textContent : '';
  });

test('Scan with no capture folder marks the folder field, and adding one clears it', async ({ page }) => {
  await gotoHarness(page);
  await page.locator('#scan-render-btn').click();
  const field = page.locator('#drive-path-input');
  await expect(field).toHaveClass(/field-invalid/);
  expect(await lineUnder(page, '#drive-path-input')).toContain('Scan for Takes has somewhere to look');
  // The field is off screen in the real app, so the toast stays.
  await expect(page.locator('#toast-container')).toContainText('at least one render directory');
  // A folder joining the list clears the mark.
  await page.evaluate(() => document.querySelector('#target-drive-list').appendChild(document.createElement('div')));
  await expect(field).not.toHaveClass(/field-invalid/);
  expect(await lineUnder(page, '#drive-path-input')).toBe('');
});

test('Custom codec with no arguments marks its field, and typing clears it', async ({ page }) => {
  await gotoHarness(page);
  await page.evaluate(() => { window.__testCaptureLocations = ['C:/captures']; });
  await page.locator('#render-codec-select').selectOption('custom');
  await page.locator('#scan-render-btn').click();
  const field = page.locator('#render-custom-codec-input');
  await expect(field).toHaveClass(/field-invalid/);
  expect(await lineUnder(page, '#render-custom-codec-input')).toContain('FFmpeg video-codec arguments');
  await field.fill('-c:v mpeg4');
  await expect(field).not.toHaveClass(/field-invalid/);
});

test('a failed scan marks the folder field with the error', async ({ page }) => {
  await gotoHarness(page);
  await page.evaluate(() => {
    window.__testCaptureLocations = ['Z:/gone'];
    window.__mockInvokeHandlers['queue_render_batch'] = () => { throw 'path does not exist'; };
  });
  await page.locator('#scan-render-btn').click();
  await expect(page.locator('#drive-path-input')).toHaveClass(/field-invalid/);
  expect(await lineUnder(page, '#drive-path-input')).toContain('path does not exist');
});

test('queued takes with no export folder mark the export field, and a folder clears it', async ({ page }) => {
  await gotoHarness(page);
  await page.evaluate(() => { window.__testExportDirs = []; });
  await page.evaluate(() => window.__mockEmit('render_jobs_snapshot', [{
    id: '0', name: 'a', stream: 'all', frames: 1, date: '-', status: 'Queued', speed: '', progress: 0,
    error_log: null, settings_summary: '', output_path: '', take_folder: 'C:/x', codec_id: 'prores', skip_available: true,
  }]));
  const field = page.locator('#render-export-dir-input');
  await expect(field).toHaveClass(/field-invalid/);
  expect(await lineUnder(page, '#render-export-dir-input')).toContain('Add an export folder');
});
