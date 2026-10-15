import { createSignal } from 'solid-js';
import type { User } from '../types';
import { api } from '../services/api';
import { loadChannels, resetChatState } from './chatStore';
import { resetSessionState } from './sessionStore';
import { setActiveVoiceChannel } from './voiceStore';
import { setAppError } from './notificationStore';
import { pendingTofu, setPendingTofu } from './tofuStore';

export const [currentUser, setCurrentUser] = createSignal<User | null>(null);
export const [serverUrl, setServerUrl] = createSignal<string>('https://127.0.0.1:8443');
export const [authLoading, setAuthLoading] = createSignal<boolean>(false);
export const [authError, setAuthError] = createSignal<string | null>(null);

// The certificate-confirmation state lives in a leaf module (see tofuStore.ts); re-exported here.
export { pendingTofu, setPendingTofu };

export async function loginUser(username: string, password: string) {
  setAuthLoading(true);
  setAuthError(null);
  try {
    const user = await api.login(serverUrl(), username, password);
    setCurrentUser(user);
    await loadChannels();
    return true;
  } catch (err: any) {
    setAuthError(String(err));
    return false;
  } finally {
    setAuthLoading(false);
  }
}

export async function registerUser(username: string, displayName: string, password: string, inviteCode?: string) {
  setAuthLoading(true);
  setAuthError(null);
  try {
    const user = await api.register(serverUrl(), username, displayName, password, inviteCode);
    setCurrentUser(user);
    await loadChannels();
    return true;
  } catch (err: any) {
    setAuthError(String(err));
    return false;
  } finally {
    setAuthLoading(false);
  }
}

export async function logoutUser() {
  let failure: unknown;
  try { await api.logout(); } catch (error) { failure = error; }
  resetSessionState();
  setActiveVoiceChannel(null);
  resetChatState();
  setPendingTofu(null);
  setCurrentUser(null);
  if (failure) setAppError(String(failure));
}
