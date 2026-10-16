import { createSignal } from 'solid-js';
import type { Channel, Message, IncomingMessage } from '../types';
import { api } from '../services/api';
import { currentUser } from './authStore';
import { setAppError } from './notificationStore';

export const [channels, setChannels] = createSignal<Channel[]>([]);
export const [activeChannel, setActiveChannel] = createSignal<Channel | null>(null);
export const [messages, setMessages] = createSignal<Message[]>([]);
export const [chatLoading, setChatLoading] = createSignal<boolean>(false);
let historyRequest = 0;
const receivedMessageIds = new Set<string>();

function chronological(items: Message[]) {
  return items.sort((a, b) => a.timestamp - b.timestamp ||
    (a.id.startsWith('local-') || b.id.startsWith('local-') ? 0 : a.id.localeCompare(b.id)));
}

export function resetChatState() {
  historyRequest++;
  receivedMessageIds.clear();
  setActiveChannel(null);
  setChannels([]);
  setMessages([]);
  setChatLoading(false);
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

export async function selectChannel(channel: Channel) {
  const request = ++historyRequest;
  if (activeChannel()?.id !== channel.id) {
    setMessages([]);
    receivedMessageIds.clear();
  }
  setActiveChannel(channel);
  const isCurrent = () => request === historyRequest && activeChannel()?.id === channel.id;
  if (channel.channel_type === 'text') {
    setChatLoading(true);
    try {
      const history = await api.fetchChannelMessages(channel.id, undefined, 50);
      if (!isCurrent()) return;
      const historyMessages = history.messages.map((message) => ({ ...message, channel_id: channel.id }));
      const historyIds = new Set(historyMessages.map((message) => message.id));
      // Pending messages the server already stored (e.g. sent just before a reconnect).
      const stored = new Set(historyMessages.flatMap((message) => message.client_msg_id ? [`local-${message.client_msg_id}`] : []));
      setMessages((previous) => chronological([...historyMessages,
        ...previous.filter((message) => !historyIds.has(message.id) && !stored.has(message.id))]));
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

export async function sendMessage(content: string, attachments?: string[]) {
  const current = activeChannel();
  if (!current || !content.trim()) return;
  const user = currentUser();
  // The server echoes this id in the stored message and in a rejection, so the pending message is
  // matched by id and never by its text.
  const clientMsgId = crypto.randomUUID();
  const optimisticMsg: Message = {
    id: `local-${clientMsgId}`,
    channel_id: current.id,
    author_id: user?.id || 'me',
    author_name: user?.display_name || user?.username || 'You',
    content: content.trim(),
    attachments: [],
    timestamp: Math.floor(Date.now() / 1000),
  };
  setMessages((prev) => [...prev, optimisticMsg]);
  try {
    await api.sendTextMessage(current.id, content.trim(), attachments, clientMsgId);
  } catch (err) {
    setMessages((prev) => prev.filter((message) => message.id !== optimisticMsg.id));
    setAppError(String(err));
    console.error('Failed to send text message:', err);
  }
}

/** The server refused a message we sent: roll back exactly that one and say why. */
export function handleMessageRejected(rejection: { client_msg_id: string; code: number; message: string }) {
  const pendingId = `local-${rejection.client_msg_id}`;
  setMessages((prev) => prev.filter((message) => message.id !== pendingId));
  setAppError(`Message not sent: ${rejection.message}`);
}

export function handleIncomingMessage(incoming: IncomingMessage | Message) {
  const msg: Message = 'message_id' in incoming
    ? (({ message_id, ...rest }) => ({ ...rest, id: message_id }))(incoming)
    : incoming;
  const current = activeChannel();
  if (current && msg.channel_id === current.id) {
    if (receivedMessageIds.has(msg.id)) return;
    receivedMessageIds.add(msg.id);
    setMessages((prev) => {
      // By id when the server echoed one; by author and text only for servers that do not.
      const pendingIndex = msg.client_msg_id
        ? prev.findIndex((message) => message.id === `local-${msg.client_msg_id}`)
        : prev.findIndex((message) => message.id.startsWith('local-') &&
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
}

export function handleMessageDeleted(deleted: { message_id: string }) {
  setMessages((previous) => previous.filter((message) => message.id !== deleted.message_id));
  receivedMessageIds.delete(deleted.message_id);
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
