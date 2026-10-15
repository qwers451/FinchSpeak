import { api } from '../services/api';
import type { SessionEvents } from '../types';
import { setAppError } from './notificationStore';
import { isSharing, resetScreenState, setIsSharing, setScreenSources, setWatching, watching } from './screenState';
import { setVoiceParticipants } from './voiceStore';

export { isSharing, screenSources, watching } from './screenState';

export async function loadScreenSources() {
  try {
    setScreenSources(await api.listScreenSources());
  } catch (err) {
    setAppError(String(err));
  }
}

export async function startSharing(kind: 'monitor' | 'window', id: number) {
  try {
    await api.startScreenShare(kind, id);
    setIsSharing(true);
  } catch (err) {
    setAppError(String(err));
  }
}

export async function stopSharing() {
  setIsSharing(false);
  try {
    await api.stopScreenShare();
  } catch (err) {
    setAppError(String(err));
  }
}

/** Start watching someone's screen. Only one share is watched at a time. */
export async function watchShare(userId: string, peerId: number) {
  const previous = watching();
  try {
    if (previous && previous.user_id !== userId) await api.unsubscribeScreenShare(previous.user_id);
    await api.subscribeScreenShare(userId, peerId);
    setWatching({ user_id: userId, peer_id: peerId });
  } catch (err) {
    setAppError(String(err));
  }
}

export async function stopWatching() {
  const current = watching();
  setWatching(null);
  if (!current) return;
  try {
    await api.unsubscribeScreenShare(current.user_id);
  } catch (err) {
    setAppError(String(err));
  }
}

const markSharing = (peerId: number, sharing: boolean) =>
  setVoiceParticipants((previous) => previous.map((participant) =>
    participant.peer_id === peerId ? { ...participant, is_sharing: sharing } : participant));

export const screenHandlers: Pick<{ [K in keyof SessionEvents]: (payload: SessionEvents[K]) => void },
  'screen-share-started' | 'screen-share-stopped' | 'screen-share-ended'> = {
  'screen-share-started': (data) => {
    markSharing(data.peer_id, true);
    // The share we watch restarted, perhaps under a new peer id: keep following it.
    if (watching()?.user_id === data.user_id) setWatching({ user_id: data.user_id, peer_id: data.peer_id });
  },
  'screen-share-stopped': (data) => {
    markSharing(data.peer_id, false);
    if (watching()?.peer_id === data.peer_id) setWatching(null);
  },
  'screen-share-ended': (reason) => {
    setIsSharing(false);
    setAppError(`Screen sharing stopped: ${reason}`);
  },
};

export { resetScreenState, isSharing as isSharingScreen };
