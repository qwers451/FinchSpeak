import { createSignal } from 'solid-js';
import type { AudioDevice, Channel, VoiceParticipant } from '../types';
import { api } from '../services/api';
import { setAppError } from './notificationStore';

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

// --- audio devices -------------------------------------------------------------------------

export type DeviceSelection = { input: string | null; output: string | null };
export const [availableDevices, setAvailableDevices] =
  createSignal<{ inputs: AudioDevice[]; outputs: AudioDevice[] }>({ inputs: [], outputs: [] });
/** The user's choice; null = system default. */
export const [selectedDevices, setSelectedDevices] = createSignal<DeviceSelection>({ input: null, output: null });

const DEVICES_KEY = 'finchspeak.audioDevices';

function readSavedDevices(): DeviceSelection | null {
  try {
    const raw = localStorage.getItem(DEVICES_KEY);
    if (!raw) return null;
    const saved = JSON.parse(raw) as Partial<DeviceSelection>;
    const name = (value: unknown) => (typeof value === 'string' && value ? value : null);
    return { input: name(saved.input), output: name(saved.output) };
  } catch {
    return null; // storage blocked or corrupt: the choice just is not remembered
  }
}

function saveDevices(selection: DeviceSelection) {
  try {
    localStorage.setItem(DEVICES_KEY, JSON.stringify(selection));
  } catch {
    /* not remembered */
  }
}

export async function loadAudioDevices() {
  try {
    const devices = await api.getAudioDevices();
    setAvailableDevices({ inputs: devices.inputs, outputs: devices.outputs });
    setSelectedDevices({ input: devices.selected_input, output: devices.selected_output });
  } catch (err) {
    setAppError(String(err));
  }
}

/** Pick a device (null = system default). Applied at once; kept only if the backend accepts it. */
export async function chooseAudioDevice(kind: keyof DeviceSelection, name: string | null) {
  const next = { ...selectedDevices(), [kind]: name };
  try {
    await api.setAudioDevices(next.input, next.output);
  } catch (err) {
    setAppError(`Could not switch audio device: ${String(err)}`);
    return;
  }
  setSelectedDevices(next);
  saveDevices(next);
}

/** Push the remembered devices to the backend (it forgets them when the app restarts). */
async function restoreAudioDevices() {
  const saved = readSavedDevices();
  if (!saved) return;
  try {
    await api.setAudioDevices(saved.input, saved.output);
    setSelectedDevices(saved);
  } catch (err) {
    setAppError(String(err));
  }
}

export async function joinVoice(channel: Channel) {
  await restoreAudioDevices();
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
