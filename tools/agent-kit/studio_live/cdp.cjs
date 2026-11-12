// Evaluates JavaScript in a running Studio's page over WebView2's remote debugging port
// and prints the result as JSON. The code runs as the body of an async function, so it
// can `await` and must `return` what it wants printed.
//
//   node cdp.cjs --port 9322 -e "return document.title"
//   node cdp.cjs --port 9322 --file steps.js
//   node cdp.cjs --port 9322 --watch 60 --out watch.log
//       every 2 s for 60 s, logs the capture footer and the toasts when they change
//
// Playwright comes from a studio/node_modules: --studio <dir> (default: this repo's studio/).
// Never call browser.close() over CDP: that closes Studio. The script just exits.
const fs = require('fs');
const path = require('path');

function parseArgs(argv) {
  const args = { port: 9322, studio: path.resolve(__dirname, '../../../studio') };
  for (let i = 0; i < argv.length; i += 1) {
    const flag = argv[i];
    const value = argv[i + 1];
    if (flag === '--port') args.port = Number(value);
    else if (flag === '--studio') args.studio = value;
    else if (flag === '-e') args.code = value;
    else if (flag === '--file') args.code = fs.readFileSync(value, 'utf8');
    else if (flag === '--watch') args.watch = Number(value);
    else if (flag === '--out') args.out = value;
    else continue;
    i += 1;
  }
  return args;
}

const WATCH_READ = () => {
  const texts = (q) => [...document.querySelectorAll(q)]
    .map((e) => e.innerText.replace(/\s+/g, ' ').trim()).filter(Boolean);
  return { footer: texts('#batch-status').join(' | '), toasts: texts('.toast').join(' || ') };
};

async function watch(page, seconds, out) {
  const end = Date.now() + seconds * 1000;
  let last = '';
  while (Date.now() < end) {
    const state = await page.evaluate(WATCH_READ).catch((e) => ({ error: String(e) }));
    const line = JSON.stringify(state);
    if (line !== last) {
      const stamped = `${new Date().toISOString().slice(11, 19)} ${line}\n`;
      if (out) fs.appendFileSync(out, stamped);
      else process.stdout.write(stamped);
      last = line;
    }
    await new Promise((r) => setTimeout(r, 2000));
  }
}

(async () => {
  const args = parseArgs(process.argv.slice(2));
  const { chromium } = require(require.resolve('playwright', { paths: [args.studio] }));
  const browser = await chromium.connectOverCDP(`http://127.0.0.1:${args.port}`);
  const page = browser.contexts()[0].pages()[0];
  if (args.watch) {
    await watch(page, args.watch, args.out);
  } else {
    if (!args.code) throw new Error('nothing to run: pass -e "<code>" or --file <file>');
    const result = await page.evaluate(`(async () => { ${args.code} })()`);
    console.log(typeof result === 'string' ? result : JSON.stringify(result, null, 1));
  }
  process.exit(0);
})().catch((e) => {
  console.error(String(e));
  process.exit(1);
});
