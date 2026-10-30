// analyzer_report_logic.js
// Pure helpers of the Demo Analyzer's report tabs: team colours and labels,
// durations, clock text, mortality spans. No DOM, no Tauri, so the web
// analyzer can share them (#238) and Vitest can pin their output.

import { STRINGS } from './strings.js';
import { durationText } from './time_format.js';

export const TEAM_COLORS = {
  Allies: '#4caf50',
  British: '#daa520',
  Axis: '#e05252',
  Spectators: '#cccc33',
  Unassigned: '#aaaaaa',
};

// ── Small formatting helpers ─────────────────────────────────────────────────

export function teamColor(team) { return TEAM_COLORS[team] || '#ffffff'; }

export function teamLabel(team, alliesAreBritish) {
  if (team === 'Allies' || team === 'British') return alliesAreBritish ? STRINGS.ANALYZER.BRITISH_LABEL : STRINGS.ANALYZER.ALLIES_LABEL;
  if (team === 'Axis') return STRINGS.ANALYZER.AXIS_LABEL;
  if (team === 'Spectators') return STRINGS.ANALYZER.SPECTATORS_LABEL;
  return STRINGS.ANALYZER.UNASSIGNED_LABEL;
}

export function durSecs(d) { return d ? (d.secs || 0) + (d.nanos || 0) / 1e9 : 0; }

export function formatDuration(totalSecs) {
  return durationText(totalSecs);
}

export function formatGameTime(totalSecs) {
  totalSecs = Math.max(0, totalSecs || 0);
  const m = Math.floor(totalSecs / 60);
  const s = Math.floor(totalSecs % 60);
  const cc = Math.floor((totalSecs - Math.floor(totalSecs)) * 100);
  return `${m}:${String(s).padStart(2, '0')}:${String(cc).padStart(2, '0')}`;
}

export function isConnected(player) {
  return player.connection && typeof player.connection === 'object' && 'Connected' in player.connection;
}

export function playerClientId(player) {
  return isConnected(player) ? player.connection.Connected.client_id : null;
}

export function mortalityLifespans(mortality) {
  if (!mortality || mortality.length === 0) return [];
  const spans = [];
  let aliveAt = null;
  for (const change of mortality) {
    const [time, state] = change;
    const secs = durSecs(time.viewdemo_offset);
    if (state === 'Alive') aliveAt = secs;
    else if (state === 'Dead' && aliveAt !== null) {
      if (secs >= aliveAt) spans.push(secs - aliveAt);
      aliveAt = null;
    }
  }
  return spans;
}

export function groupConsecutiveWeapons(names) {
  const parts = [];
  let i = 0;
  while (i < names.length) {
    let j = i;
    while (j + 1 < names.length && names[j + 1] === names[i]) j++;
    const count = j - i + 1;
    parts.push(count > 1 ? `${names[i]} x${count}` : names[i]);
    i = j + 1;
  }
  return parts.join(', ');
}
