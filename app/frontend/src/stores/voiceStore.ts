import { createSignal } from 'solid-js';
import type { Channel, VoiceParticipant } from '../types';
import { api } from '../services/api';

export const [activeVoiceChannel, setActiveVoiceChannel] = createSignal<Channel | null>(null);
export const [isMuted, setIsMuted] = createSignal<boolean>(false);
export const [isDeafened, setIsDeafened] = createSignal<boolean>(false);
export const [isLocalSpeaking, setIsLocalSpeaking] = createSignal<boolean>(false);
export const [speakingPeers, setSpeakingPeers] = createSignal<Set<number>>(new Set());
export const [voiceParticipants, setVoiceParticipants] = createSignal<VoiceParticipant[]>([]);
export const [pingRtt, setPingRtt] = createSignal<number | null>(null);

export async function joinVoice(channel: Channel) {
  try {
    await api.joinVoiceChannel(channel.id, channel.short_id);
    setActiveVoiceChannel(channel);
  } catch (err) {
    console.error('Failed to join voice channel:', err);
  }
}

export async function leaveVoice() {
  try {
    await api.leaveVoiceChannel();
    setActiveVoiceChannel(null);
    setSpeakingPeers(new Set());
    setVoiceParticipants([]);
  } catch (err) {
    console.error('Failed to leave voice channel:', err);
  }
}

export async function toggleMute() {
  const next = !isMuted();
  setIsMuted(next);
  await api.setVoiceState(next, isDeafened());
}

export async function toggleDeafen() {
  const next = !isDeafened();
  setIsDeafened(next);
  // Deafen implies mute
  if (next) setIsMuted(true);
  await api.setVoiceState(isMuted(), next);
}

export function handleUserSpeaking(peerId: number, speaking: boolean) {
  setSpeakingPeers((prev) => {
    const next = new Set(prev);
    if (speaking) {
      next.add(peerId);
    } else {
      next.delete(peerId);
    }
    return next;
  });
}
