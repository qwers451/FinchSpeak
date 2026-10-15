import { beforeEach, expect, it, vi } from 'vitest';
import { api } from '../services/api';
import { appError, setAppError } from './notificationStore';
import { createInvite, invites, loadInvites, revokeInvite, setInvites } from './adminStore';

const invite = (code: string) => ({ code, created_at: 1, expires_at: 2, max_uses: 1, uses: 0 });

beforeEach(() => {
  vi.restoreAllMocks();
  setAppError(null);
  setInvites([]);
});

it('AUD-027 creating an invite returns its code and refreshes the list', async () => {
  vi.spyOn(api, 'createInvite').mockResolvedValue(invite('ABC'));
  vi.spyOn(api, 'listInvites').mockResolvedValue([invite('ABC')]);
  expect(await createInvite(1, 72)).toBe('ABC');
  expect(invites().map((i) => i.code)).toEqual(['ABC']);
});

it('AUD-027 a refused invite (not an admin) is shown, not thrown', async () => {
  vi.spyOn(api, 'createInvite').mockRejectedValue('HTTP 403: Permission denied');
  expect(await createInvite(1, 72)).toBeNull();
  expect(appError()).toContain('403');
});

it('AUD-027 revoking removes the code from the list', async () => {
  setInvites([invite('ABC'), invite('DEF')]);
  const revoke = vi.spyOn(api, 'revokeInvite').mockResolvedValue();
  vi.spyOn(api, 'listInvites').mockResolvedValue([invite('DEF')]);
  await revokeInvite('ABC');
  expect(revoke).toHaveBeenCalledWith('ABC');
  expect(invites().map((i) => i.code)).toEqual(['DEF']);
  await loadInvites();
});
