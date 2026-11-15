import { describe, it, expect } from 'vitest';
import { flagTagCounts, flagTagChips } from './flag_tags.js';
import { STRINGS } from './strings.js';

const streak = (flag_tags, extra = {}) => ({ flag_tags, start_index: 0, end_index: flag_tags.length - 1, ...extra });

describe('flagTagCounts', () => {
  it('counts each tag over the whole highlight, in a fixed order', () => {
    const s = streak([['cleared_flag'], [], ['while_capping', 'cleared_flag']]);
    expect(flagTagCounts(s)).toEqual([['while_capping', 1], ['cleared_flag', 2]]);
  });

  it('counts only the Kill Range', () => {
    const s = streak([['while_capping'], [], ['cleared_flag']], { start_index: 1, end_index: 2 });
    expect(flagTagCounts(s)).toEqual([['cleared_flag', 1]]);
  });

  it('is empty for a highlight scanned before the tags were kept, or with none', () => {
    expect(flagTagCounts({ kills: [] })).toEqual([]);
    expect(flagTagCounts(streak([[], []]))).toEqual([]);
    expect(flagTagCounts(null)).toEqual([]);
  });
});

describe('flagTagChips', () => {
  it('draws one chip per tag, with a count past one and the reason as its tooltip', () => {
    const html = flagTagChips(streak([['cleared_flag'], ['cleared_flag', 'flag_defence']]));
    const chips = [...html.matchAll(/<span class="flag-tag-chip" data-tag="([a-z_]+)" title="([^"]*)">([^<]*)<\/span>/g)];
    expect(chips.map((c) => c[1])).toEqual(['flag_defence', 'cleared_flag']);
    expect(chips[0][3]).toBe(STRINGS.HIGHLIGHTS.FLAG_TAG_LABELS.flag_defence);
    expect(chips[1][3]).toBe(STRINGS.HIGHLIGHTS.flagTagCount(STRINGS.HIGHLIGHTS.FLAG_TAG_LABELS.cleared_flag, 2));
    expect(chips[1][2]).toBe(STRINGS.HIGHLIGHTS.FLAG_TAG_TITLES.cleared_flag);
  });

  it('draws nothing for a highlight with no tags', () => {
    expect(flagTagChips(streak([[]]))).toBe('');
  });
});
