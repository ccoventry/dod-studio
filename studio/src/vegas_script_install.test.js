import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./ipc_bridge.js', () => ({ vegasInstallScript: vi.fn() }));
const showToast = vi.fn();
vi.mock('./toast.js', () => ({ showToast: (...args) => showToast(...args) }));

const { startVegasScriptInstall } = await import('./vegas_script_install.js');

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

/** Just enough of a button: a disabled flag and a click handler. */
function fakeButton() {
  const button = {
    disabled: false,
    handler: null,
    addEventListener(type, fn) { if (type === 'click') button.handler = fn; },
    click() { return button.handler(); },
  };
  return button;
}

let button;
beforeEach(() => {
  showToast.mockClear();
  button = fakeButton();
  globalThis.document = { querySelector: () => button };
});

describe('startVegasScriptInstall', () => {
  it('toasts where the script went and how to see it in VEGAS', async () => {
    startVegasScriptInstall({ install: async () => 'C:\\Docs\\Vegas Script Menu\\DoD Studio Nameplates.cs' });
    await button.click();
    const [text, kind] = showToast.mock.calls[0];
    expect(text).toContain('C:\\Docs\\Vegas Script Menu\\DoD Studio Nameplates.cs');
    expect(text).toContain('Tools > Scripting > Rescan Script Menu Folder');
    expect(kind).toBe('success');
  });

  it('is disabled while the copy runs and enabled again after', async () => {
    let finish;
    startVegasScriptInstall({ install: () => new Promise((resolve) => { finish = resolve; }) });
    const running = button.click();
    expect(button.disabled).toBe(true);
    finish('x');
    await running;
    expect(button.disabled).toBe(false);
  });

  it('shows no success toast when the copy fails, and enables the button again', async () => {
    startVegasScriptInstall({ install: async () => { throw new Error('denied'); } });
    await button.click();
    await flush();
    expect(showToast).not.toHaveBeenCalled();
    expect(button.disabled).toBe(false);
  });
});
