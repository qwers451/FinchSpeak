import { expect, it, vi } from 'vitest';
import { api } from '../services/api';
import { currentUser, logoutUser, setCurrentUser } from './authStore';
import { appError } from './notificationStore';
import { isMuted, isDeafened, setIsMuted, setIsDeafened } from './voiceStore';

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
