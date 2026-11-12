import { createSignal } from 'solid-js';
import { api } from '../services/api';
import type { Channel, FlockCall } from '../types';
import { channels } from './chatStore';
import { setAppError } from './notificationStore';
import { activeVoiceChannel, joinVoice } from './voiceStore';
import { describeError } from '../i18n/errors';

/** A call card closes by itself after this long unless it is answered. */
export const FLOCK_CALL_TTL_MS = 30_000;

export type PendingFlockCall = FlockCall & { id: number };

const [calls, setCalls] = createSignal<PendingFlockCall[]>([]);
let nextId = 1;
const timers = new Map<number, ReturnType<typeof setTimeout>>();

/** Calls waiting for an answer, oldest first. */
export const pendingCalls = (): PendingFlockCall[] => calls();

/** A call from someone who already has a card replaces that card. */
export function receiveFlockCall(call: FlockCall) {
  for (const old of calls().filter((item) => item.from_user_id === call.from_user_id)) dismissCall(old.id);
  const id = nextId++;
  setCalls((list) => [...list, { ...call, id }]);
  timers.set(id, setTimeout(() => dismissCall(id), FLOCK_CALL_TTL_MS));
}

export function dismissCall(id: number) {
  clearTimeout(timers.get(id));
  timers.delete(id);
  setCalls((list) => list.filter((call) => call.id !== id));
}

/** "Лететь": join the room they called me to. Already being there changes nothing. */
export async function acceptCall(id: number) {
  const call = calls().find((item) => item.id === id);
  if (!call) return;
  dismissCall(id);
  const room = channels().find((channel) => channel.id === call.channel_id && channel.channel_type === 'voice');
  if (!room || activeVoiceChannel()?.id === room.id) return;
  await joinVoice(room);
}

/** "Позвать за собой": call my room to `target`, then fly there myself. */
export async function callFlock(target: Channel) {
  try {
    await api.callFlock(target.id);
  } catch (err) {
    setAppError(describeError(err));
  }
  await joinVoice(target);
}

/** "Позвать сюда": one person who is online, but not in my room, is called to my room. */
export async function callUserHere(userId: string) {
  try {
    await api.callUser(userId);
  } catch (err) {
    setAppError(describeError(err));
  }
}
