import { beforeEach, expect, it, vi } from 'vitest';
import { api } from '../services/api';
import { appError, setAppError } from './notificationStore';
import { createInvite, invites, loadInvites, loadMembers, members, moderate, revokeInvite, setInvites } from './adminStore';

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

const member = (id: string, extra = {}) => ({ id, username: id, display_name: id, role: 'member' as const, banned: false, online: true, ...extra });

it('AUD-045 each moderation action calls the matching API and refreshes the members', async () => {
  const list = vi.spyOn(api, 'adminUsers').mockResolvedValue([member('a')]);
  const kick = vi.spyOn(api, 'kickUser').mockResolvedValue();
  const ban = vi.spyOn(api, 'banUser').mockResolvedValue();
  const unban = vi.spyOn(api, 'unbanUser').mockResolvedValue();
  const role = vi.spyOn(api, 'setUserRole').mockResolvedValue();

  await moderate('a', 'kick');
  await moderate('a', 'ban');
  await moderate('a', 'unban');
  await moderate('a', 'make-admin');
  await moderate('a', 'make-member');

  expect([kick, ban, unban].map((spy) => spy.mock.calls.length)).toEqual([1, 1, 1]);
  expect(role.mock.calls).toEqual([['a', 'admin'], ['a', 'member']]);
  expect(list).toHaveBeenCalledTimes(5);
  expect(members().map((m) => m.id)).toEqual(['a']);
});

it('AUD-045 a refused action (e.g. banning an admin) is shown to the admin', async () => {
  vi.spyOn(api, 'adminUsers').mockResolvedValue([member('a')]);
  vi.spyOn(api, 'banUser').mockRejectedValue('HTTP 400: Make them a member before banning an admin');
  await moderate('a', 'ban');
  expect(appError()).toContain('before banning an admin');
  await loadMembers();
});
