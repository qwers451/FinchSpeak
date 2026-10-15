import { createSignal } from 'solid-js';
import type { User } from '../types';
import { api } from '../services/api';
import { loadChannels } from './chatStore';

export const [currentUser, setCurrentUser] = createSignal<User | null>(null);
export const [serverUrl, setServerUrl] = createSignal<string>('http://127.0.0.1:8443');
export const [authLoading, setAuthLoading] = createSignal<boolean>(false);
export const [authError, setAuthError] = createSignal<string | null>(null);

// TOFU Prompt state: { hostname: string, fingerprint: string } | null
export const [pendingTofu, setPendingTofu] = createSignal<{ hostname: string; fingerprint: string } | null>(null);

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

export async function registerUser(username: string, displayName: string, password: string) {
  setAuthLoading(true);
  setAuthError(null);
  try {
    const user = await api.register(serverUrl(), username, displayName, password);
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
  await api.logout();
  setCurrentUser(null);
}
