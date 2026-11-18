// Start's checks read the capture mode, size and OBS rate the batch will use
// (#1085): the open project's over Configuration's.

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

vi.mock('./ipc_bridge.js', () => ({
  logFrontendEvent: vi.fn(),
  keepProjectSettingsSnapshot: vi.fn(),
  getAppVersion: vi.fn(async () => '0.12.0'),
}));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

const { captureModeInEffect, obsSizeAndRateInEffect } = await import('./capture_in_effect.js');
const { createProjectSettings } = await import('./project_settings.js');

/** Configuration's fields, as Start's checks find them. */
const FIELDS = {
  '#config-capture-mode': 'obs',
  '#config-res-width': '1920',
  '#config-res-height': '1080',
  '#config-obs-capture-fps': '60',
};

let session;
beforeEach(() => {
  globalThis.document = { querySelector: (selector) => (selector in FIELDS ? { value: FIELDS[selector] } : null) };
  session = createProjectSettings({ markDirty: () => {} });
});
afterEach(() => {
  session.clear();
  delete globalThis.document;
});

describe('with no project open', () => {
  it("reads Configuration's fields", () => {
    expect(captureModeInEffect()).toBe('obs');
    expect(obsSizeAndRateInEffect()).toEqual({ gameWidth: 1920, gameHeight: 1080, obsCaptureFps: 60 });
  });
});

describe('with a project whose settings differ from Configuration', () => {
  it("reads the project's mode, so Start's OBS pre-flight follows the batch", () => {
    session.setBlock({ picture: { captureMode: 'frame_sequence' } });
    expect(captureModeInEffect()).toBe('frame_sequence');
  });

  it("sets OBS up at the project's size and rate", () => {
    session.setBlock({ picture: { width: 2560, height: 1440, obsCaptureFps: 120 } });
    expect(obsSizeAndRateInEffect()).toEqual({ gameWidth: 2560, gameHeight: 1440, obsCaptureFps: 120 });
  });
});
