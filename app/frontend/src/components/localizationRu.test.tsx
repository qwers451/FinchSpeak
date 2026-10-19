import { render } from 'solid-js/web';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => undefined), Channel: class { onmessage: unknown = null; } }));

import { api } from '../services/api';
import { setAdminOpen, setAuditEntries, setInvites, setMembers, setOverview } from '../stores/adminStore';
import { setCurrentUser } from '../stores/authStore';
import { setChannels } from '../stores/chatStore';
import { resetSessionState, setKnownUsers, setLocalPeerId } from '../stores/sessionStore';
import { DEFAULT_PREFS, setPrefs } from '../stores/settingsStore';
import { setScreenSources } from '../stores/screenState';
import { setActiveVoiceChannel, setPingRtt, setSpeakingPeers, setVoiceParticipants } from '../stores/voiceStore';
import { clearToasts } from '../ui';
import { AdminModal } from './admin/AdminModal';
import { Attachments } from './chat/AttachmentView';
import { Overlays } from './overlays/Overlays';
import { SharePickerModal } from './screen/SharePickerModal';
import { VoicePanel } from './VoicePanel';

/** English words that must not show up on screen when the interface language is Russian. */
const ENGLISH = /\b(Ban|Active|Expired|Used up|1 day|1 week|For good|Kick|Download|Open|No preview|unknown|You|sign in|disconnected)\b/;

const boss = { id: 'boss', username: 'boss', display_name: 'Boss', role: 'admin' as const };
const person = (id: string, role: 'admin' | 'moderator' | 'member') => ({ id, username: id, display_name: id.toUpperCase(), role, banned: false, online: true, last_seen: null });
let dispose: (() => void) | undefined;
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));
const mount = (view: () => unknown) => {
  const host = document.createElement('div');
  document.body.appendChild(host);
  dispose = render(view as () => never, host);
  return host;
};
const dialog = () => document.querySelectorAll('[role="dialog"]')[document.querySelectorAll('[role="dialog"]').length - 1] as HTMLElement;
const buttonMatching = (pattern: RegExp, within: ParentNode = document) => Array.from(within.querySelectorAll('button')).find((b) => pattern.test(b.textContent?.trim() ?? '') || pattern.test(b.getAttribute('aria-label') ?? '')) as HTMLButtonElement;
const tab = (pattern: RegExp) => Array.from(document.querySelectorAll<HTMLElement>('.setnav .si')).find((b) => pattern.test(b.textContent ?? ''))!;

beforeEach(() => {
  vi.restoreAllMocks();
  resetSessionState();
  clearToasts();
  setPrefs({ ...DEFAULT_PREFS, appearance: { ...DEFAULT_PREFS.appearance, language: 'ru' } });
  setCurrentUser(boss);
  setKnownUsers([boss]);
  setInvites([]);
  setAuditEntries([]);
  setOverview(null);
  setMembers([person('boss', 'admin'), person('alice', 'member')]);
  vi.spyOn(api, 'adminUsers').mockImplementation(async () => [person('boss', 'admin'), person('alice', 'member')]);
  vi.spyOn(api, 'listInvites').mockResolvedValue([]);
  vi.spyOn(api, 'auditLog').mockResolvedValue([]);
});
afterEach(() => {
  dispose?.();
  dispose = undefined;
  setAdminOpen(false);
  setPingRtt(null);
  document.body.innerHTML = '';
});

function openAdmin() {
  setAdminOpen(true);
  mount(() => <AdminModal />);
}

it('ru: the ban dialog and its confirmation have no English durations or buttons', async () => {
  openAdmin();
  await flush();
  const alice = Array.from(document.querySelectorAll('tbody tr')).find((r) => r.textContent?.includes('ALICE'))!;
  buttonMatching(/^Заблокировать/, alice).click();
  await flush();
  expect(dialog().textContent).not.toMatch(ENGLISH);
  expect(Array.from(dialog().querySelectorAll('button')).map((b) => b.textContent?.trim())).toEqual(expect.arrayContaining(['1 час', '1 день', '1 неделя', 'Навсегда']));
  buttonMatching(/^1 день$/, dialog()).click();
  await flush();
  expect(dialog().textContent).not.toMatch(ENGLISH);
  expect(dialog().textContent).toContain('Заблокировать: ALICE?');
  expect(dialog().textContent).toContain('Срок блокировки: 1 день');
  expect(buttonMatching(/^Заблокировать$/, dialog())).toBeTruthy();
});

it('ru: kick, unban and role confirmations are explained in Russian', async () => {
  openAdmin();
  await flush();
  const alice = Array.from(document.querySelectorAll('tbody tr')).find((r) => r.textContent?.includes('ALICE'))!;
  buttonMatching(/^Исключить$/, alice).click();
  await flush();
  expect(dialog().textContent).not.toMatch(/\b(is disconnected|sign in|can come back)\b/);
  expect(dialog().textContent).toMatch(/[А-Яа-я]{4,}/);
  buttonMatching(/^Отмена$/, dialog()).click();
  await flush();
  buttonMatching(/^Модератор$/, alice).click();
  await flush();
  expect(dialog().textContent).not.toMatch(/\b(can kick|silence them|staff powers)\b/);
  buttonMatching(/^Отмена$/, dialog()).click();
  await flush();
  buttonMatching(/^Администратор$/, alice).click();
  await flush();
  expect(dialog().textContent).not.toMatch(/\b(every power|admin has)\b/);
});

it('ru: invitations show their state in Russian', async () => {
  (api.listInvites as ReturnType<typeof vi.fn>).mockResolvedValue([
    { code: 'USED', created_at: 1, expires_at: 9_999_999_999, max_uses: 1, uses: 1 },
    { code: 'OLD', created_at: 1, expires_at: 10, max_uses: 1, uses: 0 },
    { code: 'LIVE', created_at: 1, expires_at: 9_999_999_999, max_uses: 5, uses: 1 },
  ]);
  openAdmin();
  await flush();
  tab(/Приглашения/).click();
  await flush();
  await flush();
  expect(Array.from(document.querySelectorAll('tbody tr')).map((r) => r.querySelector('.tag')?.textContent)).toEqual(['Исчерпан', 'Истёк', 'Действует']);
});

it('ru: the channel deletion title and the server page have no English words', async () => {
  setChannels([
    { id: 'c1', short_id: 1, name: 'general', channel_type: 'text', position: 0, bitrate_kbps: 64 },
    { id: 'c2', short_id: 2, name: 'games', channel_type: 'text', position: 1, bitrate_kbps: 64 },
  ]);
  vi.spyOn(api, 'serverOverview').mockResolvedValue({
    version: '1.0.0', uptime_secs: 7200, certificate_fingerprint: null, quic_port: 4433, http_port: 8443, registration: 'invite',
    backups_count: 0, last_backup_at: null, last_backup_bytes: null, backup_dir: './data/backups', uploads_bytes: 5 * 1024 * 1024, log_dir: './data/logs',
  });
  openAdmin();
  await flush();
  tab(/Каналы/).click();
  await flush();
  buttonMatching(/^Удалить.*general/).click();
  await flush();
  expect(dialog().textContent).toContain('Удалить канал general?');
  expect(dialog().textContent).not.toMatch(/Delete/);
  buttonMatching(/^Отмена$/, dialog()).click();
  await flush();
  tab(/Сервер/).click();
  await flush();
  await flush();
  expect(document.body.textContent).toContain('неизвестно');
  expect(document.body.textContent).not.toMatch(/\bunknown\b/);
  expect(document.body.textContent).toContain('2 ч'); // uptime
  expect(document.body.textContent).toContain('5 МБ'); // uploaded files
  expect(document.body.textContent).not.toMatch(/\b\d+ (h|MB|KB|GB|min|d)\b/);
});

it('ru: the link quality in the voice room is named in Russian', async () => {
  setKnownUsers([boss]);
  setLocalPeerId(1);
  setActiveVoiceChannel({ id: 'room', short_id: 2, name: 'Voice', channel_type: 'voice', position: 0, bitrate_kbps: 64 });
  setVoiceParticipants([{ user_id: 'boss', peer_id: 1, username: 'boss', channel_id: 'room' }]);
  setSpeakingPeers(new Set<number>());
  const host = mount(() => <VoicePanel />);
  for (const [ms, word] of [[30, 'отличное'], [100, 'хорошее'], [200, 'среднее'], [400, 'плохое']] as const) {
    setPingRtt(ms);
    await flush();
    expect(host.textContent).toContain(`${ms} мс · ${word}`);
  }
  setActiveVoiceChannel(null);
  setVoiceParticipants([]);
});

it('ru: attachment buttons are named in Russian', () => {
  const host = mount(() => <Attachments attachments={[{ id: 'f1', filename: 'notes.txt', size_bytes: 2048, content_type: 'text/plain', url: '', has_thumb: false } as never]} />);
  const labels = Array.from(host.querySelectorAll('[aria-label]')).map((el) => el.getAttribute('aria-label'));
  expect(labels).toContain('Скачать notes.txt');
  expect(labels.join()).not.toMatch(/Download|Open/);
  expect(host.textContent).toContain('2 КБ · TXT');
});

it('ru: the shortcut list names the push-to-talk key in Russian', async () => {
  mount(() => <Overlays />);
  window.dispatchEvent(new KeyboardEvent('keydown', { key: '?', bubbles: true, cancelable: true }));
  await flush();
  expect(dialog().textContent).toContain('Клавиша активации микрофона');
  expect(dialog().textContent).not.toContain('Push-to-talk key');
});

it('ru: a screen without a preview says so in Russian', async () => {
  setScreenSources({ monitors: [{ id: 1, name: 'DISPLAY1', width: 2560, height: 1440, is_primary: true }], windows: [] });
  vi.spyOn(api, 'listScreenSources').mockResolvedValue({ monitors: [{ id: 1, name: 'DISPLAY1', width: 2560, height: 1440, is_primary: true }], windows: [] });
  vi.spyOn(api, 'captureThumbnail').mockRejectedValue(new Error('no capture'));
  mount(() => <SharePickerModal open onClose={() => {}} />);
  await flush();
  await flush();
  await flush();
  expect(document.body.textContent).toContain('Нет предпросмотра');
  expect(document.body.textContent).not.toContain('No preview');
});

it('ru: every kind of audit log entry reads in Russian', async () => {
  const entry = (id: number, action: string, target: string | null, details: string | null = null) => ({ id, at: Math.floor(Date.now() / 1000) - id, actor_name: 'boss', action, target, details });
  (api.auditLog as ReturnType<typeof vi.fn>).mockResolvedValue([
    entry(1, 'kick', 'bob'),
    entry(2, 'ban', 'bob', 'permanent'),
    entry(3, 'ban', 'bob', '3600'),
    entry(4, 'unban', 'bob'),
    entry(5, 'role_changed', 'bob', 'member -> moderator'),
    entry(6, 'message_deleted', '42', 'channel c1'),
    entry(7, 'channel_created', 'games', 'text'),
    entry(8, 'channel_deleted', 'games'),
    entry(9, 'invite_created', 'K7M2QX9TPW4B', '3 uses'),
    entry(10, 'invite_revoked', 'K7M2QX9TPW4B'),
    entry(11, 'registration_changed', null, 'open'),
    entry(12, 'backup', null),
    entry(13, 'voice_kick', 'bob'),
    entry(14, 'voice_mute', 'bob'),
    entry(15, 'voice_unmute', 'bob'),
  ]);
  openAdmin();
  await flush();
  tab(/Журнал/).click();
  await flush();
  await flush();
  const lines = Array.from(document.querySelectorAll('.setrow .fs13')).map((line) => line.textContent);
  expect(lines).toEqual([
    'boss исключил bob',
    'boss заблокировал bob',
    'boss заблокировал bob на время',
    'boss снял блокировку: bob',
    'boss изменил роль bob (member -> moderator)',
    'boss удалил чужое сообщение',
    'boss создал канал games',
    'boss удалил канал games',
    'boss создал приглашение K7M2QX9TPW4B',
    'boss отозвал приглашение K7M2QX9TPW4B',
    'boss установил регистрацию: open',
    'boss сделал резервную копию',
    'boss отключил bob от голосовой комнаты',
    'boss заглушил bob в голосовой комнате',
    'boss разрешил говорить bob в голосовой комнате',
  ]);
});
