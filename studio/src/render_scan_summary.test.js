import { describe, it, expect } from 'vitest';
import { scanMergeMessage } from './render_scan_summary.js';

const summary = (overrides) => ({ total: 0, added: 0, refreshed: 0, kept: 0, gone: 0, gone_queued: 0, ...overrides });

describe('scanMergeMessage', () => {
  it('says what changed, in order', () => {
    const m = scanMergeMessage(summary({ total: 6, added: 3, refreshed: 2, gone: 1 }));
    expect(m.toast).toBe('Scan complete: 3 new takes, 2 refreshed, 1 gone.');
    expect(m.status).toBe('Status: Scan complete — 3 new takes, 2 refreshed, 1 gone');
    expect(m.level).toBe('success');
  });

  it('a first scan only has new takes, one is singular', () => {
    expect(scanMergeMessage(summary({ total: 1, added: 1 })).toast).toBe('Scan complete: 1 new take.');
  });

  it('a dropped queued render is a warning and says so', () => {
    const m = scanMergeMessage(summary({ total: 2, kept: 2, gone: 1, gone_queued: 1 }));
    expect(m.toast).toBe('Scan complete: 1 gone (1 was queued).');
    expect(m.level).toBe('warning');
  });

  it('takes found but nothing changed', () => {
    const m = scanMergeMessage(summary({ total: 4, kept: 4 }));
    expect(m.toast).toMatch(/no changes/);
    expect(m.level).toBe('info');
  });

  it('nothing found at all', () => {
    const m = scanMergeMessage(summary({}));
    expect(m.toast).toBe('No render takes detected.');
    expect(m.level).toBe('info');
    expect(scanMergeMessage(null).toast).toBe('No render takes detected.');
  });
});
