// info_tooltip.js — hover/focus tooltip that replaces the native `title`
// attribute for .info-icon (see #164: native tooltips need a motionless
// cursor for ~1s and silently reset on any movement).
import { test, expect } from '@playwright/test';

test.beforeEach(async ({ page }) => {
  await page.goto('/tests/e2e/info-tooltip.html');
  await page.waitForFunction(() => window.__harnessReady === true);
});

test.describe('hover and focus', () => {
  test('hovering an .info-icon shows its tooltip text', async ({ page }) => {
    const tooltip = page.locator('.info-tooltip');
    await expect(tooltip).not.toBeVisible();

    await page.locator('#basic-icon').hover();
    await expect(tooltip).toBeVisible();
    await expect(tooltip).toHaveText('Runs before the demo loads.');
  });

  test('moving the mouse off the icon hides the tooltip', async ({ page }) => {
    const tooltip = page.locator('.info-tooltip');
    await page.locator('#basic-icon').hover();
    await expect(tooltip).toBeVisible();

    // A real hover-jitter case: small in-target movement should not hide it.
    const box = await page.locator('#basic-icon').boundingBox();
    await page.mouse.move(box.x + 1, box.y + 1);
    await expect(tooltip).toBeVisible();

    await page.mouse.move(500, 500);
    await expect(tooltip).not.toBeVisible();
  });

  test('keyboard focus shows the tooltip, blur hides it', async ({ page }) => {
    const tooltip = page.locator('.info-tooltip');
    await page.locator('#basic-icon').focus();
    await expect(tooltip).toBeVisible();
    await expect(tooltip).toHaveText('Runs before the demo loads.');

    await page.locator('#basic-icon').blur();
    await expect(tooltip).not.toBeVisible();
  });

  test('an .info-icon with no tooltip text never shows a tooltip', async ({ page }) => {
    const tooltip = page.locator('.info-tooltip');
    await page.locator('#no-text-icon').hover();
    await expect(tooltip).not.toBeVisible();
  });
});

test.describe('viewport clamping', () => {
  test('clamps to the left edge instead of running off-screen', async ({ page }) => {
    const tooltip = page.locator('.info-tooltip');
    await page.locator('#top-left-icon').hover();
    await expect(tooltip).toBeVisible();
    const box = await tooltip.boundingBox();
    expect(box.x).toBeGreaterThanOrEqual(0);
  });

  test('clamps to the right edge and flips below when clipped at the top', async ({ page }) => {
    const tooltip = page.locator('.info-tooltip');
    const iconBox = await page.locator('#top-right-icon').boundingBox();
    await page.locator('#top-right-icon').hover();
    await expect(tooltip).toBeVisible();

    const viewport = page.viewportSize();
    const tipBox = await tooltip.boundingBox();
    expect(tipBox.x + tipBox.width).toBeLessThanOrEqual(viewport.width);
    // The icon sits at the very top of the viewport, so there's no room
    // above it — the tooltip must flip to render below instead.
    expect(tipBox.y).toBeGreaterThanOrEqual(iconBox.y + iconBox.height);
  });
});
