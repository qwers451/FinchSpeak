import { api } from '../services/api';
import { prefs } from './settingsStore';
import { audioContext, playTone } from './tones';

// Custom join sounds: decoded once per person, dropped when the server says theirs changed.

const cache = new Map<string, Promise<AudioBuffer | null>>();

/** Forget a person's decoded sound (they set, changed or removed it). */
export function invalidateJoinSound(userId: string) {
  cache.delete(userId);
}

export function clearJoinSounds() {
  cache.clear();
}

async function load(userId: string): Promise<AudioBuffer | null> {
  const context = audioContext();
  if (!context) return null;
  const bytes = await api.downloadJoinSound(userId);
  if (!bytes) return null;
  return context.decodeAudioData(bytes);
}

/** The decoded join sound of `userId`, or null when they have none (or it cannot be had right now). */
export function joinSoundOf(userId: string): Promise<AudioBuffer | null> {
  const known = cache.get(userId);
  if (known) return known;
  const loading = load(userId).catch(() => {
    cache.delete(userId); // a failure is not remembered: try again next time
    return null;
  });
  cache.set(userId, loading);
  return loading;
}

function play(buffer: AudioBuffer, volume: number): boolean {
  const context = audioContext();
  if (!context) return false;
  try {
    const source = context.createBufferSource();
    const gain = context.createGain();
    source.buffer = buffer;
    gain.gain.value = Math.min(1, volume);
    source.connect(gain).connect(context.destination);
    source.start();
    return true;
  } catch { return false; }
}

/** Play `userId`'s own join sound at the chosen volume; the default tone when they have none. */
export async function playJoinSound(userId: string): Promise<void> {
  const { sounds, soundVolume } = prefs().notifications;
  if (!sounds || soundVolume <= 0) return;
  const buffer = await joinSoundOf(userId);
  if (buffer && play(buffer, soundVolume)) return;
  playTone('join');
}

/** Play my own sound as a preview; false when there is none. */
export async function previewJoinSound(userId: string): Promise<boolean> {
  const buffer = await joinSoundOf(userId);
  return buffer ? play(buffer, prefs().notifications.soundVolume || 0.5) : false;
}
