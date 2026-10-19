// audit_consolidate_ui.js — Demo Auditor › Duplicates › Consolidate… (#872):
// the destination, layout and game-folder tick, the preview line, Move
// Demos with its progress and Cancel, and Undo. The pure parts are in
// audit_consolidate.js.
import { open } from '@tauri-apps/plugin-dialog';
import { listen } from '@tauri-apps/api/event';
import { themedConfirm } from './themed_confirm.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';
import {
  consolidatePlan, consolidateRun, consolidateUndo, consolidateUndoable, consolidateCancel,
} from './ipc_bridge.js';
import { consolidateRequest, hasDemos, planView, consolidateProgressView } from './audit_consolidate.js';

/**
 * @param {object} pane  What this needs from auditor_pane.js:
 *   getAudit() → { groups, singles, roots } the current results,
 *   isBusy() an audit or delete is running, setBusy(on) lock the pane's
 *   other controls, showProgress(view) / hideProgress() / setStatus(text),
 *   relocateMoved(pairs) → how many queue rows followed their demo, and
 *   onMoved() the results are stale now.
 */
export function createAuditConsolidate({
  getAudit, isBusy = () => false, setBusy = () => {}, showProgress = () => {}, hideProgress = () => {},
  setStatus = () => {}, relocateMoved = () => 0, onMoved = () => {},
}) {
  const toggleBtn = document.querySelector('#audit-consolidate-btn');
  const panel = document.querySelector('#audit-consolidate-panel');
  const destInput = document.querySelector('#audit-consolidate-dest');
  const browseBtn = document.querySelector('#audit-consolidate-browse');
  const gameCb = document.querySelector('#audit-consolidate-game');
  const previewEl = document.querySelector('#audit-consolidate-preview');
  const startBtn = document.querySelector('#audit-consolidate-start');
  const cancelBtn = document.querySelector('#audit-consolidate-cancel');
  const undoBtn = document.querySelector('#audit-consolidate-undo');
  const layoutRadios = () => [...document.querySelectorAll('input[name="audit-consolidate-layout"]')];

  let working = null; // 'run' | 'undo' while one runs
  let ready = false;
  let undoable = null;
  let planSeq = 0;

  const layout = () => layoutRadios().find((r) => r.checked)?.value ?? 'flat';

  function request() {
    const { groups, singles, roots } = getAudit();
    return consolidateRequest({
      groups, singles, roots,
      destination: destInput?.value ?? '',
      layout: layout(),
      includeGameFolders: !!gameCb?.checked,
    });
  }

  function refreshButtons() {
    const busy = isBusy() || !!working;
    if (toggleBtn) toggleBtn.disabled = busy;
    if (browseBtn) browseBtn.disabled = busy;
    if (gameCb) gameCb.disabled = busy;
    layoutRadios().forEach((r) => { r.disabled = busy; });
    if (startBtn) startBtn.disabled = busy || !ready;
    if (cancelBtn) cancelBtn.disabled = !working;
    if (undoBtn) {
      undoBtn.disabled = busy || !undoable;
      undoBtn.title = undoable ? STRINGS.AUDITOR.consolidateUndoTitle(undoable.count, undoable.destination) : '';
    }
  }

  function setPreview(text) {
    if (previewEl) previewEl.textContent = text;
  }

  /// Asks the backend what would happen; a newer ask wins over a slower one.
  async function refreshPreview() {
    ready = false;
    refreshButtons();
    const { groups, singles } = getAudit();
    const seq = ++planSeq;
    if (!hasDemos(groups, singles)) {
      setPreview(STRINGS.AUDITOR.CONSOLIDATE_NEEDS_AUDIT);
      return;
    }
    if (!destInput?.value.trim()) {
      setPreview(STRINGS.AUDITOR.CONSOLIDATE_NEEDS_DEST);
      return;
    }
    setPreview(STRINGS.AUDITOR.CONSOLIDATE_PLANNING);
    try {
      const plan = await consolidatePlan(request());
      if (seq !== planSeq) return;
      const view = planView(plan);
      ready = view.ready;
      setPreview(view.text);
      if (startBtn) startBtn.dataset.moves = String(plan.moves.length);
    } catch (e) {
      if (seq === planSeq) setPreview(STRINGS.AUDITOR.consolidatePlanFailed(e));
    }
    refreshButtons();
  }

  async function refreshUndoable() {
    try {
      undoable = await consolidateUndoable();
    } catch {
      undoable = null;
    }
    refreshButtons();
  }

  listen('consolidate_progress', (event) => {
    if (working) showProgress(consolidateProgressView(event.payload));
  });

  toggleBtn?.addEventListener('click', () => {
    if (!panel) return;
    panel.hidden = !panel.hidden;
    if (!panel.hidden) {
      refreshPreview();
      refreshUndoable();
    }
  });

  browseBtn?.addEventListener('click', async () => {
    let folder;
    try {
      folder = await open({ directory: true, multiple: false, title: STRINGS.AUDITOR.CONSOLIDATE_DIALOG_TITLE });
    } catch (err) {
      console.error('Error picking the folder to consolidate into:', err);
      return;
    }
    if (!folder) return;
    if (destInput) destInput.value = Array.isArray(folder) ? folder[0] : folder;
    refreshPreview();
  });
  gameCb?.addEventListener('change', () => refreshPreview());
  layoutRadios().forEach((r) => r.addEventListener('change', () => refreshPreview()));

  function reportFailures(outcome) {
    const failed = [...(outcome.failed ?? []), ...(outcome.recycle_failed ?? [])];
    if (failed.length) showToast(STRINGS.AUDITOR.consolidateFailedToast(failed.length, failed[0].error), 'error', 12000);
  }

  async function runJob(kind, job, status) {
    working = kind;
    setBusy(true);
    refreshButtons();
    showProgress(consolidateProgressView({ step: kind === 'undo' ? 'undoing' : 'moving' }));
    try {
      const outcome = await job();
      const queued = relocateMoved(outcome.moved ?? []);
      setStatus(status(outcome, queued));
      reportFailures(outcome);
      if ((outcome.moved?.length ?? 0) + (outcome.recycled?.length ?? 0) > 0) onMoved();
    } catch (e) {
      // ipc_bridge toasts the error.
      setStatus(STRINGS.AUDITOR.consolidatePlanFailed(e));
    } finally {
      working = null;
      hideProgress();
      setBusy(false);
      await refreshUndoable();
      refreshPreview();
    }
  }

  startBtn?.addEventListener('click', async () => {
    if (!ready || working) return;
    const n = Number(startBtn.dataset.moves || 0);
    const ok = await themedConfirm(previewEl?.textContent ?? '', {
      title: STRINGS.AUDITOR.CONSOLIDATE_CONFIRM_TITLE,
      confirmLabel: STRINGS.AUDITOR.consolidateConfirmLabel(n),
      footer: STRINGS.AUDITOR.CONSOLIDATE_CONFIRM_FOOTER,
    });
    if (!ok) return;
    const req = request();
    await runJob('run', () => consolidateRun(req), STRINGS.AUDITOR.consolidatedStatus);
  });

  cancelBtn?.addEventListener('click', async () => {
    cancelBtn.disabled = true;
    setStatus(STRINGS.AUDITOR.CONSOLIDATE_CANCELLING);
    try {
      await consolidateCancel();
    } catch {
      // The job resolves on its own.
    }
  });

  undoBtn?.addEventListener('click', async () => {
    if (!undoable || working) return;
    if (!(await themedConfirm(STRINGS.AUDITOR.CONSOLIDATE_UNDO_CONFIRM))) return;
    await runJob('undo', () => consolidateUndo(), STRINGS.AUDITOR.undoneStatus);
  });

  refreshButtons();
  return {
    /// The results changed (a new audit, a delete): the preview follows.
    resultsChanged: () => {
      if (panel && !panel.hidden) refreshPreview();
      else refreshButtons();
    },
    refreshButtons,
  };
}
