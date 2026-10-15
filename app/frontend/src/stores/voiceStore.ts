import { createSignal } from 'solid-js';
import type { Channel, VoiceParticipant } from '../types';
import { api } from '../services/api';

export const [activeVoiceChannel, setActiveVoiceChannel] = createSignal<Channel | null>(null);
export const [isMuted, setIsMuted] = createSignal<boolean>(false);
export const [isDeafened, setIsDeafened] = createSignal<boolean>(false);
export const [isLocalSpeaking, setIsLocalSpeaking] = createSignal<boolean>(false);
export const [speakingPeers, setSpeakingPeers] = createSignal<Set<number>>(new Set());
export const [allVoiceParticipants, setVoiceParticipants] = createSignal<VoiceParticipant[]>([]);
export const participantsForChannel = (channelId: string) =>
  allVoiceParticipants().filter((participant) => participant.channel_id === channelId);
export const voiceParticipants = () => {
  const channel = activeVoiceChannel();
  return channel ? participantsForChannel(channel.id) : [];
};
export const [pingRtt, setPingRtt] = createSignal<number | null>(null);
let mutedBeforeDeafen = false;

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
    setSpeakingPeers(new Set<number>());
  } catch (err) {
    console.error('Failed to leave voice channel:', err);
  }
}

export async function toggleMute() {
  const next = !isMuted();
  if (isDeafened()) return;
  await api.setVoiceState(next, isDeafened());
  setIsMuted(next);
}

export async function toggleDeafen() {
  const next = !isDeafened();
  const previousMute = isMuted();
  const muted = next ? true : mutedBeforeDeafen;
  await api.setVoiceState(muted, next);
  if (next) mutedBeforeDeafen = previousMute;
  setIsDeafened(next);
  setIsMuted(muted);
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
