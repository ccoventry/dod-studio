import { describe, it, expect } from 'vitest';
import { STRINGS } from './strings.js';

// The toast formatters: plural and count wording that is easy to break and
// only ever seen at the end of a long scan or before a capture.

describe('scanCompleteToast', () => {
  it('says how many demos were found', () => {
    expect(STRINGS.MAIN.scanCompleteToast(12)).toBe('Scan complete (12 demo(s) found)');
  });

  it('mentions unchanged demos only when there are some (#456)', () => {
    expect(STRINGS.MAIN.scanCompleteToast(3, 0)).toBe('Scan complete (3 demo(s) found)');
    expect(STRINGS.MAIN.scanCompleteToast(3, 40)).toBe(
      'Scan complete (3 new or changed demo(s) found, 40 already in the queue and unchanged)'
    );
  });
});

describe('review refusedToast (#880)', () => {
  it('says a review is running and to end it first', () => {
    expect(STRINGS.REVIEW.refusedToast('viewdemo')).toBe(
      "A review is running in the game, so it didn't load the preview. End the review first (Stop), then try again."
    );
  });

  it('names a seek as a jump, and anything else plainly', () => {
    expect(STRINGS.REVIEW.refusedToast('ds_demo_seek_to')).toContain("didn't jump to that time");
    expect(STRINGS.REVIEW.refusedToast('spec_mode')).toContain("didn't do that");
    expect(STRINGS.REVIEW.refusedToast(undefined)).toContain('A review is running in the game');
  });
});

describe('scanCancelledToast', () => {
  it('says nothing was added when Cancel came while listing folders (#843)', () => {
    expect(STRINGS.MAIN.scanCancelledToast(0)).toBe('Scan cancelled, nothing added');
  });

  it('counts the demos parsed before the cancel', () => {
    expect(STRINGS.MAIN.scanCancelledToast(4)).toBe('Scan cancelled (4 demo(s) added before cancel)');
  });
});

describe('skippedDemosToast', () => {
  const skipped = (n) =>
    Array.from({ length: n }, (_, i) => ({ name: `d${i + 1}.dem`, reason: 'too short' }));

  it('names up to three demos with their reasons', () => {
    expect(STRINGS.MAIN.skippedDemosToast(skipped(2))).toBe(
      '2 demo(s) could not be read and were skipped: d1.dem (too short); d2.dem (too short)'
    );
  });

  it('counts the rest past three instead of listing them', () => {
    const text = STRINGS.MAIN.skippedDemosToast(skipped(5));
    expect(text).toContain('d3.dem (too short); and 2 more');
    expect(text).not.toContain('d4.dem');
    expect(text.startsWith('5 demo(s)')).toBe(true);
  });

  it('has no "and N more" at exactly three', () => {
    expect(STRINGS.MAIN.skippedDemosToast(skipped(3))).not.toContain('more');
  });
});

describe('bannedCommandsWarning', () => {
  it('is singular for one command', () => {
    const text = STRINGS.CAPTURE.bannedCommandsWarning(1);
    expect(text).toMatch(/^1 command in /);
    expect(text).toContain('fix or remove it');
  });

  it('is plural otherwise', () => {
    const text = STRINGS.CAPTURE.bannedCommandsWarning(3);
    expect(text).toMatch(/^3 commands in /);
    expect(text).toContain('fix or remove them');
  });
});

describe('finish step (#440)', () => {
  it('mentions clips in progress only when there are some', () => {
    expect(STRINGS.FINISH.progressStatus(3, 12, 2)).toBe('Finishing clips: 3 of 12, 2 in progress');
    expect(STRINGS.FINISH.progressStatus(12, 12, 0)).toBe('Finishing clips: 12 of 12');
  });

  it('says how many clips are ready, and what went wrong only when something did', () => {
    expect(STRINGS.FINISH.doneSummary(12, 0, 0)).toBe('12 clips ready.');
    expect(STRINGS.FINISH.doneSummary(1, 0, 0)).toBe('1 clip ready.');
    expect(STRINGS.FINISH.doneSummary(10, 1, 1)).toBe('10 clips ready, 1 failed, 1 cancelled — see the Render tab.');
  });
});

describe('scanCapToast (#531)', () => {
  it('names the bound that stopped the walk', () => {
    expect(STRINGS.MAIN.scanCapToast({ depth: true, entries: false, max_depth: 6, max_entries: 50000 })).toBe(
      'Some demos may be missing: folders more than 6 levels down were not read. Add a folder further down to scan the rest.'
    );
    expect(STRINGS.MAIN.scanCapToast({ depth: true, entries: true, max_depth: 6, max_entries: 50000 })).toContain(
      'not read, and the walk stopped after 50,000 files and folders.'
    );
  });
});

describe('AUDITOR.doneStatus', () => {
  it('counts demos inside archives apart from the loose ones (#929)', () => {
    expect(STRINGS.AUDITOR.doneStatus(3, 0, '0.0 s', 82))
      .toBe('Done in 0.0 s: 3 demos checked and 82 inside archives, 0 with copies.');
  });

  it('leaves archives out when none were looked inside', () => {
    expect(STRINGS.AUDITOR.doneStatus(1, 0, '0.4 s')).toBe('Done in 0.4 s: 1 demo checked, 0 with copies.');
  });
});
