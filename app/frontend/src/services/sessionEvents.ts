import type { UnlistenFn } from '@tauri-apps/api/event';
import type { SessionEvents } from '../types';
import { api } from './api';
import { appendMissingProfiles, currentSessionGeneration, knownUsers, localPeerId, restoreSessionSnapshot, sessionHandlers, sessionUserId, setAppError } from '../stores/sessionStore';
import { currentUser } from '../stores/authStore';
import { describeError } from '../i18n/errors';

const PROFILE_REFRESH_INTERVAL_MS = 10_000;
type RefreshScope = { userId: string; isCurrent: () => boolean };

export async function subscribeSessionEvents(): Promise<{ dispose: UnlistenFn; refresh: () => Promise<void> }> {
  let disposed = false;
  let pending: { scope: RefreshScope; full: boolean; updates: (() => void)[]; done: Promise<void> } | undefined;
  let profiles: { scope: RefreshScope; queued: Set<string>; attempted: Map<string, number> } | undefined;
  const stateEvents = new Set<keyof SessionEvents>(['authenticated', 'presence-update', 'channel-created', 'user-joined-voice', 'user-left-voice', 'user-voice-state-changed', 'connection-lost', 'reconnecting', 'reconnect-failed']);
  const expectedUser = () => currentUser()?.id ?? sessionUserId();
  const currentScope = (): RefreshScope | undefined => {
    const userId = expectedUser();
    if (!userId || disposed) return;
    const generation = currentSessionGeneration();
    return { userId, isCurrent: () => !disposed && expectedUser() === userId && currentSessionGeneration() === generation };
  };
  const startRefresh = (scope: RefreshScope, full: boolean) => {
    const peerId = localPeerId();
    const updates: (() => void)[] = [];
    const flight = { scope, full, updates, done: Promise.resolve() };
    pending = flight;
    flight.done = (async (): Promise<void> => {
        try {
          const snapshot = await api.getSessionSnapshot();
          if (!scope.isCurrent() || snapshot?.user_id !== scope.userId) return;
          if (flight.full) {
            restoreSessionSnapshot(snapshot);
            updates.forEach((apply) => apply());
          } else if (localPeerId() === peerId && snapshot.connected && snapshot.initial_state) {
            appendMissingProfiles(snapshot.initial_state);
          }
        } catch (error) {
          if (scope.isCurrent()) throw error;
        } finally {
          pending = undefined;
          // A presence event received during the request may still need a newer snapshot.
          refreshProfiles();
        }
      })();
    return flight.done;
  };
  const refresh = (): Promise<void> => {
    const scope = currentScope();
    if (!scope) return Promise.resolve();
    if (!pending) return startRefresh(scope, true);
    if (pending.scope.isCurrent()) {
      // An explicit refresh can use the same snapshot and replay events buffered since it began.
      pending.full = true;
      return pending.done;
    }
    const afterFlight = () => scope.isCurrent() ? refresh() : undefined;
    return pending.done.then(afterFlight, afterFlight);
  };
  function refreshProfiles() {
    if (pending || !profiles?.scope.isCurrent() || !profiles.queued.size) return;
    const { scope, queued, attempted } = profiles;
    const knownIds = new Set(knownUsers().map((user) => user.id));
    const missing = [...queued].filter((id) => !knownIds.has(id));
    queued.clear();
    if (!missing.length) return;
    const now = Date.now();
    for (const id of missing) attempted.set(id, now);
    void startRefresh(scope, false).catch((error) => { if (scope.isCurrent()) setAppError(describeError(error)); });
  }
  const queueProfile = (id: string) => {
    if (knownUsers().some((user) => user.id === id)) return;
    if (!profiles?.scope.isCurrent()) {
      const scope = currentScope();
      if (!scope) return;
      profiles = { scope, queued: new Set(), attempted: new Map() };
    }
    const lastAttempt = profiles.attempted.get(id);
    if (lastAttempt !== undefined && Date.now() - lastAttempt < PROFILE_REFRESH_INTERVAL_MS) return;
    profiles.queued.add(id);
    refreshProfiles();
  };
  const dispatch = <K extends keyof SessionEvents>(event: K, payload: SessionEvents[K]) => {
    if (disposed) return;
    const apply = () => sessionHandlers[event](payload);
    if (pending?.scope.isCurrent() && stateEvents.has(event)) pending.updates.push(apply);
    apply();
    if (event === 'presence-update') {
      queueProfile((payload as SessionEvents['presence-update']).user_id);
    }
  };
  const register = <K extends keyof SessionEvents>(event: K) =>
    api.onSessionEvent(event, (payload) => dispatch(event, payload));
  const registrations = await Promise.allSettled(
    (Object.keys(sessionHandlers) as (keyof SessionEvents)[]).map(register),
  );
  const unlisteners: UnlistenFn[] = [];
  const failures: unknown[] = [];
  for (const result of registrations) {
    if (result.status === 'fulfilled') unlisteners.push(result.value);
    else failures.push(result.reason);
  }
  const dispose = () => {
    if (disposed) return;
    disposed = true;
    unlisteners.forEach((unlisten) => unlisten());
  };
  if (failures.length) {
    dispose();
    throw new AggregateError(failures, 'Failed to subscribe to session events');
  }
  return { dispose, refresh };
}
