import { render } from 'solid-js/web';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => undefined), Channel: class { onmessage: unknown = null; } }));

import { api } from '../../services/api';
import { auditEntries, describeAudit, overview, setAdminOpen, setAuditEntries, setInvites, setMembers, setOverview } from '../../stores/adminStore';
import { setCurrentUser } from '../../stores/authStore';
import { resetSessionState, sessionHandlers, setKnownUsers, setLocalPeerId } from '../../stores/sessionStore';
import { setActiveVoiceChannel, setSpeakingPeers, setVoiceParticipants } from '../../stores/voiceStore';
import { clearToasts } from '../../ui';
import { DEFAULT_PREFS, setPrefs } from '../../stores/settingsStore';
import { VoicePanel } from '../VoicePanel';
import { AdminModal } from './AdminModal';

const person = (id: string, role: 'admin' | 'moderator' | 'member', extra = {}) => ({ id, username: id, display_name: id.toUpperCase(), role, banned: false, online: true, last_seen: null, ...extra });
const boss = { id: 'boss', username: 'boss', display_name: 'Boss', role: 'admin' as const };
const mod = { id: 'moddy', username: 'moddy', display_name: 'Moddy', role: 'moderator' as const };

let dispose: (() => void) | undefined;
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));
const button = (text: string, within: ParentNode = document) => Array.from(within.querySelectorAll('button')).find((b) => b.textContent?.trim() === text || b.getAttribute('aria-label') === text) as HTMLButtonElement;
const dialog = () => document.querySelectorAll('[role="dialog"]')[document.querySelectorAll('[role="dialog"]').length - 1] as HTMLElement;

function open(user: typeof boss | typeof mod) {
  setCurrentUser(user);
  setAdminOpen(true);
  const host = document.createElement('div');
  document.body.appendChild(host);
  dispose = render(() => <AdminModal />, host);
}

beforeEach(() => {
  setPrefs({ ...DEFAULT_PREFS, appearance: { ...DEFAULT_PREFS.appearance, language: 'en' } });
  vi.restoreAllMocks();
  resetSessionState();
  clearToasts();
  setInvites([]);
  setAuditEntries([]);
  setOverview(null);
  setMembers([person('boss', 'admin'), person('moddy', 'moderator'), person('alice', 'member'), person('bob', 'member', { online: false, last_seen: Math.floor(Date.now() / 1000) - 7200 })]);
  vi.spyOn(api, 'adminUsers').mockImplementation(async () => [person('boss', 'admin'), person('moddy', 'moderator'), person('alice', 'member'), person('bob', 'member', { online: false, last_seen: Math.floor(Date.now() / 1000) - 7200 })]);
  vi.spyOn(api, 'listInvites').mockResolvedValue([]);
  vi.spyOn(api, 'auditLog').mockResolvedValue([]);
});
afterEach(() => {
  dispose?.();
  dispose = undefined;
  setAdminOpen(false);
  document.body.innerHTML = '';
});

it('an admin sees every tab, a moderator only the members and the log', async () => {
  open(boss);
  await flush();
  expect(Array.from(document.querySelectorAll('.setnav .si')).map((b) => b.textContent)).toEqual(['Members', 'Invitations', 'Channels', 'Server', 'Log']);
  dispose?.();
  document.body.innerHTML = '';
  open(mod);
  await flush();
  expect(Array.from(document.querySelectorAll('.setnav .si')).map((b) => b.textContent)).toEqual(['Members', 'Log']);
});

it('lists members with role, state and last visit', async () => {
  open(boss);
  await flush();
  const rows = Array.from(document.querySelectorAll('tbody tr'));
  expect(rows).toHaveLength(4);
  expect(rows[3].textContent).toContain('Offline');
  expect(rows[3].textContent).toContain('2 h ago');
  expect(rows[2].textContent).toContain('Online');
});

it('kicking asks first and says what happens', async () => {
  const kick = vi.spyOn(api, 'kickUser').mockResolvedValue();
  open(boss);
  await flush();
  const alice = Array.from(document.querySelectorAll('tbody tr')).find((r) => r.textContent?.includes('ALICE'))!;
  button('Kick', alice).click();
  await flush();
  expect(dialog().textContent).toContain('is disconnected and has to sign in again');
  expect(kick).not.toHaveBeenCalled();
  button('Kick', dialog()).click();
  await flush();
  expect(kick).toHaveBeenCalledWith('alice');
});

it('banning offers durations, confirms, and passes the length on', async () => {
  const ban = vi.spyOn(api, 'banUser').mockResolvedValue();
  open(boss);
  await flush();
  const alice = Array.from(document.querySelectorAll('tbody tr')).find((r) => r.textContent?.includes('ALICE'))!;
  button('Ban…', alice).click();
  await flush();
  button('1 day', dialog()).click();
  await flush();
  expect(dialog().textContent).toContain('cannot sign in for 1 day');
  button('Ban', dialog()).click();
  await flush();
  expect(ban).toHaveBeenCalledWith('alice', 86_400);
});

it('a permanent ban says it lasts until it is lifted, not "for until"', async () => {
  const ban = vi.spyOn(api, 'banUser').mockResolvedValue();
  open(boss);
  await flush();
  const alice = Array.from(document.querySelectorAll('tbody tr')).find((r) => r.textContent?.includes('ALICE'))!;
  button('Ban…', alice).click();
  await flush();
  button('For good', dialog()).click();
  await flush();
  expect(dialog().textContent).toContain('cannot sign in until you lift the ban.');
  expect(dialog().textContent).not.toContain('for until');
  button('Ban', dialog()).click();
  await flush();
  expect(ban).toHaveBeenCalledWith('alice');
});

it('a moderator can kick members but is not offered bans, roles or staff', async () => {
  open(mod);
  await flush();
  const rows = Array.from(document.querySelectorAll('tbody tr'));
  const text = (name: string) => rows.find((r) => r.textContent?.includes(name))!;
  expect(button('Kick', text('ALICE'))).toBeTruthy();
  expect(button('Ban…', text('ALICE'))).toBeFalsy();
  expect(text('BOSS').querySelector('button')).toBeNull();
  expect(text('ALICE').querySelector('.role-pill')).toBeNull();
});

it('an admin changes a role through a confirmation', async () => {
  const role = vi.spyOn(api, 'setUserRole').mockResolvedValue();
  open(boss);
  await flush();
  const alice = Array.from(document.querySelectorAll('tbody tr')).find((r) => r.textContent?.includes('ALICE'))!;
  button('Moderator', alice).click();
  await flush();
  expect(dialog().textContent).toContain('can kick people, silence them in voice and delete messages');
  button('Moderator', dialog()).click();
  await flush();
  expect(role).toHaveBeenCalledWith('alice', 'moderator');
});

it('an invitation is shown big with a copy button and states', async () => {
  vi.spyOn(api, 'createInvite').mockResolvedValue({ code: 'K7M2QX9TPW4B', created_at: 1, expires_at: 2, max_uses: 3, uses: 0 });
  (api.listInvites as ReturnType<typeof vi.fn>).mockResolvedValue([
    { code: 'USED', created_at: 1, expires_at: 9_999_999_999, max_uses: 1, uses: 1 },
    { code: 'OLD', created_at: 1, expires_at: 10, max_uses: 1, uses: 0 },
    { code: 'LIVE', created_at: 1, expires_at: 9_999_999_999, max_uses: 5, uses: 1 },
  ]);
  open(boss);
  await flush();
  Array.from(document.querySelectorAll<HTMLElement>('.setnav .si')).find((b) => b.textContent === 'Invitations')!.click();
  await flush();
  await flush();
  expect(Array.from(document.querySelectorAll('tbody tr')).map((r) => r.querySelector('.tag')?.textContent)).toEqual(['Used up', 'Expired', 'Active']);
  button('Create invitation').click();
  await flush();
  await flush();
  expect(document.querySelector('.invite-code')?.textContent).toBe('K7M2QX9TPW4B');
});

it('the log reads in words, and the server page switches registration', async () => {
  (api.auditLog as ReturnType<typeof vi.fn>).mockResolvedValue([
    { id: 2, at: Math.floor(Date.now() / 1000) - 60, actor_name: 'boss', action: 'ban', target: 'bob', details: 'permanent' },
    { id: 1, at: Math.floor(Date.now() / 1000) - 3600, actor_name: 'moddy', action: 'voice_mute', target: 'alice', details: null },
  ]);
  const registration = vi.spyOn(api, 'setRegistration').mockResolvedValue();
  vi.spyOn(api, 'serverOverview').mockResolvedValue({
    version: '1.0.0', uptime_secs: 7200, certificate_fingerprint: 'AA:BB', quic_port: 4433, http_port: 8443, registration: 'invite',
    backups_count: 3, last_backup_at: 1_700_000_000, last_backup_bytes: 2048, backup_dir: './data/backups', uploads_bytes: 5 * 1024 * 1024, log_dir: './data/logs',
  });
  open(boss);
  await flush();
  Array.from(document.querySelectorAll<HTMLElement>('.setnav .si')).find((b) => b.textContent === 'Log')!.click();
  await flush();
  await flush();
  expect(document.body.textContent).toContain('boss banned bob');
  expect(document.body.textContent).toContain('moddy silenced alice in the voice room');

  Array.from(document.querySelectorAll<HTMLElement>('.setnav .si')).find((b) => b.textContent === 'Server')!.click();
  await flush();
  await flush();
  expect(document.body.textContent).toContain('QUIC 4433');
  expect(document.body.textContent).toContain('AA:BB');
  expect(overview()?.registration).toBe('invite');
  document.querySelector<HTMLElement>('[aria-label="Anyone can register"]')!.click();
  await flush();
  expect(registration).toHaveBeenCalledWith('open');
});

it('describes log entries in plain words', () => {
  expect(describeAudit({ id: 1, at: 1, actor_name: 'a', action: 'role_changed', target: 'bob', details: 'member -> moderator' })).toBe('changed the role of bob (member -> moderator)');
  expect(describeAudit({ id: 1, at: 1, actor_name: 'a', action: 'something_new', target: null, details: null })).toBe('something_new');
  expect(auditEntries()).toEqual([]);
});

// ---- voice moderation in the room

const tileUser = (id: string, role: 'admin' | 'moderator' | 'member') => ({ id, username: id, display_name: id.toUpperCase(), role });

function room(viewer: typeof boss | typeof mod) {
  setCurrentUser(viewer);
  setKnownUsers([viewer, tileUser('alice', 'member'), tileUser('boss', 'admin')]);
  setLocalPeerId(1);
  setActiveVoiceChannel({ id: 'room', short_id: 2, name: 'Voice', channel_type: 'voice', position: 0, bitrate_kbps: 64 });
  setVoiceParticipants([
    { user_id: viewer.id, peer_id: 1, username: viewer.username, channel_id: 'room' },
    { user_id: 'alice', peer_id: 2, username: 'alice', channel_id: 'room' },
    { user_id: 'boss', peer_id: 3, username: 'boss', channel_id: 'room' },
  ]);
  setSpeakingPeers(new Set<number>());
  const host = document.createElement('div');
  document.body.appendChild(host);
  dispose = render(() => <VoicePanel />, host);
  return host;
}

it('a moderator can silence and remove a member in the room, but not an admin', async () => {
  const moderate = vi.spyOn(api, 'moderateVoice').mockResolvedValue();
  const host = room({ ...mod });
  const alice = host.querySelector('[data-peer-id="2"]')!;
  button('Silence', alice).click();
  button('Remove', alice).click();
  await flush();
  expect(moderate.mock.calls).toEqual([['alice', 'mute'], ['alice', 'kick']]);
  expect(host.querySelector('[data-peer-id="3"] .mod-actions')).toBeNull();
  expect(host.querySelector('[data-peer-id="1"] .mod-actions')).toBeNull();
});

it('an admin may moderate everybody; a server-muted person shows the badge and can be allowed again', async () => {
  const moderate = vi.spyOn(api, 'moderateVoice').mockResolvedValue();
  const host = room({ ...boss });
  expect(host.querySelector('[data-peer-id="3"] .mod-actions')).not.toBeNull();
  sessionHandlers['server-mute-changed']({ peer_id: 2, server_muted: true });
  await flush();
  const alice = host.querySelector('[data-peer-id="2"]')!;
  expect(alice.querySelector('[title="Silenced by a moderator"]')).not.toBeNull();
  button('Allow to speak', alice).click();
  await flush();
  expect(moderate).toHaveBeenCalledWith('alice', 'unmute');
});

it('members get no moderation controls', () => {
  const host = room({ id: 'carl', username: 'carl', display_name: 'Carl', role: 'member' } as never);
  expect(host.querySelector('.mod-actions')).toBeNull();
});
