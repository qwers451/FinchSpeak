import { afterEach, beforeEach, expect, it, vi } from 'vitest';

const systemIdleSeconds = vi.hoisted(() => vi.fn());
const setStatus = vi.hoisted(() => vi.fn());
vi.mock('../services/api', () => ({ api: { systemIdleSeconds, setStatus } }));

import { setCurrentUser } from './authStore';
import { appError, setAppError } from './notificationStore';
import { sessionHandlers, setPresence } from './sessionStore';
import { startAutoIdle } from './idleStore';

const POLL_MS = 30_000;
const IDLE_AFTER_MS = 600_000;
let stop: (() => void) | undefined;
let lastInputAt = 0;
let warn: ReturnType<typeof vi.spyOn>;

/** Idle time follows the fake clock: the user last touched the keyboard or mouse at `lastInputAt`. */
function touch() {
  lastInputAt = Date.now();
}

/** Signs in and lets the self presence echo arrive, as the server sends it. */
function signIn(status: 'online' | 'idle' | 'donotdisturb' | 'offline', text: string | null = null) {
  setCurrentUser({ id: 'me', username: 'me', display_name: 'Me', role: 'member', status, status_text: text });
  setPresence({ me: status });
}

beforeEach(() => {
  vi.useFakeTimers();
  lastInputAt = Date.now();
  systemIdleSeconds.mockReset().mockImplementation(async () => Math.floor((Date.now() - lastInputAt) / 1000));
  setStatus.mockReset().mockResolvedValue(undefined);
  warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
  setAppError(null);
  signIn('online');
});

afterEach(() => {
  stop?.();
  stop = undefined;
  setCurrentUser(null);
  warn.mockRestore();
  vi.useRealTimers();
});

it('goes idle after 600 s away and keeps the status text', async () => {
  signIn('online', 'in a meeting');
  stop = startAutoIdle();
  await vi.advanceTimersByTimeAsync(IDLE_AFTER_MS - POLL_MS);
  expect(setStatus).not.toHaveBeenCalled();
  await vi.advanceTimersByTimeAsync(POLL_MS);
  expect(setStatus).toHaveBeenCalledTimes(1);
  expect(setStatus).toHaveBeenCalledWith('idle', 'in a meeting');
});

it('returns online on activity after an automatic idle', async () => {
  stop = startAutoIdle();
  await vi.advanceTimersByTimeAsync(IDLE_AFTER_MS);
  expect(setStatus).toHaveBeenLastCalledWith('idle', null);
  touch();
  await vi.advanceTimersByTimeAsync(POLL_MS);
  expect(setStatus).toHaveBeenLastCalledWith('online', null);
  expect(setStatus).toHaveBeenCalledTimes(2);
});

it('auto_idle_respects_manual_status', async () => {
  // Do not disturb is never changed, however long the user is away.
  signIn('donotdisturb');
  stop = startAutoIdle();
  await vi.advanceTimersByTimeAsync(IDLE_AFTER_MS * 2);
  expect(setStatus).not.toHaveBeenCalled();
  stop();
  stop = undefined;

  // A manually chosen idle is not changed by the automatic rule until the user has been away.
  signIn('idle');
  stop = startAutoIdle();
  touch();
  await vi.advanceTimersByTimeAsync(POLL_MS);
  expect(setStatus).not.toHaveBeenCalled();
});

it('retries the automatic idle on the next poll when the save fails, without an error banner', async () => {
  setStatus.mockRejectedValueOnce(new Error('offline'));
  stop = startAutoIdle();
  await vi.advanceTimersByTimeAsync(IDLE_AFTER_MS);
  expect(setStatus).toHaveBeenCalledTimes(1);
  await vi.advanceTimersByTimeAsync(POLL_MS);
  expect(setStatus).toHaveBeenCalledTimes(2);
  expect(setStatus).toHaveBeenLastCalledWith('idle', null);
  expect(appError()).toBeNull();
});

it('retries the restore on the next poll when the save fails, without an error banner', async () => {
  stop = startAutoIdle();
  await vi.advanceTimersByTimeAsync(IDLE_AFTER_MS);
  touch();
  setStatus.mockRejectedValueOnce(new Error('offline'));
  await vi.advanceTimersByTimeAsync(POLL_MS);
  expect(setStatus).toHaveBeenCalledTimes(2);
  expect(setStatus).toHaveBeenLastCalledWith('online', null);
  await vi.advanceTimersByTimeAsync(POLL_MS);
  expect(setStatus).toHaveBeenCalledTimes(3);
  expect(setStatus).toHaveBeenLastCalledWith('online', null);
  await vi.advanceTimersByTimeAsync(POLL_MS);
  expect(setStatus).toHaveBeenCalledTimes(3);
  expect(appError()).toBeNull();
});

it('restores online even when the presence echo never arrives', async () => {
  stop = startAutoIdle();
  await vi.advanceTimersByTimeAsync(IDLE_AFTER_MS);
  // The server echo is lost: the local status is still what the user had before.
  signIn('online');
  touch();
  await vi.advanceTimersByTimeAsync(POLL_MS);
  expect(setStatus).toHaveBeenLastCalledWith('online', null);
});

it('does not restore online when the user switched to do not disturb while away', async () => {
  stop = startAutoIdle();
  await vi.advanceTimersByTimeAsync(IDLE_AFTER_MS);
  expect(setStatus).toHaveBeenCalledTimes(1);
  signIn('donotdisturb');
  touch();
  await vi.advanceTimersByTimeAsync(POLL_MS * 2);
  expect(setStatus).toHaveBeenCalledTimes(1);
});

it('auto_idle_reads_my_status_from_presence_when_user_has_no_status', async () => {
  // Restart: the presence echo ('online') came before the user record, which has no status field.
  sessionHandlers['presence-update']({ user_id: 'me', status: 'online' });
  setCurrentUser({ id: 'me', username: 'me', display_name: 'Me', role: 'member' });
  stop = startAutoIdle();
  await vi.advanceTimersByTimeAsync(IDLE_AFTER_MS);
  expect(setStatus).toHaveBeenCalledWith('idle', null);
});

it('auto_idle_respects_do_not_disturb_from_presence_when_user_has_no_status', async () => {
  sessionHandlers['presence-update']({ user_id: 'me', status: 'donotdisturb' });
  setCurrentUser({ id: 'me', username: 'me', display_name: 'Me', role: 'member' });
  stop = startAutoIdle();
  await vi.advanceTimersByTimeAsync(IDLE_AFTER_MS * 2);
  expect(setStatus).not.toHaveBeenCalled();
});

it('stops polling when the returned function is called', async () => {
  stop = startAutoIdle();
  stop();
  stop = undefined;
  await vi.advanceTimersByTimeAsync(IDLE_AFTER_MS * 2);
  expect(systemIdleSeconds).not.toHaveBeenCalled();
});
