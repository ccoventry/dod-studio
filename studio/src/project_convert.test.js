import { describe, it, expect, vi, beforeEach } from 'vitest';
import {
  createProjectConvert, isConvertibleProject, getKeptJsonProjects, setKeptJsonProjects, KEPT_JSON_CAP,
} from './project_convert.js';

// A loaded .json project offers once to switch to .dodstudio (#528).

function harness({ offer = 'D:\\p\\monday.dodstudio', answer = true, convert } = {}) {
  const deps = {
    persist: vi.fn(async () => {}),
    offer: vi.fn(async () => offer),
    convert: vi.fn(convert || (async (path) => path.replace(/\.json$/i, '.dodstudio'))),
    confirm: vi.fn(async () => answer),
    toast: vi.fn(),
  };
  return { deps, convert: createProjectConvert(deps) };
}

beforeEach(() => setKeptJsonProjects([]));

describe('isConvertibleProject', () => {
  it('is a .json project, never an autosave or a .dodstudio', () => {
    expect(isConvertibleProject('D:\\p\\monday.json')).toBe(true);
    expect(isConvertibleProject('D:/p/OLD.JSON')).toBe(true);
    expect(isConvertibleProject('D:\\p\\monday.autosave.json')).toBe(false);
    expect(isConvertibleProject('D:\\p\\monday.dodstudio')).toBe(false);
    expect(isConvertibleProject('')).toBe(false);
  });
});

describe('offerConvert', () => {
  it('Switch renames the project and resolves its new path', async () => {
    const { deps, convert } = harness();
    expect(await convert.offerConvert('D:\\p\\monday.json')).toBe('D:\\p\\monday.dodstudio');
    expect(deps.confirm).toHaveBeenCalledTimes(1);
    const [, opts] = deps.confirm.mock.calls[0];
    expect(opts.confirmLabel).toBe('Switch to .dodstudio');
    expect(opts.cancelLabel).toBe('Keep as .json');
    expect(opts.details[0].primary).toBe('monday.json → monday.dodstudio');
    expect(deps.convert).toHaveBeenCalledWith('D:\\p\\monday.json');
    expect(deps.toast).toHaveBeenCalledWith('Project switched to monday.dodstudio.', 'success');
    expect(getKeptJsonProjects()).toEqual([]);
  });

  it('Keep as .json is remembered for that path and never asked again', async () => {
    const { deps, convert } = harness({ answer: false });
    expect(await convert.offerConvert('D:\\p\\monday.json')).toBe(null);
    expect(deps.convert).not.toHaveBeenCalled();
    expect(getKeptJsonProjects()).toEqual(['D:\\p\\monday.json']);
    expect(deps.persist).toHaveBeenCalledTimes(1);

    // Either slash, any case: the same file.
    expect(await convert.offerConvert('d:/P/Monday.json')).toBe(null);
    expect(deps.confirm).toHaveBeenCalledTimes(1);
    expect(deps.offer).toHaveBeenCalledTimes(1);
  });

  it('asks nothing when the backend has nothing to offer (read-only, name taken, gone)', async () => {
    const { deps, convert } = harness({ offer: null });
    expect(await convert.offerConvert('D:\\p\\monday.json')).toBe(null);
    expect(deps.confirm).not.toHaveBeenCalled();
    expect(getKeptJsonProjects()).toEqual([]);
  });

  it('never asks for a .dodstudio or an autosave', async () => {
    const { deps, convert } = harness();
    expect(await convert.offerConvert('D:\\p\\monday.dodstudio')).toBe(null);
    expect(await convert.offerConvert('D:\\p\\monday.autosave.json')).toBe(null);
    expect(deps.offer).not.toHaveBeenCalled();
  });

  it('a failed rename keeps the project as .json and is not remembered as Keep', async () => {
    const { deps, convert } = harness({ convert: async () => { throw 'D:\\p\\monday.dodstudio already exists'; } });
    expect(await convert.offerConvert('D:\\p\\monday.json')).toBe(null);
    expect(deps.toast).toHaveBeenCalledWith('The project stays as .json: D:\\p\\monday.dodstudio already exists', 'error');
    expect(getKeptJsonProjects()).toEqual([]);
  });
});

describe('the saved Keep list', () => {
  it('loads strings only, once each, capped', () => {
    setKeptJsonProjects(['D:/a.json', 'd:\\A.json', 7, '', 'D:/b.json']);
    expect(getKeptJsonProjects()).toEqual(['D:/a.json', 'D:/b.json']);
    setKeptJsonProjects(undefined);
    expect(getKeptJsonProjects()).toEqual([]);
    setKeptJsonProjects(Array.from({ length: KEPT_JSON_CAP + 5 }, (_, i) => `D:/${i}.json`));
    expect(getKeptJsonProjects()).toHaveLength(KEPT_JSON_CAP);
  });
});
