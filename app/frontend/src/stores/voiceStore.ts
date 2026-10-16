import { createSignal } from 'solid-js';
import type { AudioDevice, Channel, VoiceParticipant, VoiceSettings } from '../types';
import { api } from '../services/api';
import { setAppError } from './notificationStore';
import { resetScreenState } from './screenState';

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

// --- live microphone level, microphone test, link quality ----------------------------------

/** Quietest level the meter shows; also what "nothing" is. */
export const MIC_FLOOR_DB = -70;
export const [micLevelDb, setMicLevelDb] = createSignal<number>(MIC_FLOOR_DB);
export const [micTestPhase, setMicTestPhase] = createSignal<'idle' | 'recording' | 'playing'>('idle');
export type LinkQuality = 'good' | 'fair' | 'poor';
export const [voiceQuality, setVoiceQuality] = createSignal<Record<number, { quality: LinkQuality; loss: number }>>({});

/** Meter fill, 0..1, for a level in dBFS. */
export const levelFraction = (db: number) => Math.min(1, Math.max(0, (db - MIC_FLOOR_DB) / -MIC_FLOOR_DB));

export function handleMicLevel(level: { db: number }) {
  setMicLevelDb(Number.isFinite(level.db) ? level.db : MIC_FLOOR_DB);
}

export function handleMicTest(test: { phase: 'recording' | 'playing' | 'done' }) {
  setMicTestPhase(test.phase === 'done' ? 'idle' : test.phase);
}

export function handleVoiceQuality(report: { peer_id: number; quality: LinkQuality; loss: number }[]) {
  setVoiceQuality(Object.fromEntries(report.map((entry) => [entry.peer_id, { quality: entry.quality, loss: entry.loss }])));
}

export async function startMicTest(seconds = 3) {
  try {
    await api.testMicrophone(seconds);
  } catch (err) {
    setAppError(String(err).replace(/^Error:\s*/, ''));
  }
}
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

// --- microphone settings -------------------------------------------------------------------

export const DEFAULT_VOICE_SETTINGS: VoiceSettings = { input_gain: 1, vad_threshold_db: -45, push_to_talk: false, key_down: false, monitor: false };
export const [voiceSettings, setVoiceSettings] = createSignal<VoiceSettings>(DEFAULT_VOICE_SETTINGS);
/** The push-to-talk key, e.g. "F8". */
export const [pushToTalkKey, setPushToTalkKey] = createSignal<string | null>(null);

const VOICE_SETTINGS_KEY = 'finchspeak.voiceSettings';

type SavedVoice = { settings: Omit<VoiceSettings, 'key_down'>; key: string | null };

function readSavedVoice(): SavedVoice | null {
  try {
    const raw = localStorage.getItem(VOICE_SETTINGS_KEY);
    if (!raw) return null;
    const saved = JSON.parse(raw) as Partial<SavedVoice>;
    const number = (value: unknown, fallback: number) => (typeof value === 'number' && Number.isFinite(value) ? value : fallback);
    const s = saved.settings ?? ({} as Partial<SavedVoice['settings']>);
    return {
      settings: {
        input_gain: number(s.input_gain, 1),
        vad_threshold_db: number(s.vad_threshold_db, -45),
        push_to_talk: s.push_to_talk === true,
        monitor: s.monitor === true,
      },
      key: typeof saved.key === 'string' && saved.key ? saved.key : null,
    };
  } catch {
    return null;
  }
}

function saveVoice() {
  try {
    const { key_down: _down, ...settings } = voiceSettings();
    localStorage.setItem(VOICE_SETTINGS_KEY, JSON.stringify({ settings, key: pushToTalkKey() }));
  } catch {
    /* not remembered */
  }
}

/** Change some microphone settings; applied at once and remembered. */
export async function changeVoiceSettings(change: Partial<VoiceSettings>) {
  const next = { ...voiceSettings(), ...change };
  if (next.push_to_talk && !pushToTalkKey()) {
    setAppError('Choose a push-to-talk key first');
    return;
  }
  try {
    await api.setVoiceSettings(next);
  } catch (err) {
    setAppError(String(err));
    return;
  }
  setVoiceSettings(next);
  saveVoice();
}

export async function choosePushToTalkKey(key: string | null) {
  try {
    await api.setPushToTalkKey(key);
  } catch (err) {
    setAppError(String(err));
    return;
  }
  setPushToTalkKey(key);
  if (!key && voiceSettings().push_to_talk) await changeVoiceSettings({ push_to_talk: false });
  saveVoice();
}

/** Turn a key press into the shortcut text the backend understands ("Control+Shift+K"); null for a bare modifier. */
export function shortcutFromEvent(event: Pick<KeyboardEvent, 'code' | 'ctrlKey' | 'altKey' | 'shiftKey' | 'metaKey'>): string | null {
  const code = event.code;
  if (/^(Control|Alt|Shift|Meta|OS)(Left|Right)$/.test(code)) return null;
  const key = code.startsWith('Key') ? code.slice(3) : code.startsWith('Digit') ? code.slice(5) : code;
  if (!key) return null;
  const parts = [event.ctrlKey && 'Control', event.altKey && 'Alt', event.shiftKey && 'Shift', event.metaKey && 'Super'].filter(Boolean) as string[];
  return [...parts, key].join('+');
}

/** Push the remembered microphone settings and key to the backend (it forgets them on restart). */
export async function restoreVoiceSettings() {
  const saved = readSavedVoice();
  if (!saved) return;
  try {
    if (saved.key) await api.setPushToTalkKey(saved.key);
    setPushToTalkKey(saved.key);
    const settings = { ...saved.settings, key_down: false, push_to_talk: saved.settings.push_to_talk && !!saved.key };
    await api.setVoiceSettings(settings);
    setVoiceSettings(settings);
  } catch (err) {
    setAppError(String(err));
  }
}

export async function joinVoice(channel: Channel) {
  await restoreAudioDevices();
  await restoreVoiceSettings();
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
    resetScreenState();
    setActiveVoiceChannel(null);
    setSpeakingPeers(new Set<number>());
    setMicLevelDb(MIC_FLOOR_DB);
    setMicTestPhase('idle');
    setVoiceQuality({});
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
