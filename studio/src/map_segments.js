// map_segments.js
//
// The Demo Analyzer's map picker (#217). A demo that kept recording through a
// level change holds one map segment per signon (`demo_info.map_segments`),
// and each can be analysed on its own. Most demos have exactly one, and then
// there is no picker.

import { STRINGS } from './strings.js';

/** `m:ss`, or `h:mm:ss` from an hour up. */
export function formatSegmentTime(totalSecs) {
  const s = Math.max(0, Math.floor(totalSecs || 0));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const ss = String(s % 60).padStart(2, '0');
  return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${ss}` : `${m}:${ss}`;
}

/**
 * The picker's options for an analyzer report, or `null` when the demo holds
 * a single map. The option matching the segment the report covers
 * (`state.map_segment`) is the selected one.
 */
export function mapSegmentOptions(report) {
  const segments = (report && report.demo_info && report.demo_info.map_segments) || [];
  if (segments.length < 2) return null;
  const current = (report.state && report.state.map_segment) || 0;
  return segments.map((seg, index) => ({
    index,
    label: STRINGS.ANALYZER.mapSegmentOption(
      seg.map_name,
      formatSegmentTime(seg.start_secs),
      formatSegmentTime(seg.end_secs)
    ),
    selected: index === current,
  }));
}

/**
 * What to ask the backend for when the user picks segment `index`. The
 * segment the analyzer chose by itself is requested as `null`, the default
 * analysis, so it keeps hitting the same cache entry as every other open.
 */
export function segmentToRequest(index, autoSegment) {
  return index === autoSegment ? null : index;
}
