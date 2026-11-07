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
import { setProfileMessage, setProfileOpen } from './profileState';

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
  cancelResumeRetry();
  setAuthError(null);
  setAuthLoading(true);
  try {
    await settleResumeInFlight();
    lastAttempt = { kind: 'login', username, password, stay };
    const user = await api.login(serverUrl(), username, password, stay);
    lastAttempt = null;
    remember(serverUrl(), username);
    setRememberedUsername(username);
    setPendingInvite(null);
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
  cancelResumeRetry();
  setAuthError(null);
  setAuthLoading(true);
  try {
    await settleResumeInFlight();
    lastAttempt = { kind: 'register', username, displayName, password, inviteCode, stay };
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

// The last automatic resume failed (the Retry button resumes again instead of signing in with an empty password).
export const [resumeFailed, setResumeFailed] = createSignal<boolean>(false);
// While the server is unreachable (e.g. the network is not up yet at Windows start) the resume is tried again
// after these waits; the user typing or signing in stops it.
const RESUME_RETRY_WAITS_MS = [2_000, 5_000, 15_000, 30_000];
let resumeRetryTimer: ReturnType<typeof setTimeout> | undefined;
let resumeRetriesDone = 0;
// Bumped by cancelResumeRetry(): an attempt that started earlier is stale and must not touch the state.
let attemptGeneration = 0;
// The one resume attempt in flight; Retry joins it instead of starting a second one.
let resumeInFlight: Promise<boolean> | null = null;
// Runs after a resumed session (the App refreshes the live session state there).
let afterResume: (() => Promise<void>) | undefined;

/**
 * Stops the scheduled automatic resume retries and forgets the failed resume, so the Retry button
 * signs in with what the user typed. Called when the user types or signs in.
 */
export function cancelResumeRetry() {
  attemptGeneration++;
  if (resumeRetryTimer !== undefined) clearTimeout(resumeRetryTimer);
  resumeRetryTimer = undefined;
  resumeRetriesDone = 0;
  setResumeFailed(false);
}

/** Test hook: forgets the resume timers, the attempt in flight and the after-resume hook. */
export function __resetResumeForTests() {
  cancelResumeRetry();
  resumeInFlight = null;
  resumeRetriesDone = 0;
  afterResume = undefined;
  setResuming(false);
}

function isServerUnreachable(err: unknown): boolean {
  const lower = String(err).toLowerCase();
  return lower.includes('cannot reach') || lower.includes('connection refused') || lower.includes('timed out');
}

async function attemptResume(): Promise<boolean> {
  const username = rememberedUsername();
  if (!username) return false;
  const generation = attemptGeneration;
  // Stale once the user cancelled the retries (typed or signed in) or signed in meanwhile.
  const stale = () => generation !== attemptGeneration || currentUser() !== null;
  let user: User | null;
  try {
    user = await api.resumeSession(serverUrl(), username);
  } catch (err: any) {
    if (stale()) return false;
    setAuthError(String(err));
    setResumeFailed(true);
    if (isServerUnreachable(err) && resumeRetriesDone < RESUME_RETRY_WAITS_MS.length) {
      const wait = RESUME_RETRY_WAITS_MS[resumeRetriesDone++];
      resumeRetryTimer = setTimeout(() => {
        resumeRetryTimer = undefined;
        if (currentUser() !== null || resumeInFlight) return;
        startResumeAttempt().catch((error) => setAppError(describeError(error)));
      }, wait);
    }
    return false;
  }
  if (stale()) return false;
  cancelResumeRetry();
  if (!user) return false;
  setAuthError(null);
  setCurrentUser(user);
  // The generation is taken after this attempt's own cancelResumeRetry(); a later cancel or a sign-out makes it stale.
  const applied = attemptGeneration;
  const stillCurrent = () => applied === attemptGeneration && currentUser() === user;
  await loadChannels();
  if (!stillCurrent()) return false;
  await afterResume?.();
  return true;
}

// A manual sign-in or sign-out runs after an automatic resume still in flight: the resume may already have
// opened a backend session, and the user action must replace it or end it. The resume result is ignored here.
async function settleResumeInFlight() {
  const pending = resumeInFlight;
  if (pending) await pending.catch(() => false);
}

function startResumeAttempt(): Promise<boolean> {
  const attempt = attemptResume().finally(() => {
    if (resumeInFlight === attempt) resumeInFlight = null;
  });
  resumeInFlight = attempt;
  return attempt;
}

/**
 * Signs in with the session saved on this device for the last server; false means show the form.
 * `onSignedIn` runs after a session is resumed, including one resumed by a scheduled retry.
 * While an attempt is already running, this joins it rather than starting a second one.
 */
export async function resumeLastSession(onSignedIn?: () => Promise<void>): Promise<boolean> {
  if (!rememberedUsername()) return false;
  if (onSignedIn) afterResume = onSignedIn;
  if (resumeInFlight) return resumeInFlight;
  cancelResumeRetry();
  setResuming(true);
  try {
    return await startResumeAttempt();
  } finally {
    setResuming(false);
  }
}

export async function logoutUser() {
  lastAttempt = null;
  cancelResumeRetry();
  await settleResumeInFlight();
  let failure: unknown;
  try { await api.logout(); } catch (error) { failure = error; }
  resetSessionState();
  setActiveVoiceChannel(null);
  resetChatState();
  setPendingTofu(null);
  setPendingMismatch(null);
  setSessionEnded(null);
  setPendingInvite(null);
  setProfileOpen(false);
  setProfileMessage(null);
  setCurrentUser(null);
  // A token the server already revoked (401) is signed out all the same: nothing is left to end.
  if (failure && !/\bHTTP 401\b/.test(String(failure))) setAppError(describeError(failure));
}
