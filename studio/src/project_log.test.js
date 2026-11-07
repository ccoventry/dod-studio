import { describe, it, expect } from 'vitest';
import {
  projectSavedLine, projectOpenedLine, projectFailedLine, missingDemosLine, changedDemosLine,
  projectBackupStartedLine, projectBackedUpLine, projectUpdatedLine, projectNewerLine, projectNeedsCopyLine,
} from './project_log.js';

describe('project format lines', () => {
  it('say what is being backed up and where it went, with how long it took', () => {
    expect(projectBackupStartedLine('D:/p/a.json', 1, 2)).toBe('[project] updating D:/p/a.json from format 1 to 2: backing it up first');
    expect(projectBackedUpLine('D:/p/a.before-v2.dodstudio', 7)).toBe('[project] backed up to D:/p/a.before-v2.dodstudio (7 ms)');
  });

  it('give the summary\'s counts, or say the takes were not checked', () => {
    expect(projectUpdatedLine('a.json', { clips: 3, takesFound: 2, takesNotFound: 1 }, 'b'))
      .toBe('[project] updated a.json: 3 clip(s), 2 take(s) found, 1 not found, backup b');
    expect(projectUpdatedLine('a.json', { clips: 3, takesFound: null, takesNotFound: null }, 'b'))
      .toBe('[project] updated a.json: 3 clip(s), takes not checked, backup b');
  });

  it('name a newer file and a refused save', () => {
    expect(projectNewerLine('a.json', 5, 2)).toContain('format 5');
    expect(projectNeedsCopyLine('a.json')).toContain('a.json');
  });
});

describe('project log lines', () => {
  it('say what was saved or opened, with the demo count', () => {
    expect(projectSavedLine('D:/p/a.dodstudio', 12)).toBe('[project] saved D:/p/a.dodstudio: 12 demo(s)');
    expect(projectOpenedLine('D:/p/a.dodstudio', 0)).toBe('[project] opened D:/p/a.dodstudio: 0 demo(s)');
  });
  it('name the failed action and the file when there is one', () => {
    expect(projectFailedLine('save', 'D:/p/a.dodstudio', 'disk full')).toBe('[project] could not save D:/p/a.dodstudio: disk full');
    expect(projectFailedLine('save', null, 'x')).toBe('[project] could not save: x');
  });
  it('count missing and changed demos', () => {
    expect(missingDemosLine(5, 0, 0)).toBe('[project] all 5 demo(s) are where the project says');
    expect(missingDemosLine(5, 2, 1)).toBe('[project] 2 of 5 demo(s) are missing, 1 found elsewhere');
    expect(changedDemosLine(3, true)).toBe('[project] 3 demo(s) changed since they were scanned, rescanning');
  });
});
