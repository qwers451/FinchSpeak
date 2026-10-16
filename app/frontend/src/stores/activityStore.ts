import { createSignal } from 'solid-js';
import { api } from '../services/api';
import type { ChannelUnread, Message } from '../types';
import { activeChannel, messages } from './chatStore';
import { currentUser } from './authStore';

// --- unread messages ---------------------------------------------------------------------------

export type Unread = { count: number; mention: boolean; mentions?: number; firstUnreadId?: string | null };
export const [unread, setUnread] = createSignal<Record<string, Unread>>({});

/** Does `text` address the user (@display name or @login)? Only a fallback: the server sends the real `mentions`. */
export function mentions(text: string, user = currentUser()): boolean {
  if (!user) return false;
  const lower = text.toLowerCase();
  return [user.display_name, user.username].filter(Boolean).some((name) => lower.includes(`@${name.toLowerCase()}`));
}

/** Is the user named in this message (by the server's list when there is one)? */
export function isMentioned(message: Pick<Message, 'content' | 'mentions'>, user = currentUser()): boolean {
  if (!user) return false;
  return message.mentions ? message.mentions.includes(user.id) : mentions(message.content, user);
}

/** What the server says is unread (on sign-in and after a reconnect). */
export function setUnreadFromServer(list: ChannelUnread[] | undefined) {
  const open = activeChannel()?.id;
  setUnread(Object.fromEntries((list ?? [])
    .filter((entry) => entry.count > 0 && !(entry.channel_id === open && document.hasFocus()))
    .map((entry) => [entry.channel_id, { count: entry.count, mention: entry.mentions > 0, mentions: entry.mentions, firstUnreadId: entry.first_unread_id ?? null }])));
}

/** A message arrived: count it unless it is ours or the channel is open. */
export function noteIncoming(message: Pick<Message, 'channel_id' | 'author_id' | 'content' | 'mentions'> & { id?: string; message_id?: string }) {
  if (message.author_id === currentUser()?.id) return;
  if (activeChannel()?.id === message.channel_id && document.hasFocus()) return;
  const mentioned = isMentioned(message);
  setUnread((previous) => {
    const entry = previous[message.channel_id] ?? { count: 0, mention: false };
    return { ...previous, [message.channel_id]: {
      count: entry.count + 1, mention: entry.mention || mentioned,
      mentions: (entry.mentions ?? 0) + (mentioned ? 1 : 0), firstUnreadId: entry.firstUnreadId ?? message.id ?? message.message_id ?? null,
    } };
  });
}

let readTimer: ReturnType<typeof setTimeout> | undefined;
const readPending = new Map<string, string>();

/** Tell the server how far a channel has been read (batched: only the newest per channel is sent). */
function reportRead(channelId: string, messageId: string) {
  readPending.set(channelId, messageId);
  clearTimeout(readTimer);
  readTimer = setTimeout(() => {
    for (const [channel, message] of readPending) void api.markChannelRead(channel, message).catch(() => { /* retried with the next message */ });
    readPending.clear();
  }, 300);
}

/** The channel counts as read; pass the newest stored message id so the server remembers it too. */
export function markRead(channelId: string, lastMessageId?: string | null) {
  if (lastMessageId && !lastMessageId.startsWith('local-')) reportRead(channelId, lastMessageId);
  setUnread((previous) => {
    if (!(channelId in previous)) return previous;
    const { [channelId]: _read, ...rest } = previous;
    return rest;
  });
}

// --- typing ------------------------------------------------------------------------------------

const TYPING_SHOWN_MS = 4000;
const TYPING_SEND_EVERY_MS = 3000;

/** channel id -> user id -> { name, until }. */
export const [typing, setTyping] = createSignal<Record<string, Record<string, { name: string; until: number }>>>({});

export function noteTyping(event: { channel_id: string; user_id: string; username: string }, now = Date.now()) {
  setTyping((previous) => ({
    ...previous,
    [event.channel_id]: { ...previous[event.channel_id], [event.user_id]: { name: event.username, until: now + TYPING_SHOWN_MS } },
  }));
}

/** Names of the people typing in `channelId` at `now` (nobody once their hint has expired). */
export function typingNames(channelId: string, now = Date.now()): string[] {
  return Object.values(typing()[channelId] ?? {}).filter((entry) => entry.until > now).map((entry) => entry.name);
}

/** A message from someone ends their typing hint. */
export function clearTyping(channelId: string, userId: string) {
  setTyping((previous) => {
    const channel = { ...previous[channelId] };
    delete channel[userId];
    return { ...previous, [channelId]: channel };
  });
}

let lastTypingSent = 0;

/** Call on every keystroke; tells the server at most every few seconds. */
export function notifyTyping(channelId: string, now = Date.now()) {
  if (now - lastTypingSent < TYPING_SEND_EVERY_MS) return;
  lastTypingSent = now;
  void api.sendTyping(channelId).catch(() => { /* a missing hint is not worth an error */ });
}

// Coming back to the window marks the open channel as read.
if (typeof window !== 'undefined') {
  window.addEventListener('focus', () => {
    const channel = activeChannel();
    if (channel) markRead(channel.id, [...messages()].reverse().find((message) => !message.state)?.id);
  });
}

export function resetActivity() {
  setUnread({});
  setTyping({});
  lastTypingSent = 0;
}
