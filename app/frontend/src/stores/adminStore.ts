import { createSignal } from 'solid-js';
import { api } from '../services/api';
import type { AdminUser, AuditEntry, Invite, ServerOverview } from '../types';
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

export type MemberAction = 'kick' | 'ban' | 'unban' | 'make-admin' | 'make-moderator' | 'make-member';

/** Run a moderation action on a member and refresh the list (errors are shown, not thrown). `durationSecs`: a ban for that long. */
export async function moderate(userId: string, action: MemberAction, durationSecs?: number) {
  try {
    if (action === 'kick') await api.kickUser(userId);
    else if (action === 'ban') await (durationSecs ? api.banUser(userId, durationSecs) : api.banUser(userId));
    else if (action === 'unban') await api.unbanUser(userId);
    else await api.setUserRole(userId, action === 'make-admin' ? 'admin' : action === 'make-moderator' ? 'moderator' : 'member');
  } catch (err) {
    setAppError(String(err));
  }
  await loadMembers();
}

// --- action log -----------------------------------------------------------------------------

export const AUDIT_PAGE = 50;
export const [auditEntries, setAuditEntries] = createSignal<AuditEntry[]>([]);
export const [auditHasMore, setAuditHasMore] = createSignal(false);

/** Newest entries (replaces the list). */
export async function loadAudit() {
  try {
    const page = await api.auditLog(undefined, AUDIT_PAGE);
    setAuditEntries(page);
    setAuditHasMore(page.length === AUDIT_PAGE);
  } catch (err) {
    setAppError(String(err));
  }
}

/** The next older page. */
export async function loadMoreAudit() {
  const last = auditEntries()[auditEntries().length - 1];
  if (!last) return;
  try {
    const page = await api.auditLog(last.id, AUDIT_PAGE);
    const known = new Set(auditEntries().map((entry) => entry.id));
    setAuditEntries([...auditEntries(), ...page.filter((entry) => !known.has(entry.id))]);
    setAuditHasMore(page.length === AUDIT_PAGE);
  } catch (err) {
    setAppError(String(err));
  }
}

/** One line for an entry, in words. */
export function describeAudit(entry: AuditEntry): string {
  const target = entry.target ? ` ${entry.target}` : '';
  const text: Record<string, string> = {
    kick: `kicked${target}`,
    ban: `banned${target}${entry.details && entry.details !== 'permanent' ? ' for a while' : ''}`,
    unban: `lifted the ban of${target}`,
    role_changed: `changed the role of${target} (${entry.details ?? ''})`,
    message_deleted: 'deleted a message of someone else',
    channel_created: `created the channel${target}`,
    channel_deleted: `deleted the channel${target}`,
    invite_created: `created the invitation${target}`,
    invite_revoked: `revoked the invitation${target}`,
    registration_changed: `set registration to ${entry.details ?? '?'}`,
    backup: 'made a backup',
    voice_kick: `sent${target} out of the voice room`,
    voice_mute: `silenced${target} in the voice room`,
    voice_unmute: `un-silenced${target} in the voice room`,
  };
  return text[entry.action] ?? `${entry.action}${target}`;
}

// --- server page ----------------------------------------------------------------------------

export const [overview, setOverview] = createSignal<ServerOverview | null>(null);

export async function loadOverview() {
  try {
    setOverview(await api.serverOverview());
  } catch (err) {
    setAppError(String(err));
  }
}

export async function changeRegistration(mode: 'open' | 'invite') {
  try {
    await api.setRegistration(mode);
  } catch (err) {
    setAppError(String(err));
  }
  await loadOverview();
}

export async function makeBackup(): Promise<boolean> {
  try {
    await api.backupNow();
    await loadOverview();
    return true;
  } catch (err) {
    setAppError(String(err));
    return false;
  }
}

/** Staff action on someone in a voice room. */
export async function moderateVoice(userId: string, action: 'kick' | 'mute' | 'unmute') {
  try {
    await api.moderateVoice(userId, action);
  } catch (err) {
    setAppError(String(err));
  }
}
