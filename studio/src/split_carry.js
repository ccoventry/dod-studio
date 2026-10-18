// split_carry.js
// Carries a split demo's highlight work onto its pieces (#217).
//
// Splitting a demo that recorded more than one map scans each piece into the
// queue in its place, and a scan makes new highlight objects. Everything the
// user did on the original (Review mark, note, status, pick, clip name, Kill
// Range) would go with its row, so this finds each worked-on highlight again
// in a piece and puts that work back.
//
// Which piece: the original's analysis stops at its second signon
// (`analysis::use_segment_boundary`), so its highlights all come from the
// first map, and only the piece the split wrote for map 0 (`Written.index`)
// can hold them. That piece is the original cut off before the second signon,
// so its kills normally have the same ticks and times; the matching below
// still doesn't rely on that.
//
// Same highlight = same player and the same kills: the weapons, and victims
// when both scans name them, in order, with the same gaps between kill times
// (within KILL_GAP_TOLERANCE). Kills are compared across all of a player's
// highlights in the piece, not one at a time, so a life the piece's scan cut
// into two highlights still matches. A pattern that fits more than one place
// (a single Kar kill, say) is settled by the piece's time offset, measured
// from the highlights whose pattern fits exactly one place; if that still
// leaves more than one, or none, the work is reported, never guessed.

import { HIGHLIGHT_STATUS } from './status_colors.js';
import { isRangeModified, streakUid } from './take_index.js';
import { highlightStartSeconds } from './highlight_time.js';
import { clockRound } from './time_format.js';
import { fileNameOf } from './path_display.js';
import { STRINGS } from './strings.js';

/** Seconds a kill's gap from the first kill may differ by between scans. */
export const KILL_GAP_TOLERANCE = 0.5;
/** Seconds a candidate's time offset may differ from the piece's. */
const OFFSET_TOLERANCE = 1.0;

export const UNCARRIED_REASON = Object.freeze({
  NO_PIECE: 'no-piece',
  NOT_FOUND: 'not-found',
  AMBIGUOUS: 'ambiguous',
  TAKEN: 'taken',
});

/** True when `streak` carries anything the user set (what preserveHighlightState copies). */
export function hasWork(streak) {
  if (!streak) return false;
  if (streak.status !== undefined && streak.status !== HIGHLIGHT_STATUS.NONE) return true;
  if (streak.statusByHand || streak.selected === true || streak.curation) return true;
  if (streak.notes && streak.notes.trim()) return true;
  if (streak.clipName && streak.clipName.trim()) return true;
  return isRangeModified(streak);
}

/** Same player: SteamID when both have one, else name, else slot. */
export function samePlayer(a, b) {
  if (a.steam_id && b.steam_id) return a.steam_id === b.steam_id;
  if (a.target_player && b.target_player) return a.target_player === b.target_player;
  return a.player_index === b.player_index;
}

/** The pieces that can hold the original's highlights: map 0's, else (no
 *  index, as from an older backend) those on the original's map. */
function candidatePieces(original, pieces) {
  if (pieces.some((p) => Number.isInteger(p.splitIndex))) return pieces.filter((p) => p.splitIndex === 0);
  if (original.map_name) return pieces.filter((p) => p.map_name === original.map_name);
  return pieces;
}

/** One player's kills in a piece, in time order, each knowing its highlight. */
function playerKills(piece, like) {
  const flat = [];
  (piece.streaks || []).forEach((streak) => {
    if (!samePlayer(streak, like)) return;
    (streak.kills || []).forEach((kill, k) => {
      flat.push({ streak, k, time: kill[1], weapon: kill[2], victim: streak.victims?.[k] });
    });
  });
  return flat.sort((x, y) => x.time - y.time);
}

/** Every place in `piece` the original highlight's kills fit. */
function placesFor(original, piece) {
  const kills = original.kills || [];
  if (kills.length === 0) return [];
  const flat = playerKills(piece, original);
  const places = [];
  for (let j = 0; j + kills.length <= flat.length; j++) {
    const fits = kills.every((kill, i) => {
      const f = flat[j + i];
      if (f.weapon !== kill[2]) return false;
      const victim = original.victims?.[i];
      if (victim && f.victim && victim !== f.victim) return false;
      return Math.abs((f.time - flat[j].time) - (kill[1] - kills[0][1])) <= KILL_GAP_TOLERANCE;
    });
    if (fits) places.push({ piece, flat, at: j, offset: flat[j].time - kills[0][1] });
  }
  return places;
}

function median(values) {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)];
}

/** Where the selected kills (the Kill Range) land: the piece highlight that
 *  holds most of them, and the range in its own kill indices. */
function rangeTarget(original, place) {
  const last = Math.max((original.kills || []).length - 1, 0);
  const end = Math.min(original.end_index ?? last, last);
  const start = Math.min(original.start_index ?? 0, end);
  const picked = place.flat.slice(place.at + start, place.at + end + 1);
  const byStreak = new Map();
  picked.forEach((f) => {
    if (!byStreak.has(f.streak)) byStreak.set(f.streak, []);
    byStreak.get(f.streak).push(f.k);
  });
  let best = null;
  byStreak.forEach((ks, streak) => {
    if (!best || ks.length > best.ks.length) best = { streak, ks };
  });
  return {
    target: best.streak,
    start_index: Math.min(...best.ks),
    end_index: Math.max(...best.ks),
    narrowed: byStreak.size > 1,
  };
}

/**
 * Matches the original demo's worked-on highlights to the pieces' highlights.
 * `pieces` are the scanned demos, each with `splitIndex` (the split's
 * `Written.index`) when known. Pure: changes nothing.
 *
 * Resolves `{ carried: [{ from, to, piece, start_index, end_index, narrowed }],
 * uncarried: [{ from, reason }], withWork }`.
 */
export function matchSplitHighlights(original, pieces) {
  const streaks = original?.streaks || [];
  const worked = streaks.filter(hasWork);
  const result = { carried: [], uncarried: [], withWork: worked.length };
  if (worked.length === 0) return result;
  const candidates = candidatePieces(original, pieces || []);
  if (candidates.length === 0) {
    result.uncarried = worked.map((from) => ({ from, reason: UNCARRIED_REASON.NO_PIECE }));
    return result;
  }

  const placesOf = new Map(streaks.map((s) => [s, candidates.flatMap((p) => placesFor(s, p))]));
  // Each piece's offset from the original, from every highlight (worked on or
  // not) with two or more kills that fits exactly one place.
  const offsets = new Map();
  streaks.forEach((s) => {
    const places = placesOf.get(s);
    if ((s.kills || []).length < 2 || places.length !== 1) return;
    const [only] = places;
    if (!offsets.has(only.piece)) offsets.set(only.piece, []);
    offsets.get(only.piece).push(only.offset);
  });
  const pieceOffset = new Map([...offsets].map(([piece, list]) => [piece, median(list)]));

  const taken = new Set();
  worked.forEach((from) => {
    const places = placesOf.get(from).filter((p) => !pieceOffset.has(p.piece)
      || Math.abs(p.offset - pieceOffset.get(p.piece)) <= OFFSET_TOLERANCE);
    if (places.length === 0) {
      result.uncarried.push({ from, reason: UNCARRIED_REASON.NOT_FOUND });
      return;
    }
    if (places.length > 1) {
      result.uncarried.push({ from, reason: UNCARRIED_REASON.AMBIGUOUS });
      return;
    }
    const range = rangeTarget(from, places[0]);
    if (taken.has(range.target)) {
      result.uncarried.push({ from, reason: UNCARRIED_REASON.TAKEN });
      return;
    }
    taken.add(range.target);
    result.carried.push({ from, to: range.target, piece: places[0].piece, ...range });
  });
  return result;
}

/**
 * Puts each carried highlight's work on its match, Kill Range included,
 * and returns old uid -> new uid for the take index. Callers redraw the
 * matched highlights (updateStreakVisuals) since their range may change.
 */
export function applyCarriedWork(original, carried) {
  const uids = new Map();
  carried.forEach(({ from, to, piece, start_index, end_index }) => {
    if (from.status !== undefined) to.status = from.status;
    if (from.statusByHand) to.statusByHand = true;
    if (from.selected !== undefined) to.selected = from.selected;
    if (from.notes !== undefined) to.notes = from.notes;
    if (from.clipName !== undefined) to.clipName = from.clipName;
    if (from.curation !== undefined) to.curation = from.curation;
    to.start_index = start_index;
    to.end_index = end_index;
    uids.set(streakUid(original.path, from), streakUid(piece.path, to));
  });
  return uids;
}

function workText(s) {
  const last = Math.max((s.kills || []).length - 1, 0);
  return STRINGS.MAIN.splitWorkText({
    curation: s.curation,
    status: s.status !== HIGHLIGHT_STATUS.NONE ? s.status : undefined,
    picked: s.selected === true,
    range: isRangeModified(s) ? `${(s.start_index ?? 0) + 1}-${(s.end_index ?? last) + 1} of ${last + 1}` : '',
    clipName: s.clipName?.trim(),
    notes: s.notes?.trim(),
  });
}

/**
 * What a split did with the original's work: the toast (null when nothing
 * had work on it) and the activity log lines, one per highlight whose work
 * didn't carry or whose Kill Range was narrowed, naming its time, player and
 * every piece of work, so nothing goes without a trace.
 */
export function carryReport(original, result, piecePaths) {
  const name = fileNameOf(original.path);
  const kept = result.carried.length;
  const total = result.withWork;
  if (total === 0) return { toast: null, log: [] };
  const at = (s) => clockRound(highlightStartSeconds(s, original.tickrate));
  const who = (s) => s.target_player || `player ${s.player_index}`;
  const log = [STRINGS.MAIN.splitCarryLog(name, kept, total, piecePaths.map(fileNameOf))];
  result.carried.filter((c) => c.narrowed).forEach((c) => {
    log.push(STRINGS.MAIN.splitNarrowedLog(at(c.from), who(c.from), fileNameOf(c.piece.path)));
  });
  result.uncarried.forEach(({ from, reason }) => {
    log.push(STRINGS.MAIN.splitUncarriedLog(at(from), who(from), from.timeline_string || '',
      STRINGS.MAIN.SPLIT_UNCARRIED_REASON[reason], workText(from)));
  });
  return {
    toast: { text: STRINGS.MAIN.splitCarryToast(name, kept, total), type: kept === total ? 'success' : 'warning' },
    log,
  };
}
