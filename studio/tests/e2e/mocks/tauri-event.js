// Test-only stand-in for `@tauri-apps/api/event`, paired with `tauri-core.js`.
//
// A test simulates the backend emitting an event by calling
// `window.__mockEmit(eventName, payload)` (exposed below) from
// `page.evaluate`, which invokes every listener registered for that name in
// the same shape the real `listen()` callback receives: `{ event, payload }`.
window.__mockEventListeners = window.__mockEventListeners || {};

export function listen(eventName, callback) {
  (window.__mockEventListeners[eventName] ||= []).push(callback);
  return Promise.resolve(() => {
    const listeners = window.__mockEventListeners[eventName];
    const idx = listeners ? listeners.indexOf(callback) : -1;
    if (idx !== -1) listeners.splice(idx, 1);
  });
}

window.__mockEmit = function __mockEmit(eventName, payload) {
  const listeners = window.__mockEventListeners[eventName] || [];
  // Snapshot before iterating: a listener could register/unregister another
  // listener for the same event from inside its own callback.
  [...listeners].forEach((cb) => cb({ event: eventName, payload }));
};
