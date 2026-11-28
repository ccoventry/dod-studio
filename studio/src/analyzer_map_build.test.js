import { describe, it, expect, vi } from 'vitest';

vi.mock('./ipc_bridge.js', () => ({ checkDemoMaps: vi.fn() }));

import { checkDemoMaps } from './ipc_bridge.js';
import { STRINGS } from './strings.js';
import { mapBuildProblem, mapBuildNotice, findMapBuildProblem, renderMapBuildBanner } from './analyzer_map_build.js';

const wrong = { state: 'wrongBuild', detail: '`dod_chemille` is a different build, the demo wants `bb690c32`, this one is `e4d5f058`' };

describe('the Demo Analyzer map build notice (#1539)', () => {
  it('picks the wrong-build row and nothing else', () => {
    expect(mapBuildProblem([{ state: 'ok' }, wrong])).toBe(wrong);
    expect(mapBuildProblem([{ state: 'missing' }, { state: 'unverifiable' }])).toBeNull();
    expect(mapBuildProblem(undefined)).toBeNull();
  });

  it('says which builds differ', () => {
    expect(mapBuildNotice(wrong)).toContain('bb690c32');
    expect(mapBuildNotice(wrong)).toContain('e4d5f058');
    expect(mapBuildNotice(null)).toBeNull();
  });

  it('asks for nothing without a game path, and passes the demo and game path through', async () => {
    checkDemoMaps.mockResolvedValue([wrong]);
    expect(await findMapBuildProblem('a.dem', '')).toBeNull();
    expect(checkDemoMaps).not.toHaveBeenCalled();
    expect(await findMapBuildProblem('a.dem', 'C:/g/hl.exe')).toBe(wrong);
    expect(checkDemoMaps).toHaveBeenCalledWith(['a.dem'], 'C:/g/hl.exe');
  });

  it('shows the banner for a problem and hides it otherwise', () => {
    const el = { hidden: true, textContent: '' };
    renderMapBuildBanner(el, wrong);
    expect(el.hidden).toBe(false);
    expect(el.textContent).toContain('bb690c32');
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
