import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';

// main.js holds the app's state, the factories' wiring and boot order; the
// features live in their own modules (#683 took it from 2,155 lines to under
// 800; a second pass took it from 900 to under 560). The budget is a backstop,
// not a target: build a feature in its own module from its first line and wire
// it in with one startX/createX call. Growing past the budget should be a
// decision, not drift: raise the number here only with a reason in the PR.
const BUDGET = 565; // +1: the Render tab gets the project's markDirty, so Find Moved Videos can save the clips' new paths (#1442); +4: Review asking about a demo whose file is missing (review_missing_demos.js), one import and a factory call over three lines (#1376); +5: demo_file_recheck.js's import and call, and the two project_demos.js functions it takes, so a demo renamed while Studio is open shows as missing; +2: startDamagedDemoToasts import and call (#1373); +2: the project's own folders reach the Render tab, finish step and clip names (#1085); +1: the settings snapshot older projects start from is taken once settings load (#1085); +5: File > Project Settings, one createProjectSettingsPage call with its comment and import (#1085); +2: the New UI switch's import and createNewUi call (#1085)

describe('main.js', () => {
  it(`stays under ${BUDGET} lines`, () => {
    const lines = readFileSync(new URL('./main.js', import.meta.url), 'utf8').split('\n').length;
    expect(lines).toBeLessThanOrEqual(BUDGET);
  });
});
