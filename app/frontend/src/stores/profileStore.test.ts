import { expect, it, vi } from 'vitest';
import { api } from '../services/api';
import { currentUser, logoutUser, setCurrentUser } from './authStore';
import { endSession, loadSessions } from './profileStore';
import { sessions, setSessions } from './sessionStore';

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => { resolve = res; });
  return { promise, resolve };
}

const alice = { id: 'alice', username: 'alice', display_name: 'Alice', role: 'member' as const };
const bob = { id: 'bob', username: 'bob', display_name: 'Bob', role: 'member' as const };
const deviceA = { id: 'device-a', device_name: 'PC-A', created_at: 1, last_seen_at: 1, current: false };
const deviceB = { id: 'device-b', device_name: 'PC-B', created_at: 2, last_seen_at: 2, current: true };

it('a late device list never crosses accounts', async () => {
  vi.spyOn(api, 'listChannels').mockResolvedValue([]);
  setCurrentUser(alice);
  const lateA = deferred<typeof deviceA[]>();
  vi.spyOn(api, 'listSessions').mockReturnValueOnce(lateA.promise);
  const pendingA = loadSessions();
  vi.spyOn(api, 'logout').mockResolvedValue();
  await logoutUser();
  vi.spyOn(api, 'login').mockResolvedValue(bob);
  vi.spyOn(api, 'listSessions').mockResolvedValueOnce([deviceB]);
  const { loginUser } = await import('./authStore');
  await loginUser('bob', 'hunter2hunter2');
  await loadSessions();
  expect(sessions()).toEqual([deviceB]);
  lateA.resolve([deviceA]);
  await pendingA;
  expect(currentUser()).toEqual(bob);
  expect(sessions()).toEqual([deviceB]);
  await logoutUser();
  vi.restoreAllMocks();
});

it('a late device list does not refill the list after sign-out', async () => {
  vi.spyOn(api, 'listChannels').mockResolvedValue([]);
  setCurrentUser(alice);
  const lateA = deferred<typeof deviceA[]>();
  vi.spyOn(api, 'listSessions').mockReturnValueOnce(lateA.promise);
  const pendingA = loadSessions();
  vi.spyOn(api, 'logout').mockResolvedValue();
  await logoutUser();
  lateA.resolve([deviceA]);
  await pendingA;
  expect(sessions()).toEqual([]);
  vi.restoreAllMocks();
});

it('ending this computer from the device list closes the profile and its old message', async () => {
  const { endSession, profileOpen, setProfileOpen, profileMessage, setProfileMessage } = await import('./profileStore');
  setCurrentUser(alice);
  setSessions([deviceB]);
  setProfileOpen(true);
  setProfileMessage('Password changed. Other devices are signed out.');
  vi.spyOn(api, 'logout').mockResolvedValue();
  await endSession(deviceB.id);
  expect(currentUser()).toBeNull();
  expect(profileOpen()).toBe(false);
  expect(profileMessage()).toBeNull();
  vi.restoreAllMocks();
});

it('a sign-out clears the profile message', async () => {
  const { profileMessage, setProfileMessage } = await import('./profileStore');
  setCurrentUser(alice);
  setProfileMessage('Name changed.');
  vi.spyOn(api, 'logout').mockResolvedValue();
  await logoutUser();
  expect(profileMessage()).toBeNull();
  vi.restoreAllMocks();
});
