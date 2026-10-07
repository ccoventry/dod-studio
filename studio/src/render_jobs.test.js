import { describe, it, expect } from 'vitest';
import { sortJobs, nextSort, batchProgress } from './render_jobs.js';

const job = (id, extra) => ({ id, name: `clip-${id}`, status: 'Queued', progress: 0, ...extra });

describe('sortJobs', () => {
  const jobs = [
    job('0', { name: 'b', frames: 300, status: 'Finished', output_size_bytes: 10 }),
    job('1', { name: 'A', frames: 100, status: 'Rendering' }),
    job('2', { name: 'c', frames: 100, status: 'Queued' }),
  ];

  it('keeps job order without a sort', () => {
    expect(sortJobs(jobs, null).map((j) => j.id)).toEqual(['0', '1', '2']);
  });

  it('sorts names ignoring case, both ways', () => {
    expect(sortJobs(jobs, { column: 'name', dir: 'asc' }).map((j) => j.id)).toEqual(['1', '0', '2']);
    expect(sortJobs(jobs, { column: 'name', dir: 'desc' }).map((j) => j.id)).toEqual(['2', '0', '1']);
  });

  it('keeps job order for ties, and groups statuses in working order', () => {
    expect(sortJobs(jobs, { column: 'frames', dir: 'asc' }).map((j) => j.id)).toEqual(['1', '2', '0']);
    expect(sortJobs(jobs, { column: 'status', dir: 'asc' }).map((j) => j.id)).toEqual(['1', '2', '0']);
  });

  it('does not reorder the array it was given', () => {
    sortJobs(jobs, { column: 'name', dir: 'asc' });
    expect(jobs.map((j) => j.id)).toEqual(['0', '1', '2']);
  });
});

describe('nextSort', () => {
  it('cycles ascending, descending, off, and restarts on a new column', () => {
    let s = nextSort(null, 'name');
    expect(s).toEqual({ column: 'name', dir: 'asc' });
    s = nextSort(s, 'name');
    expect(s).toEqual({ column: 'name', dir: 'desc' });
    expect(nextSort(s, 'name')).toBe(null);
    expect(nextSort(s, 'size')).toEqual({ column: 'size', dir: 'asc' });
  });
});

describe('batchProgress', () => {
  it('averages done, rendering and queued jobs, leaving cancelled out', () => {
    expect(batchProgress([
      job('0', { status: 'Finished' }),
      job('1', { status: 'Rendering', progress: 50 }),
      job('2', { status: 'Queued' }),
      job('3', { status: 'Error' }),
      job('4', { status: 'Cancelled' }),
    ])).toBe(63);
  });

  it('is null with nothing to count', () => {
    expect(batchProgress([])).toBe(null);
    expect(batchProgress([job('0', { status: 'Cancelled' })])).toBe(null);
  });
});
