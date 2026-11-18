// "This project was made with POST. Switch?" (#1085): asked, never switched
// by itself; Keep makes the picked install the project's.

import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./ipc_bridge.js', () => ({ findInstalls: vi.fn(), logFrontendEvent: vi.fn() }));
vi.mock('./themed_confirm.js', () => ({ themedConfirm: vi.fn() }));
vi.mock('./install_picker.js', () => ({
  installText: (install) => install.name,
  switchInstall: vi.fn(async () => true),
}));

const PRE = 'D:\\Steam\\steamapps\\common\\Half-Life - PRE-Anniversary for Movies\\hl.exe';
const POST = 'D:\\Steam\\steamapps\\common\\Half-Life - POST-Anniversary for Movies\\hl.exe';
let picked = PRE;
globalThis.document = { querySelector: (sel) => (sel === '#hl-path-input' ? { value: picked } : null) };

const { offerProjectInstall } = await import('./project_install_offer.js');
const { withMadeWith } = await import('./project_settings_logic.js');
const ipc = await import('./ipc_bridge.js');
const { themedConfirm } = await import('./themed_confirm.js');
const { switchInstall } = await import('./install_picker.js');
const { STRINGS } = await import('./strings.js');

const installs = [
  { name: 'Half-Life - PRE-Anniversary for Movies', hl_exe: PRE },
  { name: 'Half-Life - POST-Anniversary for Movies', hl_exe: POST },
];

/** A session's createProjectSettings, holding `block`. */
function settingsHolding(block) {
  let held = block;
  return { block: () => held, setBlock: (next) => { held = next; } };
}

beforeEach(() => {
  picked = PRE;
  vi.mocked(ipc.findInstalls).mockResolvedValue(installs);
  vi.mocked(themedConfirm).mockReset();
  vi.mocked(switchInstall).mockClear();
});

describe('offerProjectInstall', () => {
  it('asks to switch to the install the project was made with', async () => {
    vi.mocked(themedConfirm).mockResolvedValue(true);
    const answer = await offerProjectInstall({ path: 'p', settings: settingsHolding(withMadeWith({}, POST)), markDirty: vi.fn() });
    expect(answer).toBe('switched');
    expect(vi.mocked(themedConfirm).mock.calls[0][0])
      .toBe(STRINGS.PROJECT_SETTINGS.installOffer(installs[1].name, installs[0].name));
    expect(switchInstall).toHaveBeenCalledWith(POST);
  });

  it('keeps the picked install as the project\'s on Keep, and marks it unsaved', async () => {
    vi.mocked(themedConfirm).mockResolvedValue(false);
    const held = settingsHolding(withMadeWith({ picture: { width: 1 } }, POST));
    const markDirty = vi.fn();
    expect(await offerProjectInstall({ path: 'p', settings: held, markDirty })).toBe('kept');
    expect(switchInstall).not.toHaveBeenCalled();
    expect(held.block().install.madeWith.hlExe).toBe(PRE);
    expect(held.block().picture).toEqual({ width: 1 });
    expect(markDirty).toHaveBeenCalledOnce();
  });

  it('asks nothing when the project\'s install is picked', async () => {
    picked = POST;
    expect(await offerProjectInstall({ path: 'p', settings: settingsHolding(withMadeWith({}, POST)), markDirty: vi.fn() })).toBe(null);
    expect(themedConfirm).not.toHaveBeenCalled();
  });

  it('asks nothing for a project with no install, or none open', async () => {
    expect(await offerProjectInstall({ path: 'p', settings: settingsHolding({}), markDirty: vi.fn() })).toBe(null);
    expect(await offerProjectInstall({ path: 'p', settings: settingsHolding(null), markDirty: vi.fn() })).toBe(null);
    expect(themedConfirm).not.toHaveBeenCalled();
  });
});
