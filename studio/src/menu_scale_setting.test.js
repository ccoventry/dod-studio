import { describe, it, expect } from 'vitest';
import { menuScaleView } from './menu_scale_setting.js';
import { STRINGS } from './strings.js';

const status = (over) => ({ applies: true, on: false, foreign: false, path: 'X:\\Games\\HL\\dod_addon\\resource\\TrackerScheme.res', ...over });

describe('menuScaleView (#985)', () => {
  it('is hidden with no install, and on a pre-Anniversary one', () => {
    expect(menuScaleView(null).visible).toBe(false);
    expect(menuScaleView(status({ applies: false })).visible).toBe(false);
  });

  it('shows what is on disk on an Anniversary install', () => {
    expect(menuScaleView(status())).toEqual({
      visible: true, checked: false, disabled: false, title: STRINGS.CAPTURE_CONFIG.MENU_SCALE_TITLE,
    });
    expect(menuScaleView(status({ on: true })).checked).toBe(true);
  });

  it('stays visible while our copy is on, so it can be taken out', () => {
    const view = menuScaleView(status({ applies: false, on: true }));
    expect(view.visible).toBe(true);
    expect(view.checked).toBe(true);
    expect(view.disabled).toBe(false);
  });

  it('greys out over a file that is not ours and says why', () => {
    const view = menuScaleView(status({ foreign: true }));
    expect(view.disabled).toBe(true);
    expect(view.checked).toBe(false);
    expect(view.title).toContain('TrackerScheme.res');
  });

  it('tells the user it needs -addons', () => {
    expect(STRINGS.CAPTURE_CONFIG.MENU_SCALE_TITLE).toContain('-addons');
  });
});
