import { describe, it, expect, vi } from 'vitest';

vi.mock('./themed_confirm.js', () => ({ themedConfirm: vi.fn(async () => false) }));

import { batchCloseMessage, confirmCloseDuringBatch } from './batch_close_prompt.js';
import { themedConfirm } from './themed_confirm.js';
import { STRINGS } from './strings.js';

describe('closing Studio during a batch (#545)', () => {
  it('closes without asking when no batch is running', async () => {
    const ok = await confirmCloseDuringBatch({ isRunning: () => false, isLocalBuild: async () => false });
    expect(ok).toBe(true);
    expect(themedConfirm).not.toHaveBeenCalled();
  });

  it('asks while a batch runs, and Keep Studio open keeps it open', async () => {
    const ok = await confirmCloseDuringBatch({ isRunning: () => true, isLocalBuild: async () => false });
    expect(ok).toBe(false);
    expect(themedConfirm).toHaveBeenCalledWith(
      STRINGS.BATCH_CLOSE_MODAL.MESSAGE,
      expect.objectContaining({ confirmLabel: STRINGS.BATCH_CLOSE_MODAL.CLOSE_BUTTON }),
    );
  });

  it('warns a local build that closing takes the game with it', () => {
    expect(batchCloseMessage(true)).toContain(STRINGS.BATCH_CLOSE_MODAL.LOCAL_BUILD_NOTE);
    expect(batchCloseMessage(false)).not.toContain(STRINGS.BATCH_CLOSE_MODAL.LOCAL_BUILD_NOTE);
  });
});
