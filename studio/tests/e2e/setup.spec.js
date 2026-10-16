// setup_card.js and setup_window.js — the Capture page's setup checklist and
// the Setup window (#599).
//
// Pins what a new user sees: the card's count and one fix button per row, the
// card going away once the required steps are set, the window's Steam-install
// warning and build chip, folders listed with their free space, Skip and
// Finish.
import { test, expect } from '@playwright/test';

async function loadHarness(page) {
  await page.goto('/tests/e2e/setup.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

const calls = (page) => page.evaluate(() => window.__calls);

test('the card counts what is done and offers a fix on each missing row', async ({ page }) => {
  await loadHarness(page);
  const card = page.locator('#setup-card');
  await expect(card).toBeVisible();
  await expect(card.locator('.setup-chip')).toHaveText('2 of 6');
  await expect(card.locator('.setup-row')).toHaveCount(6);
  await expect(card.locator('.setup-row.done')).toHaveCount(2);

  await card.locator('.setup-row[data-step="hlae"] button').click();
  expect(await calls(page)).toContainEqual(['fix', 'hlae']);

  // A done row's button opens the window on that step instead.
  await card.locator('.setup-row[data-step="captures"] button').click();
  await expect(page.locator('#setup-modal')).toBeVisible();
  await expect(page.locator('#setup-step-title')).toHaveText('4. Choose where captures go');
  await expect(page.locator('#setup-step-content')).toContainText('412 GB free on D:');
});

test('the card hides once every required step is set', async ({ page }) => {
  await loadHarness(page);
  await page.evaluate(() => {
    window.__snapshot.hlaePath = 'C:\\HLAE\\HLAE.exe';
    window.__snapshot.hlaeState = 'ok';
    window.__draw();
  });
  await expect(page.locator('#setup-card')).toBeHidden();
});

test("the Half-Life step warns about Steam's own install and names the build", async ({ page }) => {
  await loadHarness(page);
  await page.evaluate(() => window.__win.open(0));
  const content = page.locator('#setup-step-content');
  await expect(content).toContainText('25th Anniversary build');
  await expect(page.locator('#setup-hl-play-warning')).toBeVisible();
  await content.locator('#setup-hl-browse').click();
  expect(await calls(page)).toContainEqual(['pickHl']);
  // Done, so the footer says Next; the HLAE step is not, so it says Skip.
  await expect(page.locator('#setup-next-btn')).toHaveText('Next');
  await page.locator('#setup-next-btn').click();
  await expect(page.locator('#setup-next-btn')).toHaveText('Skip this step');
  await page.locator('#setup-hlae-get').click();
  expect(await calls(page)).toContainEqual(['openHlaePage']);
});

test('the demo step suggests the movie copy\'s dod folder and Finish hands it over', async ({ page }) => {
  await loadHarness(page);
  await page.evaluate(() => window.__win.open('demos'));
  await expect(page.locator('#setup-step-content')).toContainText('C:\\Steam\\steamapps\\common\\Half-Life\\dod');
  await page.locator('#setup-demos-browse').click();
  await expect(page.locator('#setup-step-content')).toContainText('E:\\Demos');
  await expect(page.locator('#setup-next-btn')).toHaveText('Finish');
  await page.locator('#setup-next-btn').click();
  await expect(page.locator('#setup-modal')).toBeHidden();
  expect(await calls(page)).toContainEqual(['onFinish', 'E:\\Demos']);
});

test('Skip setup for now closes the window', async ({ page }) => {
  await loadHarness(page);
  await page.evaluate(() => window.__win.open(2));
  await page.locator('#setup-skip-btn').click();
  await expect(page.locator('#setup-modal')).toBeHidden();
  expect(await calls(page)).toContainEqual(['onSkip']);
});
