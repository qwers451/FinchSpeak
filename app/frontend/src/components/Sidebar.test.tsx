import { render } from 'solid-js/web';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { setActiveChannel, setChannels } from '../stores/chatStore';
import { setCurrentUser } from '../stores/authStore';
import { setKnownUsers } from '../stores/sessionStore';
import { setActiveVoiceChannel, setVoiceParticipants } from '../stores/voiceStore';
import type { Channel, User } from '../types';
import { nestView, setNestView } from '../stores/nestView';
import { ContextMenuHost, closeContextMenu } from '../ui/ContextMenu';
import { ru } from '../i18n';
import { Sidebar } from './Sidebar';

const nav = vi.hoisted(() => ({ selectChannel: vi.fn(async () => {}), joinVoice: vi.fn(async () => {}) }));
vi.mock('../stores/chatStore', async (importOriginal) => ({ ...(await importOriginal<typeof import('../stores/chatStore')>()), selectChannel: nav.selectChannel }));
vi.mock('../stores/voiceStore', async (importOriginal) => ({ ...(await importOriginal<typeof import('../stores/voiceStore')>()), joinVoice: nav.joinVoice }));

const voice: Channel = { id: 'voice', short_id: 1, name: 'voice', channel_type: 'voice', position: 0, bitrate_kbps: 64 };
const svgAvatar = (fill: string) => `data:image/svg+xml,${encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"><rect width="1" height="1" fill="${fill}"/></svg>`)}`;
const member: User = {
  id: 'participant', username: 'bob', display_name: 'Bob', role: 'member',
  avatar_url: svgAvatar('red'),
};
const text: Channel = { id: 'text', short_id: 2, name: 'general', channel_type: 'text', position: 0, bitrate_kbps: 64 };
const admin: User = { id: 'admin', username: 'root', display_name: 'Root', role: 'admin', avatar_url: null };
let dispose: (() => void) | undefined;

beforeEach(() => { nav.selectChannel.mockClear(); nav.joinVoice.mockClear(); });

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
  setNestView(false);
  try { localStorage.removeItem('finchspeak.nestView'); } catch { /* ignore */ }
});

it('the Finch mode switch sits in the brand row with a visible label, not in the voice header', () => {
  setChannels([voice]);
  const host = document.createElement('div');
  document.body.appendChild(host);
  dispose = render(() => <Sidebar />, host);
  expect(nestView()).toBe(false);
  expect(host.querySelectorAll('.channel-item').length).toBe(1);
  expect(host.querySelector('.nav .nest-toggle')).toBeNull();
  const toggle = host.querySelector('.brand .nest-toggle') as HTMLElement;
  expect(toggle.textContent).toContain('Finch mode');
  toggle.click();
  expect(nestView()).toBe(true);
});

it('clicking the voice banner opens the active voice room without joining it again', async () => {
  setChannels([voice]);
  setActiveVoiceChannel(voice);
  const host = document.createElement('div');
  document.body.appendChild(host);
  dispose = render(() => <Sidebar />, host);
  (host.querySelector('.voice-banner .voice-connection') as HTMLElement).click();
  await flush();
  expect(nav.selectChannel).toHaveBeenCalledWith(voice);
  expect(nav.joinVoice).not.toHaveBeenCalled();
});

it('the banner leave button is labelled Leave room and does not also open the room', async () => {
  setChannels([voice]);
  setActiveVoiceChannel(voice);
  const host = document.createElement('div');
  document.body.appendChild(host);
  dispose = render(() => <Sidebar />, host);
  const leave = host.querySelector('.voice-banner [aria-label="Leave room"]') as HTMLElement;
  expect(leave).not.toBeNull();
  leave.click();
  await flush();
  expect(nav.selectChannel).not.toHaveBeenCalled();
});

it('the profile panel has a single settings button, a gear, and no second sliders icon', () => {
  setCurrentUser(member);
  const host = document.createElement('div');
  document.body.appendChild(host);
  dispose = render(() => <Sidebar />, host);
  expect(host.querySelectorAll('.upanel [aria-label="Profile and settings"]').length).toBe(1);
  expect(host.querySelector('.upanel [aria-label="Profile and settings"] use.classic-glyph')?.getAttribute('href')).toBe('#i-gear');
  expect(host.querySelector('.upanel use[href="#i-sliders"]')).toBeNull();
});

it('each channel has one actions button that opens Move up, Move down, Rename and Delete, with Delete as danger', async () => {
  const host = mountAsAdmin();
  expect(host.querySelectorAll('.ch [aria-label^="Actions for"]').length).toBe(2);
  expect(host.querySelector('.ch [aria-label="Rename general"]')).toBeNull();
  (host.querySelector('.ch [aria-label="Actions for general"]') as HTMLElement).click();
  await flush();
  const items = [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')];
  expect(items.map((item) => item.textContent?.trim())).toEqual(['Move up', 'Move down', 'Rename', 'Delete']);
  expect(items[3].classList.contains('danger')).toBe(true);
  expect(nav.selectChannel).not.toHaveBeenCalled();
  expect(nav.joinVoice).not.toHaveBeenCalled();
  closeContextMenu();
  expect(ru['Move up']).toBe('Выше');
  expect(ru['Move down']).toBe('Ниже');
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

/** Presses a key on an element the way a browser does: keydown first, and the default action (activating a button) only if keydown was not cancelled. */
function press(element: HTMLElement, key: string) {
  const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true });
  element.dispatchEvent(event);
  if (!event.defaultPrevented && element instanceof HTMLButtonElement) element.click();
  return event;
}

function mountAsAdmin() {
  setCurrentUser(admin);
  setChannels([text, voice]);
  const host = document.createElement('div');
  document.body.appendChild(host);
  dispose = render(() => <><ContextMenuHost /><Sidebar /></>, host);
  return host;
}

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

for (const key of ['Enter', ' ']) {
  for (const [label, name] of [['text', 'general'], ['voice', 'voice']] as const) {
    it(`opens the actions of a ${label} channel with "${key === ' ' ? 'Space' : key}" and renames from there, without selecting or joining it`, async () => {
      const host = mountAsAdmin();
      const more = host.querySelector(`button[aria-label="Actions for ${name}"]`) as HTMLButtonElement;
      expect(more).toBeTruthy();
      const event = press(more, key);
      await flush();
      expect(event.defaultPrevented).toBe(false);
      const rename = [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')].find((item) => item.textContent?.trim() === 'Rename')!;
      rename.click();
      await flush();
      expect(document.body.textContent).toContain('Rename channel');
      expect(nav.selectChannel).not.toHaveBeenCalled();
      expect(nav.joinVoice).not.toHaveBeenCalled();
      closeContextMenu();
    });
  }
}

it('still selects a text channel with Enter on the row itself', async () => {
  const host = mountAsAdmin();
  const row = [...host.querySelectorAll('.channel-item')].find((el) => el.textContent?.includes('general')) as HTMLElement;
  const event = press(row, 'Enter');
  expect(event.defaultPrevented).toBe(true);
  expect(nav.selectChannel).toHaveBeenCalledWith(text);
  expect(nav.joinVoice).not.toHaveBeenCalled();
});

it('still joins a voice channel with Enter on the row itself', async () => {
  const host = mountAsAdmin();
  const row = [...host.querySelectorAll('.channel-item')].find((el) => el.textContent?.includes('voice')) as HTMLElement;
  press(row, ' ');
  expect(nav.selectChannel).toHaveBeenCalledWith(voice);
  expect(nav.joinVoice).toHaveBeenCalledWith(voice);
});

it('puts the full channel name in a tooltip, because the action buttons may cut it short', () => {
  setCurrentUser(admin);
  setChannels([{ ...text, name: 'planning-and-roadmap' }, { ...voice, name: 'late-night-hangout' }]);
  const host = document.createElement('div');
  document.body.appendChild(host);
  dispose = render(() => <Sidebar />, host);
  const names = Array.from(host.querySelectorAll('.ch > .trunc')).map((name) => [name.textContent, name.getAttribute('title')]);
  expect(names).toEqual([['planning-and-roadmap', 'planning-and-roadmap'], ['late-night-hangout', 'late-night-hangout']]);
});
