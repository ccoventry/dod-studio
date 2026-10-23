import { describe, it, expect } from 'vitest';
import { createProjectOpenProgress } from './project_open_progress.js';
import { createQueueProgress } from './queue_progress.js';

// The queue's line, spinner and bar, enough for createQueueProgress.
function fakeDoc(line = 'Ready') {
  const fill = { style: { width: '0%' } };
  const els = {
    '#scan-status': { textContent: line },
    '#scan-spinner': { style: { display: 'none' } },
    '#scan-progress': { hidden: true, classList: { toggle() {}, remove() {} }, querySelector: () => fill },
  };
  return { querySelector: (sel) => els[sel] ?? null, line: () => els['#scan-status'].textContent };
}

function setUp() {
  const doc = fakeDoc();
  const progress = createQueueProgress({ doc });
  let handler = null;
  const listenFn = (name, cb) => {
    expect(name).toBe('missing_demos_search');
    handler = cb;
    return Promise.resolve(() => {});
  };
  const open = createProjectOpenProgress({ progress, listenFn });
  return { doc, progress, open, emit: (payload) => handler({ payload }) };
}

describe('project_open_progress (#958)', () => {
  it('says it is opening, then checks the demos, then names how many it looks for', () => {
    const { doc, open, emit } = setUp();
    open.opening();
    expect(doc.line()).toBe('Opening the project… · 0:00');
    open.opened();
    expect(doc.line()).toBe('Ready');

    open.searching();
    expect(doc.line()).toBe("Checking the project's demos are where they were… · 0:00");
    emit({ missing: 3 });
    expect(doc.line()).toBe('Looking for 3 missing demos… · 0:00');
    emit({ missing: 1 });
    expect(doc.line()).toBe('Looking for 1 missing demo… · 0:00');
    open.searched();
    expect(doc.line()).toBe('Ready');
  });

  it('a search event with no search running changes nothing, and ending twice is harmless', () => {
    const { doc, open, emit } = setUp();
    emit({ missing: 2 });
    expect(doc.line()).toBe('Ready');
    open.searching();
    open.searched();
    open.searched();
    open.opened();
    expect(doc.line()).toBe('Ready');
  });

  it('the grouping the loaded demos start gets the line back after the search', () => {
    const { doc, progress, open } = setUp();
    open.opening();
    open.opened();
    const grouping = progress.begin({ ifIdle: true, text: 'Grouping by match: 0 of 0' });
    expect(grouping).not.toBeNull();
    open.searching();
    progress.show(grouping, { text: 'Grouping by match: 4 of 9' });
    open.searched();
    expect(doc.line()).toBe('Grouping by match: 4 of 9 · 0:00');
  });
});
