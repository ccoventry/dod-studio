// web-analyzer/www holds hand-made copies of the analyzer's report code
// (#238): render.js from analyzer_pane.js, plus strings.js, analyzer_flags.js
// and steam_ids.js. Until the copy is replaced by a shared module, these
// tests make drift fail here instead of shipping a web analyzer that quietly
// behaves differently.
import { describe, it, expect } from 'vitest';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { STRINGS as DESKTOP } from './strings.js';
import { STRINGS as WEB } from '../../web-analyzer/www/strings.js';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const read = (...p) => fs.readFileSync(path.join(repo, ...p), 'utf8').replace(/\r\n/g, '\n');

/** Top-level functions by name, `export` and blank lines ignored. */
function functions(source) {
  const out = new Map();
  const lines = source.split('\n');
  for (let i = 0; i < lines.length; i++) {
    const m = lines[i].match(/^(?:export\s+)?(?:async\s+)?function\s+([A-Za-z0-9_$]+)\s*\(/);
    if (!m) continue;
    let j = i;
    while (j < lines.length && lines[j] !== '}') j++;
    out.set(m[1], lines.slice(i, j + 1)
      .map((l) => l.replace(/^export\s+/, '').trimEnd())
      .filter((l) => l.trim() !== ''));
    i = j;
  }
  return out;
}

/**
 * The deliberate differences: lines only the desktop has, per function. Each
 * needs Tauri or a local file, which the browser build has neither of.
 */
const DESKTOP_ONLY_LINES = {
  // Display names come from the loc file over IPC; the web shows raw names.
  weaponName: [
    'const resolved = weaponDisplayNames && weaponDisplayNames[w];',
    'if (resolved) return resolved;',
  ],
  // The Kill Map draws on a map overview image loaded over IPC.
  renderActiveTab: ["case 'kill-map': renderKillMapTab(container); break;"],
  // A dropped file has no folder.
  renderSummaryTab: ['[STRINGS.ANALYZER.FILE_PATH_LABEL, esc(r.file_dir)],'],
};

describe('web-analyzer/www/render.js is a copy of analyzer_pane.js (#238)', () => {
  const desktop = functions(read('studio', 'src', 'analyzer_pane.js'));
  const web = functions(read('web-analyzer', 'www', 'render.js'));
  const shared = [...web.keys()].filter((name) => desktop.has(name));

  it('shares most of its functions', () => {
    expect(shared.length).toBeGreaterThan(25);
  });

  it.each(shared)('%s matches, apart from the listed desktop-only lines', (name) => {
    const only = new Set(DESKTOP_ONLY_LINES[name] || []);
    const d = desktop.get(name).filter((l) => !only.has(l.trim()));
    expect(web.get(name), `copy studio/src/analyzer_pane.js's ${name} into render.js`).toEqual(d);
  });

  it('every listed desktop-only line still exists', () => {
    for (const [name, lines] of Object.entries(DESKTOP_ONLY_LINES)) {
      const trimmed = desktop.get(name).map((l) => l.trim());
      for (const l of lines) expect(trimmed, `${name}: ${l}`).toContain(l);
    }
  });
});

describe('web-analyzer/www/strings.js is a copy of STRINGS.ANALYZER (#238)', () => {
  // The web build's empty state asks for a dropped file, not a folder pick.
  const WEB_OWN = new Set(['EMPTY_PICK_DEMO_JS_FALLBACK']);

  it('agrees with the desktop on every ANALYZER string it has', () => {
    for (const [key, value] of Object.entries(WEB.ANALYZER)) {
      if (WEB_OWN.has(key)) continue;
      expect(DESKTOP.ANALYZER, key).toHaveProperty(key);
      const desk = DESKTOP.ANALYZER[key];
      if (typeof value === 'function') expect(String(value), key).toBe(String(desk));
      else expect(value, key).toEqual(desk);
    }
  });
});

describe('the other copied modules match their desktop originals (#238)', () => {
  // Each copy adds one header line naming its original; nothing else differs.
  const body = (text) => text.split('\n').filter((l) => !/a copy of studio\/src\//i.test(l));

  it.each(['analyzer_flags.js', 'steam_ids.js'])('%s', (file) => {
    const desk = body(read('studio', 'src', file)).filter((l) => l !== `// ${file}`);
    const web = body(read('web-analyzer', 'www', file));
    expect(web).toEqual(desk);
  });
});
