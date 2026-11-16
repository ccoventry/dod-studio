// Every main tab fits the window width: no horizontal page scrollbar at the
// common sizes, and with long names and paths no table needs a sideways
// scrollbar of its own, Highlight Details (POV and HLTV demos) and the
// Master Queue's badge row included (#46). Drives the real app shell against
// mocked IPC.
import { readFileSync } from 'node:fs';
import { test, expect } from './fixtures.js';
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
            get_settings: () => ({ setup_dismissed: true, pinned_folders: [] }),
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

    // Demo 0 is an HLTV demo (its table has the Player column); demo 2 is a
    // POV one. Both must fit, and a 1920 window's default split keeps Details.
    test('Highlight Details fits for an HLTV and a POV demo, and keeps Details on a wide window', async ({ page }) => {
      await bootWithDemos(page, 40);
      await page.addStyleTag({ content: '#setup-card{display:none!important}' });
      const detailWrapper = page.locator('#detail-streaks-table').locator('xpath=..');
      for (const row of [0, 2]) {
        await page.locator('#master-demo-table-body tr').nth(row).click();
        await expect(page.locator('#detail-streaks-table th.col-stack')).toHaveCount(1);
        expect(await page.locator('#detail-streaks-table th', { hasText: 'Player' }).count() > 0, `row ${row}`).toBe(row === 0);
        expect(await detailWrapper.evaluate((el) => el.scrollWidth - el.clientWidth), `row ${row}`).toBeLessThanOrEqual(0);
      }
      const details = page.locator('#detail-streaks-table th.col-details');
      if (size.width >= 1920) await expect(details).toBeVisible();
      else await expect(details).toBeHidden();
    });

    // Demo 1 has every badge: the name keeps some room, nothing paints over the
    // count columns, and the cell stays inside the table. The name cell is
    // picked by position (td:nth-child(2)) because master_pane.js has no class
    // for it and #1284 is changing that file.
    test('Master Queue badges stay inside the name cell', async ({ page }) => {
      await bootWithDemos(page, 40);
      await page.addStyleTag({ content: '#setup-card{display:none!important}' });
      const result = await page.locator('#master-demo-table-body tr').nth(1).evaluate((tr) => {
        const cell = tr.querySelector('td:nth-child(2)');
        const next = tr.querySelector('td:nth-child(3)');
        const c = cell.getBoundingClientRect();
        const kids = [...cell.children].map((k) => ({ cls: k.className, right: k.getBoundingClientRect().right, w: k.getBoundingClientRect().width }));
        return { cellRight: c.right, nextLeft: next.getBoundingClientRect().left, nameWidth: kids[0].w, kids, count: cell.children.length };
      });
      expect(result.count, 'name plus badges').toBeGreaterThan(2);
      expect(result.cellRight).toBeLessThanOrEqual(result.nextLeft + 1);
      expect(result.nameWidth).toBeGreaterThanOrEqual(40);
    });

    test('Render, Configuration and the other tabs have no page scrollbar with long paths and a full queue', async ({ page }) => {
      await bootWithDemos(page, 12);
      await page.locator('[data-capture-subtab="render"]').click();
      await page.evaluate((jobs) => window.__mockEmit('render_jobs_snapshot', jobs), makeRenderJobs(30));
      await page.locator('#render-jobs-table tbody tr').nth(29).waitFor();
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
