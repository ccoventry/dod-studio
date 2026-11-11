import { describe, it, expect } from 'vitest';
import { formatPreviewSize } from './capture_pane_logic.js';

describe('formatPreviewSize', () => {
  it('shows megabytes to two decimals', () => {
    expect(formatPreviewSize(0)).toBe('0.00 MB');
    expect(formatPreviewSize(1024 * 1024)).toBe('1.00 MB');
    expect(formatPreviewSize(5.5 * 1024 * 1024)).toBe('5.50 MB');
  });
});
