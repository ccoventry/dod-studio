import { describe, it, expect } from 'vitest';
import {
  setupChecklist,
  shouldOpenSetupAtLaunch,
  stepIndex,
  driveLabel,
  formatBytes,
  defaultDemoFolder,
  engineBuild,
  isSteamPlayInstall,
  wrongExeName,
  EXE_NAMES,
  SETUP_STEPS,
} from './setup_checklist.js';

const complete = {
  hlPath: 'D:\\Games\\Half-Life - Movies\\hl.exe',
  hlState: 'ok',
  hlaePath: 'C:\\Programs\\HLAE\\HLAE.exe',
  hlaeState: 'ok',
  ffmpegUsable: true,
  targetDrives: [{ path: 'D:\\captures', freeBytes: 400 * 1024 ** 3 }],
  renderExportDirs: [{ path: 'E:\\clips', freeBytes: 1 }],
  demoCount: 3,
  pinnedFolders: [],
};

describe('setup checklist (#599)', () => {
  it('a fresh install has nothing done and opens Setup', () => {
    const c = setupChecklist({});
    expect(c.doneCount).toBe(0);
    expect(c.total).toBe(SETUP_STEPS.length);
    expect(c.requiredDone).toBe(0);
    expect(c.requiredTotal).toBe(3);
    expect(c.allRequiredDone).toBe(false);
    expect(c.nextIndex).toBe(0);
    expect(shouldOpenSetupAtLaunch(c, false)).toBe(true);
  });

  it('everything set counts every step done and keeps Setup closed', () => {
    const c = setupChecklist(complete);
    expect(c.doneCount).toBe(6);
    expect(c.allRequiredDone).toBe(true);
    expect(shouldOpenSetupAtLaunch(c, false)).toBe(false);
  });

  it('Skip setup keeps the window closed while something is missing', () => {
    const c = setupChecklist({ ...complete, hlaePath: '' });
    expect(c.allRequiredDone).toBe(false);
    expect(shouldOpenSetupAtLaunch(c, true)).toBe(false);
  });

  it('only Half-Life, HLAE and a capture folder are required', () => {
    const c = setupChecklist({ ...complete, ffmpegUsable: false, renderExportDirs: [], demoCount: 0 });
    expect(c.allRequiredDone).toBe(true);
    expect(c.requiredDone).toBe(3);
    expect(c.doneCount).toBe(3);
    expect(c.steps.filter((s) => s.required).map((s) => s.key)).toEqual(['hl', 'hlae', 'captures']);
  });

  it('a path the check says is missing is not done, whatever the box says', () => {
    const c = setupChecklist({ ...complete, hlState: 'not_found' });
    expect(c.steps.find((s) => s.key === 'hl').done).toBe(false);
    expect(c.nextIndex).toBe(0);
  });

  it('another program than hl.exe or HLAE.exe is not done (hlds.exe sits beside hl.exe)', () => {
    const c = setupChecklist({ ...complete, hlPath: 'D:\\Games\\Half-Life - Movies\\hlds.exe' });
    expect(c.steps.find((s) => s.key === 'hl').done).toBe(false);
    const hlae = setupChecklist({ ...complete, hlaePath: 'C:\\Programs\\HLAE\\AfxHookGoldSrc.exe' });
    expect(hlae.steps.find((s) => s.key === 'hlae').done).toBe(false);
    expect(wrongExeName('D:/x/HL.EXE', EXE_NAMES.hl)).toBe('');
    expect(wrongExeName('C:\\Programs\\HLAE\\hlae.exe', EXE_NAMES.hlae)).toBe('');
    expect(wrongExeName('D:\\x\\hlds.exe', EXE_NAMES.hl)).toBe('hlds.exe');
    expect(wrongExeName('', EXE_NAMES.hl)).toBe('');
  });

  it('a path not yet checked counts by its text alone', () => {
    const c = setupChecklist({ ...complete, hlState: undefined, hlaeState: undefined });
    expect(c.requiredDone).toBe(3);
  });

  it('capture folders on drives with no space do not count', () => {
    const c = setupChecklist({ ...complete, targetDrives: [{ path: 'Q:\\captures', freeBytes: 0 }] });
    expect(c.steps.find((s) => s.key === 'captures').done).toBe(false);
    const unmeasured = setupChecklist({ ...complete, targetDrives: [{ path: 'D:\\captures' }] });
    expect(unmeasured.steps.find((s) => s.key === 'captures').done).toBe(true);
  });

  it('the next step is the first required one missing, then the first optional one', () => {
    expect(setupChecklist({ ...complete, targetDrives: [], ffmpegUsable: false }).nextIndex).toBe(stepIndex('captures'));
    expect(setupChecklist({ ...complete, ffmpegUsable: false }).nextIndex).toBe(stepIndex('ffmpeg'));
    expect(setupChecklist(complete).nextIndex).toBe(0);
  });

  it('a remembered demo folder counts as demos added before a scan lands', () => {
    const c = setupChecklist({ ...complete, demoCount: 0, pinnedFolders: ['D:\\demos'] });
    expect(c.steps.find((s) => s.key === 'demos').done).toBe(true);
  });

  it('stepIndex falls back to the first step for an unknown key', () => {
    expect(stepIndex('hlae')).toBe(1);
    expect(stepIndex('nope')).toBe(0);
  });
});

describe('drive labels and free space', () => {
  it('names the drive of a Windows path and the share of a UNC path', () => {
    expect(driveLabel('d:\\DoD Studio\\captures')).toBe('D:');
    expect(driveLabel('\\\\nas\\media\\captures')).toBe('\\\\nas\\media');
    expect(driveLabel('//nas/media')).toBe('\\\\nas\\media');
    expect(driveLabel('captures')).toBe('');
    expect(driveLabel('')).toBe('');
  });

  it('formats bytes at the scale a drive is talked about in', () => {
    expect(formatBytes(1.6 * 1024 ** 4)).toBe('1.6 TB');
    expect(formatBytes(412 * 1024 ** 3)).toBe('412 GB');
    expect(formatBytes(9.5 * 1024 ** 3)).toBe('9.5 GB');
    expect(formatBytes(512 * 1024 ** 2)).toBe('512 MB');
    expect(formatBytes(0)).toBe('0 MB');
    expect(formatBytes(null)).toBe('');
  });
});

describe('Half-Life install helpers', () => {
  it('proposes the dod folder beside hl.exe', () => {
    expect(defaultDemoFolder('D:\\HL Movies\\hl.exe')).toBe('D:\\HL Movies\\dod');
    expect(defaultDemoFolder('/games/hl/hl.exe')).toBe('/games/hl/dod');
    expect(defaultDemoFolder('hl.exe')).toBe('');
    expect(defaultDemoFolder('')).toBe('');
  });

  it("recognises Steam's own Half-Life folder, whatever the case or slashes", () => {
    expect(isSteamPlayInstall('C:\\Program Files (x86)\\Steam\\steamapps\\common\\Half-Life\\hl.exe')).toBe(true);
    expect(isSteamPlayInstall('c:/steam/STEAMAPPS/COMMON/half-life/HL.EXE')).toBe(true);
    expect(isSteamPlayInstall('C:\\Steam\\steamapps\\common\\Half-Life - PRE-Anniversary for Movies\\hl.exe')).toBe(false);
    expect(isSteamPlayInstall('')).toBe(false);
  });

  it('names the engine build from its packet-entity limit', () => {
    expect(engineBuild(1024)).toBe('anniversary');
    expect(engineBuild(256)).toBe('pre_anniversary');
    expect(engineBuild(undefined)).toBe(null);
  });
});
