import { createSignal } from 'solid-js';
import { t } from '../i18n';
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
  const target = entry.target ?? '';
  const details = entry.details ?? '';
  const line = (): string => {
    switch (entry.action) {
      case 'kick': return t('kicked {target}', { target });
      case 'ban': return entry.details && entry.details !== 'permanent' ? t('banned {target} for a while', { target }) : t('banned {target}', { target });
      case 'unban': return t('lifted the ban of {target}', { target });
      case 'role_changed': return t('changed the role of {target} ({details})', { target, details });
      case 'message_deleted': return t('deleted a message of someone else');
      case 'channel_created': return t('created the channel {target}', { target });
      case 'channel_deleted': return t('deleted the channel {target}', { target });
      case 'invite_created': return t('created the invitation {target}', { target });
      case 'invite_revoked': return t('revoked the invitation {target}', { target });
      case 'registration_changed': return t('set registration to {details}', { details: entry.details ?? '?' });
      case 'backup': return t('made a backup');
      case 'voice_kick': return t('sent {target} out of the voice room', { target });
      case 'voice_mute': return t('silenced {target} in the voice room', { target });
      case 'voice_unmute': return t('un-silenced {target} in the voice room', { target });
      default: return `${entry.action} ${target}`;
    }
  };
  return line().trim();
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
