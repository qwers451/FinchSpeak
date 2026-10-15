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
