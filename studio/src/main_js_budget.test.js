import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';

// main.js holds the app's state, the factories' wiring and boot order; the
// features live in their own modules (#683 took it from 2,155 lines to under
// 800; a second pass took it from 900 to under 560). The budget is a backstop,
// not a target: build a feature in its own module from its first line and wire
// it in with one startX/createX call. Growing past the budget should be a
// decision, not drift: raise the number here only with a reason in the PR.
const BUDGET = 550; // +1: the Render tab gets the project's markDirty, so Find Moved Videos can save the clips' new paths (#1442); +6: the Auditor's Consolidate hands the demos it moved to the project, relocateMoved wired in over five lines and the destructured function (#872)

describe('main.js', () => {
  it(`stays under ${BUDGET} lines`, () => {
    const lines = readFileSync(new URL('./main.js', import.meta.url), 'utf8').split('\n').length;
    expect(lines).toBeLessThanOrEqual(BUDGET);
  });
});
