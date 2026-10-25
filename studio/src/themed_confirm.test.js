import { describe, it, expect, beforeAll } from 'vitest';

// The OK button is red only for a destructive confirm (#880).

function fakeElement() {
  const classes = new Set();
  return {
    textContent: '',
    style: {},
    addEventListener: () => {},
    after: () => {},
    focus: () => {},
    classList: {
      toggle: (c, on) => (on ? classes.add(c) : classes.delete(c)),
      contains: (c) => classes.has(c),
    },
  };
}

const els = {
  '#themed-confirm-modal': fakeElement(),
  '#themed-confirm-title': fakeElement(),
  '#themed-confirm-ok-btn': fakeElement(),
  '#themed-confirm-cancel-btn': fakeElement(),
};
globalThis.document = { querySelector: (sel) => els[sel] || null, createElement: fakeElement };
const { initThemedConfirm, themedConfirm } = await import('./themed_confirm.js');

describe('themedConfirm OK colour', () => {
  const ok = els['#themed-confirm-ok-btn'];
  beforeAll(() => initThemedConfirm());

  it('uses the accent colour by default', () => {
    themedConfirm('Start anyway?');
    expect(ok.classList.contains('primary-btn')).toBe(true);
    expect(ok.classList.contains('danger-btn')).toBe(false);
  });

  it('is red for a destructive one, and back to accent for the next', () => {
    themedConfirm('Delete 3 takes?', { destructive: true });
    expect(ok.classList.contains('danger-btn')).toBe(true);
    expect(ok.classList.contains('primary-btn')).toBe(false);
    themedConfirm('Rescan them now?');
    expect(ok.classList.contains('danger-btn')).toBe(false);
    expect(ok.classList.contains('primary-btn')).toBe(true);
  });
});
