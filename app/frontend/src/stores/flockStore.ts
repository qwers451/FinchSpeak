import { createSignal } from 'solid-js';
import { api } from '../services/api';
import type { Channel, FlockCall } from '../types';
import { channels } from './chatStore';
import { setAppError } from './notificationStore';
import { activeVoiceChannel, joinVoice } from './voiceStore';
import { describeError } from '../i18n/errors';
import { t } from '../i18n';

/** A call card closes by itself after this long unless it is answered. */
export const FLOCK_CALL_TTL_MS = 30_000;
/** A server refusal can only answer a call that was sent this recently. */
const REFUSAL_WINDOW_MS = 5_000;

export type PendingFlockCall = FlockCall & { id: number };

const [calls, setCalls] = createSignal<PendingFlockCall[]>([]);
let nextId = 1;
const timers = new Map<number, ReturnType<typeof setTimeout>>();
let lastCallSentAt = 0;

/** Calls waiting for an answer, oldest first. */
export const pendingCalls = (): PendingFlockCall[] => calls();

/** The pending call to `channelId`, if any (a notification button answers it). */
export const pendingCallTo = (channelId: string): PendingFlockCall | undefined => calls().find((call) => call.channel_id === channelId);

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

/**
 * The server refused a call I just sent (a control error). Says why and returns true; any other
 * error, or one that came long after a call, is not ours and returns false.
 */
export function handleFlockRefusal(error: { code: number; message: string }): boolean {
  if (Date.now() - lastCallSentAt > REFUSAL_WINDOW_MS) return false;
  lastCallSentAt = 0;
  if (error.code === 429) setAppError(t('Wait a moment before calling again'));
  else if (error.code === 404) setAppError(t('This person is offline'));
  else setAppError(`${error.code}: ${error.message}`);
  return true;
}

/** Move me into a voice room. Already being there changes nothing. */
export async function flyToRoom(channelId: string) {
  const room = channels().find((channel) => channel.id === channelId && channel.channel_type === 'voice');
  if (!room || activeVoiceChannel()?.id === room.id) return;
  await joinVoice(room);
}

/** "Лететь": join the room they called me to. */
export async function acceptCall(id: number) {
  const call = calls().find((item) => item.id === id);
  if (!call) return;
  dismissCall(id);
  await flyToRoom(call.channel_id);
}

/** "Позвать за собой": call my room to `target`, then fly there myself, unless the server refused the call. */
export async function callFlock(target: Channel) {
  try {
    lastCallSentAt = Date.now();
    await api.callFlock(target.id);
  } catch (err) {
    lastCallSentAt = 0;
    setAppError(describeError(err));
    return;
  }
  await joinVoice(target);
}

/** "Позвать сюда": one person who is online, but not in my room, is called to my room. */
export async function callUserHere(userId: string) {
  try {
    lastCallSentAt = Date.now();
    await api.callUser(userId);
  } catch (err) {
    lastCallSentAt = 0;
    setAppError(describeError(err));
  }
}
