// demo_links.js
// Demo folders linked into the game folder (#376): + Link Folder to Game
// links a picked folder on any drive under the game's dod\demos\, so
// `viewdemo "demos/<link>/<demo>"` plays it in place; the list shows each
// link with its console line and a Remove button that unlinks it, leaving
// the folder alone. The rules (what is refused, the link's name) live in
// native::demo_links; the backend also writes the activity-log lines.

import { pickPath } from './pick_path.js';
import { listDemoLinks, addDemoLink, removeDemoLink } from './ipc_bridge.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';

const S = STRINGS.DEMO_LINKS;

async function pickDemoFolder() {
  const picked = await pickPath({ field: 'demo_links.folder', directory: true, multiple: false, title: S.PICK_TITLE });
  return Array.isArray(picked) ? picked[0] : picked;
}

/**
 * The links and the actions on them, without the page: `view` draws.
 *
 * @param {object} deps
 *   getHlExe() the Half-Life executable setting ('' when unset);
 *   view { render({ links, hasGame }), setBusy(busy) };
 *   pickFolder() a folder path or null (the native picker by default);
 *   toast(message, type, ms) (showToast by default).
 */
export function createDemoLinks({ getHlExe, view, pickFolder = pickDemoFolder, toast = showToast }) {
  let links = [];
  let busy = false;

  async function refresh() {
    const hlExe = getHlExe();
    if (!hlExe) {
      links = [];
    } else {
      try {
        links = await listDemoLinks(hlExe);
      } catch (err) {
        links = [];
        toast(S.listFailed(String(err)), 'error', 8000);
      }
    }
    view.render({ links, hasGame: Boolean(hlExe) });
    return links;
  }

  // Runs `work` with the controls disabled, one action at a time.
  async function whileBusy(work) {
    if (busy) return null;
    busy = true;
    view.setBusy(true);
    try {
      return await work();
    } finally {
      busy = false;
      view.setBusy(false);
    }
  }

  /** Picks a folder and links it. The new link, or null. */
  async function linkFolder() {
    const hlExe = getHlExe();
    if (!hlExe) {
      toast(S.NEED_GAME, 'error', 8000);
      return null;
    }
    const folder = await pickFolder();
    if (!folder) return null;
    return whileBusy(async () => {
      try {
        const link = await addDemoLink(hlExe, folder);
        toast(S.linked(link.name), 'success', 8000);
        await refresh();
        return link;
      } catch (err) {
        toast(String(err), 'error', 10000);
        return null;
      }
    });
  }

  /** Unlinks `name`. True when it is gone. */
  async function unlink(name) {
    const hlExe = getHlExe();
    if (!hlExe) return false;
    const done = await whileBusy(async () => {
      try {
        await removeDemoLink(hlExe, name);
        toast(S.removed(name), 'success', 6000);
        return true;
      } catch (err) {
        toast(String(err), 'error', 10000);
        return false;
      } finally {
        await refresh();
      }
    });
    return done === true;
  }

  return { refresh, linkFolder, unlink, getLinks: () => links };
}

/** One list row's text: what the user sees for a link. */
export function linkRowText(link) {
  return {
    name: link.name,
    target: S.pointsTo(link.target),
    console: S.consoleLine(link.console_prefix),
    removeTitle: S.removeTitle(link.target),
  };
}

/** Wires the section in the scan panel. `getHlExe` reads the setting. */
export function initDemoLinks(getHlExe) {
  const button = document.querySelector('#link-folder-btn');
  const list = document.querySelector('#demo-links-list');
  if (!button || !list) return null;

  let actions = null;
  const view = {
    setBusy(isBusy) {
      button.disabled = isBusy;
      button.textContent = isBusy ? S.LINKING : S.LINK_BUTTON;
      list.querySelectorAll('button').forEach((b) => { b.disabled = isBusy; });
    },
    render({ links }) {
      list.replaceChildren();
      if (links.length === 0) {
        const empty = document.createElement('li');
        empty.className = 'demo-link-empty';
        empty.textContent = S.EMPTY;
        list.append(empty);
        return;
      }
      for (const link of links) {
        const text = linkRowText(link);
        const row = document.createElement('li');
        row.className = 'demo-link-row';
        const info = document.createElement('div');
        info.className = 'demo-link-info';
        const name = document.createElement('strong');
        name.textContent = text.name;
        const target = document.createElement('span');
        target.className = 'demo-link-target';
        target.textContent = text.target;
        const line = document.createElement('code');
        line.className = 'demo-link-console';
        line.textContent = text.console;
        info.append(name, ' ', target, document.createElement('br'), line);
        const remove = document.createElement('button');
        remove.textContent = S.REMOVE;
        remove.title = text.removeTitle;
        remove.addEventListener('click', () => actions.unlink(link.name));
        row.append(info, remove);
        list.append(row);
      }
    },
  };

  actions = createDemoLinks({ getHlExe, view });
  button.addEventListener('click', () => actions.linkFolder());
  document.querySelector('#hl-path-input')?.addEventListener('change', () => actions.refresh());
  actions.refresh();
  return actions;
}
