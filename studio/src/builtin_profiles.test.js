import { describe, it, expect } from 'vitest';
import {
  mergeProfiles, savedProfiles, isBuiltinName, duplicateName, createBuiltinProfileNotice,
} from './builtin_profiles.js';

const CLEAN = { name: 'Clean clips', init_commands: ['r_decals 256', 'ds_spec_match_pov 1'], custom_commands: [] };
const OTHER = { name: 'B built-in', init_commands: ['hud_draw 0'], custom_commands: [] };
const FOTW = { name: 'FOTW', init_commands: ['mirv_fov 100'], custom_commands: [] };

describe('built-in command profiles', () => {
  it('lists built-ins first in shipped order, marked, then the user\'s by name', () => {
    const merged = mergeProfiles([CLEAN, OTHER], [FOTW, { name: 'Aim', init_commands: [] }]);
    expect(merged.map((p) => p.name)).toEqual(['Clean clips', 'B built-in', 'Aim', 'FOTW']);
    expect(merged.map((p) => !!p.builtin)).toEqual([true, true, false, false]);
  });

  it('drops a saved profile that has a built-in\'s name', () => {
    const merged = mergeProfiles([CLEAN], [{ ...FOTW, name: ' clean CLIPS ' }, FOTW]);
    expect(merged.map((p) => p.name)).toEqual(['Clean clips', 'FOTW']);
    expect(merged[0].init_commands).toEqual(CLEAN.init_commands);
  });

  it('never saves a built-in, and saves the rest without the mark', () => {
    const merged = mergeProfiles([CLEAN], [FOTW]);
    expect(savedProfiles(merged)).toEqual([FOTW]);
    expect(savedProfiles(undefined)).toEqual([]);
  });

  it('knows a built-in name whatever its case', () => {
    const merged = mergeProfiles([CLEAN], [FOTW]);
    expect(isBuiltinName(merged, ' CLEAN clips')).toBe(true);
    expect(isBuiltinName(merged, 'FOTW')).toBe(false);
    expect(isBuiltinName(merged, '')).toBe(false);
  });

  it('names a duplicate "<name> copy", then "copy 2", "copy 3"', () => {
    let list = mergeProfiles([CLEAN], []);
    expect(duplicateName(list, 'Clean clips')).toBe('Clean clips copy');
    list = mergeProfiles([CLEAN], [{ name: 'clean clips COPY', init_commands: [] }]);
    expect(duplicateName(list, 'Clean clips')).toBe('Clean clips copy 2');
    list = mergeProfiles([CLEAN], [
      { name: 'Clean clips copy', init_commands: [] },
      { name: 'Clean clips copy 2', init_commands: [] },
    ]);
    expect(duplicateName(list, 'Clean clips')).toBe('Clean clips copy 3');
    expect(duplicateName(list, 'FOTW')).toBe('FOTW copy');
  });
});

describe('the kept-copy notice', () => {
  function fakeNotice() {
    const text = { textContent: '' };
    let onOk = null;
    const ok = { addEventListener: (type, fn) => { if (type === 'click') onOk = fn; } };
    const el = {
      hidden: true,
      querySelector: (sel) => (sel === '[data-notice-text]' ? text : sel === '[data-notice-ok]' ? ok : null),
    };
    return { el, text, click: () => onOk() };
  }

  it('shows the kept names until OK, then saves once with none left', () => {
    const fake = fakeNotice();
    let saves = 0;
    const notice = createBuiltinProfileNotice({
      el: fake.el,
      text: (names) => `kept as ${names.join(', ')}`,
      onDismiss: () => { saves += 1; },
    });
    notice.set(['Clean clips (mine)']);
    expect(fake.el.hidden).toBe(false);
    expect(fake.text.textContent).toBe('kept as Clean clips (mine)');
    expect(notice.names()).toEqual(['Clean clips (mine)']);

    fake.click();
    expect(fake.el.hidden).toBe(true);
    expect(notice.names()).toEqual([]);
    expect(saves).toBe(1);
  });

  it('stays hidden with nothing to say', () => {
    const fake = fakeNotice();
    const notice = createBuiltinProfileNotice({ el: fake.el, text: () => 'x' });
    notice.set(undefined);
    expect(fake.el.hidden).toBe(true);
    notice.set(['', 3]);
    expect(fake.el.hidden).toBe(true);
    expect(notice.names()).toEqual([]);
  });
});
