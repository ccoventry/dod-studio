import { describe, it, expect, vi } from 'vitest';

vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));
vi.mock('./toast.js', () => ({ showToast: vi.fn() }));

const { previewTrimUpdate } = await import('./preview_trim_status.js');

describe('previewTrimUpdate', () => {
  it('puts a step on the button while the copy is made', () => {
    expect(previewTrimUpdate({ status: 'Making a trimmed copy of a.dem: reading it (40%)', done: false }))
      .toEqual({ label: 'Making a trimmed copy of a.dem: reading it (40%)' });
  });

  it('toasts the last line, once the preview plays the copy', () => {
    expect(previewTrimUpdate({ status: 'plays a trimmed copy', done: true }))
      .toEqual({ toast: 'plays a trimmed copy' });
  });

  it('ignores an event with no status', () => {
    expect(previewTrimUpdate(null)).toBeNull();
    expect(previewTrimUpdate({ done: true })).toBeNull();
    expect(previewTrimUpdate({ status: '' })).toBeNull();
  });
});
