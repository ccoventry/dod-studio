// release_notes_view.js — reads a release body (the Markdown
// .github/scripts/release_notes.py writes) into plain blocks for the What's
// new window and the update window (#1013). Pure: whats_new.js draws them.
//
// The body is: a line for GitHub's page ("See assets below to download."),
// the hand-written "What's new" summary, `## New` / `## Fixed` / `## Changed`
// bullets, a <details> list of every PR title, and a "Full diff" link. The
// app shows the summary and the grouped notes; the PR titles stay folded
// under "everything else", and the GitHub-only lines are left out.

/**
 * @typedef {{ type: 'heading', level: number, text: string }
 *         | { type: 'list', items: string[] }
 *         | { type: 'para', text: string }} Block
 * @returns {{ blocks: Block[], more: { summary: string, items: string[] } | null }}
 */
export function parseReleaseNotes(markdown) {
  let text = String(markdown || '').replace(/\r\n?/g, '\n').replace(/<!--[\s\S]*?-->/g, '');

  let more = null;
  const details = text.match(/<details>\s*<summary>([\s\S]*?)<\/summary>([\s\S]*?)<\/details>/i);
  if (details) {
    const items = parseBlocks(details[2]).flatMap((b) => (b.type === 'list' ? b.items : []));
    more = { summary: details[1].trim(), items };
    text = text.slice(0, details.index) + text.slice(details.index + details[0].length);
  }

  let blocks = parseBlocks(text).filter((b) => !(b.type === 'para' && /^\*\*Full diff:?\*\*/i.test(b.text)));
  // Everything before the first heading is the line for GitHub's page.
  const firstHeading = blocks.findIndex((b) => b.type === 'heading');
  blocks = firstHeading === -1 ? [] : blocks.slice(firstHeading);
  if (more && more.items.length === 0) more = null;
  return { blocks, more };
}

function parseBlocks(text) {
  const blocks = [];
  let para = null;
  let list = null;
  const endPara = () => { para = null; };
  const endList = () => { list = null; };
  for (const raw of text.split('\n')) {
    const line = raw.trimEnd();
    if (!line.trim()) { endPara(); endList(); continue; }
    const heading = line.match(/^(#{1,6})\s+(.*)$/);
    if (heading) {
      endPara(); endList();
      blocks.push({ type: 'heading', level: heading[1].length, text: heading[2].trim() });
      continue;
    }
    const item = line.match(/^\s*[-*]\s+(.*)$/);
    if (item) {
      endPara();
      if (!list) { list = { type: 'list', items: [] }; blocks.push(list); }
      list.items.push(item[1].trim());
      continue;
    }
    if (list && /^\s+\S/.test(line)) {
      // An indented line carries on the bullet above it.
      list.items[list.items.length - 1] += ` ${line.trim()}`;
      continue;
    }
    endList();
    if (!para) { para = { type: 'para', text: line.trim() }; blocks.push(para); } else { para.text += ` ${line.trim()}`; }
  }
  return blocks;
}

/**
 * A line's inline marks as spans: `code`, **bold**, *italic*. A link shows
 * as its text only: the window is for reading, and a click inside the app
 * would navigate Studio's own page away.
 * @returns {{ type: 'text'|'bold'|'italic'|'code', text: string }[]}
 */
export function inlineSpans(line) {
  const text = String(line || '').replace(/\[([^\]]+)\]\([^)]*\)/g, '$1');
  const spans = [];
  const re = /`([^`]+)`|\*\*([^*]+)\*\*|\*([^*\s][^*]*)\*/g;
  let at = 0;
  let m;
  while ((m = re.exec(text))) {
    if (m.index > at) spans.push({ type: 'text', text: text.slice(at, m.index) });
    if (m[1] !== undefined) spans.push({ type: 'code', text: m[1] });
    else if (m[2] !== undefined) spans.push({ type: 'bold', text: m[2] });
    else spans.push({ type: 'italic', text: m[3] });
    at = m.index + m[0].length;
  }
  if (at < text.length) spans.push({ type: 'text', text: text.slice(at) });
  return spans;
}

/** `v0.2.0` → `0.2.0`; the experimental build's fixed tag reads its version
 *  from the name ("DoD Studio 0.2.1-10 (experimental)"). */
export function releaseVersion(release) {
  const tag = String(release?.tag || '');
  if (/^v\d/.test(tag)) return tag.slice(1);
  const fromName = String(release?.name || '').match(/(\d+\.\d+\.\d+(?:-[0-9A-Za-z.]+)?)/);
  return fromName ? fromName[1] : tag;
}

/** The releases a channel shows: stable ones always, the experimental build
 *  too on the experimental channel, newest first as GitHub lists them. */
export function releasesForChannel(releases, channel) {
  return (releases || []).filter((r) => !r.prerelease || channel === 'experimental');
}

/** Whether to say "Updated to …" on this start: only when an earlier start
 *  saw another version. A first start, or one of a build that never saved
 *  a version, has nothing to compare with. */
export function updatedSince(lastSeen, current) {
  return Boolean(lastSeen && current && lastSeen !== current);
}

/** A bundled notes file, or null when the build has none: a local or test
 *  build has no file, and a dev server answers a missing one with its
 *  index page, which is not notes. */
export function bundledNotesOrNull(text) {
  const body = String(text || '').trim();
  if (!body || /^<!doctype|^<html/i.test(body)) return null;
  return body;
}
