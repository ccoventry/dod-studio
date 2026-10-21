import { describe, it, expect } from 'vitest';
import {
  placeholderGroups, placeholderNames, placeholderDescription, CLIP_ALIASES, WITH_FALLBACK,
} from './placeholder_catalogue.js';
import { PLACEHOLDERS } from './clip_name.js';
import { DEMO_PLACEHOLDERS } from './demo_rename.js';

describe('the placeholder catalogue (#751)', () => {
  it('puts every placeholder of each template in exactly one row', () => {
    for (const kind of ['clip', 'demo']) {
      const names = placeholderNames(kind);
      expect(new Set(names).size).toBe(names.length);
    }
  });

  it('is where both templates get their placeholders', () => {
    expect(PLACEHOLDERS).toEqual(placeholderNames('clip'));
    expect(DEMO_PLACEHOLDERS).toEqual(placeholderNames('demo'));
  });

  it('lays the shared rows out the same, with Highlight only in the clip name', () => {
    const rows = (kind) => placeholderGroups(kind).map((g) => g.key);
    expect(rows('demo')).toEqual(['player', 'sides', 'teams', 'demo']);
    expect(rows('clip')).toEqual(['player', 'sides', 'teams', 'demo', 'highlight']);
    const row = (kind, key) => placeholderGroups(kind).find((g) => g.key === key).names;
    for (const key of ['sides', 'teams', 'demo']) expect(row('clip', key)).toEqual(row('demo', key));
    expect(row('clip', 'highlight')).toContain('kills');
    expect(row('demo', 'player')).toContain('kills');
  });

  it('gives every placeholder a description in each template, and {kills} two', () => {
    for (const kind of ['clip', 'demo']) {
      for (const name of placeholderNames(kind)) expect(placeholderDescription(kind, name), `${kind} {${name}}`).not.toBe('');
    }
    expect(placeholderDescription('clip', 'kills')).not.toBe(placeholderDescription('demo', 'kills'));
    expect(placeholderDescription('clip', 'kills')).toContain('Rename Demos');
    expect(placeholderDescription('demo', 'kills')).toContain('clip name');
  });

  it('keeps {player} as an old name for {full_name}, with no chip of its own', () => {
    expect(CLIP_ALIASES.player).toBe('full_name');
    expect(placeholderNames('clip')).not.toContain('player');
    expect(WITH_FALLBACK.has('full_name')).toBe(true);
  });
});
