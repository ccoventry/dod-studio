// analyzer_protocol47.js
// A protocol-47 demo (2004 and a little after, #239): the Demo Analyzer
// reads it like any other, but today's game refuses to play it ("demo
// protocol outdated"). The notice above every tab says so and offers
// Convert, which writes a protocol-48 copy beside it as <name>_p48.dem
// (demo_convert.rs over native::patch::protocol47), lists it in the Explorer
// when it shows that folder, and opens it. The original is only read.

import { convertProtocol47 } from './ipc_bridge.js';
import { STRINGS } from './strings.js';
import { escapeHtml as esc } from './html.js';
import { fileNameOf } from './path_display.js';
import { showToast } from './toast.js';

/** The network protocol today's game refuses until the demo is converted. */
export const OLD_PROTOCOL = 47;

/** The notice for a protocol-47 demo's report, or null for any other. Pure. */
export function protocol47Notice(report) {
  return report?.demo_info?.network_protocol === OLD_PROTOCOL ? STRINGS.PROTOCOL_47.NOTICE : null;
}

/**
 * Fills `el` with the notice for `report`, or hides it. With `onConvert`, the
 * notice has a Convert button that runs `onConvert()`: the button is disabled
 * and the banner says it is converting until it resolves the line to show,
 * or throws, when the banner says why.
 */
export function renderProtocol47Banner(el, report, onConvert) {
  if (!el) return;
  const notice = protocol47Notice(report);
  el.hidden = !notice;
  el.dataset.path = report?.file_path || '';
  if (!notice || !onConvert) {
    el.textContent = notice || '';
    return;
  }
  const P = STRINGS.PROTOCOL_47;
  el.innerHTML = `<span class="analyzer-protocol47-text">${esc(notice)}</span>
    <button type="button" class="analyzer-protocol47-convert primary-btn" title="${esc(P.BUTTON_TITLE)}">${esc(P.BUTTON)}</button>
    <span class="analyzer-protocol47-status"></span>`;
  const button = el.querySelector('button');
  const status = el.querySelector('.analyzer-protocol47-status');
  button.addEventListener('click', async () => {
    button.disabled = true;
    status.textContent = P.CONVERTING;
    try {
      status.textContent = (await onConvert()) || '';
    } catch (err) {
      status.textContent = P.failed(err);
    } finally {
      button.disabled = false;
    }
  });
}

/**
 * Converts the demo at `path`, then lists the copy (`relistFolderOf(path)`)
 * and opens it (`open(copy)`) when `isShown()` says the Analyzer still shows
 * that demo: one opened while the conversion ran stays open. Resolves the
 * line the notice shows after; throws when the conversion fails.
 */
export async function convertAnalyzedDemo(path, { relistFolderOf, open, isShown }) {
  const written = await convertProtocol47(path);
  const text = STRINGS.PROTOCOL_47.done(fileNameOf(written));
  showToast(text, 'success', 6000);
  await relistFolderOf(path);
  if (isShown()) await open(written);
  return text;
}
