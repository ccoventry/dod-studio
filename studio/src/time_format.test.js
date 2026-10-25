import { describe, it, expect } from 'vitest';
import { clockFloor, clockRound, clockLong, durationText, durationTextMs } from './time_format.js';

describe('clockFloor', () => {
  it('pads seconds and rounds down', () => {
    expect(clockFloor(65.9)).toBe('1:05');
    expect(clockFloor(0)).toBe('0:00');
    expect(clockFloor(59.99)).toBe('0:59');
  });
  it('lets minutes grow past an hour', () => {
    expect(clockFloor(3725)).toBe('62:05');
  });
  it('reads missing or negative as 0:00', () => {
    expect(clockFloor(-3)).toBe('0:00');
    expect(clockFloor(undefined)).toBe('0:00');
    expect(clockFloor(NaN)).toBe('0:00');
  });
});

describe('clockRound', () => {
  it('rounds to the nearest second', () => {
    expect(clockRound(65.4)).toBe('1:05');
    expect(clockRound(59.6)).toBe('1:00');
  });
  it('reads missing or negative as 0:00', () => {
    expect(clockRound(-0.4)).toBe('0:00');
    expect(clockRound(null)).toBe('0:00');
  });
});

describe('clockLong', () => {
  it('is m:ss under an hour', () => {
    expect(clockLong(125)).toBe('2:05');
    expect(clockLong(3599.4)).toBe('59:59');
  });
  it('is h:mm:ss from an hour', () => {
    expect(clockLong(3599.6)).toBe('1:00:00');
    expect(clockLong(3725)).toBe('1:02:05');
  });
  it('reads negative as 0:00', () => {
    expect(clockLong(-10)).toBe('0:00');
  });
});

describe('durationText (#880)', () => {
  it('is one decimal and a space under a minute', () => {
    expect(durationText(2.14)).toBe('2.1 s');
    expect(durationText(2)).toBe('2.0 s');
    expect(durationText(0)).toBe('0.0 s');
    expect(durationText(59.94)).toBe('59.9 s');
  });
  it('is whole minutes and seconds from a minute', () => {
    // 59.96 would read 60.0 s: it is a minute.
    expect(durationText(59.96)).toBe('1 min 0 s');
    expect(durationText(372.4)).toBe('6 min 12 s');
    expect(durationText(3599.4)).toBe('59 min 59 s');
  });
  it('adds hours from an hour', () => {
    expect(durationText(3600)).toBe('1 h 0 min 0 s');
    expect(durationText(3723)).toBe('1 h 2 min 3 s');
  });
  it('reads missing or negative as 0.0 s', () => {
    expect(durationText(undefined)).toBe('0.0 s');
    expect(durationText(-4)).toBe('0.0 s');
    expect(durationText(NaN)).toBe('0.0 s');
  });
  it('takes milliseconds too', () => {
    expect(durationTextMs(6_840)).toBe('6.8 s');
    expect(durationTextMs(65_000)).toBe('1 min 5 s');
    expect(durationTextMs(undefined)).toBe('0.0 s');
  });
});
