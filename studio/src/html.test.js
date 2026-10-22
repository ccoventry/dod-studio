import { describe, it, expect } from 'vitest';
import { escapeHtml } from './html.js';

describe('escapeHtml', () => {
  it('escapes the characters that break markup and attributes', () => {
    expect(escapeHtml('<b a="1">&</b>')).toBe('&lt;b a=&quot;1&quot;&gt;&amp;&lt;/b&gt;');
  });

  it('escapes & first, so an entity is not double-read', () => {
    expect(escapeHtml('&lt;')).toBe('&amp;lt;');
  });

  it('turns null and undefined into an empty string, and numbers into text', () => {
    expect(escapeHtml(null)).toBe('');
    expect(escapeHtml(undefined)).toBe('');
    expect(escapeHtml(42)).toBe('42');
  });
});
