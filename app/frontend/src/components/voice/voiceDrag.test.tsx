import { render } from 'solid-js/web';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { Sidebar } from '../Sidebar';
import { setCurrentUser } from '../../stores/authStore';
import { setKnownUsers, setLocalPeerId } from '../../stores/sessionStore';
import { setChannels } from '../../stores/chatStore';
import { setVoiceParticipants } from '../../stores/voiceStore';
import { api } from '../../services/api';
const nav = vi.hoisted(() => ({ join: vi.fn(async () => {}) }));
vi.mock('../../stores/voiceStore', async (original) => ({ ...await original<typeof import('../../stores/voiceStore')>(), joinVoice: nav.join }));
const me = { id: 'me', username: 'me', display_name: 'Me', role: 'member' as const };
const bob = { id: 'bob', username: 'bob', display_name: 'Bob', role: 'member' as const };
const staff = { id: 'staff', username: 'staff', display_name: 'Staff', role: 'moderator' as const };
const rooms = ['a', 'b'].map((id, index) => ({ id, name: id, short_id: index, channel_type: 'voice' as const, position: index, bitrate_kbps: 64 }));
const people = [me, bob, staff].map((user, index) => ({ user_id: user.id, peer_id: index + 1, username: user.username, channel_id: 'a' }));
const TYPE = 'application/x-finch-voice-user';
let dispose: (() => void) | undefined;
beforeEach(() => { vi.restoreAllMocks(); nav.join.mockClear(); setCurrentUser(me); setKnownUsers([me, bob, staff]); setLocalPeerId(1); setChannels(rooms); setVoiceParticipants(people); });
afterEach(() => { dispose?.(); document.body.innerHTML = ''; });
function mount() {
  const root = document.createElement('div'); document.body.append(root);
  dispose = render(() => <Sidebar />, root);
  return root;
}
function drag(element: Element, type: string, transfer: DataTransfer) {
  const event = new Event(type, { bubbles: true, cancelable: true });
  Object.defineProperty(event, 'dataTransfer', { value: transfer });
  element.dispatchEvent(event);
  return event;
}
function transfer(value: string, mime = TYPE) {
  const dt = new DataTransfer(); dt.setData(mime, value); return dt;
}
const payload = (index: number) => JSON.stringify({ userId: people[index].user_id, peerId: people[index].peer_id, from: 'a' });
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

it('only self or permitted targets are draggable, including after permissions change', () => {
  const root = mount();
  const rows = () => [...root.querySelectorAll<HTMLElement>('.vp')];
  expect(rows().map((r) => r.getAttribute('draggable') === 'true')).toEqual([true, false, false]);
  setCurrentUser({ ...me, role: 'moderator' });
  expect(rows().map((r) => r.getAttribute('draggable') === 'true')).toEqual([true, true, false]);
  const dt = new DataTransfer();
  drag(rows()[1], 'dragstart', dt);
  expect(JSON.parse(dt.getData(TYPE))).toEqual({ userId: 'bob', peerId: 2, from: 'a' });
});

it('moves other users with the exact peer and channel, and joins when dropping self', async () => {
  setCurrentUser({ ...me, role: 'admin' });
  const move = vi.spyOn(api, 'moveVoice').mockResolvedValue();
  const root = mount();
  const target = [...root.querySelectorAll('.ch')].find((e) => e.textContent?.includes('b'))!;
  const row = [...root.querySelectorAll('.vp')][1];
  const dt = transfer(payload(1));
  drag(row, 'dragstart', dt);
  expect(drag(target, 'dragover', dt).defaultPrevented).toBe(true);
  expect(target.classList.contains('drop-target')).toBe(true);
  drag(target, 'drop', dt);
  await flush();
  expect(move).toHaveBeenCalledWith('bob', 2, 'b');
  drag(target, 'drop', transfer(payload(0)));
  expect(nav.join).toHaveBeenCalledWith(rooms[1]);
});

it('rejects malformed, stale, same-room, unauthorized and file drops', async () => {
  const move = vi.spyOn(api, 'moveVoice').mockResolvedValue();
  const root = mount();
  const targets = [...root.querySelectorAll('.ch')];
  for (const value of ['{', '{}', JSON.stringify({ userId: 'bob', peerId: 99, from: 'a' }), payload(1)]) {
    drag(targets[1], 'drop', transfer(value));
  }
  drag(targets[0], 'drop', transfer(payload(0)));
  expect(drag(targets[1], 'dragover', transfer('file', 'Files')).defaultPrevented).toBe(false);
  drag(targets[1], 'drop', transfer(payload(0), 'Files'));
  await flush();
  expect(move).not.toHaveBeenCalled();
  expect(nav.join).not.toHaveBeenCalled();
});
