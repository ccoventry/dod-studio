// Every main tab fits the window width: no horizontal page scrollbar at the
// two common sizes (#46). Drives the real app shell against mocked IPC.
import { test, expect } from './fixtures.js';

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
