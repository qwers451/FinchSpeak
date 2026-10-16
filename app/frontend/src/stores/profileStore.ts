import { createSignal } from 'solid-js';
import { api } from '../services/api';
import { currentUser, setCurrentUser } from './authStore';
import { setAppError } from './notificationStore';
import type { UserStatus } from '../types';

export const [profileOpen, setProfileOpen] = createSignal(false);
export const [profileMessage, setProfileMessage] = createSignal<string | null>(null);

export async function saveDisplayName(name: string): Promise<boolean> {
  try {
    setCurrentUser(await api.updateProfile(name));
    setProfileMessage('Name changed.');
    return true;
  } catch (err) {
    setAppError(String(err));
    return false;
  }
}

export async function savePassword(current: string, next: string, again: string): Promise<boolean> {
  if (next !== again) {
    setAppError('The new passwords do not match');
    return false;
  }
  try {
    await api.changePassword(current, next);
    setProfileMessage('Password changed. Other devices are signed out.');
    return true;
  } catch (err) {
    setAppError(String(err));
    return false;
  }
}

export const STATUS_TEXT_MAX = 80;
export const MAX_AVATAR_BYTES = 2 * 1024 * 1024;

/** Choose how you appear to others, with an optional line of text. */
export async function saveStatus(status: UserStatus, text: string): Promise<boolean> {
  try {
    await api.setStatus(status, text.trim().slice(0, STATUS_TEXT_MAX) || null);
    return true;
  } catch (err) {
    setAppError(String(err));
    return false;
  }
}

/** Send a picture (already cropped by the dialog) to the server; returns the reason when it fails. */
export async function saveAvatar(bytes: Uint8Array, contentType: string): Promise<string | null> {
  if (bytes.length > MAX_AVATAR_BYTES) return 'The picture must be smaller than 2 MB';
  try {
    const user = await api.uploadAvatar(Array.from(bytes), contentType);
    setCurrentUser({ ...(currentUser() ?? user), avatar_url: user.avatar_url });
    return null;
  } catch (err) {
    return String(err).replace(/^Error:\s*/, '');
  }
}

export async function deleteAvatar(): Promise<boolean> {
  try {
    await api.removeAvatar();
    const me = currentUser();
    if (me) setCurrentUser({ ...me, avatar_url: null });
    return true;
  } catch (err) {
    setAppError(String(err));
    return false;
  }
}
