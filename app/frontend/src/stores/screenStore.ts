import { api } from '../services/api';
import type { SessionEvents } from '../types';
import { setAppError } from './notificationStore';
import { endedShare, isSharing, resetScreenState, setEndedShare, setIsSharing, setScreenSources, setSharingInfo, setWatching, sharingInfo, watching } from './screenState';
import { allVoiceParticipants, setVoiceParticipants } from './voiceStore';
import { endReasonFromServer } from '../components/screen/viewerState';

export { endedShare, isSharing, screenSources, sharingInfo, watching } from './screenState';

export { screenAudioKey } from './volumeStore';

export async function loadScreenSources() {
  try {
    setScreenSources(await api.listScreenSources());
  } catch (err) {
    setAppError(String(err));
  }
}

/** Start sharing; returns the reason when it did not work (the picker shows it), else `null`. */
export async function startSharing(kind: 'monitor' | 'window', id: number, withAudio = false, label = ''): Promise<string | null> {
  try {
    await api.startScreenShare(kind, id, withAudio);
    setIsSharing(true);
    setSharingInfo({ label, audio: withAudio });
    return null;
  } catch (err) {
    return String(err).replace(/^Error:\s*/, '');
  }
}

export async function stopSharing() {
  setIsSharing(false);
  setSharingInfo(null);
  try {
    await api.stopScreenShare();
  } catch (err) {
    setAppError(String(err));
  }
}

/** Start watching someone's screen. Only one share is watched at a time. */
export async function watchShare(userId: string, peerId: number) {
  const previous = watching();
  setEndedShare(null);
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

/** Close the "sharer stopped" screen and go back to the room. */
export function closeEndedShare() {
  setEndedShare(null);
}

function noteEnded(peerId: number, reason: string | undefined) {
  const watched = watching();
  if (!watched || watched.peer_id !== peerId) return;
  const name = allVoiceParticipants().find((participant) => participant.peer_id === peerId)?.username ?? 'The sharer';
  setEndedShare({ user_id: watched.user_id, peer_id: peerId, name, reason: endReasonFromServer(reason) });
  setWatching(null);
}

const markSharing = (peerId: number, sharing: boolean) =>
  setVoiceParticipants((previous) => previous.map((participant) =>
    participant.peer_id === peerId ? { ...participant, is_sharing: sharing } : participant));

export const screenHandlers: Pick<{ [K in keyof SessionEvents]: (payload: SessionEvents[K]) => void },
  'screen-share-started' | 'screen-share-stopped' | 'screen-share-ended' | 'screen-audio-failed'> = {
  'screen-share-started': (data) => {
    markSharing(data.peer_id, true);
    // The share we watch restarted, perhaps under a new peer id: keep following it.
    if (watching()?.user_id === data.user_id) setWatching({ user_id: data.user_id, peer_id: data.peer_id });
  },
  'screen-share-stopped': (data) => {
    noteEnded(data.peer_id, data.reason);
    markSharing(data.peer_id, false);
  },
  'screen-audio-failed': (reason) => setAppError(`Screen sound is not shared: ${reason}`),
  'screen-share-ended': (reason) => {
    setIsSharing(false);
    setSharingInfo(null);
    setAppError(`Screen sharing stopped: ${reason}`);
  },
};

export { resetScreenState, isSharing as isSharingScreen, endedShare as endedShareSignal };
