import { createSignal } from 'solid-js';
import type { User } from '../types';
import { api } from '../services/api';
import { loadChannels, resetChatState } from './chatStore';
import { resetSessionState } from './sessionStore';
import { setActiveVoiceChannel } from './voiceStore';
import { setAppError } from './notificationStore';
import { pendingMismatch, pendingTofu, setPendingMismatch, setPendingTofu } from './tofuStore';

export const [currentUser, setCurrentUser] = createSignal<User | null>(null);
const REMEMBER_KEY = 'finchspeak.login';
const DEFAULT_SERVER = 'https://127.0.0.1:8443';

// Only the address and the login name are remembered; the password never is.
function readRemembered(): { serverUrl: string; username: string } {
  try {
    const parsed = JSON.parse(localStorage.getItem(REMEMBER_KEY) ?? 'null');
    if (parsed && typeof parsed.serverUrl === 'string' && typeof parsed.username === 'string') return parsed;
  } catch { /* unavailable or corrupt: start empty */ }
  return { serverUrl: DEFAULT_SERVER, username: '' };
}
function remember(serverUrl: string, username: string) {
  try { localStorage.setItem(REMEMBER_KEY, JSON.stringify({ serverUrl, username })); } catch { /* not critical */ }
}
const remembered = readRemembered();
export const [serverUrl, setServerUrl] = createSignal<string>(remembered.serverUrl);
export const [rememberedUsername, setRememberedUsername] = createSignal<string>(remembered.username);
export const [authLoading, setAuthLoading] = createSignal<boolean>(false);
export const [authError, setAuthError] = createSignal<string | null>(null);

// The certificate-confirmation state lives in a leaf module (see tofuStore.ts); re-exported here.
export { pendingMismatch, pendingTofu, setPendingMismatch, setPendingTofu };

export async function loginUser(username: string, password: string) {
  setAuthLoading(true);
  setAuthError(null);
  try {
    const user = await api.login(serverUrl(), username, password);
    remember(serverUrl(), username);
    setRememberedUsername(username);
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
    remember(serverUrl(), username);
    setRememberedUsername(username);
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
  setPendingMismatch(null);
  setCurrentUser(null);
  if (failure) setAppError(String(failure));
}
