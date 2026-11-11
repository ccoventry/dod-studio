import { describe, it, expect, vi } from 'vitest';

vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock('./ipc_bridge.js', () => ({ takeCaptureStates: vi.fn(), logFrontendEvent: vi.fn() }));

const { MIGRATIONS, CURRENT_SCHEMA, migrateProject, updateSummary } = await import('./project_migrations.js');
const { attachCaptures } = await import('./take_record.js');
const { projectSchema1 } = await import('./test_fixtures/project_schema1.js');

function schema1TakeIndex() {
  const project = projectSchema1();
  attachCaptures(project.takeIndex, project.takeCaptures);
  return { demos: project.demos, takeIndex: project.takeIndex };
}

describe('the list of steps', () => {
  it('leads from schema 1 up to the current one with no gap', () => {
    let at = 1;
    MIGRATIONS.forEach((step) => {
      expect(step.from).toBe(at);
      expect(step.to).toBeGreaterThan(step.from);
      at = step.to;
    });
    expect(at).toBe(CURRENT_SCHEMA);
  });
});

describe('migrateProject', () => {
  it('leaves everything already in a schema 1 file as it was', () => {
    const data = projectSchema1();
    migrateProject(data, 1);
    expect(data).toEqual(projectSchema1());
  });

  it('runs no step on a file newer than this build', () => {
    const data = { schema: CURRENT_SCHEMA + 5, settings: { picture: {} } };
    const before = structuredClone(data);
    migrateProject(data, CURRENT_SCHEMA + 5);
    expect(data).toEqual(before);
  });

  it('refuses a schema no step leads on from, rather than open it half read', () => {
    expect(() => migrateProject({}, 0)).toThrow('format 0');
  });
});

describe('updateSummary', () => {
  it('counts the clips: every tracked or captured row, Skip left out', async () => {
    const { demos, takeIndex } = schema1TakeIndex();
    const summary = await updateSummary(demos, takeIndex, async (folders) => folders.map(() => ({ exists: true })));
    // Three Keeps with a status, a Maybe with a narrowed range, an HLTV Keep.
    expect(summary.clips).toBe(5);
  });

  it('counts a take whose folder is gone, or was never recorded, as not found', async () => {
    const { demos, takeIndex } = schema1TakeIndex();
    const states = async (folders) => folders.map((_, i) => ({ exists: i === 0 }));
    expect(await updateSummary(demos, takeIndex, states)).toMatchObject({ takesFound: 1, takesNotFound: 2 });
  });

  it('leaves the take counts out when the disk check failed', async () => {
    const { demos, takeIndex } = schema1TakeIndex();
    expect(await updateSummary(demos, takeIndex, async () => null))
      .toMatchObject({ clips: 5, takesFound: null, takesNotFound: null });
  });

  it('asks nothing of the disk for a project with no recorded folders', async () => {
    const fetchStates = vi.fn();
    expect(await updateSummary([], {}, fetchStates)).toEqual({ clips: 0, takesFound: 0, takesNotFound: 0 });
    expect(fetchStates).not.toHaveBeenCalled();
  });
});
