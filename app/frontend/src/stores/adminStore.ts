import { createSignal } from 'solid-js';
import { api } from '../services/api';
import type { Invite } from '../types';
import { setAppError } from './notificationStore';

export const [adminOpen, setAdminOpen] = createSignal(false);
export const [invites, setInvites] = createSignal<Invite[]>([]);

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
