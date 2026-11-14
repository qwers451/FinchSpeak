import { createSignal } from 'solid-js';
import { t } from '../i18n';
import type { AudioDevice, Channel, VoiceParticipant, VoiceSettings } from '../types';
import { api } from '../services/api';
import { setAppError } from './notificationStore';
import { pushToast } from '../ui/toastStore';
import { describeError } from '../i18n/errors';
import { resetScreenState, setEndedShare, setWatching } from './screenState';
import { playTone, type Tone } from './tones';
import { prefs } from './settingsStore';

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
/** Actual local voice activation decision from the Rust capture pipeline. */
export const [micActivation, setMicActivation] = createSignal(false);
export const [micTestPhase, setMicTestPhase] = createSignal<'idle' | 'recording' | 'playing'>('idle');
export type LinkQuality = 'good' | 'fair' | 'poor';
export const [voiceQuality, setVoiceQuality] = createSignal<Record<number, { quality: LinkQuality; loss: number }>>({});

/** Meter fill, 0..1, for a level in dBFS. */
export const levelFraction = (db: number) => Math.min(1, Math.max(0, (db - MIC_FLOOR_DB) / -MIC_FLOOR_DB));

export function handleMicLevel(level: { db: number; speaking?: boolean }) {
  setMicLevelDb(Number.isFinite(level.db) ? level.db : MIC_FLOOR_DB);
  setMicActivation(level.speaking === true);
}

/** Drop preview state when capture can no longer supply fresh frames. */
export function resetMicPreview() {
  setMicLevelDb(MIC_FLOOR_DB);
  setMicActivation(false);
  setMicTestPhase('idle');
}

export function handleMicTest(test: { phase: 'recording' | 'playing' | 'done'; error?: string }) {
  setMicTestPhase(test.phase === 'done' ? 'idle' : test.phase);
  if (test.error) setAppError(t(test.error));
}

export function handleVoiceQuality(report: { peer_id: number; quality: LinkQuality; loss: number }[]) {
  setVoiceQuality(Object.fromEntries(report.map((entry) => [entry.peer_id, { quality: entry.quality, loss: entry.loss }])));
}

export async function startMicTest(seconds = 3) {
  try {
    await api.testMicrophone(seconds);
  } catch (err) {
    const message = String(err).replace(/^Error:\s*/, '');
    setAppError(t(message));
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
    setAppError(describeError(err));
  }
}

/** Pick a device (null = system default). Applied at once; kept only if the backend accepts it. */
export async function chooseAudioDevice(kind: keyof DeviceSelection, name: string | null) {
  const next = { ...selectedDevices(), [kind]: name };
  try {
    await api.setAudioDevices(next.input, next.output);
  } catch (err) {
    setAppError(t('Could not switch audio device: {error}', { error: describeError(err) }));
    return;
  }
  setSelectedDevices(next);
  saveDevices(next);
}

/** Push the remembered devices to the backend (it forgets them when the app restarts). */
export async function restoreAudioDevices() {
  const saved = readSavedDevices();
  if (!saved) return;
  try {
    await api.setAudioDevices(saved.input, saved.output);
    setSelectedDevices(saved);
  } catch (err) {
    setAppError(describeError(err));
  }
}

// --- microphone settings -------------------------------------------------------------------

export const DEFAULT_VOICE_SETTINGS: VoiceSettings = { input_gain: 1, vad_threshold_db: -45, push_to_talk: false, key_down: false, monitor: false, voice_enhancement: true };
export const [voiceSettings, setVoiceSettings] = createSignal<VoiceSettings>(DEFAULT_VOICE_SETTINGS);
/** The push-to-talk key, e.g. "F8". */
export const [pushToTalkKey, setPushToTalkKey] = createSignal<string | null>(null);
/** System-wide key that mutes or unmutes the microphone. */
export const [muteKey, setMuteKey] = createSignal<string | null>(null);
/** System-wide key that turns all sound off or back on. */
export const [deafenKey, setDeafenKey] = createSignal<string | null>(null);
/** System-wide key that shows or hides the mini window. None until the user chooses one. */
export const [overlayKey, setOverlayKey] = createSignal<string | null>(null);

export type HotkeyAction = 'push_to_talk' | 'toggle_mute' | 'toggle_deafen' | 'overlay';
const HOTKEY_LABELS: Record<HotkeyAction, string> = { push_to_talk: 'Push to talk', toggle_mute: 'Mute microphone', toggle_deafen: 'Turn sound off', overlay: 'Mini window' };
const hotkeySignals: Record<HotkeyAction, [() => string | null, (key: string | null) => void]> = {
  push_to_talk: [pushToTalkKey, setPushToTalkKey],
  toggle_mute: [muteKey, setMuteKey],
  toggle_deafen: [deafenKey, setDeafenKey],
  overlay: [overlayKey, setOverlayKey],
};
export const hotkeyFor = (action: HotkeyAction) => hotkeySignals[action][0]();

const VOICE_SETTINGS_KEY = 'finchspeak.voiceSettings';

type SavedVoice = { settings: Omit<VoiceSettings, 'key_down'>; key: string | null; muteKey: string | null; deafenKey: string | null; overlayKey: string | null };

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
        voice_enhancement: typeof s.voice_enhancement === 'boolean' ? s.voice_enhancement : true,
      },
      key: typeof saved.key === 'string' && saved.key ? saved.key : null,
      muteKey: typeof saved.muteKey === 'string' && saved.muteKey ? saved.muteKey : null,
      deafenKey: typeof saved.deafenKey === 'string' && saved.deafenKey ? saved.deafenKey : null,
      overlayKey: typeof saved.overlayKey === 'string' && saved.overlayKey ? saved.overlayKey : null,
    };
  } catch {
    return null;
  }
}

function saveVoice() {
  try {
    const { key_down: _down, ...settings } = voiceSettings();
    localStorage.setItem(VOICE_SETTINGS_KEY, JSON.stringify({ settings, key: pushToTalkKey(), muteKey: muteKey(), deafenKey: deafenKey(), overlayKey: overlayKey() }));
  } catch {
    /* not remembered */
  }
}

// Serialize complete settings snapshots so a slow acknowledgement cannot lose a later change.
let settingsWrite: Promise<void> = Promise.resolve();
function queueSettingsWrite(action: () => Promise<void>): Promise<void> {
  const pending = settingsWrite.then(action);
  settingsWrite = pending.catch(() => {});
  return pending;
}

/** False after the push-to-talk key failed to register: the backend is then told push-to-talk is off. */
let pushToTalkKeyRegistered = true;
const forBackend = (settings: VoiceSettings): VoiceSettings => ({ ...settings, push_to_talk: settings.push_to_talk && pushToTalkKeyRegistered });

/** Change microphone settings; publish/persist only after backend acknowledgement. */
export function changeVoiceSettings(change: Partial<VoiceSettings>) {
  return queueSettingsWrite(async () => {
    const next = { ...voiceSettings(), ...change };
    if (next.push_to_talk && !pushToTalkKey()) {
      setAppError(t('Choose a push-to-talk key first'));
      return;
    }
    try {
      await api.setVoiceSettings(forBackend(next));
    } catch (err) {
      setAppError(describeError(err));
      return;
    }
    setVoiceSettings(next);
    saveVoice();
  });
}

/** Backend key errors are English text; show them translated. */
export function hotkeyErrorText(err: unknown): string {
  const message = String(err).replace(/^Error:\s*/, '');
  let match = /^"(.*)" cannot be used as a key: ([\s\S]*)$/.exec(message);
  if (match) return t('"{key}" cannot be used as a key: {detail}', { key: match[1], detail: match[2] });
  match = /^Could not use "(.*)" as a key: ([\s\S]*)$/.exec(message);
  if (match) return t('Could not use "{key}" as a key: {detail}', { key: match[1], detail: match[2] });
  match = /^"(.*)" is already used by another action$/.exec(message);
  if (match) return t('"{key}" is already used by another action', { key: match[1] });
  if (message === 'Keys are unavailable') return t('Keys are unavailable');
  return describeError(message);
}

const sendHotkey = (action: HotkeyAction, key: string | null) =>
  action === 'push_to_talk' ? api.setPushToTalkKey(key) : api.setHotkey(action, key);

/** Choose the system-wide key for an action; null removes it. One key serves one action. */
export async function chooseHotkey(action: HotkeyAction, key: string | null) {
  const taken = key && (Object.keys(hotkeySignals) as HotkeyAction[]).find((other) => other !== action && hotkeyFor(other) === key);
  if (taken) {
    setAppError(t('"{key}" is already used for "{action}"', { key, action: t(HOTKEY_LABELS[taken]) }));
    return;
  }
  try {
    await sendHotkey(action, key);
  } catch (err) {
    setAppError(hotkeyErrorText(err));
    return;
  }
  hotkeySignals[action][1](key);
  if (action === 'push_to_talk') {
    const wasRegistered = pushToTalkKeyRegistered;
    pushToTalkKeyRegistered = true;
    if (!key && voiceSettings().push_to_talk) await changeVoiceSettings({ push_to_talk: false });
    else if (key && !wasRegistered && voiceSettings().push_to_talk) await changeVoiceSettings({}); // the working key brings push-to-talk back
  }
  saveVoice();
}

export const choosePushToTalkKey = (key: string | null) => chooseHotkey('push_to_talk', key);

/** A mute or deafen key was pressed somewhere in the system. */
export async function handleHotkey(action: string) {
  if (!activeVoiceChannel()) return; // outside a room there is nothing to mute
  try {
    if (action === 'toggle_mute') await toggleMute();
    else if (action === 'toggle_deafen') await toggleDeafen();
  } catch (err) {
    setAppError(describeError(err));
  }
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
export function restoreVoiceSettings() {
  return queueSettingsWrite(async () => {
    const saved = readSavedVoice();
    if (!saved) return;
    const failed = (action: HotkeyAction, key: string) => {
      pushToast({ kind: 'warn', title: t('Could not use {key} for "{action}": another program may be using it.', { key, action: t(HOTKEY_LABELS[action]) }) });
    };
    // Each key on its own: one that is taken must not cost the others or the microphone settings.
    // The signals keep the saved key either way, so saving later does not forget it.
    pushToTalkKeyRegistered = true;
    if (saved.key) {
      try {
        await api.setPushToTalkKey(saved.key);
      } catch {
        pushToTalkKeyRegistered = false;
        failed('push_to_talk', saved.key);
      }
    }
    setPushToTalkKey(saved.key);
    for (const [action, key] of [['toggle_mute', saved.muteKey], ['toggle_deafen', saved.deafenKey], ['overlay', saved.overlayKey]] as const) {
      if (key) {
        try {
          await api.setHotkey(action, key);
        } catch {
          failed(action, key);
        }
      }
      hotkeySignals[action][1](key);
    }
    try {
      // The preference stays as saved; only the backend is told false while the key is not registered.
      const settings = { ...saved.settings, key_down: false, push_to_talk: saved.settings.push_to_talk && !!saved.key };
      await api.setVoiceSettings(forBackend(settings));
      setVoiceSettings(settings);
    } catch (err) {
      setAppError(describeError(err));
    }
  });
}

/** A tone for something I did. Silent when the voice-actions sounds are off; the master sounds switch and volume are in playTone. */
function feedback(tone: Tone) {
  if (prefs().notifications.voiceSounds) playTone(tone);
}

/** Push-to-talk went down or up: a tone, but only in a room and with push-to-talk on. */
export function handlePushToTalk(pressed: boolean) {
  if (!activeVoiceChannel() || !voiceSettings().push_to_talk) return;
  feedback(pressed ? 'ptt-on' : 'ptt-off');
}

let joinRequestedAt = 0;

/** When the last join was sent (ms since epoch). A refusal right after it means the join did not stand. */
export function lastJoinRequestedAt(): number {
  return joinRequestedAt;
}

export async function joinVoice(channel: Channel) {
  await restoreAudioDevices();
  await restoreVoiceSettings();
  try {
    const previous = activeVoiceChannel();
    joinRequestedAt = Date.now();
    await api.joinVoiceChannel(channel.id, channel.short_id);
    // The backend ends our share (announced by screen-share-ended) and stops the one we watched;
    // only the watched share is cleared here.
    if (previous && previous.id !== channel.id) {
      setWatching(null);
      setEndedShare(null);
    }
    setActiveVoiceChannel(channel);
    feedback('join');
  } catch (err) {
    console.error('Failed to join voice channel:', err);
    setAppError(t('Could not join voice: {error}', { error: describeError(err) }));
  }
}

export async function leaveVoice() {
  const wasInRoom = activeVoiceChannel() !== null;
  try {
    await api.leaveVoiceChannel();
    resetScreenState();
    setActiveVoiceChannel(null);
    setSpeakingPeers(new Set<number>());
    resetMicPreview();
    setVoiceQuality({});
    if (wasInRoom) feedback('leave');
  } catch (err) {
    console.error('Failed to leave voice channel:', err);
  }
}

export async function toggleMute() {
  if (isDeafened()) {
    // Unmuting while deafened also turns the sound back on (Discord behaviour).
    await api.setVoiceState(false, false);
    setIsDeafened(false);
    setIsMuted(false);
    feedback('undeafen');
    return;
  }
  const next = !isMuted();
  await api.setVoiceState(next, isDeafened());
  setIsMuted(next);
  feedback(next ? 'mute' : 'unmute');
}

export async function toggleDeafen() {
  const next = !isDeafened();
  const previousMute = isMuted();
  const muted = next ? true : mutedBeforeDeafen;
  await api.setVoiceState(muted, next);
  if (next) mutedBeforeDeafen = previousMute;
  setIsDeafened(next);
  setIsMuted(muted);
  feedback(next ? 'deafen' : 'undeafen');
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
