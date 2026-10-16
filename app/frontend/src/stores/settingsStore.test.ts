import { beforeEach, expect, it } from 'vitest';
import {
  applyAppearance, DEFAULT_PREFS, isChannelMuted, muteChannel, parsePrefs, setPrefs, unmuteChannel, updateAppearance, updateNotifications, prefs, TEXT_ZOOM,
} from './settingsStore';

beforeEach(() => {
  localStorage.clear();
  setPrefs(DEFAULT_PREFS);
});

it('starts from sensible defaults when nothing or garbage was saved', () => {
  expect(parsePrefs(null)).toEqual(DEFAULT_PREFS);
  expect(parsePrefs('{broken')).toEqual(DEFAULT_PREFS);
  expect(parsePrefs('42')).toEqual(DEFAULT_PREFS);
});

it('repairs values of the wrong type or outside their range and keeps the good ones', () => {
  const repaired = parsePrefs(JSON.stringify({
    notifications: { mentions: 'yes', sounds: false, soundVolume: 7 },
    appearance: { density: 'tiny', textSize: 'large' },
    mutedChannels: { a: 123, b: null, c: 'x' },
  }));
  expect(repaired.notifications).toMatchObject({ mentions: true, sounds: false, soundVolume: 1 });
  expect(repaired.appearance).toMatchObject({ density: 'comfortable', textSize: 'large' });
  expect(repaired.mutedChannels).toEqual({ a: 123, b: null });
});

it('changes are saved for the next start', () => {
  updateNotifications({ allMessages: true });
  updateAppearance({ density: 'compact' });
  const saved = parsePrefs(localStorage.getItem('finchspeak.prefs'));
  expect(saved.notifications.allMessages).toBe(true);
  expect(saved.appearance.density).toBe('compact');
});

it('a channel can be muted for a while, for good, and turned back on', () => {
  const now = 1_000_000;
  muteChannel('a', 60_000, now);
  muteChannel('b', null, now);
  expect(isChannelMuted('a', now + 59_000)).toBe(true);
  expect(isChannelMuted('a', now + 61_000)).toBe(false);
  expect(isChannelMuted('b', now + 10 ** 12)).toBe(true);
  expect(isChannelMuted('c', now)).toBe(false);
  unmuteChannel('b');
  expect(isChannelMuted('b', now)).toBe(false);
  expect(prefs().mutedChannels).toHaveProperty('a');
});

it('the looks are put on the page', () => {
  const root = document.createElement('div');
  applyAppearance({ density: 'compact', textSize: 'large', reduceMotion: true, showMembers: true }, root);
  expect(root.dataset.density).toBe('compact');
  expect(root.dataset.reduceMotion).toBe('true');
  expect(root.style.getPropertyValue('--zoom')).toBe(String(TEXT_ZOOM.large));
});
