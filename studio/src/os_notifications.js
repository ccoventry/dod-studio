import { isPermissionGranted, requestPermission, sendNotification } from '@tauri-apps/plugin-notification';

// Windows toasts default to silent when `sound` is omitted from the options
// (tauri-plugin-notification -> notify-rust -> winrt-notification's
// Toast::sound(None) emits <audio silent="true" />), so notify() never sets
// `sound` — no explicit silencing needed. See issue #98.
let permissionGranted = false;

// Per-stage on/off, mirroring AppSettings.notify_* — defaults match the Rust
// side's serde defaults (all true) so a notify() call before settings have
// loaded still fires rather than silently dropping.
let enabledFlags = {
  patching: true,
  demo_loading: true,
  between_clips: true,
  captures_done: true,
  renders_done: true,
  error: true,
  updates: true,
};

export async function initOsNotifications() {
  permissionGranted = await isPermissionGranted();
  if (!permissionGranted) {
    const permission = await requestPermission();
    permissionGranted = permission === 'granted';
  }
  return permissionGranted;
}

// Called once after every settings load/save so a toggle flip takes effect
// immediately, with no restart needed.
export function updateNotificationSettings(settings) {
  if (!settings) return;
  enabledFlags = {
    patching: settings.notify_patching !== false,
    demo_loading: settings.notify_demo_loading !== false,
    between_clips: settings.notify_between_clips !== false,
    captures_done: settings.notify_captures_done !== false,
    renders_done: settings.notify_renders_done !== false,
    error: settings.notify_error !== false,
    updates: settings.notify_updates !== false,
  };
}

// Lets a listener check another notification kind's toggle before deciding
// whether to fire its own — e.g. the demo-loading toast skips itself when
// between-clips notifications are also on, since the upcoming "fast-forwarding
// to clip 1" toast covers the same ground with more detail.
export function isNotificationEnabled(kind) {
  return !!enabledFlags[kind];
}

export function notify(kind, title, body) {
  if (!permissionGranted || !enabledFlags[kind]) return;
  sendNotification({ title, body });
}
