import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./ipc_bridge.js', () => ({ convertProtocol47: vi.fn() }));
vi.mock('./toast.js', () => ({ showToast: vi.fn() }));

const { convertProtocol47 } = await import('./ipc_bridge.js');
const { protocol47Notice, convertAnalyzedDemo } = await import('./analyzer_protocol47.js');
const { STRINGS } = await import('./strings.js');

describe('protocol47Notice', () => {
  it('speaks up for a protocol-47 demo', () => {
    expect(protocol47Notice({ demo_info: { network_protocol: 47 } })).toBe(STRINGS.PROTOCOL_47.NOTICE);
  });

  it('is null for a protocol-48 demo, a report without the field, and no report', () => {
    expect(protocol47Notice({ demo_info: { network_protocol: 48 } })).toBeNull();
    expect(protocol47Notice({ demo_info: {} })).toBeNull();
    expect(protocol47Notice(null)).toBeNull();
  });
});

describe('convertAnalyzedDemo', () => {
  beforeEach(() => {
    convertProtocol47.mockReset();
  });

  it('converts, lists the copy and opens it', async () => {
    convertProtocol47.mockResolvedValue('D:\\demos\\old_p48.dem');
    const relistFolderOf = vi.fn().mockResolvedValue();
    const open = vi.fn().mockResolvedValue();
    const text = await convertAnalyzedDemo('D:\\demos\\old.dem', { relistFolderOf, open });
    expect(convertProtocol47).toHaveBeenCalledWith('D:\\demos\\old.dem');
    expect(relistFolderOf).toHaveBeenCalledWith('D:\\demos\\old.dem');
    expect(open).toHaveBeenCalledWith('D:\\demos\\old_p48.dem');
    expect(text).toBe(STRINGS.PROTOCOL_47.done('old_p48.dem'));
  });

  it('opens nothing when the conversion fails', async () => {
    convertProtocol47.mockRejectedValue(new Error('already a protocol 48 demo: nothing to convert'));
    const open = vi.fn();
    await expect(convertAnalyzedDemo('a.dem', { relistFolderOf: vi.fn(), open })).rejects.toThrow('already');
    expect(open).not.toHaveBeenCalled();
  });
});
