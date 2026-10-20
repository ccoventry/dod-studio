import { describe, it, expect } from 'vitest';
import { damageNotice, sizeText } from './analyzer_damage.js';

describe('damageNotice', () => {
  it('says where a damaged demo stopped being read, and why', () => {
    const report = {
      demo_info: {
        damage: {
          read_up_to_secs: 125.9,
          frame_index: 7,
          byte_offset: 1000,
          bytes_unread: 15 * 1024 * 1024,
          reason: 'message length too long: 70000',
        },
      },
    };
    expect(damageNotice(report)).toBe(
      'Damaged demo: read up to 2:05. The rest of the file (15.0 MB) doesn\'t parse '
      + '(message length too long: 70000), so everything here stops at 2:05.',
    );
  });

  it('is null for a demo that read to the end, and for an older cache entry', () => {
    expect(damageNotice({ demo_info: { damage: null } })).toBeNull();
    expect(damageNotice({ demo_info: {} })).toBeNull();
    expect(damageNotice(null)).toBeNull();
  });
});

describe('sizeText', () => {
  it('reads in KB under a megabyte and in MB from one', () => {
    expect(sizeText(633)).toBe('1 KB');
    expect(sizeText(1500)).toBe('2 KB');
    expect(sizeText(1024 * 1024)).toBe('1.0 MB');
  });
});
