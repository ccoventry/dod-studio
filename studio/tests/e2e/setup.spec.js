// setup_card.js and setup_window.js — the Highlights page's setup checklist and
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

test('the Half-Life step offers Make a movie copy once hl.exe has a dod beside it (#939)', async ({ page }) => {
  await loadHarness(page);
  await page.evaluate(() => window.__win.open(0));
  // No folders listed yet: nothing to copy.
  await expect(page.locator('#setup-hl-movie-copy')).toHaveCount(0);
  await page.evaluate(() => {
    Object.assign(window.__snapshot, { gameFolder: 'dod', gameFolders: ['dod', 'valve'] });
    window.__win.render();
  });
  await page.locator('#setup-hl-movie-copy').click();
  expect(await calls(page)).toContainEqual(['openMovieCopy']);
  await page.evaluate(() => {
    window.__snapshot.hlState = 'not_found';
    window.__win.render();
  });
  await expect(page.locator('#setup-hl-movie-copy')).toHaveCount(0);
});

test("the Half-Life step picks a copy of the dod folder, and the note goes once it's picked", async ({ page }) => {
  await loadHarness(page);
  await page.evaluate(() => {
    Object.assign(window.__snapshot, { gameFolder: 'dod', gameFolders: ['dod', 'dod_movies', 'valve'] });
    window.__win.open(0);
  });
  await expect(page.locator('#setup-hl-play-warning')).toContainText('copy the dod folder');
  await page.locator('#setup-game-folder').selectOption('dod_movies');
  expect(await calls(page)).toContainEqual(['setGameFolder', 'dod_movies']);
  await page.evaluate(() => {
    window.__snapshot.gameFolder = 'dod_movies';
    window.__win.render();
  });
  await expect(page.locator('#setup-hl-play-warning')).toHaveCount(0);
  await expect(page.locator('#setup-game-folder')).toHaveValue('dod_movies');
});

test('a saved Game Folder this install lacks is named as missing, with no language warning', async ({ page }) => {
  await loadHarness(page);
  await page.evaluate(() => {
    Object.assign(window.__snapshot, { gameFolder: 'dod_movies', gameFolders: ['dod'], gameFolderLanguage: null });
    window.__win.open(0);
  });
  await expect(page.locator('#setup-hl-folder-missing')).toContainText('no "dod_movies" folder beside hl.exe');
  await expect(page.locator('#setup-game-folder')).toHaveValue('dod_movies');
  await expect(page.locator('#setup-game-folder option:checked')).toHaveText('dod_movies (not found beside hl.exe)');
  await expect(page.locator('#setup-hl-language-warning')).toHaveCount(0);
  await page.locator('#setup-game-folder').selectOption('dod');
  expect(await calls(page)).toContainEqual(['setGameFolder', 'dod']);
});

test('the FFmpeg step lets a fresh install pick ffmpeg.exe before pointing HLAE at it', async ({ page }) => {
  await loadHarness(page);
  // A fresh install: the bare `ffmpeg` is on no PATH folder, HLAE has no ini.
  await page.evaluate(() => {
    Object.assign(window.__snapshot, {
      hlaePath: 'C:\\HLAE\\HLAE.exe', hlaeState: 'ok',
      ffmpegUsable: false, ffmpegCanLink: true, appFfmpeg: '', appFfmpegProblem: null, hlaeBundledFfmpeg: null,
    });
    window.__win.open('ffmpeg');
  });
  const content = page.locator('#setup-step-content');
  await expect(content).toContainText("Not found: it isn't on the Windows PATH");
  await expect(content).toContainText('Pick ffmpeg.exe above first');
  await expect(page.locator('#setup-ffmpeg-link')).toHaveCount(0);
  await page.locator('#setup-ffmpeg-browse').click();
  await page.locator('#setup-ffmpeg-get').click();
  expect(await calls(page)).toContainEqual(['pickFfmpeg']);
  expect(await calls(page)).toContainEqual(['openFfmpegPage']);
  await expect(page.locator('#setup-next-btn')).toHaveText('Skip this step');

  // Picked: the HLAE row now offers the link.
  await page.evaluate(() => {
    window.__snapshot.appFfmpeg = 'C:\\Programs\\FFmpeg\\ffmpeg.exe';
    window.__win.render();
  });
  await expect(content).toContainText('Found FFmpeg.');
  await expect(page.locator('#setup-ffmpeg-link')).toBeVisible();
});

test("the FFmpeg step offers HLAE's own copy when DoD Studio has none", async ({ page }) => {
  await loadHarness(page);
  await page.evaluate(() => {
    Object.assign(window.__snapshot, {
      hlaePath: 'C:\\HLAE\\HLAE.exe', hlaeState: 'ok',
      ffmpegUsable: true, ffmpegCanLink: false, appFfmpeg: '',
      hlaeBundledFfmpeg: 'C:\\HLAE\\ffmpeg\\bin\\ffmpeg.exe',
    });
    window.__win.open('ffmpeg');
  });
  await expect(page.locator('#setup-step-content')).toContainText('HLAE has its own FFmpeg (C:\\HLAE\\ffmpeg\\bin\\ffmpeg.exe)');
  // HLAE can record video, but Render can't yet, so the step is not done.
  await expect(page.locator('#setup-next-btn')).toHaveText('Skip this step');
  await page.locator('#setup-ffmpeg-use-hlae').click();
  expect(await calls(page)).toContainEqual(['useHlaeFfmpeg']);
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
