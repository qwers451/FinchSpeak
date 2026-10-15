import { beforeEach, expect, it, vi } from 'vitest';
import { api } from '../services/api';
import { appError, setAppError } from './notificationStore';
import { activeChannel, channels, handleChannelDeleted, handleChannelUpdated, setActiveChannel, setChannels } from './chatStore';
import { moveChannel, removeChannel, renameChannel } from './channelAdminStore';
import { sessionHandlers } from './sessionStore';
import { activeVoiceChannel, setActiveVoiceChannel } from './voiceStore';
import type { Channel } from '../types';

const channel = (id: string, type: 'text' | 'voice', position: number): Channel =>
  ({ id, short_id: position, name: id, channel_type: type, position, topic: null, bitrate_kbps: 64 }) as unknown as Channel;

beforeEach(() => {
  vi.restoreAllMocks();
  setAppError(null);
  setChannels([channel('t1', 'text', 1), channel('v1', 'voice', 2), channel('t2', 'text', 3), channel('t3', 'text', 4)]);
  setActiveChannel(null);
  setActiveVoiceChannel(null);
});

it('AUD-045 renaming ignores empty or unchanged names and shows refusals', async () => {
  const update = vi.spyOn(api, 'updateChannel').mockResolvedValue();
  await renameChannel(channels()[0], '   ');
  await renameChannel(channels()[0], 't1');
  expect(update).not.toHaveBeenCalled();
  await renameChannel(channels()[0], ' Lobby ');
  expect(update).toHaveBeenCalledWith('t1', 'Lobby');

  update.mockRejectedValue('HTTP 403: Permission denied');
  await renameChannel(channels()[0], 'again');
  expect(appError()).toContain('403');
});

it('AUD-045 moving swaps with the neighbour of the same kind and sends the whole order', async () => {
  const reorder = vi.spyOn(api, 'reorderChannels').mockResolvedValue();
  await moveChannel(channels()[2], -1); // t2 up: swaps with t1 (skipping the voice channel)
  expect(reorder).toHaveBeenLastCalledWith(['t2', 'v1', 't1', 't3']);
  await moveChannel(channels()[0], -1); // already first of its kind
  await moveChannel(channels()[3], 1); // already last of its kind
  expect(reorder).toHaveBeenCalledTimes(1);
});

it('AUD-045 an update from the server renames and reorders the list', () => {
  setActiveChannel(channels()[0]);
  handleChannelUpdated({ ...channels()[0], name: 'lobby', position: 9 });
  expect(channels().map((c) => c.id)).toEqual(['v1', 't2', 't3', 't1']);
  expect(activeChannel()?.name).toBe('lobby');
});

it('AUD-045 deleting the open channel moves to another text channel', () => {
  vi.spyOn(api, 'fetchChannelMessages').mockResolvedValue({ channel_id: 't2', messages: [], has_more: false } as never);
  setActiveChannel(channels()[0]);
  handleChannelDeleted('t1');
  expect(channels().some((c) => c.id === 't1')).toBe(false);
  expect(activeChannel()?.id).toBe('t2');
});

it('AUD-045 deleting the voice channel we are in leaves it', async () => {
  const leave = vi.spyOn(api, 'leaveVoiceChannel').mockResolvedValue();
  setActiveVoiceChannel(channels()[1]);
  sessionHandlers['channel-deleted']({ channel_id: 'v1' });
  expect(channels().some((c) => c.id === 'v1')).toBe(false);
  expect(leave).toHaveBeenCalled();
  await vi.waitFor(() => expect(activeVoiceChannel()).toBeNull());
});

it('AUD-045 a refused delete is shown', async () => {
  vi.spyOn(api, 'deleteChannel').mockRejectedValue('HTTP 400: The last text channel cannot be deleted');
  await removeChannel(channels()[0]);
  expect(appError()).toContain('last text channel');
});
