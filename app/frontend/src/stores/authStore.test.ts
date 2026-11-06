import { expect, it, vi } from 'vitest';
import { api } from '../services/api';
import { currentUser, logoutUser, setCurrentUser } from './authStore';
import { appError } from './notificationStore';
import { sessions } from './profileStore';
import { setSessions } from './sessionStore';
import { isMuted, isDeafened, setIsMuted, setIsDeafened } from './voiceStore';

it('sign-out clears the device list so the next account never sees it', async () => {
  setSessions([{ id: 's1', device_name: 'DESKTOP-HOME', created_at: 1, last_seen_at: 1, current: true }]);
  vi.spyOn(api, 'logout').mockResolvedValue();
  await logoutUser();
  expect(sessions()).toEqual([]);
  vi.restoreAllMocks();
});

it('preserves mute preferences when Rust logout preserves its audio flags', async () => {
  vi.spyOn(api, 'logout').mockResolvedValue();
  setIsMuted(true);
  setIsDeafened(true);
  await logoutUser();
  expect(isMuted()).toBe(true);
  expect(isDeafened()).toBe(true);
  vi.restoreAllMocks();
});

it('clears local auth and reports when remote token revocation fails', async () => {
  vi.spyOn(api, 'logout').mockRejectedValue(new Error('remote revoke failed'));
  setCurrentUser({ id: 'me', username: 'me', display_name: 'Me', role: 'member' });
  await expect(logoutUser()).resolves.toBeUndefined();
  expect(currentUser()).toBeNull();
  expect(appError()).toContain('remote revoke failed');
  vi.restoreAllMocks();
});

it('resumeLastSession signs in with the remembered account', async () => {
  const { resumeLastSession, setRememberedUsername } = await import('./authStore');
  setCurrentUser(null);
  setRememberedUsername('alice');
  const user = { id: 'u', username: 'alice', display_name: 'Alice', role: 'member' as const };
  const spy = vi.spyOn(api, 'resumeSession').mockResolvedValue(user);
  await expect(resumeLastSession()).resolves.toBe(true);
  expect(spy).toHaveBeenCalledWith(expect.any(String), 'alice');
  expect(currentUser()).toEqual(user);
  setCurrentUser(null);
  vi.restoreAllMocks();
});

it('resume_failure_keeps_user_on_form_with_error', async () => {
  const { resumeLastSession, setRememberedUsername, authError } = await import('./authStore');
  setCurrentUser(null);
  setRememberedUsername('alice');
  vi.spyOn(api, 'resumeSession').mockRejectedValue('Cannot reach the server: timeout');
  await expect(resumeLastSession()).resolves.toBe(false);
  expect(currentUser()).toBeNull();
  expect(authError()).toContain('Cannot reach');
  vi.restoreAllMocks();
});

it('resume is skipped without a remembered username', async () => {
  const { resumeLastSession, setRememberedUsername } = await import('./authStore');
  setRememberedUsername('');
  const spy = vi.spyOn(api, 'resumeSession').mockResolvedValue(null);
  await expect(resumeLastSession()).resolves.toBe(false);
  expect(spy).not.toHaveBeenCalled();
  vi.restoreAllMocks();
});

it('a successful sign-in forgets the pending invite', async () => {
  const { setPendingInvite, pendingInvite } = await import('./inviteLink');
  const { loginUser } = await import('./authStore');
  setPendingInvite({ server: 'https://h:8443', code: 'K7M2QX9TPW4B', fp: null });
  vi.spyOn(api, 'login').mockResolvedValue({ id: 'u', username: 'alice', display_name: 'Alice', role: 'member' });
  await loginUser('alice', 'hunter2hunter2');
  expect(pendingInvite()).toBeNull();
  setCurrentUser(null);
  vi.restoreAllMocks();
});

it('sign-out forgets the pending invite', async () => {
  const { setPendingInvite, pendingInvite } = await import('./inviteLink');
  setPendingInvite({ server: 'https://h:8443', code: 'K7M2QX9TPW4B', fp: null });
  vi.spyOn(api, 'logout').mockResolvedValue();
  await logoutUser();
  expect(pendingInvite()).toBeNull();
  vi.restoreAllMocks();
});

it('a session that the server ended forgets the pending invite', async () => {
  const { setPendingInvite, pendingInvite } = await import('./inviteLink');
  const { sessionHandlers } = await import('./sessionStore');
  setPendingInvite({ server: 'https://h:8443', code: 'K7M2QX9TPW4B', fp: null });
  sessionHandlers['connection-lost']('token revoked');
  expect(pendingInvite()).toBeNull();
  sessionHandlers['connection-lost']('');
});

it('an unreachable server during the automatic resume is retried with backoff', async () => {
  vi.useFakeTimers();
  const { resumeLastSession, setRememberedUsername, resumeFailed } = await import('./authStore');
  setCurrentUser(null);
  setRememberedUsername('alice');
  const user = { id: 'u', username: 'alice', display_name: 'Alice', role: 'member' as const };
  const spy = vi.spyOn(api, 'resumeSession')
    .mockRejectedValueOnce('Cannot reach the server: connection refused')
    .mockRejectedValueOnce('Cannot reach the server: connection refused')
    .mockResolvedValue(user);
  await expect(resumeLastSession()).resolves.toBe(false);
  expect(resumeFailed()).toBe(true);
  await vi.advanceTimersByTimeAsync(2_000);
  expect(spy).toHaveBeenCalledTimes(2);
  await vi.advanceTimersByTimeAsync(5_000);
  expect(spy).toHaveBeenCalledTimes(3);
  expect(currentUser()).toEqual(user);
  expect(resumeFailed()).toBe(false);
  setCurrentUser(null);
  vi.restoreAllMocks();
  vi.useRealTimers();
});

it('the automatic resume gives up after four retries', async () => {
  vi.useFakeTimers();
  const { resumeLastSession, setRememberedUsername } = await import('./authStore');
  setCurrentUser(null);
  setRememberedUsername('alice');
  const spy = vi.spyOn(api, 'resumeSession').mockRejectedValue('Cannot reach the server: timeout');
  await resumeLastSession();
  await vi.advanceTimersByTimeAsync(2_000 + 5_000 + 15_000 + 30_000 + 120_000);
  expect(spy).toHaveBeenCalledTimes(5);
  vi.restoreAllMocks();
  vi.useRealTimers();
});

it('a rejected resume is not retried', async () => {
  vi.useFakeTimers();
  const { resumeLastSession, setRememberedUsername } = await import('./authStore');
  setCurrentUser(null);
  setRememberedUsername('alice');
  const spy = vi.spyOn(api, 'resumeSession').mockRejectedValue('Session revoked');
  await resumeLastSession();
  await vi.advanceTimersByTimeAsync(120_000);
  expect(spy).toHaveBeenCalledTimes(1);
  vi.restoreAllMocks();
  vi.useRealTimers();
});

it('typing a sign-in stops the automatic resume retries', async () => {
  vi.useFakeTimers();
  const { resumeLastSession, loginUser, setRememberedUsername } = await import('./authStore');
  setCurrentUser(null);
  setRememberedUsername('alice');
  const resume = vi.spyOn(api, 'resumeSession').mockRejectedValue('Cannot reach the server: timeout');
  vi.spyOn(api, 'login').mockRejectedValue('Invalid username or password');
  await resumeLastSession();
  await loginUser('alice', 'wrong-password');
  await vi.advanceTimersByTimeAsync(120_000);
  expect(resume).toHaveBeenCalledTimes(1);
  vi.restoreAllMocks();
  vi.useRealTimers();
});

it('logging out with an already revoked token is not an error', async () => {
  const { appError } = await import('./notificationStore');
  vi.spyOn(api, 'logout').mockRejectedValue('Logged out locally, but could not revoke server token: HTTP 401: Session revoked');
  setCurrentUser({ id: 'me', username: 'me', display_name: 'Me', role: 'member' });
  await expect(logoutUser()).resolves.toBeUndefined();
  expect(currentUser()).toBeNull();
  expect(appError()).toBeNull();
  vi.restoreAllMocks();
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

it('an automatic resume that fails after a manual sign-in is not retried and shows no error', async () => {
  vi.useFakeTimers();
  const { resumeLastSession, loginUser, setRememberedUsername, resumeFailed, authError } = await import('./authStore');
  setCurrentUser(null);
  setRememberedUsername('alice');
  const inFlight = deferred<null>();
  const resume = vi.spyOn(api, 'resumeSession').mockReturnValue(inFlight.promise as never);
  const manual = { id: 'bob', username: 'bob', display_name: 'Bob', role: 'member' as const };
  vi.spyOn(api, 'login').mockResolvedValue(manual);
  const pending = resumeLastSession();
  const signIn = loginUser('bob', 'hunter2hunter2');
  inFlight.reject('Cannot reach the server: connection refused');
  await signIn;
  await expect(pending).resolves.toBe(false);
  await vi.advanceTimersByTimeAsync(120_000);
  expect(resume).toHaveBeenCalledTimes(1);
  expect(currentUser()).toEqual(manual);
  expect(authError()).toBeNull();
  expect(resumeFailed()).toBe(false);
  setCurrentUser(null);
  vi.restoreAllMocks();
  vi.useRealTimers();
});

it('an automatic resume that succeeds after a manual sign-in does not replace the manual user', async () => {
  const { resumeLastSession, loginUser, setRememberedUsername } = await import('./authStore');
  setCurrentUser(null);
  setRememberedUsername('alice');
  const inFlight = deferred<{ id: string; username: string; display_name: string; role: 'member' }>();
  vi.spyOn(api, 'resumeSession').mockReturnValue(inFlight.promise);
  const manual = { id: 'bob', username: 'bob', display_name: 'Bob', role: 'member' as const };
  vi.spyOn(api, 'login').mockResolvedValue(manual);
  const pending = resumeLastSession();
  const signIn = loginUser('bob', 'hunter2hunter2');
  inFlight.resolve({ id: 'alice', username: 'alice', display_name: 'Alice', role: 'member' });
  await expect(pending).resolves.toBe(false);
  await signIn;
  expect(currentUser()).toEqual(manual);
  setCurrentUser(null);
  vi.restoreAllMocks();
});

it('Retry while an automatic resume is in flight does not start a second resume', async () => {
  const { resumeLastSession, setRememberedUsername } = await import('./authStore');
  setCurrentUser(null);
  setRememberedUsername('alice');
  const inFlight = deferred<{ id: string; username: string; display_name: string; role: 'member' }>();
  const resume = vi.spyOn(api, 'resumeSession').mockReturnValue(inFlight.promise);
  const first = resumeLastSession();
  const retry = resumeLastSession();
  expect(resume).toHaveBeenCalledTimes(1);
  const user = { id: 'alice', username: 'alice', display_name: 'Alice', role: 'member' as const };
  inFlight.resolve(user);
  await expect(first).resolves.toBe(true);
  await expect(retry).resolves.toBe(true);
  expect(resume).toHaveBeenCalledTimes(1);
  expect(currentUser()).toEqual(user);
  setCurrentUser(null);
  vi.restoreAllMocks();
});

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
const alice = { id: 'alice', username: 'alice', display_name: 'Alice', role: 'member' as const };
const bob = { id: 'bob', username: 'bob', display_name: 'Bob', role: 'member' as const };

it('a manual sign-in waits for a running automatic resume, so its login is the last backend call', async () => {
  const { resumeLastSession, loginUser, setRememberedUsername } = await import('./authStore');
  setCurrentUser(null);
  setRememberedUsername('alice');
  const calls: string[] = [];
  const inFlight = deferred<typeof alice>();
  vi.spyOn(api, 'resumeSession').mockImplementation(() => { calls.push('resume'); return inFlight.promise; });
  const login = vi.spyOn(api, 'login').mockImplementation(async () => { calls.push('login'); return bob; });
  const pending = resumeLastSession();
  const signIn = loginUser('bob', 'hunter2hunter2');
  await settle();
  expect(login).not.toHaveBeenCalled();
  inFlight.resolve(alice);
  await expect(signIn).resolves.toBe(true);
  await pending;
  expect(calls).toEqual(['resume', 'login']);
  expect(currentUser()).toEqual(bob);
  setCurrentUser(null);
  vi.restoreAllMocks();
});

it('a sign-out waits for a running automatic resume, then ends the session the resume opened', async () => {
  const { resumeLastSession, setRememberedUsername } = await import('./authStore');
  setCurrentUser(null);
  setRememberedUsername('alice');
  const calls: string[] = [];
  const inFlight = deferred<typeof alice>();
  vi.spyOn(api, 'resumeSession').mockImplementation(() => { calls.push('resume'); return inFlight.promise; });
  const logout = vi.spyOn(api, 'logout').mockImplementation(async () => { calls.push('logout'); });
  const pending = resumeLastSession();
  const signOut = logoutUser();
  await settle();
  expect(logout).not.toHaveBeenCalled();
  inFlight.resolve(alice);
  await signOut;
  await pending;
  expect(calls).toEqual(['resume', 'logout']);
  expect(currentUser()).toBeNull();
  vi.restoreAllMocks();
});

it('a resumed session signed out while its channels load does not run the after-resume refresh', async () => {
  const { resumeLastSession, setRememberedUsername } = await import('./authStore');
  setCurrentUser(null);
  setRememberedUsername('alice');
  vi.spyOn(api, 'resumeSession').mockResolvedValue(alice);
  const channels = deferred<never[]>();
  vi.spyOn(api, 'listChannels').mockReturnValue(channels.promise);
  const afterResume = vi.fn(async () => {});
  const pending = resumeLastSession(afterResume);
  await settle();
  expect(currentUser()).toEqual(alice);
  setCurrentUser(null);
  channels.resolve([]);
  await expect(pending).resolves.toBe(false);
  expect(afterResume).not.toHaveBeenCalled();
  vi.restoreAllMocks();
});

it('a sign-in shows progress at once while it waits for a running automatic sign-in', async () => {
  const { resumeLastSession, loginUser, setRememberedUsername, authLoading } = await import('./authStore');
  setCurrentUser(null);
  setRememberedUsername('alice');
  const inFlight = deferred<null>();
  vi.spyOn(api, 'resumeSession').mockReturnValue(inFlight.promise);
  vi.spyOn(api, 'login').mockResolvedValue(bob);
  const pending = resumeLastSession();
  const signIn = loginUser('bob', 'hunter2hunter2');
  expect(authLoading()).toBe(true);
  inFlight.resolve(null);
  await signIn;
  await pending;
  expect(authLoading()).toBe(false);
  setCurrentUser(null);
  vi.restoreAllMocks();
});
