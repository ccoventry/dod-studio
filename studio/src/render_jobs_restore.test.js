import { describe, it, expect, vi } from 'vitest';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRenderJobsRestore } from './render_jobs_restore.js';

const queued = [{ id: '0', status: 'Queued' }, { id: '1', status: 'Finished' }];

describe('createRenderJobsRestore (#771)', () => {
  it("paints the backend's queue on init, with no snapshot yet", async () => {
    const applyJobs = vi.fn();
    const restore = createRenderJobsRestore({ fetchJobs: async () => queued, applyJobs });
    expect(await restore.restore()).toBe(true);
    expect(applyJobs).toHaveBeenCalledWith(queued);
  });

  it('leaves a snapshot that arrived while the request was out alone', async () => {
    const applyJobs = vi.fn();
    let reply;
    const restore = createRenderJobsRestore({
      fetchJobs: () => new Promise((resolve) => { reply = resolve; }),
      applyJobs,
    });
    const pending = restore.restore();
    restore.noteSnapshot();
    reply(queued);
    expect(await pending).toBe(false);
    expect(applyJobs).not.toHaveBeenCalled();
  });

  it('paints nothing when the request failed', async () => {
    const applyJobs = vi.fn();
    const restore = createRenderJobsRestore({ fetchJobs: async () => null, applyJobs });
    expect(await restore.restore()).toBe(false);
    expect(applyJobs).not.toHaveBeenCalled();
  });

  it('is what the Render tab runs on init, through get_render_jobs', () => {
    const dir = path.dirname(fileURLToPath(import.meta.url));
    const pane = fs.readFileSync(path.join(dir, 'render_pane.js'), 'utf8');
    const bridge = fs.readFileSync(path.join(dir, 'ipc_bridge.js'), 'utf8');
    expect(pane).toMatch(/createRenderJobsRestore\(\{\s*fetchJobs: getRenderJobs/);
    expect(pane).toMatch(/jobsRestore\.restore\(\)/);
    expect(bridge).toMatch(/invoke\("get_render_jobs"\)\s*\.catch/);
    const rs = fs.readFileSync(path.join(dir, '..', 'src-tauri', 'src', 'lib.rs'), 'utf8');
    expect(rs).toMatch(/^\s*get_render_jobs,$/m);
  });
});
