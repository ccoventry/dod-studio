// Every main tab fits the window width: no horizontal page scrollbar at the
// two common sizes (#46). Drives the real app shell against mocked IPC.
import { readFileSync } from 'node:fs';
import { test, expect } from '@playwright/test';
import { bootWithDemos, makeRenderJobs } from './sizing-data.js';

const SIZES = [
  { width: 1280, height: 720 },
  { width: 1920, height: 1080 },
];
const TABS = ['workspace', 'demo-auditor', 'demo-analyzer', 'hd-textures', 'overviews', 'blender'];

for (const size of SIZES) {
  test.describe(`${size.width}x${size.height}`, () => {
    test.use({ viewport: size });

    for (const tab of TABS) {
      test(`${tab} has no horizontal page scrollbar`, async ({ page }) => {
        await page.addInitScript(() => {
          window.__mockInvokeHandlers = {
            get_settings: () => ({ pinned_folders: [] }),
            scan_demo_folders: () => [],
          };
        });
        await page.goto('/index.html');
        await page.locator(`.nav-tab-btn[data-nav="${tab}"]`).click();
        await page.waitForTimeout(150);
        const overflow = await page.evaluate(() => {
          const root = document.documentElement;
          return {
            html: root.scrollWidth - root.clientWidth,
            body: document.body.scrollWidth - document.body.clientWidth,
          };
        });
        expect(overflow.html).toBeLessThanOrEqual(0);
        expect(overflow.body).toBeLessThanOrEqual(0);
      });
    }
  });
}

// The same check with realistic, awkward data (#46): 40 demos with long names
// and paths, long settings, a Render queue, and an analysed demo. A table may
// not need a sideways scrollbar of its own either.
const WIDE_SIZES = [...SIZES, { width: 2560, height: 1440 }];

async function pageOverflow(page) {
  return page.evaluate(() => {
    const root = document.documentElement;
    return { html: root.scrollWidth - root.clientWidth, body: document.body.scrollWidth - document.body.clientWidth };
  });
}

for (const size of WIDE_SIZES) {
  test.describe(`with data ${size.width}x${size.height}`, () => {
    test.use({ viewport: size });

    test('Master Queue and Highlight Details fit their panes with long names', async ({ page }) => {
      await bootWithDemos(page, 40);
      await page.addStyleTag({ content: '#setup-card{display:none!important}' });
      const queueWrapper = page.locator('#master-demo-table').locator('xpath=..');
      expect(await queueWrapper.evaluate((el) => el.scrollWidth - el.clientWidth)).toBeLessThanOrEqual(0);
      const detailWrapper = page.locator('#detail-streaks-table').locator('xpath=..');
      expect(await detailWrapper.evaluate((el) => el.scrollWidth - el.clientWidth)).toBeLessThanOrEqual(0);
      const o = await pageOverflow(page);
      expect(o.html).toBeLessThanOrEqual(0);
      expect(o.body).toBeLessThanOrEqual(0);
    });

    test('Render, Configuration and the other tabs have no page scrollbar with long paths and a full queue', async ({ page }) => {
      await bootWithDemos(page, 12);
      await page.locator('[data-capture-subtab="render"]').click();
      await page.evaluate((jobs) => window.__mockEmit('render_jobs_snapshot', jobs), makeRenderJobs(30));
      await page.waitForTimeout(100);
      expect((await pageOverflow(page)).html).toBeLessThanOrEqual(0);
      const jobsWrapper = page.locator('#render-jobs-table').locator('xpath=..');
      expect(await jobsWrapper.evaluate((el) => el.scrollWidth - el.clientWidth)).toBeLessThanOrEqual(0);
      await page.locator('[data-capture-subtab="configuration"]').click();
      for (const tab of await page.locator('.config-tab-btn').evaluateAll((b) => b.map((x) => x.dataset.tab))) {
        await page.locator(`.config-tab-btn[data-tab="${tab}"]`).click();
        expect((await pageOverflow(page)).html, tab).toBeLessThanOrEqual(0);
      }
      for (const tab of TABS.slice(1)) {
        await page.locator(`.nav-tab-btn[data-nav="${tab}"]`).click();
        expect((await pageOverflow(page)).html, tab).toBeLessThanOrEqual(0);
      }
    });

    test('an analysed demo with a long file name stays inside the Summary tab', async ({ page }) => {
      await bootWithDemos(page, 12);
      const report = JSON.parse(readFileSync('tests/e2e/fixtures/analyzer-report.json', 'utf8'));
      report.file_name = 'wsod25_group_stage_round_0_vs_the_extraordinarily_long_clan_name_gaming_dod_anzio_final_pov_m00cat.dem';
      report.file_path = `D:/Demos/${report.file_name}`;
      report.file_dir = 'D:/Demos';
      report.file_size_mb = 80;
      report.file_created_unix_secs = 1760000000;
      await page.evaluate((r) => { window.__mockInvokeHandlers.analyze_demo_full = () => r; }, report);
      await page.locator('.nav-tab-btn[data-nav="demo-analyzer"]').click();
      await page.locator('#analyzer-quick-links .quicklink-row').first().click();
      await page.locator('#analyzer-demo-tbody tr[data-path]').first().click();
      await page.locator('.analyzer-kv-table .kv-value').first().waitFor();
      // The file name wraps inside its column instead of running under the next one.
      const clipped = await page.locator('.analyzer-kv-table .kv-value').evaluateAll(
        (cells) => cells.filter((c) => c.scrollWidth > c.clientWidth + 1).length,
      );
      expect(clipped).toBe(0);
      expect((await pageOverflow(page)).html).toBeLessThanOrEqual(0);
    });
  });
}

test.describe('Highlight Details at the narrowest common window (#46)', () => {
  test.use({ viewport: { width: 1280, height: 720 } });

  test('stacks Time and Dur. into one column and needs no inner scrollbar', async ({ page }) => {
    await bootWithDemos(page, 5);
    await expect(page.locator('#detail-streaks-table th.col-stack')).toBeVisible();
    await expect(page.locator('#detail-streaks-table th.col-wide').first()).toBeHidden();
    await expect(page.locator('#detail-streaks-table .streak-stack-when').first()).toContainText('·');
  });
});
