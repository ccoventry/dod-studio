const { chromium } = require('playwright');
const fs = require('fs'), path = require('path');
const [R, OUT, ...only] = process.argv.slice(2);
const canvas = JSON.parse(fs.readFileSync(path.join(R, 'canvas.json'), 'utf8'));
const rjs = fs.readFileSync(path.join(__dirname, 'render.js'), 'utf8');
(async () => {
  const b = await chromium.launch();
  const ctx = await b.newContext({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 });
  const page = await ctx.newPage();
  const res = [];
  for (const f of canvas.order) {
    const bd = canvas.boards[f]; if (!bd || !['app', 'review'].includes(bd.page)) continue;
    if (only.length && !only.includes(f)) continue;
    let html = fs.readFileSync(path.join(R, f), 'utf8').replace('<script src="./support.js"></script>', '');
    html = html.replace('</body>', '<script>' + rjs + '</script></body>');
    await page.setViewportSize({ width: bd.w || 1440, height: 900 });
    let err = null; page.removeAllListeners('pageerror'); page.on('pageerror', e => err = e.message);
    await page.setContent(html, { waitUntil: 'load', timeout: 20000 }).catch(e => err = e.message);
    await page.waitForTimeout(300);
    const name = f.replace('.dc.html', '.png');
    await page.screenshot({ path: path.join(OUT, name), fullPage: true });
    res.push([bd.page, f, bd.title, name, err]);
  }
  fs.writeFileSync(path.join(OUT, 'index.json'), JSON.stringify(res, null, 1));
  await b.close();
  console.log(res.length, res.filter(r => r[4]).map(r => r[1] + ': ' + r[4]));
})();
