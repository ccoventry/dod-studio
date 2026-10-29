// setup_card.js
// The Highlights page's setup checklist (#599): "Finish setting up, 3 of 6",
// one row per step with its fix button. Shown while something Start Capture
// Batch needs is missing, gone once it is all set. Draws from a snapshot and
// calls back for every action, so it does no IPC of its own
// (setup_wiring.js supplies both).
import { STRINGS } from './strings.js';
import { escapeHtml } from './html.js';
import { driveLabel, formatBytes } from './setup_checklist.js';

/** The one-line summary of a step that is done. */
export function doneSummary(key, snapshot) {
  const S = STRINGS.SETUP;
  switch (key) {
    case 'hl': return snapshot.hlPath || '';
    case 'hlae': return snapshot.hlaePath || '';
    case 'ffmpeg': return S.FFMPEG_FOUND;
    case 'captures':
    case 'renders': {
      const list = (key === 'captures' ? snapshot.targetDrives : snapshot.renderExportDirs) || [];
      const spaces = list
        .filter((f) => typeof f.freeBytes === 'number' && f.freeBytes > 0)
        .map((f) => S.freeOn(formatBytes(f.freeBytes), driveLabel(f.path)));
      return [S.foldersSummary(list.length), ...spaces].join(' · ');
    }
    case 'demos':
      return snapshot.demoCount > 0
        ? S.demosSummary(snapshot.demoCount)
        : S.foldersSummary((snapshot.pinnedFolders || []).length);
    default: return '';
  }
}

/**
 * Draws the card into `container`, or hides it once every required step is
 * done.
 * @param {HTMLElement} container  #setup-card
 * @param {{ checklist: object, snapshot: object }} view
 * @param {{ fix: (key: string) => void, open: (key: string) => void }} actions
 *   `fix` runs a row's button on a step not done yet (a picker, mostly);
 *   `open` opens the Setup window on that step.
 * @returns {boolean} whether the card is showing
 */
export function renderSetupCard(container, { checklist, snapshot }, actions) {
  if (!container) return false;
  if (checklist.allRequiredDone) {
    container.hidden = true;
    container.innerHTML = '';
    return false;
  }
  const S = STRINGS.SETUP;
  const pct = Math.round((checklist.doneCount / checklist.total) * 100);
  const rows = checklist.steps.map((step) => {
    const tick = step.done ? 'ok' : (step.required ? 'todo' : 'opt');
    const detail = step.done ? doneSummary(step.key, snapshot) : S.CARD_TODO[step.key];
    const label = step.done ? S.CARD_CHANGE : S.CARD_FIX[step.key];
    return `<div class="setup-row${step.done ? ' done' : ''}" data-step="${step.key}">
        <span class="setup-tick ${tick}">${step.done ? '✓' : ''}</span>
        <span class="setup-row-text" title="${escapeHtml(S.CARD_ROW_TITLE)}">${escapeHtml(S.STEP_NAMES[step.key])}
          <small>${escapeHtml(detail)}</small></span>
        <button type="button" class="setup-row-btn" data-step="${step.key}" data-done="${step.done ? '1' : ''}">${escapeHtml(label)}</button>
      </div>`;
  }).join('');
  container.innerHTML = `<div class="setup-card-head">
      <h4>${escapeHtml(S.CARD_TITLE)}</h4>
      <span class="setup-chip info" title="${escapeHtml(S.cardTitleAttr(checklist.requiredDone, checklist.requiredTotal))}">${escapeHtml(S.cardProgress(checklist.doneCount, checklist.total))}</span>
      <span class="setup-meter"><i style="width: ${pct}%"></i></span>
      <button type="button" class="setup-open-btn">${escapeHtml(S.OPEN_SETUP)}</button>
    </div>${rows}`;
  container.hidden = false;

  container.querySelector('.setup-open-btn')?.addEventListener('click', () => {
    actions.open(checklist.steps[checklist.nextIndex]?.key || 'hl');
  });
  container.querySelectorAll('.setup-row-btn').forEach((btn) => {
    btn.addEventListener('click', () => {
      if (btn.dataset.done) actions.open(btn.dataset.step);
      else actions.fix(btn.dataset.step);
    });
  });
  container.querySelectorAll('.setup-row-text').forEach((el) => {
    el.addEventListener('click', () => actions.open(el.parentElement.dataset.step));
  });
  return true;
}
