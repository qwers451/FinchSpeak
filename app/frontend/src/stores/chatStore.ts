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
      setMessages((previous) => chronological([...historyMessages, ...previous.filter((message) => !historyIds.has(message.id))]));
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
  const optimisticMsg: Message = {
    id: `local-${crypto.randomUUID()}`,
    channel_id: current.id,
    author_id: user?.id || 'me',
    author_name: user?.display_name || user?.username || 'You',
    content: content.trim(),
    attachments: [],
    timestamp: Math.floor(Date.now() / 1000),
  };
  setMessages((prev) => [...prev, optimisticMsg]);
  try {
    await api.sendTextMessage(current.id, content.trim(), attachments);
  } catch (err) {
    setMessages((prev) => prev.filter((message) => message.id !== optimisticMsg.id));
    setAppError(String(err));
    console.error('Failed to send text message:', err);
  }
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
      const pendingIndex = prev.findIndex((message) => message.id.startsWith('local-') &&
        message.author_id === msg.author_id && message.content === msg.content);
      const filtered = prev.filter((_, index) => index !== pendingIndex);
      return filtered.some((message) => message.id === msg.id) ? filtered : chronological([...filtered, msg]);
    });
  }
}
