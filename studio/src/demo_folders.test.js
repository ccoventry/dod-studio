import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./analyzer_pane.js', () => ({
  initAnalyzerPane: vi.fn(),
  refreshAnalyzerQuickLinks: vi.fn(),
}));

import { initAnalyzerPane, refreshAnalyzerQuickLinks } from './analyzer_pane.js';
import { createDemoFolders } from './demo_folders.js';

beforeEach(() => vi.clearAllMocks());

describe('createDemoFolders Recent list', () => {
  it('redraws the Explorer after + Add Folder saves a folder to Recent', async () => {
    const order = [];
    const persist = vi.fn(async () => { order.push('persist'); });
    refreshAnalyzerQuickLinks.mockImplementation(() => order.push('redraw'));
    const folders = createDemoFolders({ persist });
    folders.load({ demo_folder_history: ['D:/old'] });

    await folders.recordVisit('D:/new');

    expect(folders.settingsState().demoFolderHistory).toEqual(['D:/new', 'D:/old']);
    // Redrawn once the list holds the folder, after it is saved.
    expect(order).toEqual(['persist', 'redraw']);
  });

  it("leaves the Explorer's own visits to the pane's own redraw", async () => {
    const folders = createDemoFolders({ persist: vi.fn(async () => {}) });
    folders.startAnalyzerPane();
    const { recordDemoFolderVisit, getDemoFolderHistory } = initAnalyzerPane.mock.calls[0][0];

    await recordDemoFolderVisit('D:/visited');

    expect(getDemoFolderHistory()).toEqual(['D:/visited']);
    expect(refreshAnalyzerQuickLinks).not.toHaveBeenCalled();
  });

  it('keeps Recent most-recent-first, deduped and capped at 10', async () => {
    const folders = createDemoFolders({ persist: vi.fn(async () => {}) });
    for (let i = 0; i < 12; i++) await folders.recordVisit(`D:/f${i}`);
    await folders.recordVisit('D:/f5');
    const history = folders.settingsState().demoFolderHistory;
    expect(history).toHaveLength(10);
    expect(history[0]).toBe('D:/f5');
    expect(history.filter((f) => f === 'D:/f5')).toHaveLength(1);
  });
});
