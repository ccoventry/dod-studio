// capture_summary_ui.js
// Draws the capture summary strip (#443) in the Capture footer and makes
// each part open its setting.

import { summaryParts } from './capture_summary.js';
import { setCaptureDetailSubtab } from './nav.js';
import { STRINGS } from './strings.js';

let getSetup = () => null;

/** Opens Configuration on `tab` and focuses `field` there. */
function openSetting(tab, field) {
  setCaptureDetailSubtab('configuration');
  document.querySelector(`.config-tab-btn[data-tab="${tab}"]`)?.click();
  const el = document.querySelector(field);
  if (el) {
    el.focus();
    el.scrollIntoView?.({ block: 'nearest' });
  }
}

/** Redraws the strip from the current settings. Cheap: call on any change. */
export function renderCaptureSummary() {
  const strip = document.querySelector('#capture-summary-strip');
  const setup = getSetup();
  if (!strip || !setup) return;
  const nodes = [];
  summaryParts(setup).forEach((part, i) => {
    if (i > 0) nodes.push(document.createTextNode(' · '));
    const link = document.createElement('a');
    link.href = '#';
    link.className = 'capture-summary-part';
    link.dataset.part = part.key;
    link.textContent = part.text;
    link.title = STRINGS.CAPTURE_SUMMARY.LINK_TITLE;
    if (part.blocking) link.style.color = '#f44336';
    link.addEventListener('click', (e) => {
      e.preventDefault();
      openSetting(part.tab, part.field);
    });
    nodes.push(link);
  });
  strip.replaceChildren(...nodes);
}

/**
 * `getCurrentSetup()` returns `summaryParts`' input from the live settings.
 * Redraws whenever anything in Configuration changes.
 */
export function initCaptureSummary(getCurrentSetup) {
  getSetup = getCurrentSetup;
  const panel = document.querySelector('#export-config-panel');
  panel?.addEventListener('input', renderCaptureSummary);
  panel?.addEventListener('change', renderCaptureSummary);
  renderCaptureSummary();
}
