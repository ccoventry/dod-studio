import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';

// A line ceiling per hot frontend file (#1179), beside main_js_budget.test.js
// and the Rust twin native/tests/file_ceilings.rs. A prompt to look, not a
// limit to design around: a file past its number is asking whether it still
// has one job (docs/coding_standards.md, section 1). Move the code that
// belongs to another area into its own module, or raise the number with the
// reason in the PR. Each is the size on the day it was set plus about 3%.
const CEILINGS = {
  'capture_pane.js': 1400,
  'hd_pane.js': 1350,
  'render_pane.js': 1200,
  'overviews_pane.js': 1150,
  'analyzer_report.js': 1150,
  'analyzer_browser.js': 900,
  'detail_pane.js': 750,
};

describe('hot frontend files', () => {
  for (const [file, ceiling] of Object.entries(CEILINGS)) {
    it(`${file} stays under ${ceiling} lines`, () => {
      const lines = readFileSync(new URL(`./${file}`, import.meta.url), 'utf8').split('\n').length;
      expect(lines).toBeLessThanOrEqual(ceiling);
    });
  }
});
