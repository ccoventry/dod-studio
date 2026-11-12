import { describe, it, expect } from 'vitest';
import { lookupView, lengthsView, pickingView, writingView } from './project_vegas_progress.js';

describe('project VEGAS export progress views', () => {
  it('slides while finding the videos, naming the step and the count', () => {
    const view = lookupView(63);
    expect(view.indeterminate).toBe(true);
    expect(view.text).toContain('63');
  });

  it('fills as lengths are read, naming the one being read', () => {
    expect(lengthsView(0, 4)).toMatchObject({ pct: 0, indeterminate: false });
    expect(lengthsView(0, 4).text).toContain('1 of 4');
    expect(lengthsView(2, 4)).toMatchObject({ pct: 50 });
    expect(lengthsView(2, 4).text).toContain('3 of 4');
    expect(lengthsView(4, 4)).toMatchObject({ pct: 100 });
    expect(lengthsView(4, 4).text).toContain('4 of 4');
  });

  it('slides for the save dialog and the write, with a line each', () => {
    expect(pickingView().indeterminate).toBe(true);
    expect(writingView().indeterminate).toBe(true);
    expect(pickingView().text).not.toBe(writingView().text);
  });
});
