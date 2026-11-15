// builtin_profiles.js
// Built-in command profiles (#911), like Clean clips: DoD Studio ships them
// (studio/src-tauri/src/builtin_profiles.rs), the Commands tab lists them
// beside the user's own, and they can only be applied or duplicated. They are
// never saved into the settings file, so every update replaces them; a user
// who wants changes duplicates one into a profile of their own.

import { normaliseProfiles } from './command_profiles.js';

/** Built-in profiles first, in the order shipped, then the user's sorted by
 *  name. Built-ins carry `builtin: true`. A saved profile with a built-in's
 *  name (ignoring case) is left out: the backend renames those at load, so
 *  one here would be a stray. */
export function mergeProfiles(builtins, saved) {
  const locked = normaliseProfiles(builtins).map((p) => ({ ...p, builtin: true }));
  // normaliseProfiles sorts; keep the shipped order instead.
  const order = (Array.isArray(builtins) ? builtins : []).map((p) => String(p?.name ?? '').trim());
  locked.sort((a, b) => order.indexOf(a.name) - order.indexOf(b.name));
  const own = normaliseProfiles(saved).filter((p) => !isBuiltinName(locked, p.name));
  return [...locked, ...own];
}

/** The user's own profiles, for the settings file. */
export function savedProfiles(profiles) {
  return (profiles || []).filter((p) => !p.builtin).map(({ builtin: _builtin, ...p }) => p);
}

/** Whether `name` is a built-in profile's name, ignoring case and spaces. */
export function isBuiltinName(profiles, name) {
  const wanted = String(name || '').trim().toLowerCase();
  return !!wanted && (profiles || []).some((p) => p.builtin && p.name.toLowerCase() === wanted);
}

/** The name Duplicate gives a copy of `name`: "<name> copy", then
 *  "<name> copy 2", "copy 3"… past every name in `profiles` (ignoring case). */
export function duplicateName(profiles, name) {
  const taken = new Set((profiles || []).map((p) => p.name.toLowerCase()));
  const base = `${String(name || '').trim()} copy`;
  if (!taken.has(base.toLowerCase())) return base;
  for (let n = 2; ; n += 1) {
    const candidate = `${base} ${n}`;
    if (!taken.has(candidate.toLowerCase())) return candidate;
  }
}

/**
 * The one-time notice for the user's changed copies of a built-in profile
 * that were renamed at load ("Clean clips (mine)"). `el` holds the text in
 * `[data-notice-text]` and an OK button in `[data-notice-ok]`; `onDismiss()`
 * saves settings once it is closed, so it never shows again.
 */
export function createBuiltinProfileNotice({ el, text, onDismiss = () => {} } = {}) {
  let names = [];
  const render = () => {
    if (!el) return;
    el.hidden = names.length === 0;
    const textEl = el.querySelector('[data-notice-text]');
    if (textEl) textEl.textContent = names.length ? text(names) : '';
  };
  el?.querySelector('[data-notice-ok]')?.addEventListener('click', () => {
    names = [];
    render();
    onDismiss();
  });
  return {
    /** Shows the notice for `list` (empty hides it). */
    set(list) {
      names = Array.isArray(list) ? list.filter((n) => typeof n === 'string' && n.trim()) : [];
      render();
    },
    /** The names still waiting for the notice, for the settings file. */
    names: () => names,
  };
}
