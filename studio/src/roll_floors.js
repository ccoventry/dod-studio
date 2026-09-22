// roll_floors.js
// Says when the pre-roll or post-roll is shorter than this capture needs.
//
// The rolls used to be taste. They are load-bearing now: playback returns to
// real time one pre-roll before recording, so the audio resync, the stopsound
// flush, the decal sweep's lead and any Scheduled Command's offset all have to
// fit inside it — and anything that does not fit runs while the engine is still
// fast-forwarding with its audio buffers unflushed.
//
// Advice, not a clamp. The terms are knowable but the engine's audio guidance is
// a 2-4s range rather than a constant, and whether the decal burst itself needs
// real-time playback is unverified. A number derived partly from an unknown has
// no business overwriting what someone chose.

import { getRollFloors } from './ipc_bridge.js';
import { STRINGS } from './strings.js';

/// Supplies the Scheduled Commands, which capture_pane.js owns. Read at check
/// time rather than captured, since the list is edited live.
let getCustomCommands = () => [];

function bannerEl() {
  return document.querySelector('#roll-floor-banner');
}

/** Re-check the rolls against what the current configuration needs. */
export async function refreshRollFloors() {
  const el = bannerEl();
  if (!el) return;

  const preRoll = parseFloat(document.querySelector('#config-pre-roll')?.value) || 0;
  const postRoll = parseFloat(document.querySelector('#config-post-roll')?.value) || 0;
  const decalFlush = document.querySelector('#config-decal-flush')?.checked ?? true;
  const customCommands = (getCustomCommands() || [])
    .filter((c) => c?.command?.trim())
    .map((c) => ({
      command: c.command.trim(),
      offset_seconds: Number(c.offsetSeconds) || 0,
      relation: c.relation === 'After' ? 'After' : 'Before',
    }));

  const report = await getRollFloors(preRoll, postRoll, decalFlush, customCommands);
  if (!report) return;

  const problems = [];
  if (report.preRoll < report.preRollFloor) {
    problems.push(
      STRINGS.ROLLS.tooShort('Pre-roll', report.preRoll, report.preRollFloor, report.preRollBinding)
    );
  }
  if (report.postRoll < report.postRollFloor) {
    problems.push(
      STRINGS.ROLLS.tooShort('Post-roll', report.postRoll, report.postRollFloor, report.postRollBinding)
    );
  }

  if (problems.length === 0) {
    el.hidden = true;
    el.innerHTML = '';
    return;
  }

  el.hidden = false;
  el.style.cssText =
    'margin: 0 0 10px; padding: 10px 12px; border: 1px solid #b58900; ' +
    'border-radius: 4px; background: #2a2410; color: #e8dcb0; font-size: 12px;';
  el.innerHTML = `
    <strong>${STRINGS.ROLLS.BANNER_TITLE}</strong>
    <ul style="margin:6px 0 6px 18px; padding:0;">
      ${problems.map((p) => `<li>${p}</li>`).join('')}
    </ul>
    <div style="opacity:.8">${STRINGS.ROLLS.ADVICE}</div>`;
}

/**
 * Wire the check to everything that can move a floor: the two roll inputs, and
 * the decal flush switch (its lead is one of the terms). The Scheduled Command
 * list is the fourth term — capture_pane.js calls `refreshRollFloors` when that
 * list changes, since it owns it.
 */
export function initRollFloors(customCommandSource) {
  if (typeof customCommandSource === 'function') getCustomCommands = customCommandSource;
  ['#config-pre-roll', '#config-post-roll', '#config-decal-flush',
   '#config-record-start-lead', '#config-record-stop-trail'].forEach((sel) => {
    const input = document.querySelector(sel);
    if (input) input.addEventListener('change', refreshRollFloors);
  });
  refreshRollFloors();
}
