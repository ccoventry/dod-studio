import { describe, it, expect } from 'vitest';
import { analyzerLoadingView, analyzerLoadingHtml } from './analyzer_loading.js';
import { playersBarView } from './players.js';

describe('analyzer_loading (#958)', () => {
  it('slides until the first %, then fills by it', () => {
    expect(analyzerLoadingView(null)).toEqual({ text: 'Analyzing demo…', short: 'Analyzing…', pct: null });
    expect(analyzerLoadingView({ processed: 40, total: 100 }))
      .toEqual({ text: 'Analyzing demo… 40%', short: 'Analyzing… 40%', pct: 40 });
    expect(analyzerLoadingView({ processed: 140, total: 100 }).pct).toBe(100);
  });

  it('a cached demo says it is read from the cache', () => {
    expect(analyzerLoadingView({ cached: true, processed: 0, total: 0 }))
      .toEqual({ text: 'Reading from the analyzer cache…', short: 'Reading from the cache…', pct: null });
  });

  it('draws the line with its bar', () => {
    const sliding = analyzerLoadingHtml(analyzerLoadingView(null));
    expect(sliding).toContain('<p class="analyzer-empty">Analyzing demo…</p>');
    expect(sliding).toContain('analyzer-loading-bar indeterminate');
    const filled = analyzerLoadingHtml(analyzerLoadingView({ processed: 30, total: 100 }));
    expect(filled).not.toContain('indeterminate');
    expect(filled).toContain('width: 30%;');
  });
});

describe('the Players page bar (#958)', () => {
  it('slides until the first demo is read, then fills by the count', () => {
    expect(playersBarView(0, 40)).toEqual({ pct: 0, indeterminate: true });
    expect(playersBarView(10, 40)).toEqual({ pct: 25, indeterminate: false });
    expect(playersBarView(0, 0)).toEqual({ pct: 0, indeterminate: true });
  });
});
