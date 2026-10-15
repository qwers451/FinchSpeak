import { api } from '../services/api';
import type { Channel } from '../types';
import { channels } from './chatStore';
import { setAppError } from './notificationStore';

export async function renameChannel(channel: Channel, name: string) {
  const trimmed = name.trim();
  if (!trimmed || trimmed === channel.name) return;
  try {
    await api.updateChannel(channel.id, trimmed);
  } catch (err) {
    setAppError(String(err));
  }
}

export async function removeChannel(channel: Channel) {
  try {
    await api.deleteChannel(channel.id);
  } catch (err) {
    setAppError(String(err));
  }
}

/** Move a channel one place up or down among channels of its own kind. */
export async function moveChannel(channel: Channel, direction: -1 | 1) {
  const all = [...channels()].sort((a, b) => a.position - b.position);
  const sameKind = all.filter((c) => c.channel_type === channel.channel_type);
  const index = sameKind.findIndex((c) => c.id === channel.id);
  const neighbour = sameKind[index + direction];
  if (index < 0 || !neighbour) return;
  const order = all.map((c) => c.id);
  const [a, b] = [order.indexOf(channel.id), order.indexOf(neighbour.id)];
  [order[a], order[b]] = [order[b], order[a]];
  try {
    await api.reorderChannels(order);
  } catch (err) {
    setAppError(String(err));
  }
}
