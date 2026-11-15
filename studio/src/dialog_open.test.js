import { describe, it, expect } from 'vitest';
import { isAnyDialogOpen } from './themed_confirm.js';

// Start-up prompts wait for isAnyDialogOpen, so they come one at a time: the
// render crash-recovery window and "Capture the rest?" opened on top of each
// other after a crash.

function page(...displays) {
  const modals = displays.map((display) => ({ display }));
  return {
    doc: { querySelectorAll: (sel) => (sel === '.modal' ? modals : []) },
    view: { getComputedStyle: (el) => ({ display: el.display }) },
  };
}

describe('isAnyDialogOpen', () => {
  it('sees an app window that is on screen', () => {
    const { doc, view } = page('none', 'flex', 'none');
    expect(isAnyDialogOpen(doc, view)).toBe(true);
  });

  it('is false when every window is hidden', () => {
    const { doc, view } = page('none', 'none');
    expect(isAnyDialogOpen(doc, view)).toBe(false);
  });

  it('is false with no page at all', () => {
    expect(isAnyDialogOpen(undefined, undefined)).toBe(false);
  });
});
