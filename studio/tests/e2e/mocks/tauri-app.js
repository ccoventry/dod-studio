// Test-only stand-in for `@tauri-apps/api/app`, paired with `tauri-core.js`.
export function getVersion() {
  return Promise.resolve('0.0.0-e2e');
}
