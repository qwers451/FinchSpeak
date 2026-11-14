import { createEffect, createRoot, createSignal, on } from 'solid-js';
import { describeError } from '../i18n/errors';
import { api } from '../services/api';
import type { SoundInfo } from '../types';
import { currentUser } from './authStore';
import { encodeBoardSound } from './joinSoundEncode';
import { isLocallyMuted } from './localMuteStore';
import { setAppError } from './notificationStore';
import { serverFeatures } from './sessionStore';
import { audioContext } from './tones';
import { activeVoiceChannel, isDeafened } from './voiceStore';
import { SOUNDBOARD_VOLUME_KEY, volumeFor } from './volumeStore';

// The shared soundboard. Nothing is mixed into voice: each client plays a played sound from its own cache.

const HIDDEN_KEY = 'finchspeak.soundboardHidden';

function readHidden(): boolean {
  try {
    return localStorage.getItem(HIDDEN_KEY) === 'true';
  } catch {
    return false;
  }
}

/** "Не слышать саундборд": sounds other people play are not played for me (I can still play them). */
export const [soundboardHidden, setHiddenState] = createSignal(readHidden());

export function setSoundboardHidden(hidden: boolean) {
  setHiddenState(hidden);
  try {
    localStorage.setItem(HIDDEN_KEY, hidden ? 'true' : 'false');
  } catch { /* kept for this session only */ }
}

/** Whether the soundboard panel is open in the voice room. */
export const [soundboardOpen, setSoundboardOpen] = createSignal(false);

// Leaving the room closes the board, so joining again does not pop it open.
createRoot(() => createEffect(on(activeVoiceChannel, (room) => { if (!room) setSoundboardOpen(false); }, { defer: true })));

/** The latest sound played in the room, and who played it: the tile of that person shows their name for a moment. */
export const [lastSound, setLastSound] = createSignal<{ userId: string; at: number } | null>(null);

/** The server announces the soundboard only when it is switched on. */
export const soundboardAvailable = (): boolean => serverFeatures().includes('soundboard');

const [sounds, setSounds] = createSignal<SoundInfo[]>([]);

/** The sounds on the board, in the order the server lists them. */
export const boardSounds = (): SoundInfo[] => sounds();

/** Reload the board from the server. */
export async function loadBoard(): Promise<void> {
  try {
    setSounds(await api.soundboard());
  } catch (err) {
    setAppError(describeError(err));
  }
}

/** Whether I may remove this sound: I uploaded it, or I am a moderator or an admin. */
export function canRemove(sound: SoundInfo): boolean {
  const me = currentUser();
  if (!me) return false;
  return me.id === sound.uploader_id || me.role === 'admin' || me.role === 'moderator';
}

// Decoded sounds, kept until they are removed or the app restarts.
const buffers = new Map<string, Promise<AudioBuffer | null>>();

function bufferOf(soundId: string): Promise<AudioBuffer | null> {
  const known = buffers.get(soundId);
  if (known) return known;
  const loading = (async () => {
    const context = audioContext();
    if (!context) return null;
    const bytes = await api.downloadBoardSound(soundId);
    if (!bytes) return null;
    return context.decodeAudioData(bytes);
  })().catch(() => {
    if (buffers.get(soundId) === loading) buffers.delete(soundId);
    return null;
  });
  buffers.set(soundId, loading);
  return loading;
}

/** Same perceptual curve as `volume_gain` in finch-audio: squared up to 100 %, linear to 200 %. */
export function volumeGain(fraction: number): number {
  if (!Number.isFinite(fraction)) return 0;
  const p = Math.min(2, Math.max(0, fraction));
  return p <= 1 ? p * p : p;
}

async function playCached(soundId: string, volume: number): Promise<void> {
  const buffer = await bufferOf(soundId);
  const context = audioContext();
  if (!buffer || !context) return;
  try {
    const source = context.createBufferSource();
    const gain = context.createGain();
    source.buffer = buffer;
    gain.gain.value = volumeGain(volume);
    source.connect(gain).connect(context.destination);
    source.start();
  } catch { /* the sound is skipped */ }
}

/** Someone played a sound in my room. Each client plays it itself, at my own soundboard volume. */
export function handleSoundPlayed(event: { sound_id: string; user_id: string }): void {
  setLastSound({ userId: event.user_id, at: Date.now() });
  if (soundboardHidden() || isDeafened() || isLocallyMuted(event.user_id)) return;
  const volume = volumeFor(SOUNDBOARD_VOLUME_KEY);
  if (volume <= 0) return;
  void playCached(event.sound_id, volume);
}

/** A click on a sound: the server relays it to my room (I hear it through the same path). */
export async function playBoardSound(soundId: string): Promise<void> {
  try {
    await api.playBoardSound(soundId);
  } catch (err) {
    setAppError(describeError(err));
  }
}

/** Upload a sound from any audio file: it is trimmed to 5 seconds and encoded as 24 kHz mono WAV. */
export async function uploadBoardSound(file: File, name: string, emoji?: string): Promise<void> {
  const wav = await encodeBoardSound(file);
  const sound = await api.uploadBoardSound(wav, name, emoji);
  setSounds((list) => [...list, sound]);
}

export async function removeBoardSound(soundId: string): Promise<void> {
  await api.deleteBoardSound(soundId);
  buffers.delete(soundId);
  setSounds((list) => list.filter((sound) => sound.id !== soundId));
}

/** Test helper: forget the board and cached sounds. */
export function resetSoundboardForTests(): void {
  buffers.clear();
  setSounds([]);
}
