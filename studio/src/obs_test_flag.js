// obs_test_flag.js
// Whether the batch running now is an OBS capture test's clip (#646). Its
// own module so capture_pane.js and obs_capture_test.js can both import it
// without importing each other. While it is set, the capture pane still
// shows the batch's progress but skips everything that belongs to a real
// batch: clearing Last Batch, the end toasts and OS notifications.

let testBatch = false;

export function setObsTestBatch(value) {
  testBatch = value === true;
}

export function isObsTestBatch() {
  return testBatch;
}
