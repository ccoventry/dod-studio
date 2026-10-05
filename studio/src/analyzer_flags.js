// analyzer_flags.js
// The Demo Analyzer's Flags tab (#192): what the analysis already reads about
// the flags (analysis::objective) shown for the first time. Per team:
// captures, how many were breaks (a flag the other team held), and the
// capture attempts it blocked; per flag: who holds it at the end and how
// often it changed hands; every capture with its cappers; and each player's
// capture credits and objective points.
//
// The summary is pure (flagSummary) so it can be tested on its own; the tab
// takes analyzer_pane.js's own helpers rather than importing them.

import { STRINGS } from './strings.js';

const ALLIED = new Set(['Allies', 'British']);
const side = (team) => (ALLIED.has(team) ? 'allied' : team === 'Axis' ? 'axis' : null);

/**
 * A flag's name as a person would say it. Stock maps name flags by a map
 * token (`POINT_ANZIO_PLAZA`), which reads as "Plaza"; a literal name
 * ("the alley") is kept as it is.
 */
export function flagLabel(name, map) {
  if (!name || !/^POINT_[A-Z0-9_]+$/.test(name)) return name;
  const mapWord = String(map || '').replace(/^dod_/i, '').split('_')[0].toUpperCase();
  let words = name.replace(/^POINT_/, '');
  if (mapWord && words.startsWith(`${mapWord}_`)) words = words.slice(mapWord.length + 1);
  return words.split('_').filter(Boolean).map((w) => w[0] + w.slice(1).toLowerCase()).join(' ');
}

/** A capture of a flag the other side held (analysis's `is_break`). */
export function isBreak(capture) {
  const from = side(capture.previous_owner);
  const to = side(capture.team);
  return Boolean(from && to && from !== to);
}

/**
 * Everything the tab shows, from a report's `state`:
 * `{ teams: { allied, axis }, flags: [...], captures: [...], players: [...] }`.
 */
export function flagSummary(state, map = null) {
  const objectives = state?.objectives || {};
  const captures = objectives.captures || [];
  const attempts = objectives.attempts || [];
  const names = new Map((state?.players || []).map((p) => [p.id, p.name]));
  const team = () => ({ captures: 0, breaks: 0, blocks: 0, attempts: 0 });
  const teams = { allied: team(), axis: team() };

  for (const c of captures) {
    const s = side(c.team);
    if (!s) continue;
    teams[s].captures += 1;
    if (isBreak(c)) teams[s].breaks += 1;
  }
  for (const a of attempts) {
    const s = side(a.team);
    if (!s) continue;
    teams[s].attempts += 1;
    // A cancelled attempt is a block, credited to the side defending.
    if (a.outcome === 'Cancelled') teams[s === 'allied' ? 'axis' : 'allied'].blocks += 1;
  }

  const byArea = new Map();
  for (const c of captures) {
    if (c.area_index == null) continue;
    byArea.set(c.area_index, (byArea.get(c.area_index) || 0) + 1);
  }
  const blockedByArea = new Map();
  for (const a of attempts.filter((x) => x.outcome === 'Cancelled')) {
    blockedByArea.set(a.area_index, (blockedByArea.get(a.area_index) || 0) + 1);
  }
  const flags = (objectives.flags || []).map((f) => ({
    area: f.area_index,
    name: flagLabel(f.name, map) || null,
    owner: f.owner,
    captures: byArea.get(f.area_index) || 0,
    blocked: blockedByArea.get(f.area_index) || 0,
  }));

  const rows = captures.map((c) => ({
    time: c.time,
    flag: flagLabel(c.flag_name, map),
    team: c.team,
    brk: isBreak(c),
    cappers: [c.capper, ...(c.co_cappers || [])].filter(Boolean).map((id) => names.get(id) || id),
  }));

  const players = (state?.players || [])
    .filter((p) => (p.cap_credits || 0) > 0 || (p.obj_points || 0) > 0)
    .map((p) => ({ name: p.name, team: p.team, caps: p.cap_credits || 0, points: p.obj_points || 0 }))
    .sort((a, b) => b.caps - a.caps || b.points - a.points || a.name.localeCompare(b.name));

  return { teams, flags, captures: rows, players };
}

/**
 * Draws the tab into `container`. `h` is analyzer_pane.js's helpers:
 * `{ esc, teamColor, teamLabel, durSecs, formatMMSS }`.
 */
export function renderFlagsTab(container, report, h) {
  const A = STRINGS.ANALYZER;
  const british = report.state.allies_are_british;
  const sum = flagSummary(report.state, report.demo_info?.map_name);
  if (!sum.flags.length && !sum.captures.length && !sum.players.length) {
    container.innerHTML = `<p class="analyzer-empty">${A.FLAGS_NONE}</p>`;
    return;
  }
  const dot = (team) => `<span class="analyzer-team-dot" style="background:${h.teamColor(team)};"></span>`;
  const alliedTeam = british ? 'British' : 'Allies';
  const teamCard = (key, teamName) => {
    const t = sum.teams[key];
    return `
      <div class="analyzer-stat-card">
        <div class="stat-title">${dot(teamName)} ${h.esc(h.teamLabel(teamName, british))}</div>
        <div class="stat-value">${t.captures}</div>
        <div class="stat-badge text-muted">${A.flagsTeamBadge(t.captures, t.breaks, t.blocks, t.attempts)}</div>
      </div>`;
  };

  const flagRows = sum.flags.map((f) => `
    <tr>
      <td>${h.esc(f.name || A.flagArea(f.area))}</td>
      <td>${dot(f.owner)} ${h.esc(h.teamLabel(f.owner, british))}</td>
      <td style="text-align:right;">${f.captures}</td>
      <td style="text-align:right;">${f.blocked}</td>
    </tr>`).join('');

  const captureRows = sum.captures.map((c) => `
    <tr>
      <td>${h.formatMMSS(h.durSecs(c.time.viewdemo_offset))}</td>
      <td>${h.esc(c.flag)}</td>
      <td>${dot(c.team)} ${h.esc(h.teamLabel(c.team, british))}</td>
      <td>${c.brk ? A.FLAGS_BREAK : ''}</td>
      <td>${h.esc(c.cappers.join(', ') || A.EMPTY_DASH)}</td>
    </tr>`).join('');

  const playerRows = sum.players.map((p) => `
    <tr>
      <td>${dot(p.team)} ${h.esc(p.name)}</td>
      <td style="text-align:right;">${p.caps}</td>
      <td style="text-align:right;">${p.points}</td>
    </tr>`).join('');

  container.innerHTML = `
    <div class="analyzer-stat-cards analyzer-flag-cards">${teamCard('allied', alliedTeam)}${teamCard('axis', 'Axis')}</div>
    <div class="analyzer-stacked-sections">
      <div>
        <h4 class="analyzer-section-title">${A.FLAGS_TITLE}</h4>
        <div class="table-wrapper">
          <table class="analyzer-table">
            <thead><tr><th>${A.COL_FLAG}</th><th>${A.COL_OWNER_AT_END}</th><th style="text-align:right;">${A.COL_CAPTURES}</th><th style="text-align:right;">${A.COL_BLOCKED}</th></tr></thead>
            <tbody>${flagRows || `<tr><td colspan="4" class="table-empty">${A.FLAGS_NO_LAYOUT}</td></tr>`}</tbody>
          </table>
        </div>
      </div>
      <div>
        <h4 class="analyzer-section-title">${A.CAPTURES_TITLE}</h4>
        <div class="table-wrapper" style="max-height:320px;">
          <table class="analyzer-table">
            <thead><tr><th>${A.COL_TIME}</th><th>${A.COL_FLAG}</th><th>${A.COL_TEAM}</th><th></th><th>${A.COL_CAPPERS}</th></tr></thead>
            <tbody>${captureRows || `<tr><td colspan="5" class="table-empty">${A.FLAGS_NO_CAPTURES}</td></tr>`}</tbody>
          </table>
        </div>
      </div>
      <div>
        <h4 class="analyzer-section-title">${A.CAPPERS_TITLE}</h4>
        <div class="table-wrapper">
          <table class="analyzer-table">
            <thead><tr><th>${A.COL_PLAYER}</th><th style="text-align:right;" title="${A.COL_CAP_CREDITS_TITLE}">${A.COL_CAP_CREDITS}</th><th style="text-align:right;" title="${A.COL_OBJ_POINTS_TITLE}">${A.COL_OBJ_POINTS}</th></tr></thead>
            <tbody>${playerRows || `<tr><td colspan="3" class="table-empty">${A.FLAGS_NO_CAPTURES}</td></tr>`}</tbody>
          </table>
        </div>
      </div>
    </div>`;
}
