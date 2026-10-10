import { describe, it, expect } from 'vitest';
import { clockFloor, clockRound, clockLong, elapsedWords } from './time_format.js';

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

describe('elapsedWords', () => {
  it('is seconds alone under a minute', () => {
    expect(elapsedWords(0)).toBe('0s');
    expect(elapsedWords(45)).toBe('45s');
  });
  it('pads seconds after minutes', () => {
    expect(elapsedWords(245)).toBe('4m 05s');
    expect(elapsedWords(3600)).toBe('60m 00s');
  });
});
