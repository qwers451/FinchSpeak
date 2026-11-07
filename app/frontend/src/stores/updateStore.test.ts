import { afterEach, beforeEach, expect, it, vi } from 'vitest';

const checkUpdate = vi.hoisted(() => vi.fn());
const toast = vi.hoisted(() => vi.fn());
vi.mock('../services/api', () => ({ api: { checkUpdate } }));
vi.mock('../ui', () => ({ pushToast: toast }));

import { availableUpdate, setAvailableUpdate, startUpdateChecks } from './updateStore';

const FIRST_MS = 15_000;
const DAY_MS = 24 * 60 * 60 * 1000;
let stop: (() => void) | undefined;

beforeEach(() => {
  vi.useFakeTimers();
  checkUpdate.mockReset().mockResolvedValue(null);
  toast.mockReset();
  setAvailableUpdate(null);
});
afterEach(() => {
  stop?.();
  stop = undefined;
  vi.useRealTimers();
});

it('does not check before 15 seconds and checks once at 15 seconds', async () => {
  stop = startUpdateChecks();
  await vi.advanceTimersByTimeAsync(FIRST_MS - 1);
  expect(checkUpdate).not.toHaveBeenCalled();
  await vi.advanceTimersByTimeAsync(1);
  expect(checkUpdate).toHaveBeenCalledTimes(1);
});

it('checks again 24 hours after the first check', async () => {
  stop = startUpdateChecks();
  await vi.advanceTimersByTimeAsync(FIRST_MS);
  await vi.advanceTimersByTimeAsync(DAY_MS - 1);
  expect(checkUpdate).toHaveBeenCalledTimes(1);
  await vi.advanceTimersByTimeAsync(1);
  expect(checkUpdate).toHaveBeenCalledTimes(2);
});

it('shows one toast per version', async () => {
  checkUpdate.mockResolvedValue({ version: '9.9.9', notes: 'n' });
  stop = startUpdateChecks();
  await vi.advanceTimersByTimeAsync(FIRST_MS);
  await vi.advanceTimersByTimeAsync(DAY_MS);
  expect(checkUpdate).toHaveBeenCalledTimes(2);
  expect(toast).toHaveBeenCalledTimes(1);
  expect(availableUpdate()?.version).toBe('9.9.9');
});

it('stays silent when a check fails and stops when disposed', async () => {
  const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
  checkUpdate.mockRejectedValue(new Error('offline'));
  stop = startUpdateChecks();
  await vi.advanceTimersByTimeAsync(FIRST_MS);
  expect(toast).not.toHaveBeenCalled();
  expect(warn).toHaveBeenCalled();
  stop();
  await vi.advanceTimersByTimeAsync(DAY_MS);
  expect(checkUpdate).toHaveBeenCalledTimes(1);
  warn.mockRestore();
});
