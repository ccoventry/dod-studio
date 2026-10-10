import { describe, it, expect } from 'vitest';
import { worldToOverview, engagementByWeapon, engagementOverall, unitsToMetres } from './kill_map.js';

// dod_anzio.txt and dod_saints2_b2.txt as shipped.
const anzio = { zoom: 1.11, origin: [307.06, 372.72, -334], rotated: false };
const saints = { zoom: 1.25, origin: [-704, 196, -96], rotated: true };

describe('worldToOverview (#448)', () => {
  it('puts ORIGIN at the centre of the image', () => {
    expect(worldToOverview([307.06, 372.72, 0], anzio)).toEqual({ u: 0.5, v: 0.5 });
    expect(worldToOverview([-704, 196, 0], saints)).toEqual({ u: 0.5, v: 0.5 });
  });

  it('points +X up and +Y left on an unrotated map', () => {
    const c = worldToOverview([307.06, 372.72, 0], anzio);
    const north = worldToOverview([407.06, 372.72, 0], anzio);
    const west = worldToOverview([307.06, 472.72, 0], anzio);
    expect(north.v).toBeLessThan(c.v);
    expect(north.u).toBeCloseTo(c.u);
    expect(west.u).toBeLessThan(c.u);
    expect(west.v).toBeCloseTo(c.v);
  });

  it('spans 8192/zoom units across and 8192/(zoom*1.33) down', () => {
    const halfWidth = 4096 / anzio.zoom;
    const halfHeight = 4096 / (anzio.zoom * 1.33);
    expect(worldToOverview([307.06, 372.72 + halfWidth, 0], anzio).u).toBeCloseTo(0);
    expect(worldToOverview([307.06 - halfHeight, 372.72, 0], anzio).v).toBeCloseTo(1);
  });

  it('turns a rotated map a quarter turn: +Y is up, +X is right', () => {
    const c = worldToOverview([-704, 196, 0], saints);
    const plusY = worldToOverview([-704, 296, 0], saints);
    const plusX = worldToOverview([-604, 196, 0], saints);
    expect(plusY.v).toBeLessThan(c.v);
    expect(plusY.u).toBeCloseTo(c.u);
    expect(plusX.u).toBeGreaterThan(c.u);
    expect(plusX.v).toBeCloseTo(c.v);
  });

  it('a real death on anzio lands on the image', () => {
    // The first death in ktps8w3-dyelife_ih_anzio_h2.dem.
    const { u, v } = worldToOverview([-528, 883.34375, -291.96875], anzio);
    expect(u).toBeGreaterThan(0);
    expect(u).toBeLessThan(1);
    expect(v).toBeGreaterThan(0);
    expect(v).toBeLessThan(1);
  });
});

describe('engagement distance (#448)', () => {
  const kills = [
    { weapon: 'K98', distance: 1000, teamkill: false },
    { weapon: 'K98', distance: 500, teamkill: false },
    { weapon: 'Mp40', distance: 300, teamkill: false },
    { weapon: 'Mp40', distance: 9000, teamkill: true },
    { weapon: 'Garand', distance: null, teamkill: false },
  ];

  it('averages per weapon, leaving out teamkills and unmeasured kills', () => {
    expect(engagementByWeapon(kills)).toEqual([
      { weapon: 'K98', count: 2, average: 750, longest: 1000 },
      { weapon: 'Mp40', count: 1, average: 300, longest: 300 },
    ]);
  });

  it('weights the overall average by kills', () => {
    expect(engagementOverall(kills)).toEqual({ count: 3, average: 600, longest: 1000 });
  });

  it('has no overall figure without a measured kill', () => {
    expect(engagementOverall([])).toBeNull();
    expect(engagementOverall(undefined)).toBeNull();
  });

  it('converts units to metres at an inch each', () => {
    expect(unitsToMetres(1000)).toBeCloseTo(25.4);
  });
});
