import { describe, it, expect } from 'vitest';
import {
  parseReleaseNotes, inlineSpans, releaseVersion, releasesForChannel, updatedSince, bundledNotesOrNull,
} from './release_notes_view.js';

// The shape release_notes.py writes (v0.2.0's body, cut down).
const BODY = [
  'See assets below to download.',
  '',
  "## What's new",
  '',
  '### Capture',
  '- **Seek Between Clips:** a batch now plays your original demos',
  '  and jumps between clips.',
  '- Untick *Keep the Game Behind* to watch it.',
  '',
  '## Fixed',
  '',
  '- `ds_hud_deathmsg` keeps its size. (#710)',
  '',
  '<details><summary>All 3 pull requests</summary>',
  '',
  '- CI: cache from dev only (#346)',
  '- Docs: tidy (#649)',
  '',
  '</details>',
  '',
  '**Full diff:** https://github.com/ccoventry/dod-studio/compare/v0.1.4...4fa6e97',
].join('\r\n');

describe('parseReleaseNotes', () => {
  const { blocks, more } = parseReleaseNotes(BODY);

  it("drops the line for GitHub's page and the Full diff link", () => {
    expect(blocks[0]).toEqual({ type: 'heading', level: 2, text: "What's new" });
    expect(JSON.stringify(blocks)).not.toContain('See assets');
    expect(JSON.stringify(blocks)).not.toContain('Full diff');
  });

  it('keeps headings and bullets, joining a bullet carried onto the next line', () => {
    expect(blocks.map((b) => b.type)).toEqual(['heading', 'heading', 'list', 'heading', 'list']);
    expect(blocks[2].items).toEqual([
      '**Seek Between Clips:** a batch now plays your original demos and jumps between clips.',
      'Untick *Keep the Game Behind* to watch it.',
    ]);
  });

  it('folds the PR titles into "everything else"', () => {
    expect(more).toEqual({ summary: 'All 3 pull requests', items: ['CI: cache from dev only (#346)', 'Docs: tidy (#649)'] });
    expect(JSON.stringify(blocks)).not.toContain('#346');
  });

  it('a body with no notes (v0.1.x) reads as empty', () => {
    expect(parseReleaseNotes('See assets below to download.')).toEqual({ blocks: [], more: null });
    expect(parseReleaseNotes(null)).toEqual({ blocks: [], more: null });
  });

  it('leaves out HTML comments', () => {
    const { blocks: b } = parseReleaseNotes('## New\n<!-- note -->\n- A (#1)');
    expect(b).toEqual([{ type: 'heading', level: 2, text: 'New' }, { type: 'list', items: ['A (#1)'] }]);
  });
});

describe('inlineSpans', () => {
  it('reads code, bold and italic, and shows a link as its text', () => {
    expect(inlineSpans('Use `ds_x 1` for **this**, *not* [that](https://x.y).')).toEqual([
      { type: 'text', text: 'Use ' },
      { type: 'code', text: 'ds_x 1' },
      { type: 'text', text: ' for ' },
      { type: 'bold', text: 'this' },
      { type: 'text', text: ', ' },
      { type: 'italic', text: 'not' },
      { type: 'text', text: ' that.' },
    ]);
  });

  it('a lone asterisk stays text', () => {
    expect(inlineSpans('2 * 3')).toEqual([{ type: 'text', text: '2 * 3' }]);
  });
});

describe('releases', () => {
  const releases = [
    { tag: 'experimental-latest', name: 'DoD Studio 0.2.1-10 (experimental)', prerelease: true },
    { tag: 'v0.2.0', name: 'DoD Studio v0.2.0', prerelease: false },
  ];

  it('reads the version from the tag, or from the experimental build name', () => {
    expect(releases.map(releaseVersion)).toEqual(['0.2.1-10', '0.2.0']);
  });

  it('shows the experimental build only on the experimental channel', () => {
    expect(releasesForChannel(releases, 'stable').map((r) => r.tag)).toEqual(['v0.2.0']);
    expect(releasesForChannel(releases, 'experimental')).toHaveLength(2);
  });
});

describe('updatedSince', () => {
  it('only when an earlier start saw another version', () => {
    expect(updatedSince('0.2.0', '0.2.1')).toBe(true);
    expect(updatedSince('0.2.1', '0.2.1')).toBe(false);
    expect(updatedSince(null, '0.2.1')).toBe(false);
    expect(updatedSince('0.2.0', null)).toBe(false);
  });
});

describe('bundledNotesOrNull', () => {
  it("takes notes, not a dev server's index page or an empty file", () => {
    expect(bundledNotesOrNull('## New\n- A')).toBe('## New\n- A');
    expect(bundledNotesOrNull('<!doctype html><html>')).toBeNull();
    expect(bundledNotesOrNull('  ')).toBeNull();
  });
});
