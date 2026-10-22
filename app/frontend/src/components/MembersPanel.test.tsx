import { render } from 'solid-js/web';
import { afterEach, beforeEach, expect, it } from 'vitest';
import { resetSessionState, setKnownUsers, setPresence } from '../stores/sessionStore';
import { DEFAULT_PREFS, setPrefs } from '../stores/settingsStore';
import type { Role, User } from '../types';
import { MembersPanel } from './MembersPanel';

const user = (id: string, role: Role, display_name = id): User => ({ id, username: id, display_name, role });
let dispose: (() => void) | undefined;
const mount = () => {
  const host = document.createElement('div');
  document.body.appendChild(host);
  dispose = render(() => <MembersPanel />, host);
  return host;
};
const setLanguage = (language: 'ru' | 'en') => setPrefs({ ...DEFAULT_PREFS, appearance: { ...DEFAULT_PREFS.appearance, language } });
/** Section headings and the names under them, in screen order. */
const layout = (host: HTMLElement) =>
  Array.from(host.querySelectorAll('.sec-label, .mem')).map((el) => (el.classList.contains('sec-label') ? `# ${el.textContent}` : el.querySelector('.trunc')!.textContent));

beforeEach(() => {
  resetSessionState();
  setLanguage('en');
});
afterEach(() => {
  dispose?.();
  dispose = undefined;
  document.body.innerHTML = '';
});

it('groups online members as admins, moderators and the rest, then everyone offline', () => {
  setKnownUsers([user('m1', 'member', 'Mia'), user('mod', 'moderator', 'Moe'), user('adm', 'admin', 'Ada'), user('gone', 'member', 'Gus')]);
  setPresence({ m1: 'online', mod: 'idle', adm: 'donotdisturb', gone: 'offline' });
  expect(layout(mount())).toEqual(['# Admins — 1', 'Ada', '# Moderators — 1', 'Moe', '# Online — 1', 'Mia', '# Offline — 1', 'Gus']);
});

it('does not show empty groups', () => {
  setKnownUsers([user('m1', 'member', 'Mia'), user('m2', 'member', 'Max')]);
  setPresence({ m1: 'online', m2: 'online' });
  expect(layout(mount())).toEqual(['# Online — 2', 'Max', 'Mia']);
});

it('puts an offline admin or moderator into the offline group, without splitting offline users by role', () => {
  setKnownUsers([user('adm', 'admin', 'Ada'), user('mod', 'moderator', 'Moe'), user('m1', 'member', 'Mia')]);
  setPresence({ adm: 'offline', mod: 'offline', m1: 'online' });
  expect(layout(mount())).toEqual(['# Online — 1', 'Mia', '# Offline — 2', 'Ada', 'Moe']);
});

it('sorts names inside a group by display name, falling back to the login', () => {
  setKnownUsers([user('zed', 'member', 'Zed'), user('anna', 'member', ''), user('bob', 'member', 'bob')]);
  setPresence({ zed: 'online', anna: 'online', bob: 'online' });
  expect(layout(mount())).toEqual(['# Online — 3', 'anna', 'bob', 'Zed']);
});

it('sorts with the rules of the interface language', () => {
  setLanguage('ru');
  setKnownUsers([user('1', 'member', 'Яна'), user('2', 'member', 'Ёлка'), user('3', 'member', 'Анна'), user('4', 'member', 'Егор')]);
  setPresence({ '1': 'online', '2': 'online', '3': 'online', '4': 'online' });
  expect(layout(mount())).toEqual(['# В сети — 4', 'Анна', 'Егор', 'Ёлка', 'Яна']);
});

it('titles the staff groups in Russian', () => {
  setLanguage('ru');
  setKnownUsers([user('adm', 'admin', 'Ada'), user('mod', 'moderator', 'Moe')]);
  setPresence({ adm: 'online', mod: 'online' });
  expect(layout(mount())).toEqual(['# Администраторы — 1', 'Ada', '# Модераторы — 1', 'Moe']);
});

it('has no role label in a row: the group heading says it', () => {
  setKnownUsers([user('adm', 'admin', 'Ada')]);
  setPresence({ adm: 'online' });
  const host = mount();
  expect(host.querySelector('.mem .tag')).toBeNull();
  expect(host.querySelector('.mem')!.textContent).not.toContain('Admin');
});

it('keeps the rows on screen when someone else changes status, instead of rebuilding the panel', () => {
  setKnownUsers([user('adm', 'admin', 'Ada'), user('m1', 'member', 'Mia'), user('m2', 'member', 'Max')]);
  setPresence({ adm: 'online', m1: 'online', m2: 'online' });
  const host = mount();
  const before = Array.from(host.querySelectorAll('.mem'));
  setPresence({ adm: 'online', m1: 'online', m2: 'idle' });
  const after = Array.from(host.querySelectorAll('.mem'));
  expect(after).toHaveLength(3);
  expect(after).toEqual(before);
  expect(layout(host)).toEqual(['# Admins — 1', 'Ada', '# Online — 2', 'Max', 'Mia']);
});
