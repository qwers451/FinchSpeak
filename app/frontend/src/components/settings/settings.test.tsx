import { render } from 'solid-js/web';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { api } from '../../services/api';
import { avatarSrc, clearAvatarCache, userIdOf } from '../../stores/avatarCache';
import { setCurrentUser, currentUser } from '../../stores/authStore';
import { setProfileMessage, setProfileOpen } from '../../stores/profileStore';
import { resetSessionState, sessionHandlers, setKnownUsers } from '../../stores/sessionStore';
import { DEFAULT_PREFS, prefs, setPrefs } from '../../stores/settingsStore';
import { setChannels } from '../../stores/chatStore';
import { clearToasts } from '../../ui';
import { checkAvatarFile, CROP_SIDE, placement } from './AvatarCropDialog';
import { SettingsModal } from './SettingsModal';

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
