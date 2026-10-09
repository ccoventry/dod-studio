// command_profiles_ui.js
// Configuration > Commands' profile row (#442): pick a saved set of Initial
// and Scheduled Commands to replace both lists with it, or name the current
// lists and save them. Laid out like Render Output's preset row (#108).
//
// A profile only ever fills the two lists; the lists stay what the warning
// banners and Start Capture Batch check, so an applied command is checked
// exactly like a typed one.

import {
  normaliseProfiles, profileLists, profileState, saveProfile, renameProfile, deleteProfile,
} from './command_profiles.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';

let profiles = [];
// The profile last applied or saved; see profileState.
let activeName = '';
let initialised = false;
let getLists = () => ({ init_commands: [], custom_commands: [] });
let applyLists = () => {};
let onProfilesChange = () => {};

const $ = (selector) => document.querySelector(selector);

/** The saved profiles, for the settings file. */
export function getCommandProfiles() {
  return profiles;
}

/** The profile the lists were last applied from or saved to, for the
 *  settings file, so "(edited)" survives a restart. */
export function getActiveCommandProfile() {
  return activeName;
}

/** Loads profiles from saved settings. */
export function setCommandProfiles(list, active) {
  profiles = normaliseProfiles(list);
  activeName = typeof active === 'string' ? active : '';
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

  const renameBtn = $('#command-profile-rename');
  if (renameBtn) renameBtn.disabled = !state.name;
  const deleteBtn = $('#command-profile-delete');
  if (deleteBtn) deleteBtn.disabled = !state.name;
  const saveChangesBtn = $('#command-profile-save-changes');
  if (saveChangesBtn) saveChangesBtn.style.display = state.edited ? '' : 'none';
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
    profiles = saveProfile(profiles, name, getLists());
    activeName = profiles.find((p) => p.name.toLowerCase() === name.toLowerCase())?.name || '';
    if (nameEl) nameEl.value = '';
    syncCommandProfileRow();
    onProfilesChange();
  });

  $('#command-profile-save-changes')?.addEventListener('click', () => {
    if (!activeName) return;
    profiles = saveProfile(profiles, activeName, getLists());
    syncCommandProfileRow();
    onProfilesChange();
  });

  $('#command-profile-rename')?.addEventListener('click', () => {
    const from = profileState(profiles, activeName, getLists()).name;
    const to = nameEl?.value.trim() || '';
    if (!from) return;
    if (!to) {
      nameEl?.focus();
      return;
    }
    const renamed = renameProfile(profiles, from, to);
    if (!renamed) {
      showToast(STRINGS.CAPTURE_CONFIG.profileNameTakenToast(to), 'error');
      return;
    }
    profiles = renamed;
    activeName = to;
    if (nameEl) nameEl.value = '';
    syncCommandProfileRow();
    onProfilesChange();
  });

  $('#command-profile-delete')?.addEventListener('click', () => {
    const name = profileState(profiles, activeName, getLists()).name;
    if (!name) return;
    const before = snapshot(false);
    profiles = deleteProfile(profiles, name);
    activeName = '';
    syncCommandProfileRow();
    onProfilesChange();
    showToast(STRINGS.CAPTURE_CONFIG.profileDeletedToast(name), 'info', 6000, undoAction(before));
  });

  syncCommandProfileRow();
}
