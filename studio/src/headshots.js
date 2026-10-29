// headshots.js
// What a highlight's kills say about headshots (#1128). The scan keeps one
// verdict per kill in `streak.headshots` ('yes', 'no' or 'unknown', beside
// `kills`), told from the helmet-pop event in the demo
// (analysis/src/headshot.rs): 'unknown' is a double kill on one frame the
// demo can't settle, a British victim, whose helmet may not pop, or any kill
// with no pop in an HLTV demo, which loses about half of them. Pure, no DOM.

/** The Kill Range's kills only: `start_index..=end_index`, as every other
 *  per-kill field is read. `null` when the highlight carries no verdicts
 *  (a project saved before #1128, until the demo is scanned again). */
export function headshotCounts(streak) {
  const verdicts = streak?.headshots;
  if (!Array.isArray(verdicts) || verdicts.length === 0) return null;
  const start = Math.min(streak.start_index ?? 0, verdicts.length - 1);
  const end = Math.max(start, Math.min(streak.end_index ?? verdicts.length - 1, verdicts.length - 1));
  const range = verdicts.slice(start, end + 1);
  const count = (wanted) => range.filter((v) => v === wanted).length;
  return { yes: count('yes'), no: count('no'), unknown: count('unknown'), kills: range.length };
}

/** Whether the highlight has a headshot among its kills: the Headshots
 *  filter. An unknown kill is not counted. */
export function hasHeadshot(streak) {
  return (headshotCounts(streak)?.yes ?? 0) > 0;
}

/** `{headshots}`: the headshots in the Kill Range, or `null` (no value) when
 *  the highlight has no verdicts or every kill is unknown. */
export function headshotsPlaceholder(streak) {
  const counts = headshotCounts(streak);
  if (!counts || counts.unknown === counts.kills) return null;
  return String(counts.yes);
}

/** The Highlight Details cell: the count, with `+?` when some kills can't be
 *  told, `?` when none can, and a dash when the highlight has no verdicts. */
export function headshotsCell(streak) {
  const counts = headshotCounts(streak);
  if (!counts) return '–';
  if (counts.unknown === counts.kills) return '?';
  return counts.unknown > 0 ? `${counts.yes}+?` : String(counts.yes);
}
