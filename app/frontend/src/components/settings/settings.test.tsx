import { render } from 'solid-js/web';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { api } from '../../services/api';
import { avatarSrc, clearAvatarCache, userIdOf } from '../../stores/avatarCache';
import { setCurrentUser, currentUser } from '../../stores/authStore';
import { logoutUser } from '../../stores/authStore';
import { endSession, loadSessions, profileOpen, setProfileMessage, setProfileOpen } from '../../stores/profileStore';
import { resetSessionState, sessionHandlers, setKnownUsers } from '../../stores/sessionStore';
import { DEFAULT_PREFS, prefs, setPrefs } from '../../stores/settingsStore';
import { setChannels } from '../../stores/chatStore';
import { nestView, setNestView } from '../../stores/nestView';
import { clearToasts } from '../../ui';
import { checkAvatarFile, CROP_SIDE, placement } from './AvatarCropDialog';
import { SettingsModal } from './SettingsModal';

vi.mock('../../stores/authStore', async (importOriginal) => ({ ...(await importOriginal<typeof import('../../stores/authStore')>()), logoutUser: vi.fn().mockResolvedValue(undefined) }));

let dispose: (() => void) | undefined;
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));
const button = (text: string) => Array.from(document.querySelectorAll('button')).find((b) => b.textContent?.includes(text)) as HTMLButtonElement;
const me = { id: '11111111-1111-1111-1111-111111111111', username: 'alice', display_name: 'Alice Cooper', role: 'member' as const };

beforeEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
  resetSessionState();
  clearToasts();
  clearAvatarCache();
  setPrefs({ ...DEFAULT_PREFS, appearance: { ...DEFAULT_PREFS.appearance, language: 'en' } });
  setCurrentUser(me);
  setKnownUsers([me]);
  setChannels([{ id: 'c1', short_id: 1, name: 'general', channel_type: 'text', position: 0, bitrate_kbps: 64 }]);
  setProfileMessage(null);
  setProfileOpen(true);
  const host = document.createElement('div');
  document.body.appendChild(host);
  dispose = render(() => <SettingsModal />, host);
});
afterEach(() => {
  dispose?.();
  document.body.innerHTML = '';
});

it('saves a new display name', async () => {
  const save = vi.spyOn(api, 'updateProfile').mockResolvedValue({ ...me, display_name: 'Alice C.' });
  const input = Array.from(document.querySelectorAll<HTMLElement>('.field')).find((f) => f.textContent?.startsWith('Display name'))!.querySelector('input')!;
  input.value = 'Alice C.';
  input.dispatchEvent(new Event('input', { bubbles: true }));
  await flush();
  button('Save name').click();
  await flush();
  expect(save).toHaveBeenCalledWith('Alice C.');
  expect(currentUser()?.display_name).toBe('Alice C.');
});

it('choosing a status sends it with the text, trimmed', async () => {
  const status = vi.spyOn(api, 'setStatus').mockResolvedValue();
  const text = Array.from(document.querySelectorAll<HTMLInputElement>('input')).find((i) => i.placeholder.includes('meeting'))!;
  text.value = '  In a meeting ';
  text.dispatchEvent(new Event('input', { bubbles: true }));
  await flush();
  document.querySelectorAll<HTMLElement>('[role="radio"]')[2].click();
  await flush();
  expect(status).toHaveBeenCalledWith('donotdisturb', 'In a meeting');
});

it('shows the status somebody else set me via the server', () => {
  sessionHandlers['presence-update']({ user_id: me.id, status: 'idle', status_text: 'Lunch' });
  expect(currentUser()?.status).toBe('idle');
  expect(document.querySelector('[aria-checked="true"][role="radio"]')?.textContent).toContain('Idle');
});

it('refuses a password change whose repeat does not match', async () => {
  const change = vi.spyOn(api, 'changePassword').mockResolvedValue();
  const [current, next, again] = Array.from(document.querySelectorAll<HTMLInputElement>('input[type="password"], input'))
    .filter((i) => ['Current password', 'New password', 'Repeat the new password'].includes(i.closest('.field')?.querySelector('label')?.textContent ?? ''));
  for (const [el, value] of [[current, 'old-password'], [next, 'brand-new-pass'], [again, 'brand-new-typo']] as const) {
    el.value = value;
    el.dispatchEvent(new Event('input', { bubbles: true }));
  }
  await flush();
  expect(document.body.textContent).toContain('The passwords do not match');
  expect(button('Change password').disabled).toBe(true);
  expect(change).not.toHaveBeenCalled();
});

it('notification choices are remembered', async () => {
  button('Notifications').click();
  await flush();
  document.querySelector<HTMLElement>('[aria-label="Every message"]')!.click();
  await flush();
  expect(prefs().notifications.allMessages).toBe(true);
  button('1 hour').click();
  await flush();
  expect(Object.keys(prefs().mutedChannels)).toEqual(['c1']);
  expect(document.body.textContent).toContain('Turn on');
});

it('appearance changes reach the page', async () => {
  button('Appearance').click();
  await flush();
  Array.from(document.querySelectorAll<HTMLElement>('[role="radio"]')).find((r) => r.textContent === 'Compact')!.click();
  await flush();
  expect(document.documentElement.dataset.density).toBe('compact');
  document.querySelector<HTMLElement>('[aria-label="Members panel"]')!.click();
  await flush();
  expect(prefs().appearance.showMembers).toBe(false);
  setPrefs(DEFAULT_PREFS);
});

it('the nest view of voice rooms is switched on and off in appearance', async () => {
  try {
    button('Appearance').click();
    await flush();
    const toggle = () => document.querySelector<HTMLElement>('[aria-label="Rooms as nests"]')!;
    expect(nestView()).toBe(false);
    toggle().click();
    await flush();
    expect(nestView()).toBe(true);
    expect(localStorage.getItem('finchspeak.nestView')).toBe('1');
    toggle().click();
    await flush();
    expect(nestView()).toBe(false);
  } finally {
    setNestView(false);
    localStorage.removeItem('finchspeak.nestView');
  }
});

it('autostart toggle reflects and changes the OS setting', async () => {
  vi.spyOn(api, 'autostartEnabled').mockResolvedValue(false);
  const set = vi.spyOn(api, 'setAutostart').mockResolvedValue();
  button('Appearance').click();
  await flush();
  const toggle = document.querySelector<HTMLElement>('[aria-label="Start with Windows"]')!;
  expect(toggle.getAttribute('aria-checked')).toBe('false');
  toggle.click();
  await flush();
  expect(set).toHaveBeenCalledWith(true);
  expect(toggle.getAttribute('aria-checked')).toBe('true');
});

it('switches the mounted settings dialog language and updates the document language', async () => {
  button('Appearance').click();
  await flush();
  expect(document.querySelector('[aria-label="Language"]')).toBeTruthy();
  document.querySelectorAll<HTMLElement>('[role="radio"]').forEach((radio) => {
    if (radio.textContent === 'Русский') radio.click();
  });
  await flush();
  expect(document.documentElement.lang).toBe('ru');
  expect(button('Внешний вид')).toBeTruthy();
  expect(document.body.textContent).toContain('Тема');
});

it('the about page shows the server and its certificate and copies diagnostics', async () => {
  vi.spyOn(api, 'serverInfo').mockResolvedValue({ server_url: 'https://chat.example:8443', hostname: 'chat.example', fingerprint: 'AA:BB' });
  const write = vi.fn(async () => {});
  vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText: write } });
  button('Keys and about').click();
  await flush();
  await flush();
  expect(document.body.textContent).toContain('https://chat.example:8443');
  expect(document.body.textContent).toContain('AA:BB');
  button('Copy diagnostics').click();
  expect(write).toHaveBeenCalledWith(expect.stringContaining('Certificate: AA:BB'));
  vi.unstubAllGlobals();
});

// ---- picture helpers

it('checks a chosen picture before anything is read', () => {
  expect(checkAvatarFile({ type: 'image/gif', size: 10 })).toContain('PNG, JPEG or WebP');
  expect(checkAvatarFile({ type: 'image/png', size: 20 * 1024 * 1024 })).toContain('too large');
  expect(checkAvatarFile({ type: 'image/jpeg', size: 1000 })).toBeNull();
});

it('the crop always covers the square and cannot be dragged into a gap', () => {
  const wide = placement(1000, 500, 1, 9999, 9999);
  expect(wide.height).toBe(CROP_SIDE);
  expect(wide.left).toBeLessThanOrEqual(0);
  expect(wide.left + wide.width).toBeGreaterThanOrEqual(CROP_SIDE);
  const zoomed = placement(500, 500, 2, -9999, 9999);
  expect(zoomed.width).toBe(CROP_SIDE * 2);
  expect(zoomed.left).toBe(-CROP_SIDE);
  expect(zoomed.top).toBe(0);
  expect(placement(500, 500, 0.2, 5, 5).width).toBe(CROP_SIDE);
});

it('pictures are fetched once per address and other addresses pass through', async () => {
  const fetchPicture = vi.spyOn(api, 'downloadAvatar').mockResolvedValue(new Uint8Array([1]).buffer as ArrayBuffer);
  vi.stubGlobal('URL', { createObjectURL: () => 'blob:pic', revokeObjectURL: vi.fn() });
  const address = `/api/users/${me.id}/avatar?v=5`;
  expect(userIdOf(address)).toBe(me.id);
  expect(await avatarSrc(address)).toBe('blob:pic');
  expect(await avatarSrc(address)).toBe('blob:pic');
  expect(fetchPicture).toHaveBeenCalledTimes(1);
  expect(await avatarSrc(`/api/users/${me.id}/avatar?v=6`)).toBe('blob:pic');
  expect(fetchPicture).toHaveBeenCalledTimes(2);
  expect(await avatarSrc('data:image/png;base64,AAAA')).toBe('data:image/png;base64,AAAA');
  vi.unstubAllGlobals();
});

it('a changed picture reaches the person everywhere', () => {
  sessionHandlers['avatar-changed']({ user_id: me.id, avatar_url: `/api/users/${me.id}/avatar?v=9` });
  expect(currentUser()?.avatar_url).toContain('v=9');
  sessionHandlers['avatar-changed']({ user_id: me.id, avatar_url: null });
  expect(currentUser()?.avatar_url).toBeNull();
});

// ---- push-to-talk key capture

const pttButton = () => document.querySelector('button[aria-label="Push-to-talk key"]') as HTMLButtonElement;
const pressEscape = (element: HTMLElement) => {
  const event = new KeyboardEvent('keydown', { key: 'Escape', code: 'Escape', bubbles: true, cancelable: true });
  element.dispatchEvent(event);
  return event;
};

it('Esc while choosing the push-to-talk key cancels the choice and keeps the dialog open', async () => {
  button('Keys and about').click();
  await flush();
  pttButton().click();
  await flush();
  expect(pttButton().textContent).toContain('Press a key');

  pressEscape(pttButton());
  await flush();
  expect(profileOpen()).toBe(true);
  expect(document.querySelector('[role="dialog"]')).toBeTruthy();
  expect(pttButton().textContent).toContain('Choose key');
});

it('Esc outside key capture still closes the settings dialog', async () => {
  button('Keys and about').click();
  await flush();
  pressEscape(pttButton());
  await flush();
  expect(profileOpen()).toBe(false);
});

it('voice and audio opens as a page inside the settings window, not as a second dialog', async () => {
  vi.spyOn(api, 'getAudioDevices').mockResolvedValue({ inputs: [], outputs: [], selected_input: null, selected_output: null });
  button('Voice and audio').click();
  await flush();
  expect(profileOpen()).toBe(true);
  expect(document.querySelectorAll('[role="dialog"]').length).toBe(1);
  expect(document.querySelector('[aria-label="Microphone"]')).not.toBeNull();
  expect(document.querySelector('[aria-label="Mute microphone key"]')).not.toBeNull();
  expect(button('Voice and audio').getAttribute('aria-current')).toBe('page');
});

const sessions = [
  { id: 's1', device_name: 'DESKTOP-HOME', created_at: 1_700_000_000, last_seen_at: 1_700_000_500, current: true },
  { id: 's2', device_name: null, created_at: 1_700_000_000, last_seen_at: 1_700_000_100, current: false },
  { id: 's3', device_name: 'LAPTOP', created_at: 1_700_000_000, last_seen_at: 1_700_000_100, current: false },
];

it('lists the signed-in devices and marks this computer', async () => {
  vi.spyOn(api, 'listSessions').mockResolvedValue(sessions);
  dispose?.();
  document.body.innerHTML = '';
  const host = document.createElement('div');
  document.body.appendChild(host);
  dispose = render(() => <SettingsModal />, host);
  await flush();
  const text = document.body.textContent ?? '';
  expect(text).toContain('Devices');
  expect(text).toContain('DESKTOP-HOME');
  expect(text).toContain('This computer');
  expect(text).toContain('Unknown device');
  expect(text).toContain('LAPTOP');
});

it('ending_the_current_session_logs_out', async () => {
  vi.spyOn(api, 'listSessions').mockResolvedValue(sessions);
  const end = vi.spyOn(api, 'endSession').mockResolvedValue();
  await loadSessions();
  await endSession('s1');
  expect(logoutUser).toHaveBeenCalled();
  expect(end).not.toHaveBeenCalled();
  await endSession('s3');
  expect(end).toHaveBeenCalledWith('s3');
});

it('signs out everywhere else', async () => {
  vi.spyOn(api, 'listSessions').mockResolvedValue(sessions);
  const others = vi.spyOn(api, 'endOtherSessions').mockResolvedValue();
  dispose?.();
  document.body.innerHTML = '';
  const host = document.createElement('div');
  document.body.appendChild(host);
  dispose = render(() => <SettingsModal />, host);
  await flush();
  button('Sign out everywhere else').click();
  await flush();
  expect(others).toHaveBeenCalled();
});

it('has no close-to-tray toggle: the close button always keeps FinchSpeak in the tray', async () => {
  button('Appearance').click();
  await flush();
  expect(document.body.textContent).not.toContain('Close button hides to the tray');
});

it('offers a theme from the new picture; Apply sets the custom colors, Not now keeps the theme', async () => {
  const colors = { background: '#101010', sidebar: '#202020', surface: '#303030', text: '#f0f0f0', accent: '#ff8800', signal: '#00ff88' };
  const themes = await import('../../stores/themeColors');
  vi.spyOn(themes, 'paletteFromImage').mockResolvedValue(colors);
  vi.stubGlobal('createImageBitmap', vi.fn().mockResolvedValue({ width: 200, height: 200, close: () => undefined }));
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue({ clearRect: () => undefined, drawImage: () => undefined } as never);
  vi.spyOn(HTMLCanvasElement.prototype, 'toBlob').mockImplementation((cb) => cb(new Blob(['x'], { type: 'image/png' })));
  vi.spyOn(api, 'uploadAvatar').mockResolvedValue({ ...me, avatar_url: '/a?v=1' } as never);
  const before = prefs().appearance.theme;
  const upload = async () => {
    setProfileOpen(true);
    await flush();
    const input = document.querySelector<HTMLInputElement>('input[type=file]')!;
    Object.defineProperty(input, 'files', { value: [new File(['x'], 'a.png', { type: 'image/png' })], configurable: true });
    input.dispatchEvent(new Event('change', { bubbles: true }));
    await flush();
    button('Use this picture').click();
    await flush(); await flush();
  };
  await upload();
  expect(document.body.textContent).toContain('Match the theme to your picture?');
  expect(document.querySelectorAll('[data-testid=theme-offer-swatches] i').length).toBe(6);
  button('Not now').click();
  await flush();
  expect(prefs().appearance.theme).toBe(before);
  expect(document.body.textContent).not.toContain('Match the theme to your picture?');
  await upload();
  button('Apply theme').click();
  await flush();
  expect(prefs().appearance.theme).toBe('user');
  expect(prefs().appearance.customColors).toEqual(colors);
  vi.unstubAllGlobals();
});
