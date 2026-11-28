import { describe, it, expect, vi } from 'vitest';

vi.mock('./ipc_bridge.js', () => ({ checkDemoMaps: vi.fn(), logFrontendEvent: vi.fn() }));

import { checkDemoMaps, logFrontendEvent } from './ipc_bridge.js';
import { STRINGS } from './strings.js';
import { mapBuildProblem, mapBuildRow, mapBuildNotice, mapBuildDetail, findMapBuildProblem, renderMapBuildBanner } from './analyzer_map_build.js';

const wrong = {
  state: 'wrongBuild', mapName: 'dod_chemille', demoName: 'a.dem', expectedChecksum: 0xbb690c32, installedChecksum: 0xe4d5f058,
};

describe('the Demo Analyzer map build notice (#1539)', () => {
  it('picks the wrong-build row and nothing else', () => {
    expect(mapBuildProblem([{ state: 'ok' }, wrong])).toBe(wrong);
    expect(mapBuildProblem([{ state: 'missing' }, { state: 'unverifiable' }])).toBeNull();
    expect(mapBuildProblem(undefined)).toBeNull();
  });

  it('says it in plain words, with no checksums, and keeps the builds for the tooltip', () => {
    const notice = mapBuildNotice(wrong);
    expect(notice).toContain('another version of dod_chemille');
    expect(notice).not.toMatch(/bb690c32|e4d5f058|build|[`—]/);
    expect(notice).not.toContain('older release');
    expect(mapBuildDetail(wrong)).toBe('Map builds: demo bb690c32, installed e4d5f058');
    expect(mapBuildNotice(null)).toBeNull();
  });

  it('adds the older-release line only when the server sent a client.dll hash', () => {
    expect(mapBuildNotice({ ...wrong, olderRelease: true })).toContain('It comes from an older release of Day of Defeat.');
    expect(mapBuildNotice({ ...wrong, olderRelease: true })).not.toMatch(/1\.\d/);
  });

  it('asks for nothing without a game path, and passes the demo and game path through', async () => {
    checkDemoMaps.mockResolvedValue([wrong]);
    expect(await findMapBuildProblem('a.dem', '')).toBeNull();
    expect(checkDemoMaps).not.toHaveBeenCalled();
    expect(await findMapBuildProblem('a.dem', 'C:/g/hl.exe')).toBe(wrong);
    expect(checkDemoMaps).toHaveBeenCalledWith(['a.dem'], 'C:/g/hl.exe');
    expect(logFrontendEvent).toHaveBeenCalledTimes(1);
  });

  it('shows the banner for a problem and hides it otherwise', () => {
    const el = { hidden: true, textContent: '' };
    renderMapBuildBanner(el, wrong);
    expect(el.hidden).toBe(false);
    expect(el.textContent).toContain('dod_chemille');
    expect(el.title).toContain('bb690c32');
    renderMapBuildBanner(el, null);
    expect(el.hidden).toBe(true);
    expect(el.textContent).toBe('');
  });
});

describe('the Map checksum row (#1539)', () => {
  it('shows the header value as before, and an HLTV demo\'s server info value with its source', () => {
    const value = STRINGS.ANALYZER.mapChecksumValue;
    expect(value({ map_checksum: 2149185311, server_info_map_checksum: null })).toBe('2149185311');
    expect(value({ map_checksum: 0 })).toBe('0');
    expect(value({ map_checksum: 0, server_info_map_checksum: 0xbb690c32 }))
      .toBe('3144223794 (bb690c32), from the server info');
  });
});

describe('the Demo Analyzer notice for a demo played from the map builds (#1547)', () => {
  const fromLibrary = { state: 'fromLibrary', mapName: 'dod_chemille', demoName: 'a.dem', expectedChecksum: 0xbb690c32 };

  it('takes a different build first, then a library build, else nothing', () => {
    expect(mapBuildRow([fromLibrary, wrong])).toBe(wrong);
    expect(mapBuildRow([{ state: 'ok' }, fromLibrary])).toBe(fromLibrary);
    expect(mapBuildRow([{ state: 'ok' }])).toBeNull();
    expect(mapBuildRow(undefined)).toBeNull();
  });

  it('names the build it plays on, as news rather than a warning', () => {
    expect(mapBuildNotice(fromLibrary)).toBe(STRINGS.ANALYZER.fromLibraryMapBuildNotice('dod_chemille', 'bb690c32'));
    expect(mapBuildDetail(fromLibrary)).toBe('');
    const classes = new Set(['analyzer-warning-banner']);
    const el = { hidden: true, textContent: '', classList: { toggle: (name, on) => (on ? classes.add(name) : classes.delete(name)) } };
    renderMapBuildBanner(el, fromLibrary);
    expect(el.hidden).toBe(false);
    expect([...classes]).toEqual(['analyzer-info-banner']);
    renderMapBuildBanner(el, wrong);
    expect([...classes]).toEqual(['analyzer-warning-banner']);
  });

  it('is what findMapBuildProblem resolves with when the map builds hold the build', async () => {
    checkDemoMaps.mockResolvedValue([fromLibrary]);
    expect(await findMapBuildProblem('a.dem', 'C:/g/hl.exe')).toBe(fromLibrary);
  });
});
