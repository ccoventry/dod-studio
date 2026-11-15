// boot_watchdog.js — notices when Studio's page never finishes starting.
//
// A classic script, not a module, loaded from index.html before main.js: it
// has to run when the app's modules don't. After the hard reset of 2026-11-07
// a `tauri dev` window loaded 76 of the page's scripts and waited on the rest
// for good: no error anywhere, every button dead, the backend answering fine.
// The same silence follows any start that stops short, such as an exception
// thrown in main.js's boot or a start-up call that never answers.
//
// main.js calls `window.dodStudioBoot.done()` once its boot has run. If that
// hasn't happened by the deadline, the page reloads once, saying so on screen
// and in the activity log. If the reload doesn't start either, the banner says
// to restart Studio and the page is left alone, so it never reload-loops.
//
// Its text stays here, not in strings.js: the catalog is a module, and this
// runs exactly when modules don't.
(function () {
  'use strict';

  /** How long a start may take. A cold `tauri dev` start, Vite bundling
   *  every dependency first, takes a few seconds; a release build, one. */
  const DEADLINE_MS = 20000;
  /** Set for the reload this script makes, so the next start knows. */
  const RELOAD_KEY = 'dodstudio.bootReloaded';
  const BANNER_ID = 'boot-watchdog-banner';

  const TEXT = {
    stuck: (secs, scripts) =>
      `[boot] Studio's page didn't finish starting in ${secs} s (${scripts} of its scripts loaded): reloading it once`,
    stuckAgain: (secs, scripts) =>
      `[boot] Studio's page didn't finish starting in ${secs} s after a reload either (${scripts} of its scripts loaded): restart Studio`,
    started: (ms) => `[boot] Studio's page started in ${ms} ms`,
    startedAfterReload: (ms) => `[boot] Studio's page started after a reload, in ${ms} ms`,
    BANNER_RELOADING: "Studio didn't finish starting. Reloading the page…",
    BANNER_RESTART: "Studio didn't finish starting, even after a reload. Close Studio and start it again. The activity log has the details.",
  };

  /**
   * The watchdog itself, every effect handed in so it can be tested.
   * `store` is { get, set, remove } over one session's storage, `log` sends a
   * line to the activity log and may return a promise, `show`/`hide` drive the
   * banner, `countScripts` says how many of the page's scripts loaded.
   */
  function createBootWatchdog({ deadlineMs, now, setTimer, clearTimer, store, log, show, hide, reload, countScripts }) {
    const startedAt = now();
    let state = 'waiting';
    const secs = Math.round(deadlineMs / 1000);

    function onDeadline() {
      if (state !== 'waiting') return;
      const scripts = countScripts();
      if (store.get(RELOAD_KEY)) {
        state = 'stuck';
        show(TEXT.BANNER_RESTART);
        log(TEXT.stuckAgain(secs, scripts));
        return;
      }
      state = 'reloading';
      store.set(RELOAD_KEY, '1');
      show(TEXT.BANNER_RELOADING);
      // The line goes out before the page does; a start that finished in the
      // meantime stays.
      Promise.resolve()
        .then(() => log(TEXT.stuck(secs, scripts)))
        .catch(() => {})
        .then(() => { if (state === 'reloading') reload(); });
    }

    const timer = setTimer(onDeadline, deadlineMs);

    return {
      /** main.js's boot has run. A late start still counts: the banner goes. */
      done() {
        if (state === 'started') return;
        clearTimer(timer);
        const reloaded = !!store.get(RELOAD_KEY);
        store.remove(RELOAD_KEY);
        state = 'started';
        hide();
        const ms = Math.round(now() - startedAt);
        log(reloaded ? TEXT.startedAfterReload(ms) : TEXT.started(ms));
      },
      state: () => state,
    };
  }

  globalThis.dodStudioBootWatchdog = { create: createBootWatchdog, TEXT, RELOAD_KEY, DEADLINE_MS };

  // Only in the page itself: imported by a unit test, this just defines.
  if (typeof document === 'undefined' || !document.currentScript) return;

  const store = {
    get: (key) => { try { return sessionStorage.getItem(key); } catch { return null; } },
    set: (key, value) => { try { sessionStorage.setItem(key, value); } catch { /* no storage: no reload marker */ } },
    remove: (key) => { try { sessionStorage.removeItem(key); } catch { /* as above */ } },
  };

  function log(message) {
    const tauri = window.__TAURI_INTERNALS__;
    if (!tauri || typeof tauri.invoke !== 'function') {
      console.warn(message);
      return undefined;
    }
    return tauri.invoke('log_frontend_event', { message }).catch((err) => {
      console.error('boot watchdog: log_frontend_event failed:', err);
    });
  }

  function show(text) {
    let el = document.getElementById(BANNER_ID);
    if (!el) {
      el = document.createElement('div');
      el.id = BANNER_ID;
      el.setAttribute('role', 'alert');
      // Inline: the stylesheet may be what never arrived.
      el.style.cssText = 'position:fixed;top:0;left:0;right:0;z-index:2147483647;padding:10px 16px;'
        + 'background:#7a2e14;color:#fff;font:14px/1.4 system-ui,sans-serif;text-align:center;';
      (document.body || document.documentElement).appendChild(el);
    }
    el.textContent = text;
  }

  function hide() {
    const el = document.getElementById(BANNER_ID);
    if (el) el.remove();
  }

  function countScripts() {
    try {
      return performance.getEntriesByType('resource').filter((e) => /\.js(\?|$)/.test(e.name)).length;
    } catch {
      return 0;
    }
  }

  window.dodStudioBoot = createBootWatchdog({
    // A test may shorten it (tests/e2e/app-boot.spec.js).
    deadlineMs: Number(window.__dodStudioBootDeadlineMs) || DEADLINE_MS,
    now: () => performance.now(),
    setTimer: (fn, ms) => setTimeout(fn, ms),
    clearTimer: (id) => clearTimeout(id),
    store,
    log,
    show,
    hide,
    reload: () => location.reload(),
    countScripts,
  });
})();
