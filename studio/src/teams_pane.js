// teams_pane.js
//
// The Teams modal (#445), opened from the Master Queue's Teams button: one
// row per team found in the queue's demos, with its demo count, an editable
// name, and a "Same team as" pick for merging. The list itself is
// project_teams.js; this only draws it and hands edits back to main.js,
// which owns the state and saves it with the project.
//
// Tags come from player names, so every one goes into the DOM as text, never
// as markup.
import { STRINGS } from './strings.js';
import { buildTeamsList, renameTeam, mergeTeam, unmergeTeam, demoHasTeams } from './project_teams.js';

let modal, tableBody, unreadRow, unreadNote, readBtn;
let deps = null;
let reading = false;

/**
 * `getDemos()` and `getProjectTeams()` read main.js's current state,
 * `onChange()` is told after every edit, and `onReadMissing(paths)` scans
 * the given demos again (resolving once it has finished).
 */
export function initTeamsPane({ getDemos, getProjectTeams, onChange, onReadMissing }) {
  modal = document.querySelector('#teams-modal');
  if (!modal) return;
  deps = { getDemos, getProjectTeams, onChange, onReadMissing };
  tableBody = document.querySelector('#teams-table-body');
  unreadRow = document.querySelector('#teams-unread-row');
  unreadNote = document.querySelector('#teams-unread-note');
  readBtn = document.querySelector('#teams-read-btn');

  document.querySelector('#teams-btn')?.addEventListener('click', () => {
    modal.style.display = 'flex';
    renderTeams();
  });
  document.querySelector('#teams-close-btn')?.addEventListener('click', () => {
    modal.style.display = 'none';
  });
  readBtn?.addEventListener('click', readMissing);
}

/** Redraws the list if the modal is open (after a scan or a project load). */
export function refreshTeamsPane() {
  if (modal && modal.style.display !== 'none') renderTeams();
}

async function readMissing() {
  if (reading || !deps?.onReadMissing) return;
  const paths = deps.getDemos().filter((d) => !demoHasTeams(d)).map((d) => d.path);
  if (paths.length === 0) return;
  reading = true;
  readBtn.disabled = true;
  readBtn.textContent = STRINGS.TEAMS.READING_BUTTON;
  try {
    await deps.onReadMissing(paths);
  } finally {
    reading = false;
    readBtn.disabled = false;
    readBtn.textContent = STRINGS.TEAMS.READ_BUTTON;
    refreshTeamsPane();
  }
}

function edited() {
  deps.onChange?.();
  renderTeams();
}

function renderTeams() {
  if (!deps || !tableBody) return;
  const projectTeams = deps.getProjectTeams();
  const { rows, unread } = buildTeamsList(deps.getDemos(), projectTeams);

  if (unreadRow) unreadRow.hidden = unread === 0;
  if (unreadNote) unreadNote.textContent = STRINGS.TEAMS.unreadNote(unread);

  tableBody.replaceChildren();
  if (rows.length === 0) {
    const tr = document.createElement('tr');
    const td = document.createElement('td');
    td.colSpan = 4;
    td.className = 'table-empty';
    td.textContent = STRINGS.TEAMS.EMPTY;
    tr.appendChild(td);
    tableBody.appendChild(tr);
    return;
  }

  for (const row of rows) {
    const tr = document.createElement('tr');
    tr.dataset.tag = row.tag;

    const tagCell = document.createElement('td');
    tagCell.className = 'teams-tag';
    tagCell.append(row.tag);
    for (const tag of row.merged) {
      const also = document.createElement('span');
      also.className = 'teams-also';
      also.append(STRINGS.TEAMS.alsoTag(tag));
      const split = document.createElement('button');
      split.textContent = '×';
      split.title = STRINGS.TEAMS.splitTitle(tag);
      split.setAttribute('aria-label', STRINGS.TEAMS.splitTitle(tag));
      split.addEventListener('click', () => {
        if (unmergeTeam(projectTeams, tag)) edited();
      });
      also.appendChild(split);
      tagCell.appendChild(also);
    }

    const countCell = document.createElement('td');
    countCell.className = 'col-center teams-count';
    countCell.textContent = String(row.demoCount);
    countCell.title = row.demoNames.join('\n');

    const nameCell = document.createElement('td');
    const nameInput = document.createElement('input');
    nameInput.type = 'text';
    nameInput.className = 'teams-name-input';
    nameInput.value = row.customName ? row.name : '';
    nameInput.placeholder = row.tag;
    nameInput.spellcheck = false;
    nameInput.addEventListener('change', () => {
      if (renameTeam(projectTeams, row.tag, nameInput.value)) edited();
    });
    nameInput.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') nameInput.blur();
    });
    nameCell.appendChild(nameInput);

    const sameCell = document.createElement('td');
    const same = document.createElement('select');
    same.className = 'teams-same-select';
    const none = document.createElement('option');
    none.value = '';
    none.textContent = STRINGS.TEAMS.SAME_AS_NONE;
    same.appendChild(none);
    for (const other of rows) {
      if (other.tag === row.tag) continue;
      const option = document.createElement('option');
      option.value = other.tag;
      option.textContent = other.customName ? `${other.name} (${other.tag})` : other.tag;
      same.appendChild(option);
    }
    same.addEventListener('change', () => {
      if (same.value && mergeTeam(projectTeams, row.tag, same.value)) edited();
    });
    sameCell.appendChild(same);

    tr.append(tagCell, countCell, nameCell, sameCell);
    tableBody.appendChild(tr);
  }
}
