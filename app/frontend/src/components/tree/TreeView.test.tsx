import { render } from 'solid-js/web';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { setActiveChannel, setChannels } from '../../stores/chatStore';
import { setCurrentUser } from '../../stores/authStore';
import { setKnownUsers, setPresence } from '../../stores/sessionStore';
import { setActiveVoiceChannel, setVoiceParticipants } from '../../stores/voiceStore';
import { setUnread } from '../../stores/activityStore';
import { nestView, setNestView } from '../../stores/nestView';
import type { Channel, User } from '../../types';
import { TreeView } from './TreeView';

const nav = vi.hoisted(() => ({ selectChannel: vi.fn(async () => {}), joinVoice: vi.fn(async () => {}), leaveVoice: vi.fn(async () => {}) }));
vi.mock('../../stores/chatStore', async (importOriginal) => ({ ...(await importOriginal<typeof import('../../stores/chatStore')>()), selectChannel: nav.selectChannel }));
vi.mock('../../stores/voiceStore', async (importOriginal) => ({ ...(await importOriginal<typeof import('../../stores/voiceStore')>()), joinVoice: nav.joinVoice, leaveVoice: nav.leaveVoice }));
// The opened room is covered by its own tests; here only the tree around it matters.
vi.mock('../ChatArea', () => ({ ChatArea: () => <main class="main chat" /> }));
vi.mock('../VoicePanel', () => ({ VoicePanel: () => <main class="main voice-room" /> }));

const channel = (id: string, type: 'voice' | 'text', position: number): Channel => ({ id, short_id: position + 1, name: id, channel_type: type, position, bitrate_kbps: 64 });
const lounge = channel('lounge', 'voice', 0);
const attic = channel('attic', 'voice', 1);
const general = channel('general', 'text', 2);
const user = (id: string, role: User['role'] = 'member'): User => ({ id, username: id, display_name: id, role, avatar_url: null });

let host: HTMLElement;
let dispose: (() => void) | undefined;
const mount = () => { dispose = render(() => <TreeView />, host); };

beforeEach(() => {
  Object.values(nav).forEach((mock) => mock.mockClear());
  host = document.createElement('div');
  document.body.appendChild(host);
  setNestView(true);
  setChannels([lounge, attic, general]);
  setKnownUsers([user('me'), user('ann'), user('bob'), user('zed')]);
  setPresence({ me: 'online', ann: 'online', bob: 'idle', zed: 'offline' });
  setCurrentUser(user('me'));
  setVoiceParticipants([{ user_id: 'ann', peer_id: 7, username: 'ann', channel_id: 'lounge' }]);
});

afterEach(() => {
  dispose?.();
  dispose = undefined;
  document.body.innerHTML = '';
  setCurrentUser(null);
  setKnownUsers([]);
  setPresence({});
  setChannels([]);
  setActiveChannel(null);
  setActiveVoiceChannel(null);
  setVoiceParticipants([]);
  setUnread({});
  setNestView(false);
  try { localStorage.removeItem('finchspeak.nestView'); } catch { /* ignore */ }
});

it('draws voice rooms as nests, text channels as birdhouses and the rest of those online on the wire', () => {
  mount();
  expect(host.querySelector('.sidebar')).toBeNull();
  expect(host.querySelector('.members')).toBeNull();
  expect(host.querySelectorAll('.nest').length).toBe(2);
  expect(host.querySelectorAll('.nest.lit').length).toBe(1);
  expect(host.querySelector('.nest.lit')!.getAttribute('aria-label')).toBe('lounge, 1');
  expect(host.querySelectorAll('.birdhouse').length).toBe(1);
  // ann is in a nest and zed is offline: only me and bob sit on the wire.
  expect(host.querySelectorAll('.tree-wire-bird').length).toBe(2);
  expect(host.querySelector('.tree-offline')!.textContent).toContain('1');
});

it('a nest joins its room and a birdhouse opens its chat', () => {
  setUnread({ general: { count: 3, mention: false } });
  mount();
  expect(host.querySelector('.birdhouse .badge')!.textContent).toBe('3');
  (host.querySelector('.nest.lit') as HTMLElement).click();
  expect(nav.selectChannel).toHaveBeenCalledWith(lounge);
  expect(nav.joinVoice).toHaveBeenCalledWith(lounge);
  (host.querySelector('.birdhouse') as HTMLElement).click();
  expect(nav.selectChannel).toHaveBeenCalledWith(general);
});

it('an opened room shares the window with the tree, which can be folded or returned to', async () => {
  mount();
  expect(host.querySelector('.tree-content')).toBeNull();
  setActiveChannel(general);
  await Promise.resolve();
  expect(host.querySelector('.tree-content .main.chat')).not.toBeNull();
  expect(host.querySelector('.birdhouse.on')).not.toBeNull();
  (host.querySelector('.tree-fold') as HTMLElement).click();
  expect(host.querySelector('.tree-layout')!.classList.contains('folded')).toBe(true);
  (host.querySelector('.tree-close') as HTMLElement).click();
  expect(host.querySelector('.tree-content')).toBeNull();
  expect(host.querySelector('.tree-layout')!.classList.contains('folded')).toBe(false);
});

it('the perch shows the nest you are in and lets you fly out', async () => {
  mount();
  expect(host.querySelector('.tree-leave')).toBeNull();
  setActiveVoiceChannel(lounge);
  await Promise.resolve();
  expect(host.querySelector('.tree-voice')!.textContent).toContain('lounge');
  expect(host.querySelector('.nest.on')).not.toBeNull();
  (host.querySelector('.tree-leave') as HTMLElement).click();
  expect(nav.leaveVoice).toHaveBeenCalled();
});

it('only admins get the buttons that add a nest or a birdhouse, and everyone can go back to classic', () => {
  mount();
  expect(host.querySelector('.tree-add-voice')).toBeNull();
  setCurrentUser(user('me', 'admin'));
  expect(host.querySelector('.tree-add-voice')).not.toBeNull();
  expect(host.querySelector('.tree-add-text')).not.toBeNull();
  (host.querySelector('.tree-bar .nest-toggle') as HTMLElement).click();
  expect(nestView()).toBe(false);
});
