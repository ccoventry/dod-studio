import { describe, it, expect } from 'vitest';
import { demoMenuItems } from './demo_menu_items.js';

const fns = () => ({
  launchPreview() {}, review() {}, openInAnalyzer() {}, openFolder() {},
  split: null, useFound: null, locate: null, copyName() {}, copyPath() {}, remove() {},
});
const labels = (items) => items.filter((i) => i && !i.separator).map((i) => i.label);

describe('demoMenuItems', () => {
  it('lists the actions every demo has, remove last', () => {
    const items = demoMenuItems({ name: 'a.dem', path: 'C:\\d\\a.dem' }, fns());
    expect(labels(items)).toEqual([
      'Launch Preview', 'Review this demo', 'Open in Demo Analyzer', 'Open Folder in Explorer',
      'Copy Demo Name', 'Copy Path', 'Remove from Queue',
    ]);
    expect(items.at(-1).danger).toBe(true);
    expect(items.every((i) => !i || !i.disabled)).toBe(true);
  });

  it('adds Split Maps only when the row has that button', () => {
    const items = demoMenuItems({ name: 'a.dem' }, { ...fns(), split() {} });
    expect(labels(items)).toContain('Split Maps');
    expect(items.find((i) => i?.label === 'Split Maps').disabled).toBe(false);
  });

  it('dims Split Maps with the reason when the demo is already split or splitting', () => {
    const items = demoMenuItems({ name: 'a.dem' }, { ...fns(), split() {}, splitBlocked: 'already split next to it: a_split1_anzio.dem' });
    expect(items.find((i) => i?.label === 'Split Maps').disabled).toMatch(/already split/);
  });

  it('dims what needs the file on a missing demo, with the reason, and keeps Locate', () => {
    const items = demoMenuItems({ name: 'a.dem', missing: true }, { ...fns(), locate() {}, useFound() {} });
    const byLabel = Object.fromEntries(items.filter((i) => i && !i.separator).map((i) => [i.label, i]));
    expect(byLabel['Launch Preview'].disabled).toMatch(/missing/);
    expect(byLabel['Open Folder in Explorer'].disabled).toMatch(/missing/);
    expect(byLabel['Locate demo...'].disabled).toBeFalsy();
    expect(byLabel['Use found copy'].disabled).toBeFalsy();
    expect(byLabel['Copy Path'].disabled).toBeFalsy();
  });
});
