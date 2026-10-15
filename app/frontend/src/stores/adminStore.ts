import { createSignal } from 'solid-js';
import { api } from '../services/api';
import type { AdminUser, Invite } from '../types';
import { setAppError } from './notificationStore';

export const [adminOpen, setAdminOpen] = createSignal(false);
export const [invites, setInvites] = createSignal<Invite[]>([]);
export const [members, setMembers] = createSignal<AdminUser[]>([]);

export async function loadInvites() {
  try {
    setInvites(await api.listInvites());
  } catch (err) {
    setAppError(String(err));
  }
}

/** Create a code and refresh the list; returns the new code (or null if it failed). */
export async function createInvite(maxUses: number, expiresHours: number): Promise<string | null> {
  try {
    const invite = await api.createInvite(maxUses, expiresHours);
    await loadInvites();
    return invite.code;
  } catch (err) {
    setAppError(String(err));
    return null;
  }
}

export async function revokeInvite(code: string) {
  try {
    await api.revokeInvite(code);
    await loadInvites();
  } catch (err) {
    setAppError(String(err));
  }
}

export async function loadMembers() {
  try {
    setMembers(await api.adminUsers());
  } catch (err) {
    setAppError(String(err));
  }
}

export type MemberAction = 'kick' | 'ban' | 'unban' | 'make-admin' | 'make-member';

/** Run a moderation action on a member and refresh the list (errors are shown, not thrown). */
export async function moderate(userId: string, action: MemberAction) {
  try {
    if (action === 'kick') await api.kickUser(userId);
    else if (action === 'ban') await api.banUser(userId);
    else if (action === 'unban') await api.unbanUser(userId);
    else await api.setUserRole(userId, action === 'make-admin' ? 'admin' : 'member');
  } catch (err) {
    setAppError(String(err));
  }
  await loadMembers();
}
