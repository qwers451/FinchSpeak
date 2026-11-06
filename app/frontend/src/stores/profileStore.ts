import { createSignal } from 'solid-js';
import { api } from '../services/api';
import { currentUser, logoutUser, setCurrentUser } from './authStore';
import { sessions, setSessions } from './sessionStore';
import { setAppError } from './notificationStore';
import { t } from '../i18n';
import type { DeviceSession, UserStatus } from '../types';
import { describeError } from '../i18n/errors';

export const [profileOpen, setProfileOpen] = createSignal(false);
export const [profileMessage, setProfileMessage] = createSignal<string | null>(null);

export async function saveDisplayName(name: string): Promise<boolean> {
  try {
    setCurrentUser(await api.updateProfile(name));
    setProfileMessage('Name changed.');
    return true;
  } catch (err) {
    setAppError(describeError(err));
    return false;
  }
}

export async function savePassword(current: string, next: string, again: string): Promise<boolean> {
  if (next !== again) {
    setAppError(t('The new passwords do not match'));
    return false;
  }
  try {
    await api.changePassword(current, next);
    setProfileMessage('Password changed. Other devices are signed out.');
    return true;
  } catch (err) {
    setAppError(describeError(err));
    return false;
  }
}

export { sessions };

/** Fetch the signed-in devices of this account. */
export async function loadSessions(): Promise<DeviceSession[]> {
  try {
    const list = await api.listSessions();
    setSessions(list);
    return list;
  } catch (err) {
    setAppError(describeError(err));
    return sessions();
  }
}

/** Sign one device out; for this computer that is a normal log out. */
export async function endSession(id: string): Promise<void> {
  if (sessions().find((s) => s.id === id)?.current) {
    await logoutUser();
    return;
  }
  try {
    await api.endSession(id);
  } catch (err) {
    setAppError(describeError(err));
  }
  await loadSessions();
}

export async function endOtherSessions(): Promise<void> {
  try {
    await api.endOtherSessions();
  } catch (err) {
    setAppError(describeError(err));
  }
  await loadSessions();
}

export const STATUS_TEXT_MAX = 80;
export const MAX_AVATAR_BYTES = 2 * 1024 * 1024;
export type AvatarSaveFailure = { kind: 'local' | 'external'; message: string };

/** Choose how you appear to others, with an optional line of text. */
export async function saveStatus(status: UserStatus, text: string): Promise<boolean> {
  try {
    await api.setStatus(status, text.trim().slice(0, STATUS_TEXT_MAX) || null);
    return true;
  } catch (err) {
    setAppError(describeError(err));
    return false;
  }
}

/** Send a picture (already cropped by the dialog) to the server; returns the reason when it fails. */
export async function saveAvatar(bytes: Uint8Array, contentType: string): Promise<AvatarSaveFailure | null> {
  if (bytes.length > MAX_AVATAR_BYTES) return { kind: 'local', message: 'The picture must be smaller than 2 MB' };
  try {
    const user = await api.uploadAvatar(Array.from(bytes), contentType);
    setCurrentUser({ ...(currentUser() ?? user), avatar_url: user.avatar_url });
    return null;
  } catch (err) {
    return { kind: 'external', message: String(err).replace(/^Error:\s*/, '') };
  }
}

export async function deleteAvatar(): Promise<boolean> {
  try {
    await api.removeAvatar();
    const me = currentUser();
    if (me) setCurrentUser({ ...me, avatar_url: null });
    return true;
  } catch (err) {
    setAppError(describeError(err));
    return false;
  }
}
