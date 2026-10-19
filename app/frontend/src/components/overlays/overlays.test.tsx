import { render } from 'solid-js/web';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { api } from '../../services/api';
import { currentUser, setCurrentUser } from '../../stores/authStore';
import { activeChannel, setActiveChannel, setChannels } from '../../stores/chatStore';
import { reactToMessage, reactToVoiceMovement, trayStateNow, TONES } from '../../stores/desktop';
import { classifyEnd, sessionEnded, setSessionEnded } from '../../stores/sessionEnd';
import { resetSessionState, sessionHandlers } from '../../stores/sessionStore';
import { DEFAULT_PREFS, muteChannel, setPrefs, updateNotifications } from '../../stores/settingsStore';
import { setIsSharing } from '../../stores/screenState';
import { setActiveVoiceChannel, setIsMuted } from '../../stores/voiceStore';
import { Overlays, SessionEndedScreen, typingNow } from './Overlays';

const general = { id: 'c1', short_id: 1, name: 'general', channel_type: 'text' as const, position: 0, bitrate_kbps: 64 };
const games = { id: 'c2', short_id: 2, name: 'games', channel_type: 'text' as const, position: 1, bitrate_kbps: 64 };
const me = { id: 'me', username: 'alice', display_name: 'Alice', role: 'member' as const };
let dispose: (() => void) | undefined;
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));
const press = (key: string, init: KeyboardEventInit = {}) => window.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init }));

beforeEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
  resetSessionState();
  setPrefs({ ...DEFAULT_PREFS, appearance: { ...DEFAULT_PREFS.appearance, language: 'en' } });
  setSessionEnded(null);
  setCurrentUser(me);
  setChannels([general, games]);
  setActiveChannel(general);
  setActiveVoiceChannel(null);
  setIsMuted(false);
  setIsSharing(false);
});
afterEach(() => {
  dispose?.();
  dispose = undefined;
  document.body.innerHTML = '';
});

const message = (extra: object = {}) => ({ channel_id: 'c2', author_id: 'bob', author_name: 'Bob', content: 'hey @alice', mentions: ['me'], ...extra });

// ---- notifications and tones

it('a mention in another channel is notified; ordinary messages are not unless asked for', () => {
  vi.spyOn(document, 'hasFocus').mockReturnValue(true);
  expect(reactToMessage(message(), true)).toBe('notified');
  expect(reactToMessage(message({ mentions: [] }), false)).toBe('none');
  updateNotifications({ allMessages: true });
  expect(reactToMessage(message({ mentions: [] }), false)).toBe('notified');
});

it('nothing for my own messages, the channel I am looking at, muted channels or switched-off mentions', () => {
  vi.spyOn(document, 'hasFocus').mockReturnValue(true);
  expect(reactToMessage(message({ author_id: 'me' }), true)).toBe('none');
  expect(reactToMessage(message({ channel_id: 'c1' }), true)).toBe('none');
  muteChannel('c2', null);
  expect(reactToMessage(message(), true)).toBe('none');
  setPrefs({ ...DEFAULT_PREFS, appearance: { ...DEFAULT_PREFS.appearance, language: 'en' } });
  updateNotifications({ mentions: false });
  expect(reactToMessage(message(), true)).toBe('none');
});

it('a window in the background still hears about the channel it has open', () => {
  vi.spyOn(document, 'hasFocus').mockReturnValue(false);
  expect(reactToMessage(message({ channel_id: 'c1' }), true)).toBe('notified');
});

it('tones are short two-note chirps and the voice-room ones respect the setting', () => {
  for (const tone of Object.values(TONES)) {
    expect(tone.notes).toHaveLength(2);
    expect(tone.noteMs).toBeLessThanOrEqual(150);
  }
  const play = vi.fn();
  vi.stubGlobal('AudioContext', class { currentTime = 0; destination = {}; createOscillator = () => { play(); return { type: '', frequency: {}, connect: () => ({ connect: () => {} }), start() {}, stop() {} }; }; createGain = () => ({ gain: { setValueAtTime() {}, linearRampToValueAtTime() {}, exponentialRampToValueAtTime() {} }, connect: () => ({}) }); });
  reactToVoiceMovement('join', 'Bob', 'bob', true);
  expect(play).toHaveBeenCalledTimes(2);
  play.mockClear();
  updateNotifications({ voiceJoin: false });
  reactToVoiceMovement('join', 'Bob', 'bob', true);
  expect(play).not.toHaveBeenCalled();
  vi.unstubAllGlobals();
});

it('the tray icon follows the call', () => {
  expect(trayStateNow()).toBe('idle');
  setActiveVoiceChannel({ id: 'v', short_id: 5, name: 'Voice', channel_type: 'voice', position: 0, bitrate_kbps: 64 });
  expect(trayStateNow()).toBe('voice');
  setIsMuted(true);
  expect(trayStateNow()).toBe('muted');
  setIsSharing(true);
  expect(trayStateNow()).toBe('live');
});

it('the tray menu can mute me', async () => {
  const state = vi.spyOn(api, 'setVoiceState').mockResolvedValue();
  setActiveVoiceChannel({ id: 'v', short_id: 5, name: 'Voice', channel_type: 'voice', position: 0, bitrate_kbps: 64 });
  sessionHandlers['tray-toggle-mute'](null);
  await flush();
  expect(state).toHaveBeenCalledWith(true, false);
});

// ---- shortcuts and the switcher

it('? opens the list of shortcuts unless a text field has the focus; Esc closes it', async () => {
  const host = document.createElement('div');
  document.body.appendChild(host);
  dispose = render(() => <Overlays />, host);
  const input = document.createElement('input');
  document.body.appendChild(input);
  input.focus();
  expect(typingNow()).toBe(true);
  input.dispatchEvent(new KeyboardEvent('keydown', { key: '?', bubbles: true }));
  await flush();
  expect(document.querySelector('[role="dialog"]')).toBeNull();
  input.blur();
  press('?');
  await flush();
  expect(document.querySelector('[role="dialog"]')?.textContent).toContain('Keyboard shortcuts');
  document.querySelector('.overlay')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
  await flush();
  expect(document.querySelector('[role="dialog"]')).toBeNull();
});

it('Ctrl+K finds a channel by a part of its name and opens it with Enter', async () => {
  vi.spyOn(api, 'fetchChannelMessages').mockResolvedValue({ channel_id: 'c2', messages: [], has_more: false });
  const host = document.createElement('div');
  document.body.appendChild(host);
  dispose = render(() => <Overlays />, host);
  press('k', { code: 'KeyK', ctrlKey: true });
  await flush();
  const input = document.querySelector<HTMLInputElement>('input[aria-label="Channel name"]')!;
  input.value = 'gam';
  input.dispatchEvent(new Event('input', { bubbles: true }));
  await flush();
  expect(Array.from(document.querySelectorAll('[role="option"]')).map((o) => o.textContent)).toEqual(['games']);
  input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
  await flush();
  await flush();
  expect(activeChannel()?.id).toBe('c2');
  expect(document.querySelector('[role="dialog"]')).toBeNull();
});

it('Ctrl+K opens the switcher in a Russian layout, where the key reports "л"', async () => {
  const host = document.createElement('div');
  document.body.appendChild(host);
  dispose = render(() => <Overlays />, host);
  const event = new KeyboardEvent('keydown', { key: 'л', code: 'KeyK', ctrlKey: true, bubbles: true, cancelable: true });
  window.dispatchEvent(event);
  await flush();
  expect(event.defaultPrevented).toBe(true);
  expect(document.querySelector('[role="dialog"]')?.textContent).toContain('Jump to a channel');
  expect(document.querySelector('input[aria-label="Channel name"]')).toBeTruthy();
});

it('Ctrl+L and a bare K do not open the switcher', async () => {
  const host = document.createElement('div');
  document.body.appendChild(host);
  dispose = render(() => <Overlays />, host);
  press('l', { code: 'KeyL', ctrlKey: true });
  press('k', { code: 'KeyK' });
  await flush();
  expect(document.querySelector('[role="dialog"]')).toBeNull();
});

// ---- the session was ended

it('reads why the server sent us away, and a plain network problem is not an end', () => {
  expect(classifyEnd('connection lost: closed by peer: banned')).toBe('banned');
  expect(classifyEnd('closed by peer: kicked')).toBe('kicked');
  expect(classifyEnd('closed by peer: token revoked')).toBe('signed-out');
  expect(classifyEnd('Authentication rejected (401): Invalid or missing token')).toBe('signed-out');
  expect(classifyEnd('timed out')).toBeNull();
  expect(classifyEnd(undefined)).toBeNull();
});

it('a kick shows the explanation screen, and the way back signs out', async () => {
  vi.spyOn(api, 'logout').mockResolvedValue();
  const host = document.createElement('div');
  document.body.appendChild(host);
  dispose = render(() => <SessionEndedScreen />, host);
  sessionHandlers['connection-lost']('closed by peer: kicked');
  await flush();
  expect(sessionEnded()).toBe('kicked');
  expect(host.textContent).toContain('You were disconnected by an admin');
  sessionHandlers['reconnect-failed']('Authentication rejected (401): Invalid or missing token');
  expect(sessionEnded()).toBe('kicked');
  host.querySelector<HTMLButtonElement>('button')!.click();
  await flush();
  expect(sessionEnded()).toBeNull();
  expect(currentUser()).toBeNull();
});
