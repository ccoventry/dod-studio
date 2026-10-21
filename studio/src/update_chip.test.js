import { describe, it, expect } from 'vitest';
import { updateChipView, RECHECK_MS } from './update_chip.js';

describe('update_chip (#868)', () => {
  it('stays hidden with no update', () => {
    expect(updateChipView({ version: null }).hidden).toBe(true);
    expect(updateChipView().hidden).toBe(true);
  });

  it('names the version a check found', () => {
    const view = updateChipView({ version: '0.2.1' });
    expect(view.hidden).toBe(false);
    expect(view.text).toBe('Update 0.2.1 available');
    expect(view.title).toMatch(/click/i);
  });

  it('asks for a restart once the update has downloaded', () => {
    const view = updateChipView({ version: '0.2.1', ready: true });
    expect(view.hidden).toBe(false);
    expect(view.text).toBe('Restart to update');
  });

  it('checks again every few hours, not every few minutes', () => {
    expect(RECHECK_MS).toBeGreaterThanOrEqual(60 * 60 * 1000);
  });
});
