import { describe, it, expect } from 'vitest';
import { queueSummaryText, batchEndStatus } from './render_batch_status.js';
import { JOB_STATUS } from './render_jobs.js';

const jobs = (spec) => Object.entries(spec).flatMap(([status, n]) => Array.from({ length: n }, () => ({ status })));

describe('queueSummaryText', () => {
  it('counts only finished renders as done, and names cancelled and failed rows', () => {
    expect(queueSummaryText(jobs({ [JOB_STATUS.FINISHED]: 44, [JOB_STATUS.CANCELLED]: 21 })))
      .toBe('0 queued · 0 rendering · 44 done · 21 cancelled');
    expect(queueSummaryText(jobs({ [JOB_STATUS.QUEUED]: 2, [JOB_STATUS.RENDERING]: 1, [JOB_STATUS.ERROR]: 1 })))
      .toBe('2 queued · 1 rendering · 0 done · 1 failed');
  });

  it('stays short with nothing cancelled or failed', () => {
    expect(queueSummaryText([])).toBe('0 queued · 0 rendering · 0 done');
  });
});

describe('batchEndStatus', () => {
  it('says Cancelled when the whole batch was cancelled', () => {
    expect(batchEndStatus(0, 0, 21)).toEqual({ text: 'Status: Cancelled', status: JOB_STATUS.CANCELLED });
  });

  it('says Finished when everything rendered', () => {
    expect(batchEndStatus(5, 0, 0)).toEqual({ text: 'Status: Finished', status: JOB_STATUS.FINISHED });
  });

  it('gives the counts for a mixed batch, in the failure colour when one failed', () => {
    expect(batchEndStatus(3, 0, 18)).toEqual({ text: 'Status: Finished — 3 rendered, 18 cancelled', status: JOB_STATUS.CANCELLED });
    expect(batchEndStatus(3, 1, 0)).toEqual({ text: 'Status: Finished — 3 rendered, 1 failed', status: JOB_STATUS.ERROR });
  });

  it('says how long it took when it knows (#954)', () => {
    expect(batchEndStatus(10, 0, 0, '7 min 40 s')).toEqual({ text: 'Done in 7 min 40 s: 10 rendered', status: JOB_STATUS.FINISHED });
    expect(batchEndStatus(3, 1, 2, '42 s')).toEqual({ text: 'Done in 42 s: 3 rendered, 1 failed, 2 cancelled', status: JOB_STATUS.ERROR });
    expect(batchEndStatus(0, 0, 4, '42 s')).toEqual({ text: 'Status: Cancelled', status: JOB_STATUS.CANCELLED });
    expect(batchEndStatus(0, 0, 0, '0.1 s')).toEqual({ text: 'Status: Finished', status: JOB_STATUS.FINISHED });
  });
});
