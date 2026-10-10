// config_wiring.js
// Configuration's paths, capture mode, OBS and HLAE's FFmpeg: the path
// warnings under each field, the Browse buttons, the capture-mode switch, the
// OBS buttons and the HLAE FFmpeg link. Moved out of main.js (#683); main.js
// calls initConfigWiring once the settings are in the form.

import { open, confirm } from '@tauri-apps/plugin-dialog';
import { invoke } from '@tauri-apps/api/core';
import { checkHlaeFfmpeg, linkHlaeFfmpeg, diagnoseExecutablePaths, launchObs } from './ipc_bridge.js';
import { refreshLaunchGuard, runObsConnectionTest } from './capture_pane.js';
import { showToast } from './toast.js';
import { numberField } from './number_field.js';
import { STRINGS } from './strings.js';

// ── Path Routing: does each configured path point at a real file? ────────────
// These fields accepted anything. `validate_paths` only ran at capture launch,
// so a typo sat there looking correct until a batch failed minutes later — and
// the FFmpeg override was never checked at all. The complaint goes under the
// field that caused it rather than into a banner elsewhere, so it is visible
// while you are still looking at the box you typed into.
const PATH_FIELDS = [
  ['#hl-path-input', '#hl-path-warning'],
  ['#hlae-path-input', '#hlae-path-warning'],
  ['#ffmpeg-override-path-input', '#ffmpeg-path-warning'],
  ['#goldsrc-hooks-dll-path-input', '#goldsrc-hooks-path-warning'],
];

/**
 * Whether `hlPath` is the hl.exe in Steam's own `steamapps/common/Half-Life`
 * folder -- normally the copy people play online with, which must never have
 * HLAE or our hook DLL loaded into it (docs/vac_safety.md, #373). A warning,
 * not a block: someone may keep only that one install and never play online.
 */
function isSteamPlayInstall(hlPath) {
  const normalised = (hlPath || '').trim().replace(/\\/g, '/').toLowerCase();
  return normalised.endsWith('/steamapps/common/half-life/hl.exe');
}

export async function refreshPathWarnings() {
  const rows = PATH_FIELDS
    .map(([input, warning]) => ({
      input: document.querySelector(input),
      warning: document.querySelector(warning),
    }))
    .filter((r) => r.input && r.warning);
  // Start stays disabled while the hl.exe or HLAE path is blank: re-check it
  // whenever a path changes.
  refreshLaunchGuard();
  if (!rows.length) return;

  let states;
  try {
    states = await diagnoseExecutablePaths(rows.map((r) => r.input.value?.trim() || ""));
  } catch {
    // Already logged by the bridge. Clear rather than leave a stale complaint
    // standing next to a path it may no longer describe.
    rows.forEach((r) => { r.warning.style.display = 'none'; });
    return;
  }

  rows.forEach((row, i) => {
    let message = "";
    if (states[i] === 'not_found') message = STRINGS.CAPTURE_CONFIG.PATH_NOT_FOUND;
    else if (states[i] === 'not_a_file') message = STRINGS.CAPTURE_CONFIG.PATH_IS_A_FOLDER;
    else if (row.input.id === 'hl-path-input' && isSteamPlayInstall(row.input.value)) {
      message = STRINGS.CAPTURE_CONFIG.HL_PATH_PLAY_INSTALL;
    }
    // 'empty' says nothing on purpose: these are legitimately blank before they
    // are filled in, and the FFmpeg override is optional entirely.
    row.warning.textContent = message;
    row.warning.style.display = message ? '' : 'none';
  });
}

/** The selected capture mode id, defaulting to the path that always works. */
/**
 * OBS connection fields as the settings form currently holds them.
 */
function obsSettingsFromForm() {
  return {
    host: document.querySelector('#config-obs-host')?.value?.trim() || '127.0.0.1',
    port: parseInt(document.querySelector('#config-obs-port')?.value, 10) || 4455,
    password: document.querySelector('#config-obs-password')?.value || ''
  };
}

/**
 * Offers to stop an OBS recording left running by a previous session.
 *
 * The capture engine stops OBS on every exit path the process lives to run,
 * but a panic (release builds abort rather than unwind), a force-quit and a
 * power cut all leave nothing behind to run anything. OBS simply keeps
 * recording — into a folder only dod-studio would ever name — until the drive
 * fills. This is the only place that can notice.
 *
 * Silent unless there is something to act on: OBS not running is the ordinary
 * answer at startup, and a recording that is not ours is not ours to stop.
 */
export async function checkObsOrphanOnStartup() {
  if (currentCaptureMode() !== 'obs') return;

  const report = await invoke('obs_check_orphan', obsSettingsFromForm()).catch((err) => {
    console.error('OBS orphan check failed:', err);
    return null;
  });
  if (!report?.recording || !report.ours) return;

  const stop = await confirm(STRINGS.CAPTURE_CONFIG.obsOrphanPrompt(report.directory), {
    title: STRINGS.CAPTURE_CONFIG.OBS_ORPHAN_TITLE,
    kind: 'warning',
    okLabel: STRINGS.CAPTURE_CONFIG.OBS_ORPHAN_STOP,
    cancelLabel: STRINGS.CAPTURE_CONFIG.OBS_ORPHAN_LEAVE
  }).catch(() => false);
  if (!stop) return;

  await invoke('obs_recover_orphan', obsSettingsFromForm())
    .then((video) => {
      showToast(
        video
          ? STRINGS.CAPTURE_CONFIG.obsOrphanRecovered(video)
          : STRINGS.CAPTURE_CONFIG.OBS_ORPHAN_GONE,
        'success'
      );
    })
    .catch((err) => {
      console.error('OBS orphan recovery failed:', err);
      showToast(STRINGS.CAPTURE_CONFIG.obsOrphanFailed(err), 'error');
    });
}

export function currentCaptureMode() {
  return document.querySelector('#config-capture-mode')?.value || 'frame_sequence';
}

export function applyCaptureModeUI() {
  const mode = currentCaptureMode();
  const video = mode === 'direct_to_video';
  const obs = mode === 'obs';
  const agr = mode === 'agr';

  // Kept in step rather than read: the backend still accepts `ffmpeg_capture`
  // from older payloads, and leaving it stale would make the two disagree for
  // anything that has not moved to the enum yet.
  const legacy = document.querySelector('#config-ffmpeg-capture');
  if (legacy) legacy.checked = video;

  document.querySelectorAll('.setting-label[data-capture-mode]').forEach((label) => {
    label.classList.toggle('active', (label.dataset.captureMode === 'video') === video);
  });
  // Hidden rather than disabled, in both other modes: frame-sequence mode
  // has its own, unrelated answer to "what codec" — Render Studio's own
  // codec picker, a different set of options (ProRes/DNxHR/H.264) for a
  // different purpose (final delivery, not the capture-time lossless
  // intermediate). OBS mode does not consume this setting today either. If
  // OBS capture grows its own codec choice later (it can already ask for a
  // container in Custom Output mode), this is where that would show — but
  // "will eventually" is not "does now", and showing it today would claim a
  // connection to OBS capture that does not exist yet.
  const codecGroup = document.querySelector('#capture-codec-group');
  if (codecGroup) codecGroup.style.display = video ? '' : 'none';

  // Capture FPS is non-real-time for the other two modes but meaningless for
  // OBS, which has its own separate OBS Capture FPS field below — showing
  // both invites setting the wrong one.
  const captureFpsGroup = document.querySelector('#capture-fps-group');
  if (captureFpsGroup) captureFpsGroup.style.display = obs || agr ? 'none' : '';

  // AGR mode records no video, so Capture FPS gives way to its own rate. An
  // empty AGR FPS still means "the same as Capture FPS", so the placeholder
  // shows the number that will actually be used.
  const agrFpsGroup = document.querySelector('#agr-fps-group');
  if (agrFpsGroup) agrFpsGroup.style.display = agr ? '' : 'none';
  const agrFpsInput = document.querySelector('#config-agr-fps');
  if (agrFpsInput) {
    agrFpsInput.placeholder = String(numberField('#config-capture-fps', 300, { integer: true, positive: true }));
  }

  // The OBS block follows the same rule: hidden rather than disabled,
  // because showing a dead connection form in frame-sequence mode would
  // suggest OBS is involved when it is not.
  const obsGroup = document.querySelector('#obs-settings-group');
  if (obsGroup) obsGroup.style.display = obs ? '' : 'none';
}

// ── HLAE's own FFmpeg ─────────────────────────────────────────────────────────
// `mirv_movie_ffmpeg` makes HLAE spawn FFmpeg itself, and it does not consult
// the app's FFmpeg setting — it looks only in its own folder or at an ffmpeg.ini
// beside it. With neither present, direct-to-video capture runs to completion
// and produces no video, so the state is surfaced here rather than discovered
// after a batch. See docs/direct_to_video_capture.md.
export async function refreshHlaeFfmpegStatus() {
  const statusEl = document.querySelector('#hlae-ffmpeg-status');
  const linkBtn = document.querySelector('#hlae-ffmpeg-link-btn');
  if (!statusEl || !linkBtn) return;

  const hlaePath = document.querySelector('#hlae-path-input')?.value?.trim() || "";
  const unknown = () => {
    statusEl.textContent = STRINGS.CAPTURE_CONFIG.HLAE_FFMPEG_UNKNOWN;
    linkBtn.style.display = 'none';
  };
  if (!hlaePath) return unknown();

  // Passed in so the check can say whether HLAE and Render Studio agree, not
  // just whether HLAE has an answer at all.
  const ffmpegPath =
    document.querySelector('#ffmpeg-override-path-input')?.value?.trim() || "ffmpeg";

  let result;
  try {
    result = await checkHlaeFfmpeg(hlaePath, ffmpegPath);
  } catch {
    // Already logged by the bridge. Say nothing rather than assert a state.
    return unknown();
  }

  const s = result?.state || {};
  // Outranks every message below it, because it questions the thing they are
  // all about: if the hook DLL is not there, capture cannot work regardless of
  // what HLAE's ffmpeg folder contains. Still only a note — an unusual layout
  // should not stop someone who knows their install works.
  if (result.missing_hook_dll) {
    statusEl.textContent =
      STRINGS.CAPTURE_CONFIG.HLAE_FFMPEG_NO_HOOK_DLL(result.missing_hook_dll);
    linkBtn.style.display = result?.can_link ? '' : 'none';
    return;
  }

  // The toggle is on and HLAE has nothing to pipe to. Worth saying more
  // sharply than the generic "no FFmpeg" line below, because this is the
  // combination that produces a capture which runs to completion and records
  // no video at all.
  if (document.querySelector('#config-ffmpeg-capture')?.checked && !result.usable) {
    statusEl.textContent = STRINGS.CAPTURE_CONFIG.FFMPEG_CAPTURE_UNAVAILABLE;
    linkBtn.style.display = result?.can_link ? '' : 'none';
    return;
  }

  switch (s.state) {
    case 'bundled':
      statusEl.textContent = STRINGS.CAPTURE_CONFIG.HLAE_FFMPEG_BUNDLED(s.path);
      break;
    case 'linked':
      // Outranks everything below: if the override is not FFmpeg, saying the
      // two "disagree" describes a real difference and hides the actual
      // problem, and the button would only write the wrong program in.
      if (result.app_ffmpeg_problem) {
        statusEl.textContent =
          STRINGS.CAPTURE_CONFIG.HLAE_FFMPEG_BAD_OVERRIDE(result.app_ffmpeg_problem);
      } else if (!s.target_exists) {
        // A stale pointer outranks a disagreement: it is not pointed at
        // anything at all, so which build it disagrees with is moot.
        statusEl.textContent = STRINGS.CAPTURE_CONFIG.HLAE_FFMPEG_STALE(s.target);
      } else if (result.agrees_with_app === false && result.app_ffmpeg) {
        statusEl.textContent =
          STRINGS.CAPTURE_CONFIG.HLAE_FFMPEG_DIVERGED(s.target, result.app_ffmpeg);
      } else {
        statusEl.textContent = STRINGS.CAPTURE_CONFIG.HLAE_FFMPEG_LINKED(s.target);
      }
      break;
    case 'missing':
      statusEl.textContent = result.app_ffmpeg_problem
        ? STRINGS.CAPTURE_CONFIG.HLAE_FFMPEG_BAD_OVERRIDE(result.app_ffmpeg_problem)
        : STRINGS.CAPTURE_CONFIG.HLAE_FFMPEG_MISSING;
      break;
    default:
      return unknown();
  }
  // Offered only where it can actually be acted on: never over a bundled
  // binary, and never over an existing ini, which is left alone on purpose.
  linkBtn.style.display = result?.can_link ? '' : 'none';
}

/**
 * Wires Configuration's path, capture-mode, OBS and HLAE FFmpeg controls.
 * @param {object} app  persistAppSettings() from settings_sync.js
 */
export function initConfigWiring({ persistAppSettings }) {
  const hlaeBrowseBtn = document.querySelector('#hlae-browse-btn');
  if (hlaeBrowseBtn) {
    hlaeBrowseBtn.addEventListener('click', async () => {
      try {
        const selected = await open({
          multiple: false,
          filters: [{ name: STRINGS.MAIN.EXECUTABLE_FILTER_NAME, extensions: ['exe'] }],
          title: STRINGS.MAIN.SELECT_HLAE_EXE_TITLE
        });
        if (selected) {
          const path = Array.isArray(selected) ? selected[0] : selected;
          const inputEl = document.querySelector('#hlae-path-input');
          if (inputEl) inputEl.value = path;
          await persistAppSettings();
          await refreshHlaeFfmpegStatus();
          await refreshPathWarnings();
        }
      } catch (err) {
        console.error("Error selecting HLAE executable:", err);
      }
    });
  }

  // Typing a path by hand is the other way in, so re-check on blur/Enter as
  // well as after the picker. The FFmpeg override matters too: it does not
  // change what HLAE points at, which is exactly why a change there can leave
  // the two pointed at different builds without anything saying so.
  for (const id of ['#hlae-path-input', '#ffmpeg-override-path-input']) {
    document.querySelector(id)
      ?.addEventListener('change', () => { refreshHlaeFfmpegStatus(); });
  }
  // Every path field, including Half-Life, which the row above says nothing
  // about.
  //
  // These also persist on change (blur/Enter), not only via their Browse
  // buttons and the save-on-close in onCloseRequested. A path typed by hand
  // into a field whose picker was never used survived only until the app next
  // exited *cleanly* — and during development the app is routinely killed and
  // rebuilt instead, which skips onCloseRequested entirely. The GoldSrc Hooks
  // DLL path is the field where that bites hardest: losing it silently
  // un-injects the companion DLL on the next run (see build_hlae_process).
  for (const [input] of PATH_FIELDS) {
    document.querySelector(input)
      ?.addEventListener('change', () => { refreshPathWarnings(); persistAppSettings(); });
  }
  refreshPathWarnings();

  // Toggling capture-to-video changes what the row above needs to say: with it
  // on, "HLAE has no FFmpeg" stops being a note about an unused feature and
  // becomes the reason the next batch will record nothing.
  document.querySelector('#config-ffmpeg-capture')
    ?.addEventListener('change', () => {
      applyCaptureModeUI();
      refreshHlaeFfmpegStatus();
    });
  // The mode selector is the authority. It keeps the legacy checkbox in step
  // and then fires its 'change', so persistence and the HLAE FFmpeg status row
  // — both of which already hang off that event — keep working unchanged.
  document.querySelector('#config-capture-mode')
    ?.addEventListener('change', () => {
      applyCaptureModeUI();
      const legacy = document.querySelector('#config-ffmpeg-capture');
      if (legacy) legacy.dispatchEvent(new Event('change', { bubbles: true }));
      // Deliberately no auto-check here: OBS is not expected to already be
      // open just because the user switched into OBS mode, and warning about
      // that on every switch was pure noise. Test Connection covers the
      // manual case; Start Capture Batch runs the real pre-flight (and
      // launches/retries OBS itself) right before it matters.
    });
  applyCaptureModeUI();

  document.querySelector('#obs-test-btn')?.addEventListener('click', () => runObsConnectionTest());

  const obsBrowseBtn = document.querySelector('#obs-browse-btn');
  if (obsBrowseBtn) {
    obsBrowseBtn.addEventListener('click', async () => {
      try {
        const selected = await open({
          multiple: false,
          filters: [{ name: STRINGS.MAIN.EXECUTABLE_FILTER_NAME, extensions: ['exe'] }],
          title: STRINGS.MAIN.SELECT_OBS_EXE_TITLE
        });
        if (selected) {
          const path = Array.isArray(selected) ? selected[0] : selected;
          const inputEl = document.querySelector('#config-obs-exe-path');
          if (inputEl) inputEl.value = path;
          await persistAppSettings();
        }
      } catch (err) {
        console.error("Error selecting OBS executable:", err);
      }
    });
  }

  const obsLaunchBtn = document.querySelector('#obs-launch-btn');
  if (obsLaunchBtn) {
    obsLaunchBtn.addEventListener('click', async () => {
      const label = obsLaunchBtn.textContent;
      obsLaunchBtn.disabled = true;
      obsLaunchBtn.textContent = STRINGS.CAPTURE_CONFIG.OBS_LAUNCHING;
      try {
        await launchObs();
      } catch (err) {
        showToast(STRINGS.CAPTURE_CONFIG.obsLaunchFailed(err), 'error');
      } finally {
        obsLaunchBtn.disabled = false;
        obsLaunchBtn.textContent = label;
      }
    });
  }

  const hlaeFfmpegLinkBtn = document.querySelector('#hlae-ffmpeg-link-btn');
  if (hlaeFfmpegLinkBtn) {
    hlaeFfmpegLinkBtn.addEventListener('click', async () => {
      const hlaePath = document.querySelector('#hlae-path-input')?.value?.trim() || "";
      // Whatever Render Studio was told to use, so both halves of the pipeline
      // encode with the same build. Empty means "system ffmpeg", which the
      // backend resolves to an absolute path — HLAE's ini cannot take a bare
      // command name.
      const ffmpegPath =
        document.querySelector('#ffmpeg-override-path-input')?.value?.trim() || "ffmpeg";
      hlaeFfmpegLinkBtn.disabled = true;
      try {
        let result = await linkHlaeFfmpeg(hlaePath, ffmpegPath);

        // HLAE can live anywhere — zip or installer — so a protected location
        // like Program Files is a real possibility rather than a rare one. Ask
        // before raising the UAC prompt, so the prompt is never a surprise, and
        // say what it is for.
        if (result?.needs_elevation) {
          const agreed = await confirm(
            STRINGS.CAPTURE_CONFIG.HLAE_FFMPEG_ELEVATE_PROMPT(result.ini),
            {
              title: STRINGS.CAPTURE_CONFIG.HLAE_FFMPEG_ELEVATE_TITLE,
              okLabel: STRINGS.CAPTURE_CONFIG.HLAE_FFMPEG_ELEVATE_CONFIRM
            }
          );
          // Say so rather than going quiet. Declining is a choice, but a button
          // that does nothing visible reads as a button that failed.
          if (!agreed) {
            showToast(STRINGS.CAPTURE_CONFIG.HLAE_FFMPEG_ELEVATE_REFUSED, 'info');
            return;
          }
          result = await linkHlaeFfmpeg(hlaePath, ffmpegPath, true);
        }

        if (result?.ini && !result.needs_elevation) {
          showToast(STRINGS.CAPTURE_CONFIG.HLAE_FFMPEG_LINKED_OK(result.ini), 'success');
        }
      } catch {
        // The bridge already toasted the reason, which for a refusal is the
        // point — an existing ini is reported, never replaced.
      } finally {
        hlaeFfmpegLinkBtn.disabled = false;
        await refreshHlaeFfmpegStatus();
      }
    });
  }

  const hlBrowseBtn = document.querySelector('#hl-browse-btn');
  if (hlBrowseBtn) {
    hlBrowseBtn.addEventListener('click', async () => {
      try {
        const selected = await open({
          multiple: false,
          filters: [{ name: STRINGS.MAIN.EXECUTABLE_FILTER_NAME, extensions: ['exe'] }],
          title: STRINGS.MAIN.SELECT_HL_EXE_TITLE
        });
        if (selected) {
          const path = Array.isArray(selected) ? selected[0] : selected;
          const inputEl = document.querySelector('#hl-path-input');
          if (inputEl) inputEl.value = path;
          await persistAppSettings();
          await refreshPathWarnings();
        }
      } catch (err) {
        console.error("Error selecting Half-Life executable:", err);
      }
    });
  }

  const ffmpegBrowseBtn = document.querySelector('#ffmpeg-browse-btn');
  if (ffmpegBrowseBtn) {
    ffmpegBrowseBtn.addEventListener('click', async () => {
      try {
        const selected = await open({
          multiple: false,
          filters: [{ name: STRINGS.MAIN.EXECUTABLE_FILTER_NAME, extensions: ['exe'] }],
          title: STRINGS.MAIN.SELECT_FFMPEG_EXE_TITLE
        });
        if (selected) {
          const path = Array.isArray(selected) ? selected[0] : selected;
          const inputEl = document.querySelector('#ffmpeg-override-path-input');
          if (inputEl) inputEl.value = path;
          await persistAppSettings();
          // Picking a different FFmpeg does not move what HLAE points at, which
          // is precisely why the row below has to be re-checked: that is how
          // the two end up on different builds without anything saying so.
          await refreshHlaeFfmpegStatus();
          await refreshPathWarnings();
        }
      } catch (err) {
        console.error("Error selecting FFmpeg executable:", err);
      }
    });
  }

  const goldsrcHooksBrowseBtn = document.querySelector('#goldsrc-hooks-browse-btn');
  if (goldsrcHooksBrowseBtn) {
    goldsrcHooksBrowseBtn.addEventListener('click', async () => {
      try {
        const selected = await open({
          multiple: false,
          filters: [{ name: STRINGS.MAIN.DLL_FILTER_NAME, extensions: ['dll'] }],
          title: STRINGS.MAIN.SELECT_GOLDSRC_HOOKS_DLL_TITLE
        });
        if (selected) {
          const path = Array.isArray(selected) ? selected[0] : selected;
          const inputEl = document.querySelector('#goldsrc-hooks-dll-path-input');
          if (inputEl) inputEl.value = path;
          await persistAppSettings();
        }
      } catch (err) {
        console.error("Error selecting dodstudio_goldsrc_hooks.dll:", err);
      }
    });
  }
}
