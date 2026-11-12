import { render } from 'solid-js/web';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { api } from '../../services/api';
import { setChannels } from '../../stores/chatStore';
import { dismissCall, FLOCK_CALL_TTL_MS, pendingCalls, receiveFlockCall } from '../../stores/flockStore';
import { setActiveVoiceChannel } from '../../stores/voiceStore';
import { FlockCallCards } from './FlockCallCard';

const lounge = { id: 'lounge', short_id: 2, name: 'Lounge', channel_type: 'voice' as const, position: 1, bitrate_kbps: 64 };
let dispose: (() => void) | undefined;

beforeEach(() => {
  vi.useFakeTimers();
  setChannels([lounge]);
  setActiveVoiceChannel(null);
  vi.spyOn(api, 'joinVoiceChannel').mockResolvedValue();
  const root = document.createElement('div');
  document.body.append(root);
  dispose = render(() => <FlockCallCards />, root);
});

afterEach(() => {
  dispose?.();
  for (const pending of pendingCalls()) dismissCall(pending.id);
  document.body.innerHTML = '';
  vi.useRealTimers();
  vi.restoreAllMocks();
});

const button = (label: string) => [...document.querySelectorAll('button')].find((item) => item.textContent?.trim() === label);

it('shows who calls me to which room, with Fly and Stay', () => {
  receiveFlockCall({ from_user_id: 'alice', from_name: 'Alice', channel_id: 'lounge' });
  expect(document.querySelector('.flock-text')?.textContent).toBe('Alice is calling you to Lounge');
  expect(button('Fly')).toBeDefined();
  expect(button('Stay')).toBeDefined();
});

it('Fly joins the room', async () => {
  receiveFlockCall({ from_user_id: 'alice', from_name: 'Alice', channel_id: 'lounge' });
  button('Fly')?.click();
  await vi.waitFor(() => expect(api.joinVoiceChannel).toHaveBeenCalledWith('lounge', 2));
  expect(document.querySelector('.flock-card')).toBeNull();
});

it('Stay closes the card without joining', () => {
  receiveFlockCall({ from_user_id: 'alice', from_name: 'Alice', channel_id: 'lounge' });
  button('Stay')?.click();
  expect(document.querySelector('.flock-card')).toBeNull();
  expect(api.joinVoiceChannel).not.toHaveBeenCalled();
});

it('the card disappears by itself after 30 seconds', () => {
  receiveFlockCall({ from_user_id: 'alice', from_name: 'Alice', channel_id: 'lounge' });
  vi.advanceTimersByTime(FLOCK_CALL_TTL_MS);
  expect(document.querySelector('.flock-card')).toBeNull();
});
