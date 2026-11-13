import { createSignal } from 'solid-js';
import { api } from '../services/api';
import { isLocallyMuted } from './localMuteStore';

// How loud each person is for me, remembered between calls (keyed by user id: peer ids change).
// The sound of someone's shared screen has its own volume, under "screen:<user id>".

const KEY = 'finchspeak.volumes';
export const MIN_VOLUME = 0;
export const MAX_VOLUME = 2;
export const DEFAULT_VOLUME = 1;

/** Mixer key of the sound of a peer's shared screen (matches `screen_audio_key` in Rust). */
export const screenAudioKey = (peerId: number) => peerId + 0x4000_0000;

function read(): Record<string, number> {
  try {
    const parsed = JSON.parse(localStorage.getItem(KEY) ?? '{}');
    if (!parsed || typeof parsed !== 'object') return {};
    return Object.fromEntries(Object.entries(parsed).filter(([, value]) => typeof value === 'number' && Number.isFinite(value)).map(([id, value]) => [id, clamp(value as number)]));
  } catch {
    return {};
  }
}

const clamp = (value: number) => Math.min(MAX_VOLUME, Math.max(MIN_VOLUME, value));

export const [volumes, setVolumes] = createSignal<Record<string, number>>(read());

/** The soundboard is a little quieter than people by default. */
export const SOUNDBOARD_VOLUME_KEY = 'soundboard';
export const DEFAULT_SOUNDBOARD_VOLUME = 0.8;

/** The remembered volume for `key` (a user id, "screen:<user id>", or the soundboard). */
export const volumeFor = (key: string) => volumes()[key] ?? (key === SOUNDBOARD_VOLUME_KEY ? DEFAULT_SOUNDBOARD_VOLUME : DEFAULT_VOLUME);

function persist() {
  try {
    // Only what differs from the default is worth keeping.
    const entries = Object.entries(volumes()).filter(([, value]) => value !== DEFAULT_VOLUME);
    localStorage.setItem(KEY, JSON.stringify(Object.fromEntries(entries)));
  } catch { /* not remembered */ }
}

/** Set someone's volume now (mixer key `mixerKey`) and remember it under `key`. */
export async function setVolume(key: string, mixerKey: number | undefined, value: number) {
  if (isLocallyMuted(key)) return;
  const volume = clamp(value);
  setVolumes((previous) => ({ ...previous, [key]: volume }));
  persist();
  if (mixerKey !== undefined) await api.setSpeakerVolume(mixerKey, volume);
}

/** A person (or their shared screen) appeared: give the mixer their remembered volume. */
export async function applySavedVolume(userId: string, peerId: number) {
  const voice = isLocallyMuted(userId) ? MIN_VOLUME : volumes()[userId];
  const screen = volumes()[`screen:${userId}`];
  try {
    if (voice !== undefined && voice !== DEFAULT_VOLUME) await api.setSpeakerVolume(peerId, voice);
  } catch { /* the mixer takes the default; the slider still shows the saved value */ }
  try {
    if (screen !== undefined && screen !== DEFAULT_VOLUME) await api.setSpeakerVolume(screenAudioKey(peerId), screen);
  } catch { /* voice and screen gain are independent */ }
}

// Room music volume is 0..1 (not 0..2 like people) and defaults to half; it is kept under the "music" key.
export const MUSIC_VOLUME_KEY = 'music';
export const DEFAULT_MUSIC_VOLUME = 0.5;

const clampMusic = (value: number) => (Number.isFinite(value) ? Math.min(1, Math.max(0, value)) : DEFAULT_MUSIC_VOLUME);

/** My room music volume, 0..1. */
export const musicVolumeValue = (): number => {
  const saved = volumes()[MUSIC_VOLUME_KEY];
  return saved === undefined ? DEFAULT_MUSIC_VOLUME : clampMusic(saved);
};

/** Remember my room music volume (the player itself is told by the caller). */
export function rememberMusicVolume(value: number) {
  setVolumes((previous) => ({ ...previous, [MUSIC_VOLUME_KEY]: clampMusic(value) }));
  persist();
}
