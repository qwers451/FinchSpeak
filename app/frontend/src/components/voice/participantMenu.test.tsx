import { render } from 'solid-js/web';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { ContextMenuHost, closeContextMenu } from '../../ui/ContextMenu';
import { setCurrentUser } from '../../stores/authStore';
import { setKnownUsers, setLocalPeerId } from '../../stores/sessionStore';
import { setVoiceParticipants } from '../../stores/voiceStore';
import { setChannels } from '../../stores/chatStore';
import { api } from '../../services/api';
import { ParticipantTile } from './ParticipantTile';
import { Sidebar } from '../Sidebar';
import { MembersPanel } from '../MembersPanel';

const me = { id: 'me', username: 'me', display_name: 'Me', role: 'member' as const };
const bob = { id: 'bob', username: 'bob', display_name: 'Bob', role: 'member' as const, status: 'online' as const };
const participant = { user_id: bob.id, username: bob.username, peer_id: 2, channel_id: 'room' };
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));
let dispose: (() => void) | undefined;
beforeEach(() => {
  setCurrentUser(me);
  setKnownUsers([me, bob]);
  setLocalPeerId(1);
  setVoiceParticipants([participant]);
  setChannels([{ id: 'room', name: 'Room', channel_type: 'voice', short_id: 1, position: 0, bitrate_kbps: 64 }]);
});
afterEach(() => { closeContextMenu(); dispose?.(); document.body.innerHTML = ''; vi.restoreAllMocks(); });

function mount() {
  const root = document.createElement('div'); document.body.append(root);
  dispose = render(() => <><ContextMenuHost /><ParticipantTile participant={participant} isMe={false} avatarSize={72} /><Sidebar /><MembersPanel /></>, root);
  return root;
}

it('opens shared volume and mute actions from tile, sidebar and member row with mouse or keyboard', async () => {
  const root = mount();
  const surfaces = [root.querySelector('.vc')!, root.querySelector('.vp')!, [...root.querySelectorAll('.members .mem')].find((e) => e.textContent?.includes('Bob'))!];
  for (const [index, surface] of surfaces.entries()) {
    (surface as HTMLElement).focus();
    surface.dispatchEvent(index === 0 ? new MouseEvent('contextmenu', { bubbles: true, cancelable: true }) :
      new KeyboardEvent('keydown', { key: index === 1 ? 'ContextMenu' : 'F10', shiftKey: index === 2, bubbles: true, cancelable: true }));
    await flush();
    const menu = document.querySelector('[role="menu"]')!;
    expect(menu.textContent).toContain('Bob');
    expect(menu.querySelector('[aria-label="Volume"]')).toBeTruthy();
    expect(menu.textContent).toContain('Mute for me');
    expect(menu.textContent).not.toContain('Silence for everyone');
    closeContextMenu();
  }
  root.querySelector<HTMLButtonElement>('[aria-label="Actions for Bob"]')!.click();
  await flush();
  expect(document.querySelector('[role="menu"]')).toBeTruthy();
});

it('moderators can act on members, and stale permissions are checked again on selection', async () => {
  setCurrentUser({ ...me, role: 'moderator' });
  const moderate = vi.spyOn(api, 'moderateVoice').mockResolvedValue();
  const root = mount();
  root.querySelector('.vc')!.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
  await flush();
  const silence = [...document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')].find((e) => e.textContent?.includes('Silence for everyone'))!;
  expect(silence).toBeTruthy();
  setKnownUsers([me, { ...bob, role: 'admin' }]);
  silence.click();
  expect(moderate).not.toHaveBeenCalled();
  root.querySelector('.vc')!.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
  await flush();
  expect(document.querySelector('[role="menu"]')?.textContent).not.toContain('Silence for everyone');
  setCurrentUser({ ...me, role: 'admin' });
  closeContextMenu();
  root.querySelector('.vc')!.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
  await flush();
  expect(document.querySelector('[role="menu"]')?.textContent).toContain('Remove from the room');
});
