import { numberField } from './number_field.js';
import { STRINGS } from './strings.js';

// Illustrative gap between the two kill anchors in the timing diagram below
// — a real streak's length varies per capture; this is just enough to make
// Start Lead/Stop Trail visually distinct from Pre-roll/Post-roll.
const TIMING_DIAGRAM_ILLUSTRATIVE_STREAK_SECONDS = 8.0;

/**
 * Rebuilds the Timings tab's execution-timeline table (#150) — revives the
 * pre-Tauri egui build's "Mock Execution Timeline" (native/src/bin/gui/views/
 * capture/panels.rs, dropped in the egui purge): a sorted list of events at
 * their time relative to the first kill (0.0s), not a to-scale bar. Called
 * once at init and on every `input` event from the five timing fields.
 */
export function renderTimingDiagram() {
  const preRoll = numberField('#config-pre-roll', 0);
  const postRoll = numberField('#config-post-roll', 0);
  const startLead = numberField('#config-record-start-lead', 0);
  const stopTrail = numberField('#config-record-stop-trail', 0);
  const initialDelay = numberField('#config-initial-delay', 0);
  const streak = TIMING_DIAGRAM_ILLUSTRATIVE_STREAK_SECONDS;

  // Initial Delay only applies to patched demos: with Seek Between Clips on,
  // the hook waits for the demo to load and moves to clip 1 straight away
  // (native::patch::runtime_batch), so the field and its note are hidden.
  const seeks = document.querySelector('#config-hook-runs-batch')?.checked ?? true;
  const delayGroup = document.querySelector('#config-initial-delay-group');
  if (delayGroup) delayGroup.style.display = seeks ? 'none' : '';
  const note = document.querySelector('#timing-diagram-initial-delay-note');
  if (note) {
    note.textContent = seeks ? '' : STRINGS.CAPTURE_CONFIG.timingDiagramInitialDelayNote(initialDelay);
    note.style.display = seeks ? 'none' : '';
  }

  const C = STRINGS.CAPTURE_CONFIG;
  const events = [
    { time: -(startLead + preRoll), label: C.timingDiagramPreRollEvent(preRoll), anchor: false },
    { time: -startLead, label: C.timingDiagramRecordStartEvent(startLead), anchor: false },
    { time: 0, label: C.TIMING_DIAGRAM_FIRST_KILL_EVENT, anchor: true },
    { time: streak, label: C.TIMING_DIAGRAM_LAST_KILL_EVENT, anchor: true },
    { time: streak + stopTrail, label: C.timingDiagramRecordStopEvent(stopTrail), anchor: false },
    { time: streak + stopTrail + postRoll, label: C.timingDiagramPostRollEvent(postRoll), anchor: false },
  ].sort((a, b) => a.time - b.time);

  const tbody = document.querySelector('#timing-diagram-events');
  if (!tbody) return;
  tbody.innerHTML = events.map((e) => `
    <tr${e.anchor ? ' class="timing-diagram-anchor"' : ''}>
      <td class="timing-diagram-time">${C.timingDiagramTime(e.time)}</td>
      <td>${e.label}</td>
    </tr>`).join('');
}
