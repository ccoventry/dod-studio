// scan_workers.js
// Configuration's demo scan workers field (#246): 1..8, default 2
// (SCAN_CONCURRENCY). Each worker holds a whole analysis, so the hint puts the
// machine's memory next to the number. Deliberately no clamp to the machine's
// RAM. Kept out of main.js, which only wires (#683).

import { systemMemoryBytes } from './ipc_bridge.js';
import { numberField } from './number_field.js';
import { STRINGS } from './strings.js';
import { BYTES_PER_GB } from './byte_size.js';

/** The field's current value, clamped to 1..8. */
export function readScanWorkers() {
  return Math.min(8, Math.max(1, numberField('#config-scan-workers', 2, { integer: true, positive: true })));
}

/** Fills the memory hint and saves a settled change. `persist()` saves the settings file. */
export function startScanWorkers({ persist }) {
  let totalMemoryGb = null;
  const updateHint = () => {
    const hint = document.querySelector('#config-scan-workers-hint');
    if (hint) hint.textContent = STRINGS.CAPTURE_CONFIG.scanWorkersHint(totalMemoryGb);
  };
  systemMemoryBytes().then((bytes) => {
    if (bytes) totalMemoryGb = Math.round(bytes / BYTES_PER_GB);
    updateHint();
  });
  const input = document.querySelector('#config-scan-workers');
  input?.addEventListener('change', () => {
    input.value = readScanWorkers();
    persist();
  });
}
