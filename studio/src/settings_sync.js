// settings_sync.js
// The app settings file and the Configuration form: what persistAppSettings
// saves (read from the form and main.js's lists), and the form half of the
// startup load. Moved out of main.js (#683), which keeps the lists themselves
// (pinned folders, capture drives, ...) and hands them over through getState().

import { saveSettings } from './ipc_bridge.js';
import { getCommandsState, renderTimingDiagram } from './capture_pane.js';
import { getDemoRenameTemplates, setDemoRenameTemplates } from './demo_rename_ui.js';
import { updateNotificationSettings } from './os_notifications.js';
import { numberField } from './number_field.js';
import { setClipNameTemplate, getClipNameTemplate } from './clip_name_ui.js';
import { getCommandProfiles, getActiveCommandProfile } from './command_profiles_ui.js';
import { setRenderPresets, getRenderPresets } from './render_presets_ui.js';

/**
 * @param {object} app  What this needs from main.js: getState() the lists the
 *   form doesn't hold ({ scanPaths, demoFolderHistory, scanFoldersForDemos,
 *   analyzerExplorerWidth, targetDrives, renderExportDirs, scanWorkers }),
 *   and currentCaptureMode().
 */
export function createSettingsSync({ getState, currentCaptureMode }) {
  async function persistAppSettings() {
    const state = getState();
    const hlaePath = document.querySelector('#hlae-path-input')?.value?.trim() || "";
    const hlPath = document.querySelector('#hl-path-input')?.value?.trim() || "";
    const ffmpegPath = document.querySelector('#ffmpeg-override-path-input')?.value?.trim() || null;
    const goldsrcHooksDllPath = document.querySelector('#goldsrc-hooks-dll-path-input')?.value?.trim() || null;
    const captureFps = numberField('#config-capture-fps', 300, { integer: true, positive: true });
    const obsCaptureFps = numberField('#config-obs-capture-fps', 120, { integer: true, positive: true });
    // 0 = empty = "the same as Capture FPS" (PatcherConfig::effective_agr_fps).
    const agrFps = numberField('#config-agr-fps', 0, { integer: true, positive: true });
    const preRoll = numberField('#config-pre-roll', 2.0);
    const postRoll = numberField('#config-post-roll', 0.6);

    const resWidth = numberField('#config-res-width', 1280, { integer: true, positive: true });
    const resHeight = numberField('#config-res-height', 720, { integer: true, positive: true });
    // Defaults on when the element is missing, matching the backend default —
    // `?? true` rather than `|| false`, which would silently disable it.
    const decalFlush = document.querySelector('#config-decal-flush')?.checked ?? true;
    const captureMode = currentCaptureMode();
    // Derived from the mode, never read from the checkbox — the select is the
    // authority and the checkbox is a compatibility mirror.
    const ffmpegCapture = captureMode === 'direct_to_video';
    const ffmpegCaptureCodec = document.querySelector('#config-capture-codec')?.value || 'utvideo';
    const obsHost = document.querySelector('#config-obs-host')?.value?.trim() || '127.0.0.1';
    const obsPort = parseInt(document.querySelector('#config-obs-port')?.value, 10) || 4455;
    const obsPassword = document.querySelector('#config-obs-password')?.value || '';
    const obsExePath = document.querySelector('#config-obs-exe-path')?.value?.trim() || '';

    const autoClearLogs = document.querySelector('#config-auto-clear-logs')?.checked || false;
    const autoClearPreviews = document.querySelector('#config-auto-clear-previews')?.checked || false;
    const autoClearTempDemos = document.querySelector('#config-auto-clear-temp-demos')?.checked || false;

    // OS notification toggles (issue #98) — default on, matching decalFlush's
    // `?? true` style above rather than `|| false`, which would silently
    // disable them for anyone whose settings predate this field.
    const notifyPatching = document.querySelector('#config-notify-patching')?.checked ?? true;
    const notifyDemoLoading = document.querySelector('#config-notify-demo-loading')?.checked ?? true;
    const notifyBetweenClips = document.querySelector('#config-notify-between-clips')?.checked ?? true;
    const notifyCapturesDone = document.querySelector('#config-notify-captures-done')?.checked ?? true;
    const notifyRendersDone = document.querySelector('#config-notify-renders-done')?.checked ?? true;
    const notifyError = document.querySelector('#config-notify-error')?.checked ?? true;
    const notifyUpdates = document.querySelector('#config-notify-updates')?.checked ?? true;
    const updateChannel = document.querySelector('#config-update-channel')?.value || 'stable';
    const autoCheckUpdates = document.querySelector('#config-auto-check-updates')?.checked ?? true;

    const recordStartLead = numberField('#config-record-start-lead', 0.0);
    const recordStopTrail = numberField('#config-record-stop-trail', 0.0);
    const initialDelay = numberField('#config-initial-delay', 3.0);
    const fastForwardSpeed = numberField('#config-fast-forward-speed', 0.05, { positive: true });

    const saveLocalPatchedCopy = document.querySelector('#config-save-local-patched')?.checked || false;

    const renderCodec = document.querySelector('#render-codec-select')?.value || 'prores';
    const renderCustomCodecArgs = document.querySelector('#render-custom-codec-input')?.value || '';
    const renderFps = parseInt(document.querySelector('#render-fps-input')?.value, 10) || 300;
    const renderMaxConcurrent = parseInt(document.querySelector('#render-max-concurrent-input')?.value, 10) || 2;
    const scanWorkers = state.scanWorkers;
    // "When a batch finishes" (#440). Off unless the select says otherwise.
    const finishClipsAfterBatch = document.querySelector('#config-finish-clips')?.value === 'finish';
    const finishCodecObs = document.querySelector('#config-finish-codec-obs')?.value || 'source_copy';
    const finishCodecVideo = document.querySelector('#config-finish-codec-video')?.value || 'render_tab';
    const finishCodecFrames = document.querySelector('#config-finish-codec-frames')?.value || 'render_tab';

    const { init_commands, custom_commands } = getCommandsState();

    const settingsPayload = {
      hlae_path: hlaePath,
      hl_path: hlPath,
      ffmpeg_path: ffmpegPath,
      goldsrc_hooks_dll_path: goldsrcHooksDllPath,
      pinned_folders: state.scanPaths,
      demo_folder_history: state.demoFolderHistory,
      scan_folders_for_demos: state.scanFoldersForDemos,
      analyzer_explorer_width: state.analyzerExplorerWidth,
      language: "en",
      capture_fps: captureFps,
      obs_capture_fps: obsCaptureFps,
      agr_fps: agrFps,
      pre_roll_seconds: preRoll,
      post_roll_seconds: postRoll,
      resolution_width: resWidth,
      resolution_height: resHeight,
      decal_flush: decalFlush,
      ffmpeg_capture: ffmpegCapture,
      ffmpeg_capture_codec: ffmpegCaptureCodec,
      capture_mode: captureMode,
      obs_host: obsHost,
      obs_port: obsPort,
      obs_password: obsPassword,
      obs_exe_path: obsExePath,
      auto_clear_logs: autoClearLogs,
      auto_clear_previews: autoClearPreviews,
      auto_clear_temp_demos: autoClearTempDemos,
      notify_patching: notifyPatching,
      notify_demo_loading: notifyDemoLoading,
      notify_between_clips: notifyBetweenClips,
      notify_captures_done: notifyCapturesDone,
      notify_renders_done: notifyRendersDone,
      notify_error: notifyError,
      notify_updates: notifyUpdates,
      update_channel: updateChannel,
      auto_check_updates: autoCheckUpdates,
      clip_name_template: getClipNameTemplate(),
      demo_rename_pov_template: getDemoRenameTemplates().pov,
      demo_rename_hltv_template: getDemoRenameTemplates().hltv,
      demo_rename_lowercase: getDemoRenameTemplates().lowercase,
      record_start_lead: recordStartLead,
      record_stop_trail: recordStopTrail,
      initial_delay: initialDelay,
      fast_forward_speed: fastForwardSpeed,
      target_drives: state.targetDrives,
      init_commands,
      custom_commands,
      command_profiles: getCommandProfiles(),
      command_profile_active: getActiveCommandProfile(),
      save_local_patched_copy: saveLocalPatchedCopy,
      render_codec: renderCodec,
      render_custom_codec_args: renderCustomCodecArgs,
      render_fps: renderFps,
      render_max_concurrent: renderMaxConcurrent,
      render_presets: getRenderPresets(),
      scan_workers: scanWorkers,
      render_export_dirs: state.renderExportDirs,
      finish_clips_after_batch: finishClipsAfterBatch,
      finish_codec_obs: finishCodecObs,
      finish_codec_video: finishCodecVideo,
      finish_codec_frames: finishCodecFrames
    };
    // Reflects a just-flipped toggle immediately, rather than waiting on the
    // save round-trip below to come back through a settings reload.
    updateNotificationSettings(settingsPayload);
    try {
      await saveSettings(settingsPayload);
    } catch (err) {
      console.error("Error auto-saving settings:", err);
    }
  }

  return { persistAppSettings };
}

/**
 * Fills the Configuration form from a loaded settings file. main.js applies
 * the lists and command state afterwards.
 * @param {object} settings  get_settings' answer
 * @param {object} app  refreshHlaeFfmpegStatus() and applyCaptureModeUI()
 *   from main.js
 */
export function applySettingsToForm(settings, { refreshHlaeFfmpegStatus, applyCaptureModeUI }) {
  if (settings.hlae_path) {
    const inputEl = document.querySelector('#hlae-path-input');
    if (inputEl) inputEl.value = settings.hlae_path;
  }
  if (settings.hl_path) {
    const inputEl = document.querySelector('#hl-path-input');
    if (inputEl) inputEl.value = settings.hl_path;
    // The game folder is only known once the persisted path lands here, so
    // this is where the config scan can first say anything useful.
    // Deferred: the init commands hydrate later in this same load, and the
    // warning is about how those two interact.
  }
  if (settings.ffmpeg_path) {
    const inputEl = document.querySelector('#ffmpeg-override-path-input');
    if (inputEl) inputEl.value = settings.ffmpeg_path;
  }
  if (settings.goldsrc_hooks_dll_path) {
    const inputEl = document.querySelector('#goldsrc-hooks-dll-path-input');
    if (inputEl) inputEl.value = settings.goldsrc_hooks_dll_path;
  }
  if (settings.hlae_path) {
    // Issue #101: this reads both #hlae-path-input and
    // #ffmpeg-override-path-input, so it has to run after *both* are
    // populated above, not right after hlae_path alone — calling it
    // there read the override field before its own value had landed,
    // silently fell back to the bare "ffmpeg" PATH lookup, and produced
    // a false "there is no file at \"ffmpeg\"" warning that then never
    // got re-checked, since setting .value programmatically fires no
    // 'change' event to trigger main.js's listener.
    //
    // Not awaited: it is a status line, and blocking startup on a
    // filesystem check of somebody else's install directory would trade a
    // real cost for a cosmetic one.
    refreshHlaeFfmpegStatus();
  }
  if (settings.capture_fps) {
    const inputEl = document.querySelector('#config-capture-fps');
    if (inputEl) inputEl.value = settings.capture_fps;
  }
  if (settings.obs_capture_fps) {
    const inputEl = document.querySelector('#config-obs-capture-fps');
    if (inputEl) inputEl.value = settings.obs_capture_fps;
  }
  // 0 is "the same as Capture FPS" and stays an empty box.
  if (settings.agr_fps > 0) {
    const inputEl = document.querySelector('#config-agr-fps');
    if (inputEl) inputEl.value = settings.agr_fps;
  }
  // `!= null`, not truthiness: 0 is a real value for the five timing
  // fields, and a truthy check skipped restoring it.
  if (settings.pre_roll_seconds != null) {
    const inputEl = document.querySelector('#config-pre-roll');
    if (inputEl) inputEl.value = settings.pre_roll_seconds;
  }
  if (settings.post_roll_seconds != null) {
    const inputEl = document.querySelector('#config-post-roll');
    if (inputEl) inputEl.value = settings.post_roll_seconds;
  }
  if (settings.resolution_width) {
    const inputEl = document.querySelector('#config-res-width');
    if (inputEl) inputEl.value = settings.resolution_width;
  }
  if (settings.resolution_height) {
    const inputEl = document.querySelector('#config-res-height');
    if (inputEl) inputEl.value = settings.resolution_height;
  }
  const decalFlushEl = document.querySelector('#config-decal-flush');
  if (decalFlushEl) decalFlushEl.checked = settings.decal_flush !== false;
  const ffmpegCaptureEl = document.querySelector('#config-ffmpeg-capture');
  if (ffmpegCaptureEl) ffmpegCaptureEl.checked = !!settings.ffmpeg_capture;
  const captureCodecEl = document.querySelector('#config-capture-codec');
  if (captureCodecEl && settings.ffmpeg_capture_codec) captureCodecEl.value = settings.ffmpeg_capture_codec;
  const captureModeEl = document.querySelector('#config-capture-mode');
  if (captureModeEl) {
    // A settings file written before the selector existed has no
    // capture_mode and only ffmpeg_capture, so fall back to it rather than
    // silently resetting the user's choice to frame sequence.
    captureModeEl.value =
      settings.capture_mode || (settings.ffmpeg_capture ? 'direct_to_video' : 'frame_sequence');
  }
  const obsHostEl = document.querySelector('#config-obs-host');
  if (obsHostEl) obsHostEl.value = settings.obs_host || '127.0.0.1';
  const obsPortEl = document.querySelector('#config-obs-port');
  if (obsPortEl) obsPortEl.value = settings.obs_port || 4455;
  const obsPasswordEl = document.querySelector('#config-obs-password');
  if (obsPasswordEl) obsPasswordEl.value = settings.obs_password || '';
  const obsExePathEl = document.querySelector('#config-obs-exe-path');
  if (obsExePathEl) obsExePathEl.value = settings.obs_exe_path || '';
  applyCaptureModeUI();
  // Deliberately NOT checked here at startup — OBS is the user's own
  // program and, like HLAE, is not expected to already be running just
  // because dod-studio opened. The connectivity check runs when actively
  // switching into OBS mode below, and again as Start Capture Batch's
  // own pre-flight (capture_pane.js) — both are moments the user is
  // actually about to use it, unlike app launch.
  const autoClearLogsEl = document.querySelector('#config-auto-clear-logs');
  if (autoClearLogsEl) autoClearLogsEl.checked = !!settings.auto_clear_logs;
  const autoClearPreviewsEl = document.querySelector('#config-auto-clear-previews');
  if (autoClearPreviewsEl) autoClearPreviewsEl.checked = !!settings.auto_clear_previews;
  const autoClearTempDemosEl = document.querySelector('#config-auto-clear-temp-demos');
  if (autoClearTempDemosEl) autoClearTempDemosEl.checked = !!settings.auto_clear_temp_demos;
  const notifyPatchingEl = document.querySelector('#config-notify-patching');
  if (notifyPatchingEl) notifyPatchingEl.checked = !!settings.notify_patching;
  const notifyDemoLoadingEl = document.querySelector('#config-notify-demo-loading');
  if (notifyDemoLoadingEl) notifyDemoLoadingEl.checked = !!settings.notify_demo_loading;
  const notifyBetweenClipsEl = document.querySelector('#config-notify-between-clips');
  if (notifyBetweenClipsEl) notifyBetweenClipsEl.checked = !!settings.notify_between_clips;
  const notifyCapturesDoneEl = document.querySelector('#config-notify-captures-done');
  if (notifyCapturesDoneEl) notifyCapturesDoneEl.checked = !!settings.notify_captures_done;
  const notifyRendersDoneEl = document.querySelector('#config-notify-renders-done');
  if (notifyRendersDoneEl) notifyRendersDoneEl.checked = !!settings.notify_renders_done;
  const notifyErrorEl = document.querySelector('#config-notify-error');
  if (notifyErrorEl) notifyErrorEl.checked = !!settings.notify_error;
  const notifyUpdatesEl = document.querySelector('#config-notify-updates');
  if (notifyUpdatesEl) notifyUpdatesEl.checked = settings.notify_updates !== false;
  const updateChannelEl = document.querySelector('#config-update-channel');
  if (updateChannelEl) updateChannelEl.value = settings.update_channel || 'stable';
  const autoCheckUpdatesEl = document.querySelector('#config-auto-check-updates');
  if (autoCheckUpdatesEl) autoCheckUpdatesEl.checked = settings.auto_check_updates !== false;
  setClipNameTemplate(settings.clip_name_template);
  setDemoRenameTemplates(
    settings.demo_rename_pov_template, settings.demo_rename_hltv_template, settings.demo_rename_lowercase,
  );
  if (settings.record_start_lead != null) {
    const inputEl = document.querySelector('#config-record-start-lead');
    if (inputEl) inputEl.value = settings.record_start_lead;
  }
  if (settings.record_stop_trail != null) {
    const inputEl = document.querySelector('#config-record-stop-trail');
    if (inputEl) inputEl.value = settings.record_stop_trail;
  }
  if (settings.initial_delay != null) {
    const inputEl = document.querySelector('#config-initial-delay');
    if (inputEl) inputEl.value = settings.initial_delay;
  }
  // All five timing fields are set by this point — reflect the loaded
  // values in the Timings tab's visual timeline (#150).
  renderTimingDiagram();
  if (settings.fast_forward_speed) {
    const inputEl = document.querySelector('#config-fast-forward-speed');
    if (inputEl) inputEl.value = settings.fast_forward_speed;
  }
  const saveLocalPatchedEl = document.querySelector('#config-save-local-patched');
  if (saveLocalPatchedEl) saveLocalPatchedEl.checked = !!settings.save_local_patched_copy;
  if (settings.render_codec) {
    const inputEl = document.querySelector('#render-codec-select');
    if (inputEl) inputEl.value = settings.render_codec;
  }
  if (settings.render_custom_codec_args) {
    const inputEl = document.querySelector('#render-custom-codec-input');
    if (inputEl) inputEl.value = settings.render_custom_codec_args;
  }
  if (settings.render_fps) {
    const inputEl = document.querySelector('#render-fps-input');
    if (inputEl) inputEl.value = settings.render_fps;
  }
  if (settings.render_max_concurrent) {
    const inputEl = document.querySelector('#render-max-concurrent-input');
    if (inputEl) inputEl.value = settings.render_max_concurrent;
  }
  setRenderPresets(settings.render_presets);
  if (settings.scan_workers) {
    const inputEl = document.querySelector('#config-scan-workers');
    if (inputEl) inputEl.value = settings.scan_workers;
  }
  const finishClipsEl = document.querySelector('#config-finish-clips');
  if (finishClipsEl) finishClipsEl.value = settings.finish_clips_after_batch ? 'finish' : 'off';
  [['#config-finish-codec-obs', settings.finish_codec_obs],
   ['#config-finish-codec-video', settings.finish_codec_video],
   ['#config-finish-codec-frames', settings.finish_codec_frames]].forEach(([sel, value]) => {
    const el = document.querySelector(sel);
    // Only a value the select offers — assigning an unknown one blanks it.
    if (el && value && [...el.options].some((o) => o.value === value)) el.value = value;
  });
}
