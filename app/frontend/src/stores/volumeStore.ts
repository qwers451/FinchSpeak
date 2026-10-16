import { createSignal } from 'solid-js';
import { api } from '../services/api';

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

/** The remembered volume for `key` (a user id, or "screen:<user id>"). */
export const volumeFor = (key: string) => volumes()[key] ?? DEFAULT_VOLUME;

function persist() {
  try {
    // Only what differs from the default is worth keeping.
    const entries = Object.entries(volumes()).filter(([, value]) => value !== DEFAULT_VOLUME);
    localStorage.setItem(KEY, JSON.stringify(Object.fromEntries(entries)));
  } catch { /* not remembered */ }
}

/** Set someone's volume now (mixer key `mixerKey`) and remember it under `key`. */
export async function setVolume(key: string, mixerKey: number, value: number) {
  const volume = clamp(value);
  setVolumes((previous) => ({ ...previous, [key]: volume }));
  persist();
  await api.setSpeakerVolume(mixerKey, volume);
}

/** A person (or their shared screen) appeared: give the mixer their remembered volume. */
export async function applySavedVolume(userId: string, peerId: number) {
  const voice = volumes()[userId];
  const screen = volumes()[`screen:${userId}`];
  try {
    if (voice !== undefined && voice !== DEFAULT_VOLUME) await api.setSpeakerVolume(peerId, voice);
    if (screen !== undefined && screen !== DEFAULT_VOLUME) await api.setSpeakerVolume(screenAudioKey(peerId), screen);
  } catch { /* the mixer takes the default; the slider still shows the saved value */ }
}
