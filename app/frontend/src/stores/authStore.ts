import { createSignal } from 'solid-js';
import type { User } from '../types';
import { api } from '../services/api';
import { loadChannels, resetChatState } from './chatStore';
import { resetSessionState } from './sessionStore';
import { setActiveVoiceChannel } from './voiceStore';
import { setAppError } from './notificationStore';
import { setSessionEnded } from './sessionEnd';
import { pendingMismatch, pendingTofu, setPendingMismatch, setPendingTofu } from './tofuStore';
import { describeError } from '../i18n/errors';
import { setPendingInvite } from './inviteLink';

export const [currentUser, setCurrentUser] = createSignal<User | null>(null);
const REMEMBER_KEY = 'finchspeak.login';
const DEFAULT_SERVER = '';

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
export const [resuming, setResuming] = createSignal<boolean>(false);
export const [authError, setAuthError] = createSignal<string | null>(null);

// The certificate-confirmation state lives in a leaf module (see tofuStore.ts); re-exported here.
export { pendingMismatch, pendingTofu, setPendingMismatch, setPendingTofu };

// The last sign-in attempt, kept in memory only so it can be replayed once after the user trusts the
// server certificate. Cleared on success, on logout and after one retry.
type Attempt =
  | { kind: 'login'; username: string; password: string; stay: boolean }
  | { kind: 'register'; username: string; displayName: string; password: string; inviteCode?: string; stay: boolean };
let lastAttempt: Attempt | null = null;

// The error of an untrusted server arrives before (or after) the tofu-required event, so look at both.
function awaitsCertificateTrust(err: unknown): boolean {
  return pendingTofu() !== null || String(err).includes('is not trusted yet');
}

/** Forgets the saved attempt (and its password), e.g. when the certificate prompt is dismissed. */
export function discardPendingSignIn() {
  lastAttempt = null;
}

/** Replays the attempt that stopped on the certificate prompt; false if there was none or it failed again. */
export async function retryPendingSignIn(): Promise<boolean> {
  const attempt = lastAttempt;
  lastAttempt = null;
  if (!attempt) return false;
  return attempt.kind === 'login'
    ? loginUser(attempt.username, attempt.password, attempt.stay)
    : registerUser(attempt.username, attempt.displayName, attempt.password, attempt.inviteCode, attempt.stay);
}

export async function loginUser(username: string, password: string, stay = true) {
  lastAttempt = { kind: 'login', username, password, stay };
  setAuthLoading(true);
  setAuthError(null);
  try {
    const user = await api.login(serverUrl(), username, password, stay);
    lastAttempt = null;
    remember(serverUrl(), username);
    setRememberedUsername(username);
    setCurrentUser(user);
    await loadChannels();
    return true;
  } catch (err: any) {
    setAuthError(String(err));
    if (!awaitsCertificateTrust(err)) lastAttempt = null; // keep the password only while the certificate prompt can replay it
    return false;
  } finally {
    setAuthLoading(false);
  }
}

export async function registerUser(username: string, displayName: string, password: string, inviteCode?: string, stay = true) {
  lastAttempt = { kind: 'register', username, displayName, password, inviteCode, stay };
  setAuthLoading(true);
  setAuthError(null);
  try {
    const user = await api.register(serverUrl(), username, displayName, password, inviteCode, stay);
    lastAttempt = null;
    setPendingInvite(null);
    remember(serverUrl(), username);
    setRememberedUsername(username);
    setCurrentUser(user);
    await loadChannels();
    return true;
  } catch (err: any) {
    setAuthError(String(err));
    if (!awaitsCertificateTrust(err)) lastAttempt = null; // keep the password only while the certificate prompt can replay it
    return false;
  } finally {
    setAuthLoading(false);
  }
}

/** Signs in with the session saved on this device for the last server; false means show the form. */
export async function resumeLastSession(): Promise<boolean> {
  const username = rememberedUsername();
  if (!username) return false;
  setResuming(true);
  try {
    const user = await api.resumeSession(serverUrl(), username);
    if (!user) return false;
    setCurrentUser(user);
    await loadChannels();
    return true;
  } catch (err: any) {
    setAuthError(String(err));
    return false;
  } finally {
    setResuming(false);
  }
}

export async function logoutUser() {
  lastAttempt = null;
  let failure: unknown;
  try { await api.logout(); } catch (error) { failure = error; }
  resetSessionState();
  setActiveVoiceChannel(null);
  resetChatState();
  setPendingTofu(null);
  setPendingMismatch(null);
  setSessionEnded(null);
  setCurrentUser(null);
  if (failure) setAppError(describeError(failure));
}
