import { describe, it, expect } from 'vitest';
import { updateChipView, updateButtonsView, RECHECK_MS } from './update_chip.js';

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

  it('waits for a running batch once the update has downloaded', () => {
    const view = updateChipView({ version: '0.2.1', ready: true, batchRunning: true });
    expect(view.text).toBe('Restart after the batch');
    expect(view.title).toMatch(/batch/);
  });

  it('still names an update it has not downloaded during a batch', () => {
    expect(updateChipView({ version: '0.2.1', batchRunning: true }).text).toBe('Update 0.2.1 available');
  });

  it('checks again every few hours, not every few minutes', () => {
    expect(RECHECK_MS).toBeGreaterThanOrEqual(60 * 60 * 1000);
  });
});

describe('the update window during a batch', () => {
  it('holds Restart and Install, saying why', () => {
    const view = updateButtonsView(true);
    expect(view.held).toBe(true);
    expect(view.restartText).toBe('Restart after the batch');
    expect(view.restartTitle).toMatch(/batch/);
    expect(view.installTitle).toMatch(/closes DoD Studio/);
  });

  it('gives them back when it ends', () => {
    expect(updateButtonsView(false)).toEqual({ held: false, restartText: 'Restart to Apply', restartTitle: '', installTitle: '' });
  });
});
