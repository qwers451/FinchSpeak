import { render } from 'solid-js/web';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { setActiveChannel, setChannels } from '../../stores/chatStore';
import { setCurrentUser } from '../../stores/authStore';
import { setKnownUsers, setPresence } from '../../stores/sessionStore';
import { setActiveVoiceChannel, setVoiceParticipants } from '../../stores/voiceStore';
import { setUnread } from '../../stores/activityStore';
import { nestView, setNestView } from '../../stores/nestView';
import { setGameActivity } from '../../stores/activityGameStore';
import { api } from '../../services/api';
import { t } from '../../i18n';
import { ContextMenuHost } from '../../ui/ContextMenu';
import type { Channel, User } from '../../types';
import { profileOpen, setProfileOpen } from '../../stores/profileStore';
import { TreeView } from './TreeView';

const nav = vi.hoisted(() => ({ selectChannel: vi.fn(async () => {}), joinVoice: vi.fn(async () => {}), leaveVoice: vi.fn(async () => {}) }));
const mutes = vi.hoisted(() => ({ muted: new Set<string>() }));
vi.mock('../../stores/localMuteStore', async (importOriginal) => ({ ...(await importOriginal<typeof import('../../stores/localMuteStore')>()), isLocallyMuted: (userId: string) => mutes.muted.has(userId) }));
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
const mount = () => { dispose = render(() => <><TreeView /><ContextMenuHost /></>, host); };

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

it('the perch shows the nest you are in and lets you leave the room', async () => {
  mount();
  expect(host.querySelector('.tree-leave')).toBeNull();
  setActiveVoiceChannel(lounge);
  await Promise.resolve();
  expect(host.querySelector('.tree-voice')!.textContent).toContain('lounge');
  expect(host.querySelector('.nest.on')).not.toBeNull();
  expect(host.querySelector('.tree-leave')!.getAttribute('aria-label')).toBe('Leave room');
  (host.querySelector('.tree-leave') as HTMLElement).click();
  expect(nav.leaveVoice).toHaveBeenCalled();
});

it('only admins get the buttons that add a nest or a birdhouse, and everyone can go back to classic', () => {
  mount();
  expect(host.querySelector('.tree-add-voice')).toBeNull();
  setCurrentUser(user('me', 'admin'));
  expect(host.querySelector('.tree-add-voice')).not.toBeNull();
  expect(host.querySelector('.tree-add-text')).not.toBeNull();
  const classic = host.querySelector('.tree-bar .nest-toggle') as HTMLElement;
  expect(classic.textContent).toContain('Classic view');
  classic.click();
  expect(nestView()).toBe(false);
});

it('the profile footer has one settings button, the gear, and the avatar and name are not controls', () => {
  mount();
  expect(host.querySelectorAll('.tree-perch [aria-label="Profile and settings"]').length).toBe(1);
  expect(host.querySelector('.tree-perch [aria-label="Profile and settings"] use.classic-glyph')?.getAttribute('href')).toBe('#i-gear');
  const openers = [...host.querySelectorAll<HTMLElement>('.tree-perch button, .tree-perch [role="button"], .tree-perch [tabindex]')]
    .filter((el) => /profile|settings/i.test(el.getAttribute('aria-label') ?? ''));
  expect(openers.length).toBe(1);
  const me = host.querySelector('.tree-perch .profile-card') as HTMLElement;
  expect(me.tagName).toBe('DIV');
  expect(me.querySelector('button, [tabindex]')).toBeNull();
  openers[0].click();
  expect(profileOpen()).toBe(true);
  setProfileOpen(false);
});

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));
const dragEvent = (element: Element, type: string, transfer: DataTransfer) => {
  const event = new Event(type, { bubbles: true, cancelable: true });
  Object.defineProperty(event, 'dataTransfer', { value: transfer });
  element.dispatchEvent(event);
  return event;
};

it('right-clicking a bird in a nest opens the participant menu', async () => {
  mount();
  host.querySelector('.nest-bird')!.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
  await flush();
  const menu = document.querySelector('[role="menu"]')!;
  expect(menu.textContent).toContain('ann');
  expect(menu.textContent).toContain('Mute for me');
});

it('right-clicking a bird on the wire opens the same menu without room actions', async () => {
  setCurrentUser(user('me', 'admin'));
  mount();
  host.querySelector('.tree-wire-bird[title="bob"]')!.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
  await flush();
  const menu = document.querySelector('[role="menu"]')!;
  expect(menu.textContent).toContain('bob');
  expect(menu.textContent).not.toContain('Move to…');
  expect(menu.textContent).not.toContain('Remove from the room');
});

it('a moderator drags a participant into another nest, which moves them with their exact peer and nest', async () => {
  setCurrentUser(user('me', 'admin'));
  const move = vi.spyOn(api, 'moveVoice').mockResolvedValue();
  mount();
  const bird = host.querySelector('.nest-bird') as HTMLElement;
  const atticNest = host.querySelector('.nest[aria-label^="attic"]') as HTMLElement;
  const transfer = new DataTransfer();
  dragEvent(bird, 'dragstart', transfer);
  expect(dragEvent(atticNest, 'dragover', transfer).defaultPrevented).toBe(true);
  expect(atticNest.classList.contains('drop-target')).toBe(true);
  dragEvent(atticNest, 'drop', transfer);
  await flush();
  expect(move).toHaveBeenCalledWith('ann', 7, 'attic');
  expect(atticNest.classList.contains('drop-target')).toBe(false);
  move.mockRestore();
});

it('a_member_can_only_drag_themselves_between_nests', async () => {
  setVoiceParticipants([
    { user_id: 'ann', peer_id: 7, username: 'ann', channel_id: 'lounge' },
    { user_id: 'me', peer_id: 8, username: 'me', channel_id: 'lounge' },
  ]);
  const move = vi.spyOn(api, 'moveVoice').mockResolvedValue();
  mount();
  const birdOf = (name: string) => host.querySelector(`.nest-bird[title="${name}"]`) as HTMLElement;
  expect(birdOf('ann').getAttribute('draggable')).toBe('false');
  expect(birdOf('me').getAttribute('draggable')).toBe('true');
  const atticNest = host.querySelector('.nest[aria-label^="attic"]') as HTMLElement;
  const transfer = new DataTransfer();
  dragEvent(birdOf('me'), 'dragstart', transfer);
  dragEvent(atticNest, 'drop', transfer);
  await flush();
  expect(nav.joinVoice).toHaveBeenCalledWith(attic);
  expect(move).not.toHaveBeenCalled();
  move.mockRestore();
});

it('a muted-for-me participant shows vol-off and a participant in a game shows the game', async () => {
  mutes.muted.add('ann');
  setGameActivity({ user_id: 'ann', game: 'Enshrouded' });
  mount();
  const bird = host.querySelector('.nest-bird') as HTMLElement;
  expect(bird.querySelector('.nest-local-mute')!.getAttribute('aria-label')).toBe(t('You muted this person'));
  expect(bird.querySelector('.nest-game')!.getAttribute('aria-label')).toBe(t('Playing {game}', { game: 'Enshrouded' }));
  mutes.muted.clear();
  setGameActivity({ user_id: 'ann', game: null });
});
