import { describe, it, expect } from 'vitest';
import { fileNameOf, folderOf, shortFolder, samePath } from './path_display.js';

const post = 'C:\\Program Files (x86)\\Steam\\steamapps\\common\\Half-Life - POST-Anniversary for Movies\\dod\\test stuff\\subfolder';

describe('path_display', () => {
  it('splits a path into folder and file name', () => {
    expect(fileNameOf(`${post}\\a.dem`)).toBe('a.dem');
    expect(folderOf(`${post}\\a.dem`)).toBe(post);
  });

  it('keeps short folders whole', () => {
    expect(shortFolder('D:\\Demos\\wsod25')).toBe('D:\\Demos\\wsod25');
  });

  it('keeps the drive and as many last folders as fit', () => {
    expect(shortFolder(post, 48)).toBe('C:\\…\\dod\\test stuff\\subfolder');
    expect(shortFolder(post, 48).length).toBeLessThanOrEqual(48);
    expect(shortFolder(post, 25)).toBe('C:\\…\\test stuff\\subfolder');
  });

  it('always keeps the last folder, even when it alone is too long', () => {
    expect(shortFolder('D:\\a\\a-very-long-folder-name-for-demos', 10)).toBe('D:\\…\\a-very-long-folder-name-for-demos');
  });

  it('uses forward slashes when the path does', () => {
    expect(shortFolder('C:/one/two/three/four/five', 16)).toBe('C:/…/four/five');
  });
});

describe('samePath', () => {
  it('ignores case and slash direction', () => {
    expect(samePath('C:\\Demos\\A.dem', 'c:/demos/a.dem')).toBe(true);
    expect(samePath('C:\\demos\\a.dem', 'C:\\demos\\b.dem')).toBe(false);
  });

  it('is false when either side is missing', () => {
    expect(samePath(null, 'C:\\a.dem')).toBe(false);
    expect(samePath('C:\\a.dem', '')).toBe(false);
  });
});
