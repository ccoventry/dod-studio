// analyzer_browser.js
// The Analyzer tab's Explorer: the folder tree and Quick Links (Pinned /
// Recent / Local) in the sidebar, and the Demos table of the folder it shows,
// with its filters, sort and keyboard navigation. analyzer_pane.js starts it
// and opens the demo the user picks; the report itself is drawn there.

import { pickPath } from './pick_path.js';
import { listen } from '@tauri-apps/api/event';
import { browseDirectory, readDemoHeads, defaultBrowseDir, countDemoFiles, scanDemoFolders, indexDemoPlayers, lookupDemoPlayers } from './ipc_bridge.js';
import { readDemoHeadsInto, mapCellText } from './demo_heads.js';
import { groupPlayers, parsePlayerQuery, findPlayer } from './player_filter.js';
import { STRINGS } from './strings.js';
import { initDemoCache } from './demo_cache.js';
import { createFolderPlayers } from './folder_players.js';
import { unloadedOpenNodes } from './tree_loads.js';
import { escapeHtml as esc } from './html.js';
import { shortenPath, isFolderPath, parentDirOf, ancestorChain, isAncestorOf, demoDateISO } from './analyzer_browser_logic.js';

// ── Explorer sidebar state — a real native folder tree (drives -> subfolders,
// lazily loaded, expand/collapse) plus a 3-tier Quick Links box (Pinned /
// Recent / Local), both driving one shared `currentDir`. The demos table
// below is scoped to ONLY that single folder's contents (non-recursive) —
// mirrors dev's SidePanel::left explorer + `desktop_files`, see
// docs/archive/tauri_parity_audit.md Area 3 for the corrected design this replaced
// an earlier (wrong-shape) recursive multi-folder aggregate with. Dev's
// "Group by Match"/"Group by Player-Recorder" view modes are NOT ported:
// their grouping keys (server_ip/player_roster_hash/recorder_id) were only
// ever assigned `None` in dev's own source, so those two view modes never
// actually grouped anything there either — restoring the one real, working
// view (Flat List) is the faithful port here, not a scope cut.
let getPinnedFolders = () => [];
let pinFolder = async () => {};
let unpinFolder = async () => {};
let getDemoFolderHistory = () => [];
// Gates the Explorer tree's per-subfolder "(N)" demo-count badge — dev's
// `settings.scan_folders_for_demos`, defaults false. Quick Links counts are
// NOT gated by this (dev never gated those either), only the tree is.
let getScanFoldersForDemos = () => false;
let setScanFoldersForDemos = async () => {};
let recordDemoFolderVisit = async () => {};
let forgetDemoFolderVisit = async () => {};
let getAnalyzerExplorerWidth = () => 260;
let setAnalyzerExplorerWidth = async () => {};

let currentDir = null;
const dirCache = new Map(); // path -> DirListing from browse_directory
const openTreeNodes = new Set();
// Folders being read for the tree now, so a re-render never reads one twice.
const pendingTreeLoads = new Set();
let thisPcOpen = true;
let driveRoots = []; // DirEntryLite[]
let localFolders = []; // DemoFolderHit[] from scan_demo_folders
let localFoldersScanning = false;
let localFoldersScanTriggered = false;
const quickLinkCountCache = new Map(); // path -> demo_count

let currentFolderDemos = []; // DemoFileEntry[] for currentDir only
// The folder whose file names are still being read: the Demos table says so
// instead of showing the last folder's demos.
let listingDir = null;
// The folder the tree scrolls to once its row is drawn (its ancestors are
// read after the folder is picked, so the row can arrive later).
let revealTreePath = null;
let browserSelectedDemo = null;
// Arrow-key cursor, separate from browserSelectedDemo (the actually-loaded
// demo) — mirrors master_pane.js's keyboard-selected/table-row-selected
// split so arrowing through the list previews a lightweight outline instead
// of moving the same heavy "selected" fill Enter/click commits. #99.
let browserCursorPath = null;
// coldemoplayer-style focus scoping: click into a list and arrow keys
// apply only to it. Defaults to 'demos' so arrow-nav keeps working
// out-of-the-box exactly like before this existed. Only the Demo Browser
// still uses a cursor/commit split (analyze_demo_full is a real parse worth
// gating) — the Explorer Tree is instant-select, since browse_directory is
// cheap. #99.
let focusedList = 'demos';
let browserError = null;
let demoFilterQuery = '';
let demoFilterType = 'All';
let demoFilterMap = '';
let demoFilterDateStart = '';
let demoFilterDateEnd = '';
// #437: who is in each demo of the current folder (folder_players.js), and
// the player box's text. Cache all's queued demos are left to it (#992).
let isCaching = () => false;
const folderPlayers = createFolderPlayers({
  lookupDemoPlayers,
  indexDemoPlayers,
  isCaching: (path) => isCaching(path),
  onChange: () => {
    refreshPlayerOptions();
    renderPlayerStatus();
    // Re-render when the list can change: a filter by player, or the type
    // column's guess being replaced by the real type.
    renderDemoTable();
  },
  strings: STRINGS.ANALYZER,
});
let demoFilterPlayer = '';
let playerOptions = [];
/** The Demos table's sortable columns: index.html's `th[data-sort]` values (#35). */
const DEMO_SORT = Object.freeze({ NAME: 'name', TYPE: 'type', MAP: 'map', DATE: 'date' });
let demoSortColumn = null; // a DEMO_SORT value
let demoSortAscending = true;
// ── Explorer sidebar + single-folder demo list ───────────────────────────────

function demoTypeOf(entry) {
  // The real type once the player index has read the demo; until then the
  // listing's own guess.
  return folderPlayers.get(entry.path)?.demoType || entry.demo_type || STRINGS.ANALYZER.TYPE_POV;
}

/** The player the box picks out in `entry`, or null (also null while that
 *  demo's players haven't been read yet). */
function filteredPlayerIn(entry) {
  const query = parsePlayerQuery(demoFilterPlayer, playerOptions);
  if (!query) return null;
  return findPlayer(folderPlayers.get(entry.path)?.players, query);
}

function refreshPlayerOptions() {
  playerOptions = groupPlayers(folderPlayers.all().flatMap((d) => d.players || []));
  const list = document.querySelector('#analyzer-player-options');
  if (list) list.innerHTML = playerOptions.map((o) => `<option value="${esc(o.label)}"></option>`).join('');
}

function renderPlayerStatus() {
  const el = document.querySelector('#analyzer-player-status');
  if (!el) return;
  el.textContent = folderPlayers.status();
}

/** Starts reading who is in every demo of the current folder (#437). */
function startPlayerIndex(demos) {
  folderPlayers.start(demos.map((d) => d.path));
}

listen('demo_players', (event) => folderPlayers.onPlayers(event.payload || {}))
  .catch((err) => console.error('Failed to register demo_players listener:', err));
listen('demo_cache_progress', (event) => folderPlayers.onCacheProgress(event.payload || {}))
  .catch((err) => console.error('Failed to register demo_cache_progress listener (players):', err));

async function countDemoFilesCached(path) {
  if (quickLinkCountCache.has(path)) return quickLinkCountCache.get(path);
  const n = await countDemoFiles(path);
  quickLinkCountCache.set(path, n);
  return n;
}

function quickLinkRowHtml(folder, count, isPinned) {
  const label = shortenPath(folder);
  return `<div class="quicklink-row ${folder === currentDir ? 'selected' : ''}" data-path="${esc(folder)}" title="${esc(folder)}">
    <button class="quicklink-pin-btn ${isPinned ? 'pinned' : ''}" data-path="${esc(folder)}" title="${isPinned ? STRINGS.ANALYZER.UNPIN_FOLDER_TITLE : STRINGS.ANALYZER.PIN_FOLDER_TITLE}">📌</button>
    <span class="quicklink-label">${esc(label)} (${count})</span>
  </div>`;
}

// Dev's Windows-11-style Quick Access pattern: Pinned (explicit bookmarks),
// Recent (auto-tracked history), Local (bounded background scan) — each
// tier hidden entirely when empty, excludes anything already promoted to a
// higher tier. See docs/archive/tauri_parity_audit.md Area 3.
async function renderQuickLinksSection() {
  const container = document.querySelector('#analyzer-quick-links');
  if (!container) return;

  const pinned = (getPinnedFolders() || []).filter(isFolderPath);
  const recent = (getDemoFolderHistory() || []).filter(isFolderPath).filter((f) => !pinned.includes(f));
  const local = localFolders.map((f) => f.path).filter((f) => !pinned.includes(f));

  const allPaths = [...new Set([...pinned, ...recent, ...local])];
  await Promise.all(allPaths.map((p) => countDemoFilesCached(p)));

  const tier = (label, paths) => paths.length === 0 ? '' : `
    <div class="quicklink-tier-label">${esc(label)}</div>
    ${paths.map((p) => quickLinkRowHtml(p, quickLinkCountCache.get(p) || 0, pinned.includes(p))).join('')}`;

  if (pinned.length === 0 && recent.length === 0 && local.length === 0) {
    container.innerHTML = localFoldersScanning
      ? `<div class="quicklink-empty"><span class="spinner"></span> ${STRINGS.ANALYZER.SCANNING_WORKSPACE}</div>`
      : `<div class="quicklink-empty">${STRINGS.ANALYZER.NO_DEMO_FOLDERS_FOUND}</div>`;
  } else {
    container.innerHTML = tier(STRINGS.ANALYZER.TIER_PINNED, pinned) + tier(STRINGS.ANALYZER.TIER_RECENT, recent) + tier(STRINGS.ANALYZER.TIER_LOCAL, local);
  }

  container.querySelectorAll('.quicklink-row').forEach((row) => {
    row.addEventListener('click', (e) => {
      if (e.target.closest('.quicklink-pin-btn')) return;
      setCurrentDir(row.dataset.path);
    });
  });
  container.querySelectorAll('.quicklink-pin-btn').forEach((btn) => {
    btn.addEventListener('click', async (e) => {
      e.stopPropagation();
      const path = btn.dataset.path;
      const isPinned = (getPinnedFolders() || []).includes(path);
      if (isPinned) await unpinFolder(path); else await pinFolder(path);
      renderQuickLinksSection();
    });
  });
}

function treeRowHtml(entry) {
  const { path, name, demo_count } = entry;
  const isOpen = openTreeNodes.has(path);
  const isSelected = path === currentDir;
  const showCount = getScanFoldersForDemos() && demo_count > 0;
  const icon = showCount ? '📂' : '📁';
  const label = showCount ? `${name} (${demo_count})` : name;
  const arrow = isOpen ? '⏷' : '⏵';

  let childrenHtml = '';
  if (isOpen) {
    const listing = dirCache.get(path);
    if (listing) {
      childrenHtml = listing.subdirs.length > 0
        ? `<div class="tree-children">${listing.subdirs.map(treeRowHtml).join('')}</div>`
        : '';
    } else {
      childrenHtml = `<div class="tree-children"><div class="tree-loading">${STRINGS.ANALYZER.LOADING_LABEL}</div></div>`;
    }
  }

  return `<div class="tree-node">
    <div class="tree-row ${isSelected ? 'selected' : ''}">
      <button class="tree-toggle" data-path="${esc(path)}">${arrow}</button>
      <span class="tree-label" data-path="${esc(path)}" title="${esc(path)}">${icon} ${esc(label)}</span>
    </div>
    ${childrenHtml}
  </div>`;
}

// Flattens the currently-*visible*, navigable tree rows (open nodes only, in
// the same order treeRowHtml renders them) so Up/Down can move the selection
// through it without querying the DOM. This PC itself is excluded — it isn't
// a real folder setCurrentDir can navigate to. #99.
function flattenVisibleTreeRows() {
  const rows = [];
  if (thisPcOpen) {
    const walk = (entries) => {
      for (const entry of entries) {
        rows.push({ path: entry.path });
        if (openTreeNodes.has(entry.path)) {
          const listing = dirCache.get(entry.path);
          if (listing) walk(listing.subdirs);
        }
      }
    };
    walk(driveRoots);
  }
  return rows;
}

// A drive root (e.g. "C:\") has no real parent folder to navigate to. #99.
// parentDirOf slices off everything after the last separator, which for a
// folder directly under a drive root yields a bare "C:" — one character
// short of how driveRoots/dirCache actually key the root ("C:\\", see
// native_roots() in dir_browser.rs) — so normalize that one case back to it.
function treeParentOf(path) {
  if (/^[A-Za-z]:\\?$/.test(path)) return null;
  const parent = parentDirOf(path);
  return parent && /^[A-Za-z]:$/.test(parent) ? `${parent}\\` : parent;
}

// Native Explorer Tree: drives -> subfolders, lazily loaded and cached per
// node, genuinely expand/collapse (default closed). Mirrors dev's
// `tree.rs::render_native_dir_node` — see docs/archive/tauri_parity_audit.md Area 3.
async function renderExplorerTree() {
  const container = document.querySelector('#analyzer-tree');
  if (!container) return;
  if (driveRoots.length === 0) {
    try {
      const listing = await browseDirectory(null);
      driveRoots = listing.subdirs;
    } catch {
      driveRoots = [];
    }
  }

  const thisPcArrow = thisPcOpen ? '⏷' : '⏵';
  container.innerHTML = `<div class="tree-node">
    <div class="tree-row">
      <button class="tree-toggle" id="tree-this-pc-toggle">${thisPcArrow}</button>
      <span class="tree-label">${STRINGS.ANALYZER.THIS_PC_LABEL}</span>
    </div>
    ${thisPcOpen ? `<div class="tree-children">${driveRoots.map(treeRowHtml).join('')}</div>` : ''}
  </div>`;

  if (revealTreePath && scrollTreeRowIntoView(revealTreePath)) revealTreePath = null;
  loadUnloadedOpenNodes();

  const thisPcToggle = container.querySelector('#tree-this-pc-toggle');
  thisPcToggle?.addEventListener('click', () => {
    thisPcOpen = !thisPcOpen;
    renderExplorerTree();
  });

  container.querySelectorAll('.tree-toggle:not(#tree-this-pc-toggle)').forEach((btn) => {
    btn.addEventListener('click', () => {
      const path = btn.dataset.path;
      if (openTreeNodes.has(path)) closeTreeNode(path); else openTreeNode(path);
    });
  });
  container.querySelectorAll('.tree-label').forEach((el) => {
    el.addEventListener('click', () => setCurrentDir(el.dataset.path));
  });
}

// An open folder with nothing read shows "Loading…": read every such folder
// nobody is reading yet, and draw the tree again as each arrives. A folder
// that can't be read is stored empty, as openTreeNode does, so its
// placeholder goes away instead of staying for good (#572).
function loadUnloadedOpenNodes() {
  for (const path of unloadedOpenNodes(openTreeNodes, dirCache, pendingTreeLoads)) {
    pendingTreeLoads.add(path);
    browseDirectory(path)
      .then((listing) => dirCache.set(path, listing))
      .catch(() => dirCache.set(path, { subdirs: [], demos: [] }))
      .finally(() => {
        pendingTreeLoads.delete(path);
        renderExplorerTree();
      });
  }
}

// Shared by the toggle-button click handler above and the keyboard Right
// key (treeRight) so both open a node identically. #99.
async function openTreeNode(path) {
  if (openTreeNodes.has(path)) return;
  openTreeNodes.add(path);
  renderExplorerTree();
  if (!dirCache.has(path)) {
    try {
      dirCache.set(path, await browseDirectory(path));
    } catch {
      dirCache.set(path, { subdirs: [], demos: [] });
    }
    renderExplorerTree();
  }
}

// Shared by the toggle-button click handler and the keyboard Left key
// (treeLeft). Key mechanic: collapsing a node you're currently inside
// navigates you up to it, rather than leaving you on a hidden selection —
// matches dev/Windows Explorer both (tree.rs:373-383). #99.
async function closeTreeNode(path) {
  if (!openTreeNodes.has(path)) return;
  openTreeNodes.delete(path);
  if (isAncestorOf(path, currentDir)) {
    await setCurrentDir(path);
    return;
  }
  renderExplorerTree();
}

/** Whether the folder's row was there to scroll to. */
function scrollTreeRowIntoView(path) {
  const container = document.querySelector('#analyzer-tree');
  if (!container || !path) return false;
  const row = container.querySelector(`.tree-label[data-path="${CSS.escape(path)}"]`)?.closest('.tree-row');
  row?.scrollIntoView({ block: 'nearest' });
  return !!row;
}

// Instant select, no separate cursor: browse_directory is a cheap
// non-recursive read_dir (no demo parsing), so there's no cost to gate
// behind a commit step, unlike the Demo Browser's analyze_demo_full. #99.
function moveTreeSelection(dir) {
  const rows = flattenVisibleTreeRows();
  if (rows.length === 0) return;
  const idx = rows.findIndex((r) => r.path === currentDir);
  const newIdx = idx === -1 ? (dir > 0 ? 0 : rows.length - 1) : Math.min(rows.length - 1, Math.max(0, idx + dir));
  const target = rows[newIdx].path;
  if (target === currentDir) return;
  setCurrentDir(target).then(() => scrollTreeRowIntoView(target));
}

// Right: expands the selected folder if closed, or steps the selection into
// its first child if already open — matches Windows Explorer's tree view. #99.
function treeRight() {
  if (!currentDir) return;
  if (!openTreeNodes.has(currentDir)) {
    openTreeNode(currentDir);
    return;
  }
  const listing = dirCache.get(currentDir);
  if (listing && listing.subdirs.length > 0) {
    const target = listing.subdirs[0].path;
    setCurrentDir(target).then(() => scrollTreeRowIntoView(target));
  }
}

// Left: collapses the selected folder if open, else steps the selection up
// to its parent. #99.
function treeLeft() {
  if (!currentDir) return;
  if (openTreeNodes.has(currentDir)) {
    closeTreeNode(currentDir);
    return;
  }
  const parent = treeParentOf(currentDir);
  if (parent) setCurrentDir(parent).then(() => scrollTreeRowIntoView(parent));
}

async function forgetInvalidFolder(path) {
  if ((getPinnedFolders() || []).includes(path)) await unpinFolder(path);
  await forgetDemoFolderVisit(path);
}

// Sets the single folder the Demos panel and Explorer Tree are scoped to.
// Key mechanic: whenever a node's ancestor chain contains `currentDir`, that
// node force-opens on the next render — so simply changing `currentDir`
// (from a tree click *or* a Quick Links click) is what makes the tree
// auto-expand down to the newly selected folder (tree.rs:358-368).
async function setCurrentDir(path) {
  // currentDir (and the tree's ancestor-open state) update unconditionally,
  // even when the folder below turns out to be unreadable — an inaccessible
  // folder (permission denied, etc.) still exists and is still a real place
  // in the tree, it just has no demos to list. Previously this only ran on
  // success, so navigating onto a blocked folder left currentDir pointing at
  // whatever came before it — every Up/Down/Left/Right computed "next from
  // currentDir" landed back on the same blocked folder forever, arrow-key
  // navigation couldn't move past it in either direction. #99.
  currentDir = path;
  browserSelectedDemo = null;
  browserCursorPath = null;
  browserError = null;
  // The last folder's demos aren't this one's: the table says it is reading
  // until this folder's names arrive.
  currentFolderDemos = [];
  listingDir = path;
  revealTreePath = path;
  startPlayerIndex([]);

  // The tree reads the ancestors it opens (loadUnloadedOpenNodes) beside
  // this folder's own listing; the table waited for all of them first.
  ancestorChain(path).slice(0, -1).forEach((a) => openTreeNodes.add(a));
  renderQuickLinksSection();
  renderExplorerTree();
  renderDemoTable();

  let listing;
  try {
    listing = await browseDirectory(path);
  } catch (err) {
    // A slow read can finish after the user has moved on (#789): only the
    // folder still open may set the table.
    if (currentDir !== path) return;
    listingDir = null;
    browserError = String(err);
    currentFolderDemos = [];
    await forgetInvalidFolder(path);
    renderQuickLinksSection();
    await renderExplorerTree();
    renderDemoTable();
    return;
  }

  dirCache.set(path, listing);
  if (currentDir !== path) return;
  listingDir = null;
  browserError = null;
  const demos = listing.demos;
  currentFolderDemos = demos;
  startPlayerIndex(demos);
  readDemoHeadsInto(demos, {
    read: readDemoHeads,
    stillWanted: () => currentFolderDemos === demos,
    onRead: renderDemoTable,
  });

  renderDemoTable();
  renderQuickLinksSection();
  await renderExplorerTree();

  if (listing.demos.length > 0) {
    await recordDemoFolderVisit(path);
    renderQuickLinksSection();
  }
}

async function triggerLocalFoldersScan(force = false) {
  if (localFoldersScanTriggered && !force) return;
  localFoldersScanTriggered = true;
  localFoldersScanning = true;
  renderQuickLinksSection();
  try {
    const root = currentDir || (await defaultBrowseDir());
    localFolders = await scanDemoFolders(root);
  } catch {
    localFolders = [];
  }
  localFoldersScanning = false;
  renderQuickLinksSection();
}

function demoDateDisplay(entry) {
  if (!entry.modified_unix_secs) return STRINGS.ANALYZER.EMPTY_DASH;
  const d = new Date(entry.modified_unix_secs * 1000);
  return `${d.toLocaleDateString()} ${d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}`;
}

function passesDemoFilter(entry) {
  if (demoFilterQuery) {
    const q = demoFilterQuery.toLowerCase();
    const hay = `${entry.name} ${entry.map_name || ''} ${entry.path}`.toLowerCase();
    if (!hay.includes(q)) return false;
  }
  if (demoFilterType !== 'All' && demoTypeOf(entry) !== demoFilterType) return false;
  if (demoFilterMap && !(entry.map_name || '').toLowerCase().includes(demoFilterMap.toLowerCase())) return false;
  const iso = demoDateISO(entry);
  if (demoFilterDateStart.length === 10 && (iso.length < 10 || iso < demoFilterDateStart)) return false;
  if (demoFilterDateEnd.length === 10 && (iso.length < 10 || iso > demoFilterDateEnd)) return false;
  if (demoFilterPlayer.trim() && !filteredPlayerIn(entry)) return false;
  return true;
}

function sortedFilteredDemos() {
  let list = currentFolderDemos.filter(passesDemoFilter);
  if (demoSortColumn) {
    list = list.slice().sort((a, b) => {
      let cmp;
      switch (demoSortColumn) {
        case DEMO_SORT.NAME: cmp = a.name.toLowerCase().localeCompare(b.name.toLowerCase()); break;
        case DEMO_SORT.TYPE: cmp = demoTypeOf(a).localeCompare(demoTypeOf(b)); break;
        case DEMO_SORT.MAP: cmp = (a.map_name || '').toLowerCase().localeCompare((b.map_name || '').toLowerCase()); break;
        case DEMO_SORT.DATE: cmp = a.modified_unix_secs - b.modified_unix_secs; break;
        default: cmp = 0;
      }
      return demoSortAscending ? cmp : -cmp;
    });
  }
  return list;
}

function updateSortHeaderIndicators() {
  document.querySelectorAll('#analyzer-demo-table th[data-sort]').forEach((th) => {
    const base = STRINGS.ANALYZER.DEMO_TABLE_HEADERS[th.dataset.sort] || th.dataset.label;
    th.textContent = demoSortColumn === th.dataset.sort ? `${base} ${demoSortAscending ? '▲' : '▼'}` : base;
  });
}

function renderDemoTable() {
  const tbody = document.querySelector('#analyzer-demo-tbody');
  if (!tbody) return;
  updateSortHeaderIndicators();

  if (browserError) {
    tbody.innerHTML = `<tr><td colspan="4" class="table-empty" style="color:#f44336;">${esc(browserError)}</td></tr>`;
    return;
  }
  if (!currentDir) {
    tbody.innerHTML = `<tr><td colspan="4" class="table-empty">${STRINGS.ANALYZER.PICK_FOLDER_FROM_SIDEBAR}</td></tr>`;
    return;
  }
  if (listingDir === currentDir) {
    tbody.innerHTML = `<tr><td colspan="4" class="table-empty">${STRINGS.ANALYZER.READING_FOLDER}</td></tr>`;
    return;
  }
  if (currentFolderDemos.length === 0) {
    tbody.innerHTML = `<tr><td colspan="4" class="table-empty">${STRINGS.ANALYZER.NO_DEMOS_IN_FOLDER}</td></tr>`;
    return;
  }
  const list = sortedFilteredDemos();
  if (list.length === 0) {
    tbody.innerHTML = `<tr><td colspan="4" class="table-empty">${STRINGS.ANALYZER.NO_DEMOS_MATCH_FILTERS}</td></tr>`;
    return;
  }

  tbody.innerHTML = list.map((entry) => {
    const isSelected = entry.path === browserSelectedDemo;
    const isCursor = entry.path === browserCursorPath;
    const classes = ['analyzer-demo-row', isSelected ? 'selected' : '', isCursor ? 'keyboard-selected' : ''].filter(Boolean).join(' ');
    const player = filteredPlayerIn(entry);
    const role = player ? ` · ${player.recorder ? STRINGS.ANALYZER.ROLE_RECORDED : STRINGS.ANALYZER.ROLE_PLAYED}` : '';
    return `<tr class="${classes}" data-path="${esc(entry.path)}" title="${esc(entry.path)}">
      <td>${esc(entry.name)}</td>
      <td>${esc(demoTypeOf(entry) + role)}</td>
      <td>${esc(mapCellText(entry, { reading: STRINGS.ANALYZER.MAP_READING, dash: STRINGS.ANALYZER.EMPTY_DASH }))}</td>
      <td>${esc(demoDateDisplay(entry))}</td>
    </tr>`;
  }).join('');

  tbody.querySelectorAll('tr[data-path]').forEach((tr) => {
    tr.addEventListener('click', () => selectDemo(tr.dataset.path));
  });
}

function selectDemo(path) {
  browserSelectedDemo = path;
  renderDemoTable();
  openDemo(path);
}

function handleDemoTableKeydown(e) {
  const list = sortedFilteredDemos();
  if (list.length === 0) return;

  if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
    e.preventDefault();
    const dir = e.key === 'ArrowDown' ? 1 : -1;
    // No cursor yet — start from wherever the actual selection already is,
    // same fallback as master_pane.js, so arrowing away from a loaded demo
    // moves one row from it instead of always restarting at the top.
    const currentPath = browserCursorPath ?? browserSelectedDemo;
    const idx = list.findIndex((d) => d.path === currentPath);
    const newIdx = idx === -1 ? (dir > 0 ? 0 : list.length - 1) : Math.min(list.length - 1, Math.max(0, idx + dir));
    browserCursorPath = list[newIdx].path;
    renderDemoTable();
    const row = document.querySelector(`#analyzer-demo-tbody tr[data-path="${CSS.escape(browserCursorPath)}"]`);
    row?.scrollIntoView({ block: 'nearest' });
  } else if (e.key === 'Enter') {
    const target = browserCursorPath ?? browserSelectedDemo;
    if (target) selectDemo(target);
  } else if (e.key === 'Escape' && browserCursorPath) {
    // Clears the arrow-nav cursor only — never what loaded the
    // currently-displayed demo (only Enter/click do that via selectDemo),
    // so this can't lose or change anything shown. #28.
    e.preventDefault();
    browserCursorPath = null;
    renderDemoTable();
  }
}

function handleTreeKeydown(e) {
  if (e.key === 'ArrowDown') { e.preventDefault(); moveTreeSelection(1); }
  else if (e.key === 'ArrowUp') { e.preventDefault(); moveTreeSelection(-1); }
  else if (e.key === 'ArrowRight') { e.preventDefault(); treeRight(); }
  else if (e.key === 'ArrowLeft') { e.preventDefault(); treeLeft(); }
}

// Single global handler, dispatched by focusedList (coldemoplayer-style:
// whichever of the Explorer Tree / Demos table was last clicked). #99.
function initAnalyzerBrowserKeyboardNav() {
  document.addEventListener('keydown', (e) => {
    const pane = document.querySelector('#pane-demo-analyzer');
    if (!pane || pane.style.display === 'none') return;
    const tag = document.activeElement && document.activeElement.tagName;
    if (tag === 'INPUT' || tag === 'SELECT' || tag === 'TEXTAREA') return;

    if (focusedList === 'tree') handleTreeKeydown(e);
    else handleDemoTableKeydown(e);
  });
}

function initAnalyzerBrowser() {
  const refreshBtn = document.querySelector('#analyzer-tree-refresh-btn');
  if (refreshBtn) {
    refreshBtn.addEventListener('click', () => {
      dirCache.clear();
      driveRoots = [];
      quickLinkCountCache.clear();
      triggerLocalFoldersScan(true);
      renderExplorerTree();
      if (currentDir) setCurrentDir(currentDir);
    });
  }

  // Focus-scoping: click into either list and arrow keys apply to it. #99.
  document.querySelector('#analyzer-tree')?.addEventListener('click', () => { focusedList = 'tree'; });
  document.querySelector('#analyzer-demo-table')?.addEventListener('click', () => { focusedList = 'demos'; });

  const scanFoldersCb = document.querySelector('#analyzer-scan-folders-for-demos');
  if (scanFoldersCb) {
    scanFoldersCb.checked = getScanFoldersForDemos();
    scanFoldersCb.addEventListener('change', async (e) => {
      await setScanFoldersForDemos(e.target.checked);
      renderExplorerTree();
    });
  }

  const addPinBtn = document.querySelector('#analyzer-add-pin-btn');
  if (addPinBtn) {
    addPinBtn.addEventListener('click', async () => {
      try {
        const selected = await pickPath({ field: 'analyzer.pin_folder', directory: true, multiple: false, title: STRINGS.ANALYZER.ADD_PINNED_FOLDER_DIALOG_TITLE });
        if (selected) {
          const folder = Array.isArray(selected) ? selected[0] : selected;
          await pinFolder(folder);
          renderQuickLinksSection();
        }
      } catch (err) {
        console.error('Error picking folder to pin:', err);
      }
    });
  }

  document.querySelectorAll('#analyzer-demo-table th[data-sort]').forEach((th) => {
    th.addEventListener('click', () => {
      const col = th.dataset.sort;
      if (demoSortColumn === col) demoSortAscending = !demoSortAscending;
      else { demoSortColumn = col; demoSortAscending = true; }
      renderDemoTable();
    });
  });

  const searchEl = document.querySelector('#analyzer-filter-search');
  const typeEl = document.querySelector('#analyzer-filter-type');
  const mapEl = document.querySelector('#analyzer-filter-map');
  const dateStartEl = document.querySelector('#analyzer-filter-date-start');
  const dateEndEl = document.querySelector('#analyzer-filter-date-end');
  const playerEl = document.querySelector('#analyzer-filter-player');
  const resetBtn = document.querySelector('#analyzer-filter-reset');

  searchEl?.addEventListener('input', (e) => { demoFilterQuery = e.target.value; renderDemoTable(); });
  typeEl?.addEventListener('change', (e) => { demoFilterType = e.target.value; renderDemoTable(); });
  mapEl?.addEventListener('input', (e) => { demoFilterMap = e.target.value; renderDemoTable(); });
  dateStartEl?.addEventListener('input', (e) => { demoFilterDateStart = e.target.value; renderDemoTable(); });
  dateEndEl?.addEventListener('input', (e) => { demoFilterDateEnd = e.target.value; renderDemoTable(); });
  playerEl?.addEventListener('input', (e) => { demoFilterPlayer = e.target.value; renderDemoTable(); });
  resetBtn?.addEventListener('click', () => {
    demoFilterQuery = ''; demoFilterType = 'All'; demoFilterMap = ''; demoFilterDateStart = ''; demoFilterDateEnd = ''; demoFilterPlayer = '';
    if (playerEl) playerEl.value = '';
    if (searchEl) searchEl.value = '';
    if (typeEl) typeEl.value = 'All';
    if (mapEl) mapEl.value = '';
    if (dateStartEl) dateStartEl.value = '';
    if (dateEndEl) dateEndEl.value = '';
    renderDemoTable();
  });

  initAnalyzerBrowserKeyboardNav();

  renderQuickLinksSection();
  renderExplorerTree();
  triggerLocalFoldersScan();

  (async () => {
    const startDir = (getPinnedFolders() || []).filter(isFolderPath)[0]
      || (getDemoFolderHistory() || []).filter(isFolderPath)[0]
      || (await defaultBrowseDir());
    if (startDir) setCurrentDir(startDir);
  })();
}

// Explorer sidebar drag-to-resize — width persists to settings.json via
// getAnalyzerExplorerWidth/setAnalyzerExplorerWidth (main.js), the same
// pattern every other layout/preference field in this app uses.
function initExplorerResize() {
  const handle = document.querySelector('#analyzer-explorer-resize-handle');
  const sidebar = document.querySelector('#analyzer-explorer-sidebar');
  if (!handle || !sidebar) return;

  // 260px matches the sidebar's original static width (pre-resize) — known
  // to render every row cleanly. Anything narrower wraps the Explorer
  // Settings checkbox label onto two lines and truncates folder names to
  // 3-4 characters.
  const MIN_WIDTH = 260;
  // Cap tracks the cap's actual purpose — never let the Demos/Analyzer main
  // column get squeezed below a usable width — rather than a window-relative
  // percentage, which produced a confusingly different max % depending on
  // whether the window was maximized, restored, or moved to another monitor.
  const MIN_MAIN_COLUMN_WIDTH = 500;
  sidebar.style.width = `${getAnalyzerExplorerWidth()}px`;

  let startX = 0;
  let startWidth = 0;

  function onPointerMove(e) {
    const maxWidth = Math.max(MIN_WIDTH, window.innerWidth - MIN_MAIN_COLUMN_WIDTH);
    const newWidth = Math.min(maxWidth, Math.max(MIN_WIDTH, startWidth + (e.clientX - startX)));
    sidebar.style.width = `${newWidth}px`;
  }

  function onPointerUp() {
    handle.classList.remove('resizing');
    document.removeEventListener('mousemove', onPointerMove);
    document.removeEventListener('mouseup', onPointerUp);
    setAnalyzerExplorerWidth(sidebar.getBoundingClientRect().width).catch(() => {});
  }

  handle.addEventListener('mousedown', (e) => {
    e.preventDefault();
    startX = e.clientX;
    startWidth = sidebar.getBoundingClientRect().width;
    handle.classList.add('resizing');
    document.addEventListener('mousemove', onPointerMove);
    document.addEventListener('mouseup', onPointerUp);
  });
}

// What the rest of the Analyzer calls. analyzer_pane.js opens a demo through
// `openDemo`, which the browser calls when a row is picked.
let openDemo = async () => {};

/** Starts the Explorer sidebar and its Demos table. `explorer` holds the
 *  settings it reads and writes (pinned folders, folder history, the
 *  per-folder count setting, the sidebar width); `openDemo(path)` analyses a
 *  demo the user picks. Call once. */
export function startAnalyzerBrowser({
  getPinnedFolders: getPinnedFoldersCb,
  pinFolder: pinFolderCb,
  unpinFolder: unpinFolderCb,
  getDemoFolderHistory: getDemoFolderHistoryCb,
  recordDemoFolderVisit: recordDemoFolderVisitCb,
  forgetDemoFolderVisit: forgetDemoFolderVisitCb,
  getScanFoldersForDemos: getScanFoldersForDemosCb,
  setScanFoldersForDemos: setScanFoldersForDemosCb,
  getAnalyzerExplorerWidth: getAnalyzerExplorerWidthCb,
  setAnalyzerExplorerWidth: setAnalyzerExplorerWidthCb,
  openDemo: openDemoCb,
} = {}) {
  if (getPinnedFoldersCb) getPinnedFolders = getPinnedFoldersCb;
  if (pinFolderCb) pinFolder = pinFolderCb;
  if (unpinFolderCb) unpinFolder = unpinFolderCb;
  if (getDemoFolderHistoryCb) getDemoFolderHistory = getDemoFolderHistoryCb;
  if (recordDemoFolderVisitCb) recordDemoFolderVisit = recordDemoFolderVisitCb;
  if (forgetDemoFolderVisitCb) forgetDemoFolderVisit = forgetDemoFolderVisitCb;
  if (getScanFoldersForDemosCb) getScanFoldersForDemos = getScanFoldersForDemosCb;
  if (setScanFoldersForDemosCb) setScanFoldersForDemos = setScanFoldersForDemosCb;
  if (getAnalyzerExplorerWidthCb) getAnalyzerExplorerWidth = getAnalyzerExplorerWidthCb;
  if (setAnalyzerExplorerWidthCb) setAnalyzerExplorerWidth = setAnalyzerExplorerWidthCb;
  if (openDemoCb) openDemo = openDemoCb;

  initExplorerResize();
  ({ isCaching } = initDemoCache({ getDemoPaths: () => currentFolderDemos.map((demo) => demo.path) }));
  initAnalyzerBrowser();
}

/** The folder the Explorer is showing, or null before one is picked. */
export function browserFolder() {
  return currentDir;
}

// Redraws the Pinned/Recent/Local links after a change made outside the
// pane, such as the start-up pinned-folders review unpinning folders (#533).
export function refreshAnalyzerQuickLinks() {
  renderQuickLinksSection();
}

/** Points the Explorer at `path`'s folder and marks `path` in its Demos
 *  table. Resolves once the folder is listed and the row scrolled to. */
export function revealDemo(path) {
  const parentDir = parentDirOf(path);
  const listing = parentDir && parentDir !== currentDir ? setCurrentDir(parentDir) : Promise.resolve();
  markDemoSelected(path);
  return listing.then(() => scrollDemoRowIntoView(path));
}

/** Marks `path` as the analysed demo in the Demos table. */
export function markDemoSelected(path) {
  browserSelectedDemo = path;
  renderDemoTable();
}

/** Lists `path`'s folder again when the Explorer is showing it (a split
 *  just wrote new demos there). */
export async function relistFolderOf(path) {
  if (currentDir && currentDir === parentDirOf(path)) await setCurrentDir(currentDir);
}

function scrollDemoRowIntoView(path) {
  document.querySelector(`#analyzer-demo-tbody tr[data-path="${CSS.escape(path)}"]`)?.scrollIntoView({ block: 'nearest' });
}
