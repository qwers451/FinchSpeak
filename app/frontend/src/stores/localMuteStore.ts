import { createSignal } from 'solid-js';
import { api } from '../services/api';
import { allVoiceParticipants } from './voiceStore';
import { volumeFor } from './volumeStore';

const KEY = 'finchspeak.localMutes';

function readMutes(): Set<string> {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(KEY) ?? '[]');
    if (Array.isArray(value)) {
      return new Set(value.filter((id): id is string =>
        typeof id === 'string' && /^[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}$/i.test(id)));
    }
  } catch { /* use the default */ }
  return new Set();
}

const [localMutes, setLocalMutes] = createSignal(readMutes());

export const isLocallyMuted = (userId: string) => localMutes().has(userId);

export async function toggleLocalMute(userId: string, peerId?: number) {
  const next = new Set(localMutes());
  const muted = !next.delete(userId);
  if (muted) next.add(userId);
  setLocalMutes(next);
  try {
    localStorage.setItem(KEY, JSON.stringify([...next]));
  } catch { /* keep the mute for this session */ }

  const peers = new Set<number>(peerId === undefined ? [] : [peerId]);
  for (const participant of allVoiceParticipants()) {
    if (participant.user_id === userId) peers.add(participant.peer_id);
  }
  await Promise.allSettled([...peers].map((peer) => api.setSpeakerVolume(peer, muted ? 0 : volumeFor(userId))));
}
