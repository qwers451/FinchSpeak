import { render } from 'solid-js/web';
import { afterEach, expect, it, vi } from 'vitest';
import { VoicePanel } from '../VoicePanel';
import { setCurrentUser } from '../../stores/authStore';
import { setKnownUsers, setLocalPeerId } from '../../stores/sessionStore';
import { setActiveVoiceChannel, setVoiceParticipants } from '../../stores/voiceStore';
import { setTileOrder, tileOrder } from '../../stores/tileOrderStore';
let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); document.body.innerHTML = ''; vi.restoreAllMocks(); setTileOrder([]); });
function drag(element: Element, type: string, transfer: DataTransfer, x = 0) {
  const event = new MouseEvent(type, { bubbles: true, cancelable: true, clientX: x });
  Object.defineProperty(event, 'dataTransfer', { value: transfer });
  element.dispatchEvent(event); return event;
}
it('drops before or after the target half while keeping self fixed and ignoring stale users', () => {
  const users = ['me', 'b', 'c'].map((id) => ({ id, username: id, display_name: id, role: 'member' as const }));
  setCurrentUser(users[0]); setKnownUsers(users); setLocalPeerId(1);
  setActiveVoiceChannel({ id: 'room', name: 'Room', channel_type: 'voice', position: 0, short_id: 1, bitrate_kbps: 64 });
  setVoiceParticipants(users.map((u, i) => ({ user_id: u.id, username: u.username, peer_id: i + 1, channel_id: 'room' })));
  const root = document.createElement('div'); document.body.append(root); dispose = render(() => <VoicePanel />, root);
  const tile = (id: number) => root.querySelector<HTMLElement>(`[data-peer-id="${id}"]`)!;
  const dt = new DataTransfer();
  expect(drag(tile(1), 'dragstart', dt).defaultPrevented).toBe(true);
  drag(tile(3), 'dragstart', dt);
  expect(dt.types).toContain('application/x-finch-tile');
  expect(drag(tile(1), 'dragover', dt).defaultPrevented).toBe(false);
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue({ x: 0, y: 0, left: 0, top: 0, width: 200, height: 100, right: 200, bottom: 100, toJSON() {} });
  drag(tile(2), 'drop', dt, 25);
  expect([...root.querySelectorAll('.vc')].map((e) => e.getAttribute('data-peer-id'))).toEqual(['1', '3', '2']);
  drag(tile(3), 'dragstart', dt);
  drag(tile(2), 'drop', dt, 175);
  expect(tileOrder()).toEqual(['b', 'c']);
  dt.setData('application/x-finch-tile', JSON.stringify({ userId: 'gone' }));
  drag(tile(2), 'drop', dt, 25);
  expect(tileOrder()).toEqual(['b', 'c']);
});
