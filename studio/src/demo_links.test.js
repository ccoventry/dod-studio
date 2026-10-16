import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('./ipc_bridge.js', () => ({
  listDemoLinks: vi.fn(),
  addDemoLink: vi.fn(),
  removeDemoLink: vi.fn(),
}));
vi.mock('./toast.js', () => ({ showToast: vi.fn() }));

import { createDemoLinks, linkRowText } from './demo_links.js';
import { listDemoLinks, addDemoLink, removeDemoLink } from './ipc_bridge.js';

const HL = 'C:\\Games\\Half-Life\\hl.exe';
const LAN = {
  name: 'lan',
  link: 'C:\\Games\\Half-Life\\dod\\demos\\lan',
  target: 'D:\\matches\\lan',
  console_prefix: 'demos/lan/',
};

function setup({ hlExe = HL, picked = 'D:\\matches\\lan' } = {}) {
  const view = { render: vi.fn(), setBusy: vi.fn() };
  const toast = vi.fn();
  const pickFolder = vi.fn(async () => picked);
  const links = createDemoLinks({ getHlExe: () => hlExe, view, pickFolder, toast });
  return { view, toast, pickFolder, links };
}

describe('demo folder links (#376)', () => {
  beforeEach(() => {
    vi.mocked(listDemoLinks).mockReset().mockResolvedValue([]);
    vi.mocked(addDemoLink).mockReset();
    vi.mocked(removeDemoLink).mockReset().mockResolvedValue(undefined);
  });

  it('lists nothing and asks for nothing without a game set', async () => {
    const { view, links } = setup({ hlExe: '' });
    await links.refresh();
    expect(listDemoLinks).not.toHaveBeenCalled();
    expect(view.render).toHaveBeenCalledWith({ links: [], hasGame: false });
  });

  it('will not open the picker without a game set', async () => {
    const { pickFolder, toast, links } = setup({ hlExe: '' });
    expect(await links.linkFolder()).toBeNull();
    expect(pickFolder).not.toHaveBeenCalled();
    expect(toast.mock.calls[0][1]).toBe('error');
  });

  it('links the picked folder, says the console line and lists it', async () => {
    vi.mocked(addDemoLink).mockResolvedValue(LAN);
    vi.mocked(listDemoLinks).mockResolvedValue([LAN]);
    const { view, toast, links } = setup();
    expect(await links.linkFolder()).toEqual(LAN);
    expect(addDemoLink).toHaveBeenCalledWith(HL, 'D:\\matches\\lan');
    expect(toast.mock.calls[0][0]).toContain('viewdemo "demos/lan/<demo name>"');
    expect(view.render).toHaveBeenLastCalledWith({ links: [LAN], hasGame: true });
    expect(view.setBusy.mock.calls).toEqual([[true], [false]]);
  });

  it('does nothing when the picker is cancelled', async () => {
    const { view, links } = setup({ picked: null });
    expect(await links.linkFolder()).toBeNull();
    expect(addDemoLink).not.toHaveBeenCalled();
    expect(view.setBusy).not.toHaveBeenCalled();
  });

  it('shows the reason a folder is refused, and enables the button again', async () => {
    const reason = 'Cannot link C:\\Steam\\steamapps\\common: the game is installed inside it';
    vi.mocked(addDemoLink).mockRejectedValue(reason);
    const { view, toast, links } = setup({ picked: 'C:\\Steam\\steamapps\\common' });
    expect(await links.linkFolder()).toBeNull();
    expect(toast).toHaveBeenCalledWith(reason, 'error', 10000);
    expect(view.setBusy.mock.calls).toEqual([[true], [false]]);
  });

  it('Remove unlinks by name and lists again', async () => {
    const { toast, links } = setup();
    expect(await links.unlink('lan')).toBe(true);
    expect(removeDemoLink).toHaveBeenCalledWith(HL, 'lan');
    expect(toast.mock.calls[0][0]).toContain('untouched');
    expect(listDemoLinks).toHaveBeenCalledTimes(1);
  });

  it('a failed Remove says why and still lists again', async () => {
    vi.mocked(removeDemoLink).mockRejectedValue('lan is not a demo folder link');
    const { toast, links } = setup();
    expect(await links.unlink('lan')).toBe(false);
    expect(toast).toHaveBeenCalledWith('lan is not a demo folder link', 'error', 10000);
    expect(listDemoLinks).toHaveBeenCalledTimes(1);
  });

  it('a list that cannot be read shows empty with the reason', async () => {
    vi.mocked(listDemoLinks).mockRejectedValue('The Half-Life executable setting does not point into a game folder');
    const { view, toast, links } = setup();
    expect(await links.refresh()).toEqual([]);
    expect(toast.mock.calls[0][0]).toContain('does not point into a game folder');
    expect(view.render).toHaveBeenCalledWith({ links: [], hasGame: true });
  });

  it("a row shows the link, its folder and the console line", () => {
    expect(linkRowText(LAN)).toEqual({
      name: 'lan',
      target: '→ D:\\matches\\lan',
      console: 'In the game\'s console: viewdemo "demos/lan/<demo name>"',
      removeTitle: 'Remove this link. D:\\matches\\lan and its demos stay where they are.',
    });
  });
});
