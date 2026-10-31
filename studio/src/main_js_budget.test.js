import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';

// main.js holds the app's state, the factories' wiring and boot order; the
// features live in their own modules (#683 took it from 2,155 lines to under
// 800; a second pass took it from 900 to under 560). The budget is a backstop,
// not a target: build a feature in its own module from its first line and wire
// it in with one startX/createX call. Growing past the budget should be a
// decision, not drift: raise the number here only with a reason in the PR.
const BUDGET = 516;

describe('main.js', () => {
  it(`stays under ${BUDGET} lines`, () => {
    const lines = readFileSync(new URL('./main.js', import.meta.url), 'utf8').split('\n').length;
    expect(lines).toBeLessThanOrEqual(BUDGET);
  });
});
