import { afterEach, beforeEach, expect, it, vi } from 'vitest';

const systemIdleSeconds = vi.hoisted(() => vi.fn());
const saveStatus = vi.hoisted(() => vi.fn());
vi.mock('../services/api', () => ({ api: { systemIdleSeconds } }));
vi.mock('./profileStore', () => ({ saveStatus }));

import { setCurrentUser } from './authStore';
import { startAutoIdle } from './idleStore';

const POLL_MS = 30_000;
const IDLE_AFTER_MS = 600_000;
let stop: (() => void) | undefined;
let lastInputAt = 0;

/** Idle time follows the fake clock: the user last touched the keyboard or mouse at `lastInputAt`. */
function touch() {
  lastInputAt = Date.now();
}

function signIn(status: 'online' | 'idle' | 'donotdisturb' | 'offline', text: string | null = null) {
  setCurrentUser({ id: 'me', username: 'me', display_name: 'Me', role: 'member', status, status_text: text });
}

beforeEach(() => {
  vi.useFakeTimers();
  lastInputAt = Date.now();
  systemIdleSeconds.mockReset().mockImplementation(async () => Math.floor((Date.now() - lastInputAt) / 1000));
  saveStatus.mockReset().mockResolvedValue(true);
  signIn('online');
});

afterEach(() => {
  stop?.();
  stop = undefined;
  setCurrentUser(null);
  vi.useRealTimers();
});

it('goes idle after 600 s away and keeps the status text', async () => {
  signIn('online', 'in a meeting');
  stop = startAutoIdle();
  await vi.advanceTimersByTimeAsync(IDLE_AFTER_MS - POLL_MS);
  expect(saveStatus).not.toHaveBeenCalled();
  await vi.advanceTimersByTimeAsync(POLL_MS);
  expect(saveStatus).toHaveBeenCalledTimes(1);
  expect(saveStatus).toHaveBeenCalledWith('idle', 'in a meeting');
});

it('returns online on activity after an automatic idle', async () => {
  stop = startAutoIdle();
  await vi.advanceTimersByTimeAsync(IDLE_AFTER_MS);
  expect(saveStatus).toHaveBeenLastCalledWith('idle', '');
  signIn('idle');
  touch();
  await vi.advanceTimersByTimeAsync(POLL_MS);
  expect(saveStatus).toHaveBeenLastCalledWith('online', '');
  expect(saveStatus).toHaveBeenCalledTimes(2);
});

it('auto_idle_respects_manual_status', async () => {
  // Do not disturb is never changed, however long the user is away.
  signIn('donotdisturb');
  stop = startAutoIdle();
  await vi.advanceTimersByTimeAsync(IDLE_AFTER_MS * 2);
  expect(saveStatus).not.toHaveBeenCalled();
  stop();
  stop = undefined;

  // A manually chosen idle is not reset to online when the user comes back.
  signIn('idle');
  stop = startAutoIdle();
  touch();
  await vi.advanceTimersByTimeAsync(POLL_MS);
  expect(saveStatus).not.toHaveBeenCalled();
  await vi.advanceTimersByTimeAsync(IDLE_AFTER_MS);
  expect(saveStatus).not.toHaveBeenCalled();
  touch();
  await vi.advanceTimersByTimeAsync(POLL_MS);
  expect(saveStatus).not.toHaveBeenCalled();
});

it('stops polling when the returned function is called', async () => {
  stop = startAutoIdle();
  stop();
  stop = undefined;
  await vi.advanceTimersByTimeAsync(IDLE_AFTER_MS * 2);
  expect(systemIdleSeconds).not.toHaveBeenCalled();
});
