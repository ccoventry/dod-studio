import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';

// Every colour styles.css paints is a custom property defined in a `:root`
// block, so a theme can swap them (#49). A raw hex / rgb() / hsl() anywhere
// else is a colour no theme can reach: use an existing token, or add one to
// `:root`. A guideline like main_js_budget.test.js: an exception goes in
// ALLOWED with the reason, never in a looser pattern.
const ALLOWED = [];

const COLOUR = /#(?:[0-9a-f]{8}|[0-9a-f]{6}|[0-9a-f]{3,4})\b|\b(?:rgb|rgba|hsl|hsla)\(/gi;

/** styles.css without comments and without the declarations inside `:root { }`. */
function outsideRoot(css) {
  const noComments = css.replace(/\/\*[\s\S]*?\*\//g, (c) => c.replace(/[^\n]/g, ' '));
  return noComments.replace(/:root\s*\{[^}]*\}/g, (c) => c.replace(/[^\n]/g, ' '));
}

describe('styles.css colours (#49)', () => {
  it('has no raw colour outside :root', () => {
    const css = outsideRoot(readFileSync(new URL('./styles.css', import.meta.url), 'utf8'));
    const found = [];
    css.split('\n').forEach((line, i) => {
      for (const m of line.matchAll(COLOUR)) {
        if (!ALLOWED.includes(m[0].toLowerCase())) found.push(`line ${i + 1}: ${m[0]}`);
      }
    });
    expect(found).toEqual([]);
  });
});
