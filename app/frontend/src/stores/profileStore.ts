import { createSignal } from 'solid-js';
import { api } from '../services/api';
import { setCurrentUser } from './authStore';
import { setAppError } from './notificationStore';

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
