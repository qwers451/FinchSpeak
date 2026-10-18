import { render } from 'solid-js/web';
import { afterEach, expect, it } from 'vitest';
import { setActiveChannel, setChannels } from '../stores/chatStore';
import { setCurrentUser } from '../stores/authStore';
import { setKnownUsers } from '../stores/sessionStore';
import { setActiveVoiceChannel, setVoiceParticipants } from '../stores/voiceStore';
import type { Channel, User } from '../types';
import { Sidebar } from './Sidebar';

const voice: Channel = { id: 'voice', short_id: 1, name: 'voice', channel_type: 'voice', position: 0, bitrate_kbps: 64 };
const svgAvatar = (fill: string) => `data:image/svg+xml,${encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"><rect width="1" height="1" fill="${fill}"/></svg>`)}`;
const member: User = {
  id: 'participant', username: 'bob', display_name: 'Bob', role: 'member',
  avatar_url: svgAvatar('red'),
};
let dispose: (() => void) | undefined;

afterEach(() => {
  dispose?.();
  dispose = undefined;
  document.body.innerHTML = '';
  setCurrentUser(null);
  setKnownUsers([]);
  setChannels([]);
  setActiveChannel(null);
  setActiveVoiceChannel(null);
  setVoiceParticipants([]);
});

it('updates a voice participant picture when knownUsers changes and falls back to the initial when removed', async () => {
  setKnownUsers([member]);
  setChannels([voice]);
  setActiveVoiceChannel(voice);
  setVoiceParticipants([{ user_id: member.id, peer_id: 1, username: member.username, channel_id: voice.id }]);
  const host = document.createElement('div');
  document.body.appendChild(host);
  dispose = render(() => <Sidebar />, host);
  const flush = () => new Promise((resolve) => setTimeout(resolve, 0));
  const avatar = host.querySelector('.vp .av-art') as HTMLElement;
  await flush();
  expect(avatar.style.backgroundImage).toContain(encodeURIComponent('fill="red"'));

  setKnownUsers([{ ...member, avatar_url: svgAvatar('blue') }]);
  await flush();
  expect(host.querySelector('.vp .av-art')).toBe(avatar);
  expect(avatar.style.backgroundImage).toContain(encodeURIComponent('fill="blue"'));

  setKnownUsers([{ ...member, avatar_url: null }]);
  await flush();
  expect(host.querySelector('.vp .av-art')).toBe(avatar);
  expect(avatar.style.backgroundImage).toBe('');
  expect(avatar.textContent).toBe('B');
});
