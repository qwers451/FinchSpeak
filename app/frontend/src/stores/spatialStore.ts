import { createEffect, createRoot, createSignal } from 'solid-js';
import { api } from '../services/api';
import type { VoiceParticipant } from '../types';
import { localPeerId } from './sessionStore';
import { activeVoiceChannel, voiceParticipants } from './voiceStore';

// Spatial audio: everyone in the room is heard from their own place between left and right.
// People are spread out automatically; dragging someone pins them there (remembered by user id,
// since peer ids change every session).

const KEY = 'finchspeak.spatial';
/** How far to the side the automatic layout goes (1 = all the way). */
export const AUTO_SPREAD = 0.8;

type Saved = { enabled: boolean; seats: Record<string, number> };

const clampPan = (value: number) => (Number.isFinite(value) ? Math.min(1, Math.max(-1, value)) : 0);

function read(): Saved {
  try {
    const parsed = JSON.parse(localStorage.getItem(KEY) ?? '{}') as Partial<Saved>;
    const seats = Object.fromEntries(Object.entries(parsed.seats ?? {})
      .filter(([, pan]) => typeof pan === 'number' && Number.isFinite(pan))
      .map(([id, pan]) => [id, clampPan(pan as number)]));
    return { enabled: parsed.enabled === true, seats };
  } catch {
    return { enabled: false, seats: {} };
  }
}

const saved = read();
export const [spatialEnabled, setSpatialEnabledSignal] = createSignal(saved.enabled);
export const [seats, setSeats] = createSignal<Record<string, number>>(saved.seats);

function persist() {
  try { localStorage.setItem(KEY, JSON.stringify({ enabled: spatialEnabled(), seats: seats() })); } catch { /* not remembered */ }
}

/** Where `index` of `count` people sits when nobody placed them: evenly from left to right. */
export function autoPan(index: number, count: number): number {
  if (count <= 1) return 0;
  return -AUTO_SPREAD + (2 * AUTO_SPREAD * index) / (count - 1);
}

/** Everyone else in my room, in a stable order (by name, then id). */
export function roomOthers(participants: VoiceParticipant[], channelId: string | undefined, me: number | null): VoiceParticipant[] {
  return participants
    .filter((participant) => participant.channel_id === channelId && participant.peer_id !== me)
    .sort((a, b) => a.username.localeCompare(b.username) || a.user_id.localeCompare(b.user_id));
}

/** Each person's place: the one they were dragged to, or their automatic one. */
export function panLayout(others: VoiceParticipant[], placed: Record<string, number>): { participant: VoiceParticipant; pan: number; pinned: boolean }[] {
  return others.map((participant, index) => {
    const pinned = placed[participant.user_id];
    return { participant, pan: pinned ?? autoPan(index, others.length), pinned: pinned !== undefined };
  });
}

export const currentLayout = () => panLayout(roomOthers(voiceParticipants(), activeVoiceChannel()?.id, localPeerId()), seats());

export function setSpatialEnabled(enabled: boolean) {
  setSpatialEnabledSignal(enabled);
  persist();
}

/** Put someone at `pan` (-1 left .. 1 right) and remember it. */
export function placeSeat(userId: string, pan: number) {
  setSeats((previous) => ({ ...previous, [userId]: Math.round(clampPan(pan) * 100) / 100 }));
  persist();
}

/** Move someone while a drag is going on: the screen follows, storage waits for `commitSeats`. */
export function dragSeat(userId: string, pan: number) {
  setSeats((previous) => ({ ...previous, [userId]: Math.round(clampPan(pan) * 100) / 100 }));
}

/** A drag ended: remember where everyone is. */
export function commitSeats() {
  persist();
}

/** Let someone (or, with no id, everyone) go back to the automatic layout. */
export function resetSeats(userId?: string) {
  setSeats((previous) => {
    if (!userId) return {};
    const { [userId]: _gone, ...rest } = previous;
    return rest;
  });
  persist();
}

/** Tell the mixer whether spatial audio is on and where everyone is. */
export async function syncSpatial() {
  try {
    await api.setSpatialAudio(spatialEnabled());
    if (!spatialEnabled()) return;
    for (const { participant, pan } of currentLayout()) await api.setSpeakerPan(participant.peer_id, pan);
  } catch { /* the mixer keeps its last layout; the next change tries again */ }
}

const SYNC_INTERVAL_MS = 50;
let lastSync = 0;
let syncTimer: ReturnType<typeof setTimeout> | undefined;

/** Sync with the mixer at most every 50 ms; whatever is newest when the timer fires is what is sent. */
export function scheduleSpatialSync() {
  if (syncTimer !== undefined) return;
  const wait = Math.max(0, SYNC_INTERVAL_MS - (Date.now() - lastSync));
  syncTimer = setTimeout(() => {
    syncTimer = undefined;
    lastSync = Date.now();
    void syncSpatial();
  }, wait);
}

// Keep the mixer in step with the switch, the room and the layout.
if (typeof document !== 'undefined') {
  createRoot(() => {
    createEffect(() => {
      spatialEnabled();
      currentLayout();
      scheduleSpatialSync();
    });
  });
}
