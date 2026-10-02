import { describe, it, expect } from 'vitest';
import { formatSegmentTime, mapSegmentOptions, segmentToRequest } from './map_segments.js';

const report = (segments, mapSegment = 0) => ({
  demo_info: { map_segments: segments },
  state: { map_segment: mapSegment },
});

const lennonTwice = [
  { map_name: 'dod_lennon2', start_secs: 0, end_secs: 1288.4, start_frame: 0 },
  { map_name: 'dod_lennon2', start_secs: 1288.4, end_secs: 1302.6, start_frame: 738806 },
];

describe('map picker (#217)', () => {
  it('is hidden for a demo with one map', () => {
    expect(mapSegmentOptions(report([lennonTwice[0]]))).toBeNull();
  });

  it('is hidden for a report cached before segments were recorded', () => {
    expect(mapSegmentOptions({ demo_info: {}, state: {} })).toBeNull();
  });

  it('lists each map with its time range', () => {
    const options = mapSegmentOptions(report(lennonTwice));
    expect(options.map((o) => o.label)).toEqual([
      'dod_lennon2 (0:00–21:28)',
      'dod_lennon2 (21:28–21:42)',
    ]);
  });

  it('selects the segment the report covers', () => {
    expect(mapSegmentOptions(report(lennonTwice, 1)).map((o) => o.selected)).toEqual([false, true]);
  });

  it('asks for the default analysis when the analyzer\'s own choice is picked', () => {
    expect(segmentToRequest(0, 0)).toBeNull();
    expect(segmentToRequest(1, 0)).toBe(1);
    expect(segmentToRequest(0, 1)).toBe(0);
  });
});

describe('formatSegmentTime', () => {
  it('shows minutes and seconds, and hours from an hour up', () => {
    expect(formatSegmentTime(0)).toBe('0:00');
    expect(formatSegmentTime(65.9)).toBe('1:05');
    expect(formatSegmentTime(3725)).toBe('1:02:05');
  });
});
