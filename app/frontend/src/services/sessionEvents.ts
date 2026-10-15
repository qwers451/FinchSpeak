import type { UnlistenFn } from '@tauri-apps/api/event';
import type { SessionEvents } from '../types';
import { api } from './api';
import { knownUsers, restoreSessionSnapshot, sessionHandlers, sessionUserId, setAppError } from '../stores/sessionStore';
import { currentUser } from '../stores/authStore';

export async function subscribeSessionEvents(): Promise<{ dispose: UnlistenFn; refresh: () => Promise<void> }> {
  let disposed = false;
  let pending: Promise<void> | undefined;
  let buffered: (() => void)[] | null = null;
  const stateEvents = new Set<keyof SessionEvents>(['authenticated', 'presence-update', 'channel-created', 'user-joined-voice', 'user-left-voice', 'user-voice-state-changed', 'connection-lost', 'reconnecting', 'reconnect-failed']);
  const expectedUser = () => currentUser()?.id ?? sessionUserId();
  const refresh = (): Promise<void> => {
    if (pending) return pending;
    const userId = expectedUser();
    if (!userId || disposed) return Promise.resolve();
    const updates: (() => void)[] = [];
    buffered = updates;
    pending = (async () => {
      try {
        const snapshot = await api.getSessionSnapshot();
        if (disposed || expectedUser() !== userId) return;
        if (snapshot?.user_id === userId) {
          restoreSessionSnapshot(snapshot);
          updates.forEach((apply) => apply());
        }
      } finally {
        buffered = null;
        pending = undefined;
      }
    })();
    return pending;
  };
  const dispatch = <K extends keyof SessionEvents>(event: K, payload: SessionEvents[K]) => {
    const apply = () => sessionHandlers[event](payload);
    if (buffered && stateEvents.has(event)) buffered.push(apply);
    apply();
    if (event === 'presence-update') {
      const update = payload as SessionEvents['presence-update'];
      if (update.status !== 'offline' && !knownUsers().some((user) => user.id === update.user_id)) {
        void refresh().catch((error) => { if (!disposed) setAppError(String(error)); });
      }
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
