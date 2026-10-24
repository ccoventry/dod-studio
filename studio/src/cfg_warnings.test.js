import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./ipc_bridge.js', () => ({ scanGameConfigs: vi.fn() }));

import { scanGameConfigs } from './ipc_bridge.js';
import { refreshCfgWarnings } from './cfg_warnings.js';
import { STRINGS } from './strings.js';

// No DOM in these tests: just enough of one for the banners to land in.
let elements;
beforeEach(() => {
  elements = {};
  globalThis.document = {
    querySelector: (sel) => (elements[sel] ??= { hidden: true, innerHTML: '', style: {} }),
  };
});

const initBanner = () => elements['#init-commands-warning-banner'].innerHTML;

describe('Initial Commands that exec a file', () => {
  it("names the exec'd file as where a conflicting value comes from", async () => {
    scanGameConfigs.mockResolvedValue({
      conflicts: [{
        cvar: 'hud_deathnotice_time',
        values: [
          { value: '6', kind: 'config', file: 'config.cfg', line: 3 },
          { value: '12', kind: 'exec', file: 'wsod.cfg', line: 1 },
          { value: '4', kind: 'initial' },
        ],
        effective: { value: '4', kind: 'initial' },
        scheduled: false,
      }],
    });
    await refreshCfgWarnings('C:/Games/Half-Life/hl.exe', ['exec wsod.cfg', 'hud_deathnotice_time 4']);

    expect(initBanner()).toContain(STRINGS.CFG.sourceExec('wsod.cfg', 1));
  });

  it("lists an exec whose file the game won't find", async () => {
    scanGameConfigs.mockResolvedValue({ missingExec: ['exec nope.cfg'] });
    await refreshCfgWarnings('C:/Games/Half-Life/hl.exe', ['exec nope.cfg']);

    expect(initBanner()).toContain(STRINGS.CFG.MISSING_EXEC_TITLE);
    expect(initBanner()).toContain('exec nope.cfg');
  });

  it('says which setting makes the demo filter apply when it does', async () => {
    scanGameConfigs.mockResolvedValue({
      filteredInit: [{ command: 'exec wsod.cfg', rule: 'lineContains', pattern: 'exec' }],
      filterApplies: true,
    });
    await refreshCfgWarnings('C:/Games/Half-Life/hl.exe', ['exec wsod.cfg']);

    expect(initBanner()).toContain(STRINGS.CFG.FILTERED_TITLE);
    expect(STRINGS.CFG.FILTERED_ADVICE).toContain('Seek Between Clips');
  });

  it('passes Seek Between Clips to the scan', async () => {
    scanGameConfigs.mockResolvedValue({});
    await refreshCfgWarnings('C:/Games/Half-Life/hl.exe', [], [], { hookRunsBatch: false });

    expect(scanGameConfigs).toHaveBeenLastCalledWith('C:/Games/Half-Life/hl.exe', [], [], { hookRunsBatch: false });
  });
});

describe('Launch Preview', () => {
  it("says a preview doesn't run Initial or Scheduled Commands", () => {
    expect(STRINGS.HIGHLIGHTS.LAUNCH_PREVIEW_TITLE).toContain("Initial and Scheduled Commands don't run");
    expect(STRINGS.CAPTURE_CONFIG.INIT_COMMANDS_INFO_TITLE).toContain("Launch Preview and Launch Game don't");
  });
});
