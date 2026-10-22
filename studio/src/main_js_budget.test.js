import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';

// main.js holds the app's state, the factories' wiring and boot order; the
// features live in their own modules (#683 took it from 2,155 lines to under
// 800). Growing past the budget should be a decision, not drift: move the new
// code into a module with a createX factory, or raise the number here and say
// why in the PR.
const BUDGET = 900;

describe('main.js', () => {
  it(`stays under ${BUDGET} lines`, () => {
    const lines = readFileSync(new URL('./main.js', import.meta.url), 'utf8').split('\n').length;
    expect(lines).toBeLessThanOrEqual(BUDGET);
  });
});
