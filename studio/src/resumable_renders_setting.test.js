import { describe, it, expect, vi, afterEach } from 'vitest';
import { readFileSync } from 'node:fs';
import {
  DEFAULT_RESUMABLE_RENDERS, resumableFrom, readResumableRenders, applyResumableRenders, startResumableRenders,
} from './resumable_renders_setting.js';
import { STRINGS } from './strings.js';

/** A stand-in checkbox that runs its change listener on `change()`. */
function stubBox(checked) {
  let onChange = null;
  const box = {
    checked,
    addEventListener: (type, fn) => { if (type === 'change') onChange = fn; },
    change() { onChange?.(); },
  };
  vi.stubGlobal('document', { querySelector: (sel) => (sel === '#render-resumable-checkbox' ? box : null) });
  return box;
}

describe('Resumable renders (#1451)', () => {
  afterEach(() => vi.unstubAllGlobals());

  it('is on by default, as the backend', () => {
    expect(DEFAULT_RESUMABLE_RENDERS).toBe(true);
    // A settings file from before the setting has no key.
    expect(resumableFrom(undefined)).toBe(true);
    expect(resumableFrom(null)).toBe(true);
    expect(resumableFrom(true)).toBe(true);
    expect(resumableFrom(false)).toBe(false);
  });

  it('the box starts ticked on the page, with its trade-offs in a line under it', () => {
    const html = readFileSync(new URL('../index.html', import.meta.url), 'utf8');
    expect(html).toMatch(/<input type="checkbox" id="render-resumable-checkbox" checked \/>/);
    expect(html).toContain('<p class="setting-hint" data-str="RENDER.RESUMABLE_HINT">');
    expect(STRINGS.RENDER.RESUMABLE_HINT).toContain('picks up where it left off');
    expect(STRINGS.RENDER.RESUMABLE_HINT).toContain('twice the clip');
    expect(STRINGS.RENDER.RESUMABLE_HINT).toContain('Only when space is needed');
  });

  it('shows the loaded value and saves what is ticked', () => {
    const box = stubBox(true);
    applyResumableRenders(false);
    expect(box.checked).toBe(false);
    box.checked = true;
    expect(readResumableRenders()).toBe(true);
    applyResumableRenders(undefined);
    expect(box.checked).toBe(true);
  });

  it('saves the settings when the box changes', () => {
    const box = stubBox(true);
    const persist = vi.fn();
    startResumableRenders({ persist });
    box.checked = false;
    box.change();
    expect(persist).toHaveBeenCalledTimes(1);
    expect(readResumableRenders()).toBe(false);
  });

  it('saves the last loaded value when the box is not on the page', () => {
    stubBox(true);
    applyResumableRenders(false);
    vi.stubGlobal('document', { querySelector: () => null });
    expect(readResumableRenders()).toBe(false);
  });
});
