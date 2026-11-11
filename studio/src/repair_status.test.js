import { describe, it, expect, vi } from 'vitest';

vi.mock('./ipc_bridge.js', () => ({ demoRepairable: vi.fn(), logFrontendEvent: vi.fn() }));

const {
  createRepairableCheck, isUnrepairableDamage, mapsToSplit, noteRepairResult, offersRepair, repairMaps, splitDamageReason,
  REPAIRABLE_KEEP_MS,
} = await import('./repair_status.js');
const { STRINGS } = await import('./strings.js');

const DAMAGED = { path: 'C:/demos/a.dem', damage: { read_up_to_secs: 507.4, bytes_unread: 1, reason: 'x' } };
const WHOLE = { path: 'C:/demos/b.dem', damage: null };

/** A checker over `answers` (one per call, in order: a boolean stands for
 *  `{ repairable, maps: [] }`), a clock and a log. */
function checker(...answers) {
  const clock = { ms: 0 };
  const log = vi.fn();
  const ask = vi.fn(() => {
    const next = answers.shift();
    if (next instanceof Error) return Promise.reject(next);
    return Promise.resolve(typeof next === 'boolean' ? { repairable: next, maps: [] } : next);
  });
  return { clock, ask, log, status: createRepairableCheck({ ask, now: () => clock.ms, log }) };
}

describe('offersRepair', () => {
  it('offers Repair on a damaged demo the repair can fix', () => {
    expect(offersRepair(DAMAGED, true)).toBe(true);
  });

  it('keeps offering Repair while the answer is unknown or the check failed', () => {
    expect(offersRepair(DAMAGED, null)).toBe(true);
  });

  it('offers no Repair when the repair has nothing to cut out', () => {
    expect(offersRepair(DAMAGED, false)).toBe(false);
  });

  it('never offers Repair on a whole demo, whatever the answer', () => {
    expect(offersRepair(WHOLE, true)).toBe(false);
  });
});

describe('splitDamageReason', () => {
  it('says "Repair it first" while Repair is offered, the answer known or not', () => {
    expect(splitDamageReason(DAMAGED, true)).toBe(STRINGS.REPAIR.SPLIT_NEEDS_REPAIR);
    expect(splitDamageReason(DAMAGED, null)).toBe(STRINGS.REPAIR.SPLIT_NEEDS_REPAIR);
  });

  it('still greys Split out when Repair has nothing to cut, saying why', () => {
    expect(splitDamageReason(DAMAGED, false)).toBe(STRINGS.REPAIR.SPLIT_DAMAGE_CANNOT_REPAIR);
  });

  it("leaves a whole demo's Split alone", () => {
    expect(splitDamageReason(WHOLE, null)).toBeNull();
    expect(splitDamageReason(WHOLE, false)).toBeNull();
  });
});

describe('isUnrepairableDamage', () => {
  it('is true only for a damaged demo the repair has nothing to cut out of', () => {
    expect(isUnrepairableDamage(DAMAGED, false)).toBe(true);
    expect(isUnrepairableDamage(DAMAGED, null)).toBe(false);
    expect(isUnrepairableDamage(DAMAGED, true)).toBe(false);
    expect(isUnrepairableDamage(WHOLE, false)).toBe(false);
  });
});

describe('createRepairableCheck', () => {
  it('asks the backend once and reuses the answer while it is fresh', async () => {
    const { ask, clock, status } = checker(true);
    expect(await status.check(DAMAGED.path)).toBe(true);
    clock.ms = REPAIRABLE_KEEP_MS - 1;
    expect(await status.check(DAMAGED.path)).toBe(true);
    expect(ask).toHaveBeenCalledTimes(1);
    expect(status.last(DAMAGED.path)).toBe(true);
  });

  it('shares one call between the queue row and the Split button asking at once', async () => {
    const { ask, status } = checker(false);
    const [one, two] = await Promise.all([status.check(DAMAGED.path), status.check(DAMAGED.path)]);
    expect([one, two]).toEqual([false, false]);
    expect(ask).toHaveBeenCalledTimes(1);
  });

  it('answers null when the file cannot be read, so Repair stays offered', async () => {
    const { status } = checker(new Error('cannot read the file'));
    expect(await status.check(DAMAGED.path)).toBeNull();
    expect(status.last(DAMAGED.path)).toBeNull();
  });

  it('treats a missing answer as unknown, not as unrepairable', async () => {
    const { status } = checker(undefined);
    expect(await status.check(DAMAGED.path)).toBeNull();
  });

  it('logs a demo found unrepairable once, however often it is asked', async () => {
    const { clock, log, status } = checker(false, false);
    await status.check(DAMAGED.path);
    clock.ms = REPAIRABLE_KEEP_MS;
    await status.check(DAMAGED.path);
    expect(log).toHaveBeenCalledTimes(1);
    expect(log.mock.calls[0][0]).toContain(DAMAGED.path);
  });
});

describe('repairMaps', () => {
  it("takes the scan's maps when it found more than the analyzer, whose list stops at the damage", () => {
    expect(repairMaps(['dod_anzio'], ['dod_anzio', 'pandemic_aim', 'dod_thunder2_b5a']))
      .toEqual(['dod_anzio', 'pandemic_aim', 'dod_thunder2_b5a']);
  });

  it("keeps the demo's own list while the scan is unknown, failed or found no more", () => {
    expect(repairMaps(['dod_anzio'], null)).toEqual(['dod_anzio']);
    expect(repairMaps(['dod_anzio', 'dod_flash'], ['dod_anzio'])).toEqual(['dod_anzio', 'dod_flash']);
    expect(repairMaps(undefined, null)).toEqual([]);
  });
});

describe('checkMaps', () => {
  const THREE = ['dod_anzio', 'pandemic_aim', 'dod_thunder2_b5a'];

  it('answers the maps from the same call as the repairable check', async () => {
    const { ask, status } = checker({ repairable: true, maps: THREE });
    const [maps, repairable] = await Promise.all([status.checkMaps(DAMAGED.path, ['dod_anzio']), status.check(DAMAGED.path)]);
    expect(maps).toEqual(THREE);
    expect(repairable).toBe(true);
    expect(status.lastMaps(DAMAGED.path)).toEqual(THREE);
    expect(ask).toHaveBeenCalledTimes(1);
  });

  it('logs once per demo when the scan turns one map into several', async () => {
    const { clock, log, status } = checker({ repairable: true, maps: THREE }, { repairable: true, maps: THREE });
    await status.checkMaps(DAMAGED.path, ['dod_anzio']);
    clock.ms = REPAIRABLE_KEEP_MS;
    await status.checkMaps(DAMAGED.path, ['dod_anzio']);
    expect(log).toHaveBeenCalledTimes(1);
    expect(log.mock.calls[0][0]).toContain('Repair & Split');
  });

  it('logs nothing when it changes nothing offered', async () => {
    const { log, status } = checker({ repairable: true, maps: THREE });
    await status.checkMaps(DAMAGED.path, ['dod_anzio', 'pandemic_aim']);
    expect(log).not.toHaveBeenCalled();
  });

  it('answers null when the check fails, and a repair verdict keeps the maps', async () => {
    const failed = checker(new Error('cannot read the file'));
    expect(await failed.status.checkMaps(DAMAGED.path, [])).toBeNull();
    const { status } = checker({ repairable: true, maps: THREE });
    await status.check(DAMAGED.path);
    noteRepairResult(DAMAGED.path, { verdict: 'not_damaged' }, status);
    expect(status.last(DAMAGED.path)).toBe(false);
    expect(status.lastMaps(DAMAGED.path)).toEqual(THREE);
  });
});

describe('mapsToSplit', () => {
  it("asks a damaged demo's \"Already split\" with the scan's maps, any other with its own", async () => {
    const { ask, status } = checker({ repairable: true, maps: ['a', 'b', 'c'] });
    expect(await mapsToSplit(DAMAGED.path, ['a'], true, status)).toEqual(['a', 'b', 'c']);
    expect(await mapsToSplit(WHOLE.path, ['a', 'b'], false, status)).toEqual(['a', 'b']);
    expect(ask).toHaveBeenCalledTimes(1);
  });

  it("falls back to the demo's own maps when the check fails", async () => {
    const { status } = checker(new Error('gone'));
    expect(await mapsToSplit(DAMAGED.path, ['a', 'b'], true, status)).toEqual(['a', 'b']);
  });
});

describe('noteRepairResult', () => {
  it('remembers a "not damaged" verdict as nothing to repair, without asking', () => {
    const { ask, status } = checker();
    noteRepairResult(DAMAGED.path, { verdict: 'not_damaged' }, status);
    expect(status.last(DAMAGED.path)).toBe(false);
    expect(ask).not.toHaveBeenCalled();
  });

  it('leaves the answer alone for any other verdict', () => {
    const { status } = checker();
    noteRepairResult(DAMAGED.path, { verdict: 'refused' }, status);
    noteRepairResult(DAMAGED.path, { verdict: 'cancelled' }, status);
    expect(status.last(DAMAGED.path)).toBeNull();
  });
});
