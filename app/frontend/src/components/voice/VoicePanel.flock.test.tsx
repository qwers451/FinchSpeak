import { render } from 'solid-js/web';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => undefined), Channel: class { onmessage: unknown = null; } }));

import { api } from '../../services/api';
import { setChannels } from '../../stores/chatStore';
import { setKnownUsers, setPresence, setServerFeatures } from '../../stores/sessionStore';
import { setCurrentUser } from '../../stores/authStore';
import { openParticipantMenu } from './participantMenu';
import { setActiveVoiceChannel, setVoiceParticipants } from '../../stores/voiceStore';
import { setPrefs, DEFAULT_PREFS } from '../../stores/settingsStore';
import { ContextMenuHost, closeContextMenu } from '../../ui/ContextMenu';
import { VoicePanel } from '../VoicePanel';

const room = { id: 'home', short_id: 1, name: 'Home', channel_type: 'voice' as const, position: 0, bitrate_kbps: 64 };
const lounge = { id: 'lounge', short_id: 2, name: 'Lounge', channel_type: 'voice' as const, position: 1, bitrate_kbps: 64 };
let dispose: (() => void) | undefined;

beforeEach(() => {
  setPrefs({ ...DEFAULT_PREFS, appearance: { ...DEFAULT_PREFS.appearance, language: 'en' } });
  setChannels([room, lounge]);
  setActiveVoiceChannel(room);
  setVoiceParticipants([]);
  vi.spyOn(api, 'joinVoiceChannel').mockResolvedValue();
  vi.spyOn(api, 'callFlock').mockResolvedValue();
  const host = document.createElement('div');
  document.body.append(host);
  dispose = render(() => <><VoicePanel /><ContextMenuHost /></>, host);
});

afterEach(() => {
  closeContextMenu();
  dispose?.();
  dispose = undefined;
  setServerFeatures([]);
  document.body.innerHTML = '';
  vi.restoreAllMocks();
});

const button = (label: string) => [...document.querySelectorAll<HTMLButtonElement>('button')].find((item) => item.textContent?.trim() === label);

it('shows the call button only when the server has the flock feature', () => {
  setServerFeatures([]);
  expect(button('Call everyone along')).toBeUndefined();
  setServerFeatures(['flock']);
  expect(button('Call everyone along')).toBeDefined();
});

it('the button calls my room to the room picked and flies me there', async () => {
  setServerFeatures(['flock']);
  button('Call everyone along')?.click();
  await vi.waitFor(() => expect(button('Lounge')).toBeDefined());
  expect(button('Home')).toBeUndefined();
  button('Lounge')?.click();
  await vi.waitFor(() => expect(api.joinVoiceChannel).toHaveBeenCalledWith('lounge', 2));
  expect(api.callFlock).toHaveBeenCalledWith('lounge');
});

const bob = { id: 'bob', username: 'bob', display_name: 'Bob', role: 'member' as const, status: 'online' as const };
const menuButton = (label: string) => [...document.querySelectorAll<HTMLButtonElement>('button')].find((item) => item.textContent?.trim() === label);

it('"Call here" is offered for someone online who is not in my room', async () => {
  setCurrentUser({ id: 'me', username: 'me', display_name: 'Me', role: 'member' });
  setKnownUsers([{ id: 'me', username: 'me', display_name: 'Me', role: 'member' }, bob]);
  setPresence({ bob: 'online' });
  setServerFeatures(['flock']);
  setVoiceParticipants([{ user_id: 'bob', peer_id: 2, username: 'bob', channel_id: 'lounge' }]);
  openParticipantMenu({ x: 0, y: 0 }, 'bob');
  await vi.waitFor(() => expect(menuButton('Call here')).toBeDefined());
  closeContextMenu();
});

it('"Call here" is not offered in my room or to someone offline', async () => {
  setCurrentUser({ id: 'me', username: 'me', display_name: 'Me', role: 'member' });
  setKnownUsers([{ id: 'me', username: 'me', display_name: 'Me', role: 'member' }, bob]);
  setServerFeatures(['flock']);
  setPresence({ bob: 'online' });
  setVoiceParticipants([{ user_id: 'bob', peer_id: 2, username: 'bob', channel_id: 'home' }]);
  openParticipantMenu({ x: 0, y: 0 }, 'bob');
  await vi.waitFor(() => expect(menuButton('Mute for me')).toBeDefined());
  expect(menuButton('Call here')).toBeUndefined();
  closeContextMenu();

  setPresence({ bob: 'offline' });
  setVoiceParticipants([]);
  openParticipantMenu({ x: 0, y: 0 }, 'bob');
  await vi.waitFor(() => expect(menuButton('Mute for me')).toBeDefined());
  expect(menuButton('Call here')).toBeUndefined();
});
