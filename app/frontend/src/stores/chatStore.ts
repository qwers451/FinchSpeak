import { createSignal } from 'solid-js';
import type { Channel, Message } from '../types';
import { api } from '../services/api';

export const [channels, setChannels] = createSignal<Channel[]>([]);
export const [activeChannel, setActiveChannel] = createSignal<Channel | null>(null);
export const [messages, setMessages] = createSignal<Message[]>([]);
export const [chatLoading, setChatLoading] = createSignal<boolean>(false);

export async function loadChannels() {
  try {
    const list = await api.listChannels();
    setChannels(list);
    if (!activeChannel() && list.length > 0) {
      const firstText = list.find((c) => c.channel_type === 'text') || list[0];
      selectChannel(firstText);
    }
  } catch (err) {
    console.error('Failed to load channels:', err);
  }
}

export async function selectChannel(channel: Channel) {
  setActiveChannel(channel);
  if (channel.channel_type === 'text') {
    setChatLoading(true);
    try {
      const history = await api.fetchChannelMessages(channel.id, undefined, 50);
      // Backend returns messages in DESC order; reverse to ASC for chronological chat
      setMessages([...history.messages].reverse());
    } catch (err) {
      console.error('Failed to load messages for channel:', err);
      setMessages([]);
    } finally {
      setChatLoading(false);
    }
  }
}

export async function sendMessage(content: string, attachments?: string[]) {
  const current = activeChannel();
  if (!current || !content.trim()) return;
  try {
    await api.sendTextMessage(current.id, content.trim(), attachments);
  } catch (err) {
    console.error('Failed to send text message:', err);
  }
}

export function handleIncomingMessage(msg: Message) {
  const current = activeChannel();
  if (current && msg.channel_id === current.id) {
    setMessages((prev) => [...prev, msg]);
  }
}
