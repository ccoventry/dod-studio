import { describe, it, expect, beforeEach } from 'vitest';
import { readFileSync } from 'node:fs';
import { REVIEW_DEFAULTS, reviewSettingsFrom } from './review_settings.js';
import {
  loadCaptureMode, captureModeToSave, captureModeUndecided, captureModeChosen,
  settledCaptureMode, settleCaptureMode,
} from './capture_mode_default.js';

// The form's initial values are what a new install sees before the settings
// load, and must say the same as the backend's defaults.
const html = readFileSync(new URL('../index.html', import.meta.url), 'utf8');
const rust = readFileSync(new URL('../src-tauri/src/settings_manager.rs', import.meta.url), 'utf8');

/** The `<input ...>` tag with this id. */
function inputTag(id) {
  const m = html.match(new RegExp(`<input[^>]*id="${id}"[^>]*>`));
  if (!m) throw new Error(`no input #${id} in index.html`);
  return m[0];
}
const valueOf = (id) => inputTag(id).match(/value="([^"]*)"/)?.[1];
const checked = (id) => /\schecked(\s|\/|>)/.test(inputTag(id));
/** The body of a `fn name() -> T { body }` in settings_manager.rs. */
function rustDefault(name) {
  const m = rust.match(new RegExp(`fn ${name}\\(\\) -> [^{]+\\{\\s*([^}]*?)\\s*\\}`));
  if (!m) throw new Error(`no fn ${name} in settings_manager.rs`);
  return m[1];
}

describe('fresh-install defaults', () => {
  it('Capture FPS 120, Start Lead and Stop Trail 5 s', () => {
    expect(valueOf('config-capture-fps')).toBe('120');
    expect(rustDefault('default_capture_fps')).toBe('120');
    expect(valueOf('config-record-start-lead')).toBe('5.0');
    expect(valueOf('config-record-stop-trail')).toBe('5.0');
    expect(rustDefault('default_record_start_lead')).toBe('5.0');
    expect(rustDefault('default_record_stop_trail')).toBe('5.0');
  });

  it('Auto-clear Temp Demos on', () => {
    expect(checked('config-auto-clear-temp-demos')).toBe(true);
    expect(rustDefault('default_auto_clear_temp_demos')).toBe('true');
  });

  it('the per-demo and per-clip toasts off, the rest on', () => {
    for (const [id, fn] of [['patching', 'default_notify_patching'],
      ['demo-loading', 'default_notify_demo_loading'], ['between-clips', 'default_notify_between_clips']]) {
      expect(checked(`config-notify-${id}`)).toBe(false);
      expect(rustDefault(fn)).toBe('false');
    }
    for (const [id, fn] of [['captures-done', 'default_notify_captures_done'],
      ['renders-done', 'default_notify_renders_done'], ['error', 'default_notify_error'],
      ['updates', 'default_notify_updates']]) {
      expect(checked(`config-notify-${id}`)).toBe(true);
      expect(rustDefault(fn)).toBe('true');
    }
  });

  it('the FF Speed box is gone from the Timing tab', () => {
    expect(html).not.toContain('config-fast-forward-speed');
  });

  it('the Review controls start at the saved defaults', () => {
    expect(checked('review-tick-yes-cb')).toBe(REVIEW_DEFAULTS.review_tick_yes);
    expect(checked('review-fast-forward-cb')).toBe(REVIEW_DEFAULTS.review_fast_forward);
    expect(Number(valueOf('review-fast-forward-gap'))).toBe(REVIEW_DEFAULTS.review_fast_forward_gap);
    expect(rustDefault('default_review_fast_forward_gap')).toBe(String(REVIEW_DEFAULTS.review_fast_forward_gap));
    expect(rustDefault('default_review_covers')).toBe(`"${REVIEW_DEFAULTS.review_covers}".to_string()`);
    expect(rustDefault('default_review_order')).toBe(`"${REVIEW_DEFAULTS.review_order}".to_string()`);
    // The first option of each select is the default.
    expect(html).toMatch(/id="review-covers-select"[^>]*>\s*<option value="all"/);
    expect(html).toMatch(/id="review-order-select"[^>]*>\s*<option value="time"/);
  });
});

describe('reviewSettingsFrom', () => {
  it('keeps saved values and fills missing ones', () => {
    expect(reviewSettingsFrom({})).toEqual(REVIEW_DEFAULTS);
    expect(reviewSettingsFrom(null)).toEqual(REVIEW_DEFAULTS);
    expect(reviewSettingsFrom({ review_tick_yes: false, review_fast_forward_gap: 20, review_order: 'player' }))
      .toEqual({ ...REVIEW_DEFAULTS, review_tick_yes: false, review_fast_forward_gap: 20, review_order: 'player' });
    expect(reviewSettingsFrom({ review_tick_yes: 'no' }).review_tick_yes).toBe(true);
  });
});

describe('a new install\'s capture mode', () => {
  const select = () => ({ value: 'frame_sequence' });
  const found = { usable: true, state: { state: 'bundled' } };
  const missing = { usable: false, state: { state: 'missing' } };
  const noInstall = { usable: false, state: { state: 'no_install' } };

  beforeEach(() => { loadCaptureMode({ capture_mode: 'obs' }); });

  it('a saved mode stands, and is saved as it is', () => {
    expect(loadCaptureMode({ capture_mode: 'frame_sequence' })).toBe('frame_sequence');
    expect(captureModeUndecided()).toBe(false);
    expect(captureModeToSave('frame_sequence')).toBe('frame_sequence');
    const s = select();
    expect(settleCaptureMode(found, s)).toBe(null);
    expect(s.value).toBe('frame_sequence');
  });

  it('an older file with only ffmpeg_capture keeps Video', () => {
    expect(loadCaptureMode({ ffmpeg_capture: true })).toBe('direct_to_video');
    expect(captureModeUndecided()).toBe(false);
  });

  it('shows Frame sequence and saves nothing until HLAE\'s FFmpeg is found', () => {
    expect(loadCaptureMode({ capture_mode: '' })).toBe('frame_sequence');
    expect(captureModeUndecided()).toBe(true);
    expect(captureModeToSave('frame_sequence')).toBe('');
    const s = select();
    expect(settleCaptureMode(noInstall, s)).toBe(null);
    expect(settleCaptureMode(missing, s)).toBe(null);
    expect(settleCaptureMode({ ...found, missing_hook_dll: 'C:/x/AfxHookGoldSrc.dll' }, s)).toBe(null);
    expect(s.value).toBe('frame_sequence');
    expect(captureModeUndecided()).toBe(true);

    expect(settleCaptureMode(found, s)).toBe('direct_to_video');
    expect(s.value).toBe('direct_to_video');
    expect(captureModeUndecided()).toBe(false);
    expect(captureModeToSave('direct_to_video')).toBe('direct_to_video');
  });

  it('a mode picked by hand is never changed', () => {
    loadCaptureMode({});
    captureModeChosen();
    expect(captureModeToSave('frame_sequence')).toBe('frame_sequence');
    const s = select();
    expect(settleCaptureMode(found, s)).toBe(null);
    expect(s.value).toBe('frame_sequence');
  });

  it('settledCaptureMode only answers Video for a usable FFmpeg', () => {
    expect(settledCaptureMode(found)).toBe('direct_to_video');
    expect(settledCaptureMode(missing)).toBe(null);
    expect(settledCaptureMode(null)).toBe(null);
  });
});
