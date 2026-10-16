import { createSignal } from 'solid-js';
import type { Channel, Message, IncomingMessage } from '../types';
import { api } from '../services/api';
import { currentUser } from './authStore';
import { setAppError } from './notificationStore';
import { outboxForChannel, removeFromOutbox, saveToOutbox, type OutboxEntry } from './outbox';

export const [channels, setChannels] = createSignal<Channel[]>([]);
export const [activeChannel, setActiveChannel] = createSignal<Channel | null>(null);
export const [messages, setMessages] = createSignal<Message[]>([]);
export const [chatLoading, setChatLoading] = createSignal<boolean>(false);
/** Older messages exist on the server beyond the oldest one shown. */
export const [hasMoreHistory, setHasMoreHistory] = createSignal<boolean>(false);
export const [loadingOlder, setLoadingOlder] = createSignal<boolean>(false);
/** The message the next sent message answers. */
export const [replyingTo, setReplyingTo] = createSignal<Message | null>(null);

/** A message to scroll to and flash once it is on screen (from a search result). */
export const [revealTarget, setRevealTarget] = createSignal<string | null>(null);

export const HISTORY_PAGE = 50;
export const MAX_MESSAGE_CHARS = 4000;

let historyRequest = 0;
const receivedMessageIds = new Set<string>();

const isLocal = (id: string) => id.startsWith('local-');

function chronological(items: Message[]) {
  return items.sort((a, b) => a.timestamp - b.timestamp || (isLocal(a.id) || isLocal(b.id) ? 0 : a.id.localeCompare(b.id)));
}

export function resetChatState() {
  historyRequest++;
  receivedMessageIds.clear();
  setActiveChannel(null);
  setChannels([]);
  setMessages([]);
  setChatLoading(false);
  setHasMoreHistory(false);
  setLoadingOlder(false);
  setReplyingTo(null);
}

export async function loadChannels() {
  try {
    const list = await api.listChannels();
    setChannels(list);
    if (!activeChannel() && list.length > 0) {
      const firstText = list.find((c) => c.channel_type === 'text') || list[0];
      selectChannel(firstText);
    }
  } catch (err) {
    setAppError(String(err));
    console.error('Failed to load channels:', err);
  }
}

export function handleChannelCreated(channel: Channel) {
  setChannels((prev) =>
    prev.some((c) => c.id === channel.id)
      ? prev
      : [...prev, channel].sort((a, b) => a.position - b.position)
  );
}

export function handleChannelUpdated(channel: Channel) {
  setChannels((prev) => prev.map((c) => (c.id === channel.id ? channel : c)).sort((a, b) => a.position - b.position));
  if (activeChannel()?.id === channel.id) setActiveChannel(channel);
}

/** A channel was deleted: drop it, and leave it if it was open. */
export function handleChannelDeleted(channelId: string) {
  setChannels((prev) => prev.filter((c) => c.id !== channelId));
  if (activeChannel()?.id === channelId) {
    setMessages([]);
    receivedMessageIds.clear();
    const next = channels().find((c) => c.channel_type === 'text') ?? null;
    if (next) void selectChannel(next);
    else setActiveChannel(null);
  }
}

function failedFromOutbox(entry: OutboxEntry): Message {
  const user = currentUser();
  return {
    id: `local-${entry.client_msg_id}`,
    channel_id: entry.channel_id,
    author_id: user?.id || 'me',
    author_name: user?.display_name || user?.username || 'You',
    content: entry.content,
    attachments: [],
    timestamp: entry.timestamp,
    client_msg_id: entry.client_msg_id,
    state: 'failed',
    failure: 'Not sent',
  };
}

export async function selectChannel(channel: Channel) {
  const request = ++historyRequest;
  if (activeChannel()?.id !== channel.id) {
    setMessages([]);
    receivedMessageIds.clear();
    setHasMoreHistory(false);
    setReplyingTo(null);
  }
  setActiveChannel(channel);
  const isCurrent = () => request === historyRequest && activeChannel()?.id === channel.id;
  if (channel.channel_type === 'text') {
    setChatLoading(true);
    try {
      const history = await api.fetchChannelMessages(channel.id, undefined, HISTORY_PAGE);
      if (!isCurrent()) return;
      const historyMessages = history.messages.map((message) => ({ ...message, channel_id: channel.id }));
      const historyIds = new Set(historyMessages.map((message) => message.id));
      // Pending messages the server already stored (e.g. sent just before a reconnect).
      const stored = new Set(historyMessages.flatMap((message) => message.client_msg_id ? [`local-${message.client_msg_id}`] : []));
      for (const message of historyMessages) if (message.client_msg_id) removeFromOutbox(message.client_msg_id);
      const outbox = outboxForChannel(channel.id).map(failedFromOutbox);
      setHasMoreHistory(history.has_more);
      setMessages((previous) => {
        const keep = previous.filter((message) => !historyIds.has(message.id) && !stored.has(message.id));
        const keptIds = new Set(keep.map((message) => message.id));
        return chronological([...historyMessages, ...keep, ...outbox.filter((message) => !keptIds.has(message.id) && !stored.has(message.id))]);
      });
    } catch (err) {
      if (!isCurrent()) return;
      setAppError(String(err));
      console.error('Failed to load messages for channel:', err);
    } finally {
      if (isCurrent()) setChatLoading(false);
    }
  } else {
    setChatLoading(false);
  }
}

/** Load the page of messages older than the oldest one shown (scrolling up). */
export async function loadOlder() {
  const channel = activeChannel();
  if (!channel || loadingOlder() || !hasMoreHistory()) return;
  const oldest = messages().find((message) => !isLocal(message.id));
  if (!oldest) return;
  setLoadingOlder(true);
  const request = historyRequest;
  try {
    const page = await api.fetchChannelMessages(channel.id, undefined, HISTORY_PAGE, oldest.id);
    if (request !== historyRequest || activeChannel()?.id !== channel.id) return;
    const older = page.messages.map((message) => ({ ...message, channel_id: channel.id }));
    setHasMoreHistory(page.has_more);
    setMessages((previous) => {
      const known = new Set(previous.map((message) => message.id));
      return chronological([...older.filter((message) => !known.has(message.id)), ...previous]);
    });
  } catch (err) {
    setAppError(String(err));
  } finally {
    setLoadingOlder(false);
  }
}

function markFailed(pendingId: string, reason: string) {
  setMessages((prev) => prev.map((message) => (message.id === pendingId ? { ...message, state: 'failed' as const, failure: reason } : message)));
}

async function deliver(pending: Message, attachmentIds: string[], replyToId: string | undefined) {
  const clientMsgId = pending.client_msg_id!;
  saveToOutbox({
    client_msg_id: clientMsgId, channel_id: pending.channel_id, content: pending.content,
    attachments: attachmentIds, reply_to: replyToId ?? null, timestamp: pending.timestamp,
  });
  try {
    await api.sendTextMessage(pending.channel_id, pending.content, attachmentIds, clientMsgId, replyToId);
  } catch (err) {
    markFailed(pending.id, String(err));
    setAppError(`Message not sent: ${String(err)}`);
    console.error('Failed to send text message:', err);
  }
}

export async function sendMessage(content: string, attachments?: string[]) {
  const current = activeChannel();
  const text = content.trim();
  if (!current || !text) return;
  const user = currentUser();
  const replied = replyingTo();
  // The server echoes this id in the stored message and in a rejection, so the pending message is
  // matched by id and never by its text.
  const clientMsgId = crypto.randomUUID();
  const optimisticMsg: Message = {
    id: `local-${clientMsgId}`,
    channel_id: current.id,
    author_id: user?.id || 'me',
    author_name: user?.display_name || user?.username || 'You',
    content: text,
    attachments: [],
    timestamp: Math.floor(Date.now() / 1000),
    client_msg_id: clientMsgId,
    state: 'sending',
    reply_to: replied ? { message_id: replied.id, author_name: replied.author_name, snippet: replied.content.slice(0, 120) } : null,
  };
  setReplyingTo(null);
  setMessages((prev) => [...prev, optimisticMsg]);
  await deliver(optimisticMsg, attachments ?? [], replied?.id);
}

/** Send a message that failed once more, with its original id. */
export async function retryMessage(id: string) {
  const message = messages().find((candidate) => candidate.id === id && candidate.state === 'failed');
  if (!message?.client_msg_id) return;
  const saved = outboxForChannel(message.channel_id).find((entry) => entry.client_msg_id === message.client_msg_id);
  setMessages((prev) => prev.map((candidate) => (candidate.id === id ? { ...candidate, state: 'sending' as const, failure: undefined } : candidate)));
  await deliver({ ...message, state: 'sending' }, saved?.attachments ?? [], saved?.reply_to ?? message.reply_to?.message_id);
}

/** Drop a message that was never sent. */
export function discardFailed(id: string) {
  const message = messages().find((candidate) => candidate.id === id);
  if (message?.client_msg_id) removeFromOutbox(message.client_msg_id);
  setMessages((prev) => prev.filter((candidate) => candidate.id !== id));
}

/** The server refused a message we sent: keep exactly that one, marked as not sent, and say why. */
export function handleMessageRejected(rejection: { client_msg_id: string; code: number; message: string }) {
  const pendingId = `local-${rejection.client_msg_id}`;
  if (messages().some((message) => message.id === pendingId)) markFailed(pendingId, rejection.message);
  setAppError(`Message not sent: ${rejection.message}`);
}

export function handleIncomingMessage(incoming: IncomingMessage | Message) {
  const msg: Message = 'message_id' in incoming
    ? (({ message_id, ...rest }) => ({ ...rest, id: message_id }))(incoming)
    : incoming;
  if (msg.client_msg_id) removeFromOutbox(msg.client_msg_id);
  const current = activeChannel();
  if (current && msg.channel_id === current.id) {
    if (receivedMessageIds.has(msg.id)) return;
    receivedMessageIds.add(msg.id);
    setMessages((prev) => {
      // By id when the server echoed one; by author and text only for servers that do not.
      const pendingIndex = msg.client_msg_id
        ? prev.findIndex((message) => message.id === `local-${msg.client_msg_id}`)
        : prev.findIndex((message) => isLocal(message.id) && message.state !== 'failed' &&
            message.author_id === msg.author_id && message.content === msg.content);
      const filtered = prev.filter((_, index) => index !== pendingIndex);
      return filtered.some((message) => message.id === msg.id) ? filtered : chronological([...filtered, msg]);
    });
  }
}

// --- editing, deleting, reacting ---------------------------------------------------------------

const updateMessage = (id: string, change: (message: Message) => Message) =>
  setMessages((previous) => previous.map((message) => (message.id === id ? change(message) : message)));

export function handleMessageEdited(edit: { message_id: string; content: string; edited_at: number }) {
  updateMessage(edit.message_id, (message) => ({ ...message, content: edit.content, edited_at: edit.edited_at }));
  // A quote of the edited message follows it.
  setMessages((previous) => previous.map((message) =>
    message.reply_to?.message_id === edit.message_id && !message.reply_to.deleted
      ? { ...message, reply_to: { ...message.reply_to, snippet: edit.content.replace(/\s+/g, ' ').slice(0, 120) } }
      : message));
}

export function handleMessageDeleted(deleted: { message_id: string }) {
  setMessages((previous) => previous
    .filter((message) => message.id !== deleted.message_id)
    .map((message) => message.reply_to?.message_id === deleted.message_id
      ? { ...message, reply_to: { ...message.reply_to, author_name: '', snippet: '', deleted: true } }
      : message));
  receivedMessageIds.delete(deleted.message_id);
  if (replyingTo()?.id === deleted.message_id) setReplyingTo(null);
}

export function handleReactionChanged(change: { message_id: string; user_id: string; emoji: string; added: boolean }) {
  updateMessage(change.message_id, (message) => {
    const reactions = (message.reactions ?? []).map((r) => ({ ...r, user_ids: [...r.user_ids] }));
    const entry = reactions.find((r) => r.emoji === change.emoji);
    if (change.added) {
      if (!entry) reactions.push({ emoji: change.emoji, user_ids: [change.user_id] });
      else if (!entry.user_ids.includes(change.user_id)) entry.user_ids.push(change.user_id);
    } else if (entry) {
      entry.user_ids = entry.user_ids.filter((id) => id !== change.user_id);
    }
    return { ...message, reactions: reactions.filter((r) => r.user_ids.length > 0) };
  });
}

/** Show someone's new display name on the messages already on screen. */
export function renameAuthor(userId: string, name: string) {
  setMessages((previous) => previous.map((message) => (message.author_id === userId ? { ...message, author_name: name } : message)));
}

export async function editMessage(id: string, content: string) {
  const text = content.trim();
  if (!text) return;
  try {
    await api.editMessage(id, text);
  } catch (err) {
    setAppError(String(err));
  }
}

export async function deleteMessage(id: string) {
  try {
    await api.deleteMessage(id);
  } catch (err) {
    setAppError(String(err));
  }
}

/** Add your reaction, or take it back if you already reacted with this emoji. */
export async function toggleReaction(message: Message, emoji: string) {
  const me = currentUser()?.id;
  if (!me) return;
  const mine = message.reactions?.find((r) => r.emoji === emoji)?.user_ids.includes(me) ?? false;
  try {
    await api.setReaction(message.id, emoji, !mine);
  } catch (err) {
    setAppError(String(err));
  }
}

/** Open the channel of a search result and load history until the message is there. */
export async function revealMessage(channelId: string, messageId: string, maxPages = 20): Promise<boolean> {
  const channel = channels().find((candidate) => candidate.id === channelId);
  if (!channel) return false;
  if (activeChannel()?.id !== channelId || messages().length === 0) await selectChannel(channel);
  for (let page = 0; page < maxPages && !messages().some((message) => message.id === messageId); page++) {
    if (!hasMoreHistory()) break;
    await loadOlder();
  }
  const found = messages().some((message) => message.id === messageId);
  if (found) setRevealTarget(messageId);
  return found;
}
