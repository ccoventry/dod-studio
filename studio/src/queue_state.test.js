import { describe, it, expect, vi } from 'vitest';
import { createQueueState } from './queue_state.js';

function setup() {
  const deps = {
    renderList: vi.fn(),
    renderDetail: vi.fn(),
    clearChecked: vi.fn(),
    resetMapWarnings: vi.fn(),
    updateFooter: vi.fn(),
    markDirty: vi.fn(),
    afterLoad: vi.fn(),
  };
  return { queue: createQueueState(deps), deps };
}

const demo = (name) => ({ path: `demos/${name}.dem` });

describe('createQueueState', () => {
  it('starts empty with nothing selected', () => {
    const { queue } = setup();
    expect(queue.getDemos()).toEqual([]);
    expect(queue.getSelectedIdx()).toBeNull();
    expect(queue.selectedDemo()).toBeNull();
  });

  it('load selects the first demo, draws, and runs afterLoad', () => {
    const { queue, deps } = setup();
    const [a, b] = [demo('a'), demo('b')];
    queue.load([a, b]);
    expect(queue.getDemos()).toEqual([a, b]);
    expect(queue.getSelectedIdx()).toBe(0);
    expect(deps.renderList).toHaveBeenCalledWith([a, b], 0, queue.select);
    expect(deps.renderDetail).toHaveBeenCalledWith(a, 0);
    expect(deps.updateFooter).toHaveBeenCalledWith([a, b]);
    expect(deps.afterLoad).toHaveBeenCalledTimes(1);
  });

  it('load of an empty list selects nothing and skips the detail view', () => {
    const { queue, deps } = setup();
    queue.load([]);
    expect(queue.getSelectedIdx()).toBeNull();
    expect(deps.renderDetail).not.toHaveBeenCalled();
    expect(deps.afterLoad).toHaveBeenCalledTimes(1);
  });

  it('select records the index and shows that demo', () => {
    const { queue, deps } = setup();
    const [a, b] = [demo('a'), demo('b')];
    queue.load([a, b]);
    queue.select(b, 1);
    expect(queue.getSelectedIdx()).toBe(1);
    expect(queue.selectedDemo()).toBe(b);
    expect(deps.renderDetail).toHaveBeenLastCalledWith(b, 1);
  });

  it('redrawSelected draws the selected demo, and nothing when none is selected', () => {
    const { queue, deps } = setup();
    queue.redrawSelected();
    expect(deps.renderDetail).not.toHaveBeenCalled();
    const [a, b] = [demo('a'), demo('b')];
    queue.load([a, b]);
    queue.select(b, 1);
    deps.renderDetail.mockClear();
    queue.redrawSelected();
    expect(deps.renderDetail).toHaveBeenCalledWith(b, 1);
  });

  it('renderQueue redraws the list with the current selection', () => {
    const { queue, deps } = setup();
    const [a, b] = [demo('a'), demo('b')];
    queue.load([a, b]);
    queue.select(b, 1);
    queue.renderQueue();
    expect(deps.renderList).toHaveBeenLastCalledWith([a, b], 1);
  });

  describe('replace', () => {
    it('keeps the same demo selected by identity when it survives', () => {
      const { queue, deps } = setup();
      const [a, b, c] = [demo('a'), demo('b'), demo('c')];
      queue.load([a, b, c]);
      queue.select(c, 2);
      queue.replace([b, c]);
      expect(queue.getSelectedIdx()).toBe(1);
      expect(queue.selectedDemo()).toBe(c);
      expect(deps.renderDetail).toHaveBeenLastCalledWith(c, 1);
      expect(deps.clearChecked).toHaveBeenCalledTimes(1);
      expect(deps.markDirty).toHaveBeenCalledTimes(1);
      expect(deps.resetMapWarnings).not.toHaveBeenCalled();
    });

    it('falls back to the first demo when the selected one was removed', () => {
      const { queue } = setup();
      const [a, b, c] = [demo('a'), demo('b'), demo('c')];
      queue.load([a, b, c]);
      queue.select(b, 1);
      queue.replace([a, c]);
      expect(queue.getSelectedIdx()).toBe(0);
    });

    it('an emptied queue selects nothing, clears the detail view and the map banner', () => {
      const { queue, deps } = setup();
      queue.load([demo('a')]);
      queue.replace([]);
      expect(queue.getSelectedIdx()).toBeNull();
      expect(deps.renderDetail).toHaveBeenLastCalledWith(null, null);
      expect(deps.resetMapWarnings).toHaveBeenCalledTimes(1);
      expect(deps.updateFooter).toHaveBeenLastCalledWith([]);
    });

    it('selects the first demo when nothing was selected before', () => {
      const { queue } = setup();
      const a = demo('a');
      queue.replace([a]);
      expect(queue.getSelectedIdx()).toBe(0);
    });
  });

  describe('remove (the row trash button)', () => {
    it('shifts the selection down when a row above it goes', () => {
      const { queue, deps } = setup();
      const [a, b, c] = [demo('a'), demo('b'), demo('c')];
      queue.load([a, b, c]);
      queue.select(c, 2);
      expect(queue.remove(0, [b, c])).toBe(1);
      expect(queue.getSelectedIdx()).toBe(1);
      expect(deps.markDirty).toHaveBeenCalledTimes(1);
      expect(deps.updateFooter).toHaveBeenLastCalledWith([b, c]);
    });

    it('keeps the selection when a row below it goes', () => {
      const { queue } = setup();
      const [a, b, c] = [demo('a'), demo('b'), demo('c')];
      queue.load([a, b, c]);
      expect(queue.remove(2, [a, b])).toBe(0);
    });

    it('moves the selection to the first demo when the selected row goes', () => {
      const { queue, deps } = setup();
      const [a, b, c] = [demo('a'), demo('b'), demo('c')];
      queue.load([a, b, c]);
      queue.select(b, 1);
      expect(queue.remove(1, [a, c])).toBe(0);
      expect(deps.renderDetail).toHaveBeenLastCalledWith(a, 0);
    });

    it('clears the detail view when the last demo goes', () => {
      const { queue, deps } = setup();
      queue.load([demo('a')]);
      expect(queue.remove(0, [])).toBeNull();
      expect(deps.renderDetail).toHaveBeenLastCalledWith(null, null);
    });
  });

  describe('refreshAfterRelocation', () => {
    it('only redraws the list when nothing changed', () => {
      const { queue, deps } = setup();
      queue.load([demo('a')]);
      deps.renderDetail.mockClear();
      deps.markDirty.mockClear();
      queue.refreshAfterRelocation(false);
      expect(deps.renderList).toHaveBeenLastCalledWith(queue.getDemos(), 0, queue.select);
      expect(deps.renderDetail).not.toHaveBeenCalled();
      expect(deps.markDirty).not.toHaveBeenCalled();
    });

    it('redraws the selected demo and marks the project dirty when changed', () => {
      const { queue, deps } = setup();
      const a = demo('a');
      queue.load([a]);
      deps.renderDetail.mockClear();
      queue.refreshAfterRelocation(true);
      expect(deps.renderDetail).toHaveBeenCalledWith(a, 0);
      expect(deps.markDirty).toHaveBeenCalledTimes(1);
    });
  });

  it('setSelectedIdx lets a scan move the selection', () => {
    const { queue } = setup();
    queue.load([demo('a'), demo('b')]);
    queue.setSelectedIdx(1);
    expect(queue.getSelectedIdx()).toBe(1);
  });
});
