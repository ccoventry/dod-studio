// command_profiles_ui.js
// Configuration > Commands' profile row (#442): pick a saved set of Initial
// and Scheduled Commands to replace both lists with it, or name the current
// lists and save them. Laid out like Render Output's preset row (#108).
//
// A profile only ever fills the two lists; the lists stay what the warning
// banners and Start Capture Batch check, so an applied command is checked
// exactly like a typed one.
//
// Built-in profiles (#911, builtin_profiles.js) come first in the list and are
// locked: they can be applied or duplicated, never saved over, renamed or
// deleted, and they never reach the settings file.

import {
  profileLists, profileState, saveProfile, renameProfile, deleteProfile,
} from './command_profiles.js';
import {
  mergeProfiles, savedProfiles, isBuiltinName, duplicateName, createBuiltinProfileNotice,
} from './builtin_profiles.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';

// Built-in profiles first, then the user's (mergeProfiles).
let profiles = [];
let builtins = [];
// The kept copies the one-time notice is about; `notice` once wired.
let noticeNames = [];
let notice = null;
// The profile last applied or saved; see profileState.
let activeName = '';
let initialised = false;
let getLists = () => ({ init_commands: [], custom_commands: [] });
let applyLists = () => {};
let onProfilesChange = () => {};

const $ = (selector) => document.querySelector(selector);

/** The user's own profiles, for the settings file. */
export function getCommandProfiles() {
  return savedProfiles(profiles);
}

/** Every profile the row lists, built-in ones included, for reading which
 *  one the lists are on. */
export function getAllCommandProfiles() {
  return profiles;
}

/** The kept copies the notice still has to mention, for the settings file;
 *  empty once it has been closed. */
export function getBuiltinProfileNotice() {
  return notice ? notice.names() : noticeNames;
}

/** The profile the lists were last applied from or saved to, for the
 *  settings file, so "(edited)" survives a restart. */
export function getActiveCommandProfile() {
  return activeName;
}

/**
 * Loads profiles from saved settings. `builtins` (the backend's
 * `builtin_command_profiles`) is kept from the last call when left out;
 * `notice` is the settings file's `builtin_profile_notice`.
 */
export function setCommandProfiles(list, active, { builtins: shipped, notice: names } = {}) {
  if (Array.isArray(shipped)) builtins = shipped;
  profiles = mergeProfiles(builtins, list);
  activeName = typeof active === 'string' ? active : '';
  if (names !== undefined) {
    noticeNames = Array.isArray(names) ? names : [];
    notice?.set(noticeNames);
  }
  syncCommandProfileRow();
}

/** Shows the profile the lists are on, marked "(edited)" once they differ
 *  from it. Called whenever either list changes. */
export function syncCommandProfileRow() {
  // Before init the lists aren't wired up yet, and an empty stand-in would
  // match (and adopt) an empty profile.
  if (!initialised) return;
  const select = $('#command-profile-select');
  if (!select) return;
  const state = profileState(profiles, activeName, getLists());
  // A matching profile stands in for a missing one, and is then the one
  // later edits are measured against.
  activeName = state.name;

  const none = document.createElement('option');
  none.value = '';
  none.textContent = STRINGS.CAPTURE_CONFIG.PROFILE_NONE;
  select.replaceChildren(none, ...profiles.map((p) => {
    const option = document.createElement('option');
    option.value = p.name;
    option.textContent = state.edited && p.name === state.name
      ? STRINGS.CAPTURE_CONFIG.profileEditedOption(p.name)
      : p.name;
    return option;
  }));
  select.value = state.name;

  const c = STRINGS.CAPTURE_CONFIG;
  const locked = !!profiles.find((p) => p.name === state.name)?.builtin;
  const chip = $('#command-profile-builtin-chip');
  if (chip) chip.hidden = !locked;
  // Locked buttons say why in their tooltip.
  const lockButton = (button, disabled, title) => {
    if (!button) return;
    button.disabled = disabled || locked;
    button.title = locked ? c.PROFILE_BUILTIN_LOCKED_TITLE : title;
  };
  lockButton($('#command-profile-rename'), !state.name, c.PROFILE_RENAME_TITLE);
  lockButton($('#command-profile-delete'), !state.name, c.PROFILE_DELETE_TITLE);
  const saveChangesBtn = $('#command-profile-save-changes');
  lockButton(saveChangesBtn, false, c.PROFILE_SAVE_CHANGES_TITLE);
  if (saveChangesBtn) saveChangesBtn.style.display = state.edited ? '' : 'none';
  const duplicateBtn = $('#command-profile-duplicate');
  if (duplicateBtn) duplicateBtn.disabled = !state.name;

  // Save Profile saves over the picked profile when no name is typed, so it
  // is locked for a built-in one then, and for a typed built-in name.
  const saveBtn = $('#command-profile-save');
  if (saveBtn) {
    const typed = $('#command-profile-name')?.value.trim() || '';
    const builtinName = profiles.find((p) => p.builtin && p.name.toLowerCase() === typed.toLowerCase());
    saveBtn.disabled = !!builtinName || (!typed && locked);
    if (builtinName) saveBtn.title = c.profileBuiltinNameTitle(builtinName.name);
    else saveBtn.title = !typed && locked ? c.PROFILE_BUILTIN_LOCKED_TITLE : c.PROFILE_SAVE_TITLE;
  }
}

/** Replaces the user's profiles with `list`'s, keeping the built-in ones in
 *  front. */
function setOwnProfiles(list) {
  profiles = mergeProfiles(builtins, savedProfiles(list));
}

function isLocked(name) {
  return !!profiles.find((p) => p.name === name)?.builtin;
}

/** Puts back profiles, the active profile and (when given) both lists, for
 *  the Undo on a toast. */
function restore(snapshot) {
  profiles = snapshot.profiles;
  activeName = snapshot.activeName;
  if (snapshot.lists) applyLists(snapshot.lists);
  syncCommandProfileRow();
  onProfilesChange();
}

function snapshot(withLists) {
  return { profiles, activeName, lists: withLists ? getLists() : null };
}

function undoAction(before) {
  return { action: { label: STRINGS.CAPTURE_CONFIG.PROFILE_UNDO, onClick: () => restore(before) } };
}

function applyProfile(name) {
  const profile = profiles.find((p) => p.name === name);
  if (!profile) return;
  const before = snapshot(true);
  // Set first, so the list editors' own change handling already measures
  // against the new profile.
  activeName = profile.name;
  applyLists(profileLists(profile));
  syncCommandProfileRow();
  onProfilesChange();
  showToast(STRINGS.CAPTURE_CONFIG.profileAppliedToast(profile.name), 'info', 6000, undoAction(before));
}

/**
 * Wires the row.
 * - `getLists()` returns the current lists (`getCommandsState()`'s shape).
 * - `applyLists(lists)` replaces both lists with `lists`.
 * - `onChange()` saves settings after profiles or the active profile change.
 */
export function initCommandProfiles({ getLists: get, applyLists: apply, onChange } = {}) {
  if (get) getLists = get;
  if (apply) applyLists = apply;
  if (onChange) onProfilesChange = onChange;
  initialised = true;

  const select = $('#command-profile-select');
  const nameEl = $('#command-profile-name');
  // Save Profile's lock depends on the typed name.
  nameEl?.addEventListener('input', () => syncCommandProfileRow());

  notice = createBuiltinProfileNotice({
    el: $('#command-profile-notice'),
    text: STRINGS.CAPTURE_CONFIG.builtinProfileNotice,
    onDismiss: () => onProfilesChange(),
  });
  notice.set(noticeNames);

  select?.addEventListener('change', (e) => {
    if (e.target.value) {
      applyProfile(e.target.value);
      return;
    }
    // "—" leaves the profile without touching the lists.
    activeName = '';
    syncCommandProfileRow();
    onProfilesChange();
  });

  $('#command-profile-save')?.addEventListener('click', () => {
    const name = nameEl?.value.trim() || profileState(profiles, activeName, getLists()).name;
    if (!name) {
      nameEl?.focus();
      return;
    }
    if (isBuiltinName(profiles, name)) return;
    setOwnProfiles(saveProfile(profiles, name, getLists()));
    activeName = profiles.find((p) => p.name.toLowerCase() === name.toLowerCase())?.name || '';
    if (nameEl) nameEl.value = '';
    syncCommandProfileRow();
    onProfilesChange();
  });

  $('#command-profile-save-changes')?.addEventListener('click', () => {
    if (!activeName || isLocked(activeName)) return;
    setOwnProfiles(saveProfile(profiles, activeName, getLists()));
    syncCommandProfileRow();
    onProfilesChange();
  });

  $('#command-profile-rename')?.addEventListener('click', () => {
    const from = profileState(profiles, activeName, getLists()).name;
    const to = nameEl?.value.trim() || '';
    if (!from || isLocked(from)) return;
    if (!to) {
      nameEl?.focus();
      return;
    }
    const renamed = renameProfile(profiles, from, to);
    if (!renamed) {
      showToast(STRINGS.CAPTURE_CONFIG.profileNameTakenToast(to), 'error');
      return;
    }
    setOwnProfiles(renamed);
    activeName = to;
    if (nameEl) nameEl.value = '';
    syncCommandProfileRow();
    onProfilesChange();
  });

  $('#command-profile-delete')?.addEventListener('click', () => {
    const name = profileState(profiles, activeName, getLists()).name;
    if (!name || isLocked(name)) return;
    const before = snapshot(false);
    setOwnProfiles(deleteProfile(profiles, name));
    activeName = '';
    syncCommandProfileRow();
    onProfilesChange();
    showToast(STRINGS.CAPTURE_CONFIG.profileDeletedToast(name), 'info', 6000, undoAction(before));
  });

  // A copy of the picked profile's saved lists, picked in its place. The two
  // lists are left as they are, so edits made since picking show as the
  // copy's "(edited)" and Save Changes saves them into it.
  $('#command-profile-duplicate')?.addEventListener('click', () => {
    const from = profiles.find((p) => p.name === profileState(profiles, activeName, getLists()).name);
    if (!from) return;
    const before = snapshot(false);
    const name = duplicateName(profiles, from.name);
    setOwnProfiles(saveProfile(profiles, name, from));
    activeName = name;
    syncCommandProfileRow();
    onProfilesChange();
    showToast(STRINGS.CAPTURE_CONFIG.profileDuplicatedToast(name), 'info', 6000, undoAction(before));
  });

  syncCommandProfileRow();
}
