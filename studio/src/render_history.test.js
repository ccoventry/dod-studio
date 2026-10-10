import { describe, it, expect } from 'vitest';
import { historySummary, historyLines } from './render_history.js';

const at = (h, m) => new Date(2026, 8, 29, h, m).getTime();
const attempt = (extra) => ({
  started_unix_ms: at(14, 2), stream: 'all', codec: 'prores', fps: 300,
  outcome: 'finished', output_path: 'D:\\out\\clip.mov', output_size_bytes: 1000, output_exists: true, ...extra,
});
const size = (b) => `${b} B`;

describe('historySummary', () => {
  it('says New with no attempts', () => {
    expect(historySummary([])).toEqual({ label: 'New', bytes: 0 });
  });

  it('counts finished attempts and totals the outputs still on disk', () => {
    const s = historySummary([
      attempt(),
      attempt({ outcome: 'failed' }),
      attempt({ output_exists: false, output_size_bytes: 5 }),
    ]);
    expect(s).toEqual({ label: 'Rendered ×2', bytes: 1000 });
  });

  it('shows the latest outcome when it did not finish', () => {
    expect(historySummary([attempt(), attempt({ outcome: 'interrupted' })]).label).toBe('Interrupted');
    expect(historySummary([attempt({ outcome: 'cancelled' })]).label).toBe('Cancelled');
  });
});

describe('historyLines', () => {
  it('lists attempts newest first with settings and result', () => {
    const lines = historyLines([
      attempt({ started_unix_ms: at(9, 5) }),
      attempt({ started_unix_ms: at(10, 0), outcome: 'failed', codec: 'custom', custom_codec_args: '-c:v x', error: 'exit 1' }),
      attempt({ started_unix_ms: at(11, 0), output_exists: false }),
    ], size);
    expect(lines).toEqual([
      '2026-09-29 11:00 · prores @ 300fps · clip.mov was moved or deleted',
      '2026-09-29 10:00 · custom (-c:v x) @ 300fps · Failed: exit 1',
      '2026-09-29 09:05 · prores @ 300fps · clip.mov, 1000 B',
    ]);
  });
});
