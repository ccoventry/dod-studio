import { describe, it, expect } from 'vitest';
import {
  projectSavedLine, projectOpenedLine, projectFailedLine, missingDemosLine, changedDemosLine,
} from './project_log.js';

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
