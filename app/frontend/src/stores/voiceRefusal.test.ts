import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { api } from '../services/api';
import { activeVoiceChannel, joinVoice, setActiveVoiceChannel } from './voiceStore';
import { appError, setAppError } from './notificationStore';
import { sessionHandlers } from './sessionStore';
import { startSharing } from './screenStore';
import { isSharing, resetScreenState } from './screenState';

beforeEach(() => {
  localStorage.clear();
  setActiveVoiceChannel(null);
  setAppError(null);
  resetScreenState();
});

afterEach(() => vi.restoreAllMocks());

const roomA = { id: 'a', short_id: 1, name: 'A', channel_type: 'voice', position: 0, bitrate_kbps: 64 } as const;
const roomB = { id: 'b', short_id: 2, name: 'B', channel_type: 'voice', position: 1, bitrate_kbps: 64 } as const;
const joinRefused = { code: 429, message: 'Wait 3 s; too many room joins' };
const shareRefused = { code: 429, message: 'Wait 3 s; too many screen shares' };

it('puts back the room we were in when a join is refused right after it, without telling the server', async () => {
  vi.spyOn(api, 'joinVoiceChannel').mockResolvedValue(undefined);
  const leave = vi.spyOn(api, 'leaveVoiceChannel').mockResolvedValue(undefined);
  await joinVoice(roomA);
  await joinVoice(roomB);
  expect(activeVoiceChannel()?.id).toBe('b');

  sessionHandlers['server-error'](joinRefused);

  expect(activeVoiceChannel()?.id).toBe('a');
  expect(leave).not.toHaveBeenCalled();
  expect(appError()).toBe('Too many room joins. Wait 3 s');
});

it('clears the room when a first join is refused', async () => {
  vi.spyOn(api, 'joinVoiceChannel').mockResolvedValue(undefined);
  const leave = vi.spyOn(api, 'leaveVoiceChannel').mockResolvedValue(undefined);
  await joinVoice(roomA);

  sessionHandlers['server-error'](joinRefused);

  expect(activeVoiceChannel()).toBeNull();
  expect(leave).not.toHaveBeenCalled();
});

it('keeps the join when the refusal does not come right after it', async () => {
  vi.spyOn(api, 'joinVoiceChannel').mockResolvedValue(undefined);
  await joinVoice(roomA);
  vi.spyOn(Date, 'now').mockReturnValue(Date.now() + 60_000);

  sessionHandlers['server-error'](joinRefused);

  expect(activeVoiceChannel()?.id).toBe('a');
});

it('a refused join does not stop a share we have running', async () => {
  vi.spyOn(api, 'joinVoiceChannel').mockResolvedValue(undefined);
  vi.spyOn(api, 'startScreenShare').mockResolvedValue(1);
  const stop = vi.spyOn(api, 'stopScreenShare').mockResolvedValue(undefined);
  await joinVoice(roomA);
  expect(await startSharing('monitor', 1)).toBeNull();

  sessionHandlers['server-error'](joinRefused);

  expect(stop).not.toHaveBeenCalled();
  expect(isSharing()).toBe(true);
});

it('a refused share stops only the share and keeps us in the room', async () => {
  vi.spyOn(api, 'joinVoiceChannel').mockResolvedValue(undefined);
  vi.spyOn(api, 'startScreenShare').mockResolvedValue(1);
  const stop = vi.spyOn(api, 'stopScreenShare').mockResolvedValue(undefined);
  const leave = vi.spyOn(api, 'leaveVoiceChannel').mockResolvedValue(undefined);
  await joinVoice(roomA);
  expect(await startSharing('monitor', 1)).toBeNull();
  expect(isSharing()).toBe(true);

  sessionHandlers['server-error'](shareRefused);

  expect(isSharing()).toBe(false);
  expect(stop).toHaveBeenCalled();
  expect(leave).not.toHaveBeenCalled();
  expect(activeVoiceChannel()?.id).toBe('a');
  expect(appError()).toBe('Too many screen shares. Wait 3 s');
});

it('leaves other server errors to the existing handling', () => {
  sessionHandlers['server-error']({ code: 400, message: 'Join a voice room first' });
  expect(appError()).toBe('400: Join a voice room first');
});
