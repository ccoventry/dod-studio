// project_paths.js
// Which folders a project file records, and keeping the app's pinned-folder
// list to folders. Pure, so it can be tested on its own.

const isDemoFile = (p) => /\.dem$/i.test(String(p));
const folderOf = (p) => String(p).replace(/[\\/][^\\/]*$/, '');
const pathKey = (p) => String(p).replace(/\//g, '\\').replace(/\\+$/, '').toLowerCase();

/**
 * The folders a project's own demos are in, each once (paths compare the way
 * Windows does), in the order the demos first name them. This is what a
 * project file saves as `scanPaths`, not the app-wide pinned list, which
 * collects every folder and file ever added and has nothing to do with the
 * project.
 */
export function projectFolders(demos) {
  const seen = new Set();
  const folders = [];
  (demos || []).forEach((demo) => {
    if (!demo?.path) return;
    const folder = folderOf(demo.path);
    const key = pathKey(folder);
    if (folder && !seen.has(key)) {
      seen.add(key);
      folders.push(folder);
    }
  });
  return folders;
}

/**
 * The pinned list with the single `.dem` files and repeats taken out.
 * `+ Add Demo Files` used to add every file it was given, so the list grew
 * by one path per demo, forever. Folders stay, even ones that don't exist
 * right now: a drive can be unplugged.
 */
export function pinnedFoldersOnly(list) {
  const seen = new Set();
  return (list || []).filter((p) => {
    if (!p || isDemoFile(p)) return false;
    const key = pathKey(p);
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}
