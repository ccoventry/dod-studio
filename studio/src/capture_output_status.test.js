// The chip wording; the repainting is covered by tests/e2e/capture-output-status.spec.js.
import { describe, it, expect } from 'vitest';
import { rowStatusFor } from './capture_output_status.js';

describe('rowStatusFor', () => {
  it('says checking until there is an answer', () => {
    expect(rowStatusFor(undefined).text).toBe('Checking…');
  });
  it('maps each status to a tone', () => {
    expect(rowStatusFor({ status: 'ok', usable: true }).tone).toBe('ok');
    expect(rowStatusFor({ status: 'not_found', usable: true })).toMatchObject({ tone: 'note', text: 'Will be created' });
    expect(rowStatusFor({ status: 'not_found', usable: false })).toMatchObject({ tone: 'bad', text: 'Drive not found' });
    expect(rowStatusFor({ status: 'not_absolute', usable: false }).tone).toBe('bad');
    expect(rowStatusFor({ status: 'not_a_directory', usable: false }).text).toBe('That is a file');
    expect(rowStatusFor({ status: 'malformed', usable: false }).text).toBe('Not a valid path');
  });
});
