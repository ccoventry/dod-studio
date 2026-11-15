import { describe, it, expect, vi } from 'vitest';
import { precheckLine, createCapturePrecheck } from './capture_precheck.js';

// Just enough of an element for the factory: no DOM in these tests.
function fakeEl(display = 'none') {
  const classes = new Set();
  return {
    disabled: false,
    textContent: '',
    title: '',
    style: { display },
    classList: {
      toggle: (name, on) => (on ? classes.add(name) : classes.delete(name)),
      contains: (name) => classes.has(name),
    },
  };
}

describe('capture_precheck (#953)', () => {
  it('names each check in plain words', () => {
    expect(precheckLine('start')).toBe('Getting ready to capture…');
    expect(precheckLine('obs')).toBe('Checking OBS…');
    expect(precheckLine('disk')).toBe('Checking the disk space…');
    expect(precheckLine('demos')).toBe('Checking the picked demos are where they were…');
    expect(precheckLine('demos', 3, 12)).toBe('Checking the picked demos are where they were (3 of 12)…');
    expect(precheckLine('demos', 15, 12)).toBe('Checking the picked demos are where they were (12 of 12)…');
    expect(precheckLine('maps')).toBe('Checking the maps and demos for known problems…');
    expect(precheckLine('paths')).toBe('Checking the game and HLAE paths…');
    expect(precheckLine('game')).toBe('Checking whether the game is already open…');
    expect(precheckLine('steam')).toBe('Checking Steam…');
  });

  it('greys Start out and slides the bar from the click, and a stop puts both back', () => {
    const startBtn = fakeEl();
    const statusEl = fakeEl();
    const progressContainer = fakeEl('none');
    const refreshStart = vi.fn();
    const p = createCapturePrecheck({ startBtn, statusEl, progressContainer, refreshStart });

    p.begin();
    expect(p.isChecking()).toBe(true);
    expect(startBtn.disabled).toBe(true);
    expect(progressContainer.style.display).toBe('block');
    expect(progressContainer.classList.contains('indeterminate')).toBe(true);
    expect(statusEl.textContent).toBe('Getting ready to capture…');

    p.step('demos', 2, 5);
    expect(statusEl.textContent).toBe('Checking the picked demos are where they were (2 of 5)…');

    p.stop('Not enough disk space.');
    expect(p.isChecking()).toBe(false);
    expect(statusEl.textContent).toBe('Not enough disk space.');
    expect(progressContainer.style.display).toBe('none');
    expect(progressContainer.classList.contains('indeterminate')).toBe(false);
    expect(refreshStart).toHaveBeenCalledTimes(1);

    // A second stop (the click handler's finally) changes nothing.
    p.stop();
    expect(statusEl.textContent).toBe('Not enough disk space.');
    expect(refreshStart).toHaveBeenCalledTimes(1);
    // Nor does a late step.
    p.step('steam');
    expect(statusEl.textContent).toBe('Not enough disk space.');
  });

  it('a stop with no reason says it was not started', () => {
    const statusEl = fakeEl();
    const p = createCapturePrecheck({ startBtn: fakeEl(), statusEl, progressContainer: fakeEl() });
    p.begin();
    p.stop();
    expect(statusEl.textContent).toBe('Not started.');
  });

  it('hands over to the batch with the bar still shown and Start still greyed out', () => {
    const startBtn = fakeEl();
    const progressContainer = fakeEl('none');
    const refreshStart = vi.fn();
    const p = createCapturePrecheck({ startBtn, statusEl: fakeEl(), progressContainer, refreshStart });
    p.begin();
    p.handOver();
    expect(p.isChecking()).toBe(false);
    expect(startBtn.disabled).toBe(true);
    expect(progressContainer.style.display).toBe('block');
    expect(progressContainer.classList.contains('indeterminate')).toBe(false);
    p.stop();
    expect(refreshStart).not.toHaveBeenCalled();
  });
});
