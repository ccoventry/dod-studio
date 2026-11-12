// Shared Playwright `test` for every spec. It fails a test when the page throws
// an uncaught error or leaves a promise rejection unhandled (both arrive as
// `pageerror`), so a pane that crashes on boot can no longer pass quietly
// against the IPC mock's `undefined` answers.
//
// Import `test` and `expect` from here, never from '@playwright/test'.
import { test as base, expect } from '@playwright/test';

// Errors a test may leave behind. Each entry says why; keep this list short and
// fix the cause instead of adding to it. `pattern` is matched against the
// error's message.
export const ALLOWED_PAGE_ERRORS = [];

export function isAllowedPageError(message, allowed = ALLOWED_PAGE_ERRORS) {
  return allowed.some((entry) => entry.pattern.test(message));
}

export const test = base.extend({
  pageErrors: [
    async ({ page }, use) => {
      const errors = [];
      page.on('pageerror', (err) => {
        const message = `${err.name}: ${err.message}`;
        if (!isAllowedPageError(message)) errors.push(`${message}\n${err.stack ?? ''}`.trim());
      });
      await use(errors);
      expect(errors, 'the page threw or left a promise rejection unhandled').toEqual([]);
    },
    { auto: true },
  ],
});

export { expect };
