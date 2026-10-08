import { describe, it, expect } from 'vitest';
import { uiStatusText } from './status_line.js';

describe('capture status line (#534)', () => {
  it("drops the engine's log-only pointers", () => {
    const fromEngine = 'Error: Capture Engine Aborted — hl.exe exited early — see [HLAE] lines above in this log for timing. (see View Logs for details)';
    expect(uiStatusText(fromEngine)).toBe('Error: Capture Engine Aborted — hl.exe exited early');
  });

  it('drops either one on its own', () => {
    expect(uiStatusText('Capture Engine Aborted — no hl.exe (see View Logs for details)'))
      .toBe('Capture Engine Aborted — no hl.exe');
    expect(uiStatusText('x — see [HLAE] lines above in this log for timing.')).toBe('x');
  });

  it('leaves everything else alone', () => {
    expect(uiStatusText('Status: Waiting...')).toBe('Status: Waiting...');
    expect(uiStatusText(undefined)).toBe('');
  });
});
