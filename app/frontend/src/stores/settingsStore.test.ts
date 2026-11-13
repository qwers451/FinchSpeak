import { beforeEach, expect, it } from 'vitest';
import {
  applyAppearance, DEFAULT_PREFS, isChannelMuted, muteChannel, parsePrefs, setPrefs, unmuteChannel, updateAppearance, updateNotifications, prefs, TEXT_ZOOM,
} from './settingsStore';
import { t } from '../i18n';

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
  expect(repaired.appearance).toMatchObject({ theme: 'claude', language: 'ru', density: 'comfortable', textSize: 'large' });
  expect(repaired.mutedChannels).toEqual({ a: 123, b: null });
});

it('Claude Dark is the default for a fresh install', () => {
  expect(DEFAULT_PREFS.appearance).toMatchObject({ theme: 'claude', brandTheme161: true, claudeDefault17: true });
  expect(parsePrefs(null).appearance).toMatchObject({ theme: 'claude', claudeDefault17: true });
});

it('prefs saved while Bullfinch was the default switch to Claude Dark once', () => {
  const switched = parsePrefs(JSON.stringify({ appearance: { theme: 'bullfinch', brandTheme161: true } }));
  expect(switched.appearance).toMatchObject({ theme: 'claude', claudeDefault17: true });
  expect(parsePrefs(JSON.stringify(switched)).appearance.theme).toBe('claude');
});

it('a Bullfinch choice made after the switch is kept', () => {
  expect(parsePrefs(JSON.stringify({ appearance: { theme: 'bullfinch', brandTheme161: true, claudeDefault17: true } })).appearance.theme).toBe('bullfinch');
});

it('other themes saved with the 1.6 flag are kept by the switch', () => {
  expect(parsePrefs(JSON.stringify({ appearance: { theme: 'custom', brandTheme161: true } })).appearance.theme).toBe('custom');
  expect(parsePrefs(JSON.stringify({ appearance: { theme: 'user', brandTheme161: true } })).appearance.theme).toBe('user');
  expect(parsePrefs(JSON.stringify({ appearance: { theme: 'claude', brandTheme161: true } })).appearance.theme).toBe('claude');
});

it('a saved theme without any default flag falls back to Claude Dark', () => {
  expect(parsePrefs(JSON.stringify({ appearance: { theme: 'custom' } })).appearance.theme).toBe('claude');
});

it('an unknown theme value falls back to Claude Dark', () => {
  expect(parsePrefs(JSON.stringify({ appearance: { theme: 'neon', brandTheme161: true, claudeDefault17: true } })).appearance.theme).toBe('claude');
});

it('the mini window is on by default, and the saved switch is kept', () => {
  expect(DEFAULT_PREFS.overlay.enabled).toBe(true);
  expect(parsePrefs(null).overlay.enabled).toBe(true);
  expect(parsePrefs(JSON.stringify({ overlay: { enabled: false } })).overlay.enabled).toBe(false);
  expect(parsePrefs(JSON.stringify({ overlay: { enabled: 'no' } })).overlay.enabled).toBe(true);
});

it('Bullfinch paints the logo colours and counts as a dark theme', () => {
  const root = document.createElement('div');
  applyAppearance({ ...DEFAULT_PREFS.appearance, theme: 'bullfinch' }, root);
  expect(root.dataset.theme).toBe('bullfinch');
  expect(root.style.getPropertyValue('--bg-app')).toBe('#171516');
  expect(root.style.getPropertyValue('--yellow')).toBe('#ff7a1f');
  expect(root.style.colorScheme).toBe('dark');
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
  applyAppearance({ ...DEFAULT_PREFS.appearance, density: 'compact', textSize: 'large', reduceMotion: true, theme: 'custom', language: 'en' }, root);
  expect(root.dataset.density).toBe('compact');
  expect(root.dataset.theme).toBe('custom');
  expect(root.lang).toBe('en');
  expect(root.dataset.reduceMotion).toBe('true');
  expect(root.style.getPropertyValue('--zoom')).toBe(String(TEXT_ZOOM.large));
});

it('migrates old appearance preferences without losing saved settings', () => {
  const migrated = parsePrefs(JSON.stringify({ appearance: { density: 'compact', showMembers: false }, notifications: { sounds: false }, mutedChannels: { help: null } }));
  expect(migrated.appearance).toMatchObject({ theme: 'claude', language: 'ru', density: 'compact', showMembers: false });
  expect(migrated.notifications.sounds).toBe(false);
  expect(migrated.mutedChannels).toEqual({ help: null });
});

it('repairs invalid theme and language while preserving valid choices', () => {
  expect(parsePrefs(JSON.stringify({ appearance: { theme: 'light', language: 'fr', density: 'compact' } })).appearance)
    .toMatchObject({ theme: 'claude', language: 'ru', density: 'compact' });
});

it('defaults missing or unknown speaking styles to pulse', () => {
  expect(parsePrefs(null).appearance.speakingStyle).toBe('pulse');
  expect(parsePrefs('{"appearance":{"speakingStyle":"unknown"}}').appearance.speakingStyle).toBe('pulse');
  expect(parsePrefs('{"appearance":{"speakingStyle":"glow"}}').appearance.speakingStyle).toBe('glow');
});

it('puts the selected speaking style on the document root', () => {
  expect(document.documentElement.dataset.speakingStyle).toBe('pulse');
  updateAppearance({ speakingStyle: 'waves' });
  expect(document.documentElement.dataset.speakingStyle).toBe('waves');
});

it('translates reactive source strings and interpolates dynamic values', () => {
  setPrefs((current) => ({ ...current, appearance: { ...current.appearance, language: 'ru' } }));
  expect(t('Settings')).toBe('Настройки');
  expect(t('Install {version}', { version: '1.2.0' })).toBe('Установить 1.2.0');
  setPrefs((current) => ({ ...current, appearance: { ...current.appearance, language: 'en' } }));
  expect(t('Settings')).toBe('Settings');
});

it('ignores the close-to-tray value an older version saved', () => {
  const plain = parsePrefs(JSON.stringify({ appearance: { density: 'compact' } }));
  const old = parsePrefs(JSON.stringify({ appearance: { density: 'compact', closeToTray: false } }));
  expect(old).toEqual(plain);
  expect('closeToTray' in old.appearance).toBe(false);
});

it('the reply, flock call and screen share notices are on by default and keep a saved off', () => {
  expect(DEFAULT_PREFS.notifications).toMatchObject({ replies: true, flockCalls: true, screenShares: true });
  const saved = parsePrefs(JSON.stringify({ notifications: { replies: false, flockCalls: 'yes' } }));
  expect(saved.notifications).toMatchObject({ replies: false, flockCalls: true, screenShares: true });
});
