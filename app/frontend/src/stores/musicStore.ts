import { createSignal } from 'solid-js';
import { api } from '../services/api';
import { describeError } from '../i18n/errors';
import type { MusicCommand, MusicEvents, MusicRoomState } from '../types';
import { activeVoiceChannel } from './voiceStore';
import { serverFeatures } from './sessionStore';
import { setAppError } from './notificationStore';
import { DEFAULT_MUSIC_VOLUME, musicVolumeValue, rememberMusicVolume } from './volumeStore';

/** How often the local position is refreshed; between refreshes nothing is requested from the server. */
export const MUSIC_TICK_MS = 500;

type Snapshot = { channelId: string; state: MusicRoomState; serverMs: number; receivedAt: number };

const [snapshot, setSnapshot] = createSignal<Snapshot | null>(null);
const [clock, setClock] = createSignal(Date.now());
let timer: ReturnType<typeof setInterval> | undefined;
let volumeBeforeMute: number | null = null;

/** Whether the room music panel is open (the dock button, the strips and the room header open it). */
export const [musicPanelOpen, setMusicPanelOpen] = createSignal(false);

const clampUnit = (value: number) => (Number.isFinite(value) ? Math.min(1, Math.max(0, value)) : DEFAULT_MUSIC_VOLUME);

function startClock() {
  if (timer !== undefined) return;
  timer = setInterval(() => setClock(Date.now()), MUSIC_TICK_MS);
}

function stopClock() {
  if (timer === undefined) return;
  clearInterval(timer);
  timer = undefined;
}

/** Whether the server offers the room music player at all. Without it there is no button and no panel. */
export const musicAvailable = (): boolean => serverFeatures().includes('music');

/** The music state of the voice room I am in (null when I am in no room or nothing was received for it). */
export function musicState(): MusicRoomState | null {
  const current = snapshot();
  const room = activeVoiceChannel();
  if (!current || !room || room.id !== current.channelId) return null;
  return current.state;
}

/** The room the current music state belongs to. */
export const musicChannelId = (): string | null => snapshot()?.channelId ?? null;

/**
 * Where the track is now, in ms. Built from the last server state and the local clock:
 * the paused position stays, a playing one moves on.
 */
export function musicPosition(): number {
  const current = snapshot();
  const playing = musicState()?.playing;
  if (!current || !playing) return 0;
  if (playing.paused) return playing.position_ms;
  const atSend = playing.position_ms + Math.max(0, current.serverMs - playing.at_server_ms);
  return atSend + Math.max(0, clock() - current.receivedAt);
}

export function applyMusicState(payload: MusicEvents['music-state']) {
  const previous = snapshot();
  const now = Date.now();
  const roomChanged = previous?.channelId !== payload.channel_id;
  setSnapshot({ channelId: payload.channel_id, state: payload.state, serverMs: payload.server_ms, receivedAt: now });
  setClock(now);
  if (payload.state.playing) startClock();
  else stopClock();
  // The player keeps its own gain; give it my remembered volume when a room's music starts for me.
  if (roomChanged) void api.musicSetVolume(musicVolumeValue()).catch(() => {});
}

export function applyMusicError(payload: MusicEvents['music-error']) {
  setAppError(describeError(payload.message));
}

/** Handlers for the music events; subscribed in `services/sessionEvents.ts`. */
export const musicHandlers: { [K in keyof MusicEvents]: (payload: MusicEvents[K]) => void } = {
  'music-state': applyMusicState,
  'music-error': applyMusicError,
};

/** Sends a queue or playback command. A refused command shows its reason. */
export async function sendMusicCommand(command: MusicCommand): Promise<void> {
  try {
    await api.musicCommand(command);
  } catch (error) {
    setAppError(describeError(error));
  }
}

/** My room music volume, 0..1, remembered between launches. */
export const musicVolume = (): number => musicVolumeValue();

/** Sets and remembers my room music volume and passes it to the player. */
export async function setMusicVolume(value: number): Promise<void> {
  const volume = clampUnit(value);
  rememberMusicVolume(volume);
  await api.musicSetVolume(volume);
}

/** "Turn music off for me": 0 while remembering the previous volume; a second press restores it. */
export async function toggleMusicMute(): Promise<void> {
  const current = musicVolume();
  if (current > 0) {
    volumeBeforeMute = current;
    await setMusicVolume(0);
    return;
  }
  const restore = volumeBeforeMute !== null && volumeBeforeMute > 0 ? volumeBeforeMute : DEFAULT_MUSIC_VOLUME;
  volumeBeforeMute = null;
  await setMusicVolume(restore);
}

/** Forget the room's music (the user left it or the session ended). */
export function resetMusic() {
  stopClock();
  setSnapshot(null);
  setMusicPanelOpen(false);
}
