import { render } from 'solid-js/web';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => undefined), Channel: class { onmessage: unknown = null; } }));

import { api } from '../../services/api';
import { setCurrentUser } from '../../stores/authStore';
import { applyMusicState, resetMusic } from '../../stores/musicStore';
import { resetSessionState, setKnownUsers, setServerFeatures } from '../../stores/sessionStore';
import { setActiveVoiceChannel } from '../../stores/voiceStore';
import { setNestView } from '../../stores/nestView';
import { DEFAULT_PREFS, setPrefs } from '../../stores/settingsStore';
import { clearToasts } from '../../ui';
import type { Channel, MusicQueueEntry, MusicRoomState, MusicTrack, User } from '../../types';
import { Dock } from '../voice/Dock';
import { MusicPanel } from './MusicPanel';

const me: User = { id: 'alice', username: 'alice', display_name: 'Alice', role: 'member' } as User;
const bob: User = { id: 'bob', username: 'bob', display_name: 'Bob', role: 'member' } as User;
const dave: User = { id: 'dave', username: 'dave', display_name: 'Dave', role: 'admin' } as User;
const room = { id: 'room', name: 'Lounge', channel_type: 'voice' } as unknown as Channel;

const track = (id: string, title: string, artist: string | null, uploader = 'bob'): MusicTrack => ({
  id, uploader_id: uploader, title, artist, duration_ms: 200_000, size_bytes: 1000, format: 'mp3', temporary: false, created_at: 0,
});
const entry = (entryId: string, item: MusicTrack, addedBy: string): MusicQueueEntry => ({ entry_id: entryId, track: item, added_by: addedBy });

let dispose: (() => void) | undefined;
let host: HTMLElement | undefined;

const mount = (view: () => unknown) => {
  host = document.createElement('div');
  document.body.appendChild(host);
  dispose = render(view as () => never, host);
  return host;
};
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));
const buttonNamed = (scope: ParentNode, text: string) =>
  Array.from(scope.querySelectorAll<HTMLButtonElement>('button')).find((b) => (b.textContent ?? '').trim() === text || b.getAttribute('aria-label') === text);
const allLabelled = (scope: ParentNode, label: string) => scope.querySelectorAll(`[aria-label="${label}"]`);

/** Puts the room's music state in place: the room I am in, and the given queue and track. */
function setRoomMusic(state: MusicRoomState) {
  applyMusicState({ channel_id: 'room', state, server_ms: 0 });
}

beforeEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
  setPrefs({ ...DEFAULT_PREFS, appearance: { ...DEFAULT_PREFS.appearance, language: 'en' } });
  resetSessionState();
  resetMusic();
  clearToasts();
  setCurrentUser(me);
  setKnownUsers([me, bob, dave]);
  setActiveVoiceChannel(room);
  setServerFeatures(['music']);
  vi.spyOn(api, 'musicSetVolume').mockResolvedValue();
  vi.spyOn(api, 'musicCommand').mockResolvedValue();
  vi.spyOn(api, 'musicLibrary').mockResolvedValue([]);
});

afterEach(() => {
  dispose?.();
  dispose = undefined;
  host?.remove();
  host = undefined;
  resetMusic();
  setActiveVoiceChannel(null);
  setServerFeatures([]);
  setNestView(false);
});

it('shows the empty text when nothing plays and the queue is empty', async () => {
  setRoomMusic({ queue: [], playing: null });
  const view = mount(() => <MusicPanel onClose={() => {}} />);
  await flush();
  expect(view.textContent).toContain('Nothing is playing. Add a track to start.');
});

it('a member sees the remove button only on their own queue entries; staff sees all of them', async () => {
  const queue = [entry('e1', track('t1', 'One', 'A'), 'alice'), entry('e2', track('t2', 'Two', 'B'), 'bob')];
  setRoomMusic({ queue, playing: null });

  setCurrentUser(me);
  const asMember = mount(() => <MusicPanel onClose={() => {}} />);
  await flush();
  expect(allLabelled(asMember, 'Remove from queue')).toHaveLength(1);
  dispose?.();
  host?.remove();

  setCurrentUser(dave);
  const asStaff = mount(() => <MusicPanel onClose={() => {}} />);
  await flush();
  expect(allLabelled(asStaff, 'Remove from queue')).toHaveLength(2);
});

it('a member deletes only their own library tracks; staff deletes any', async () => {
  vi.mocked(api.musicLibrary).mockResolvedValue([track('t1', 'One', 'A', 'alice'), track('t2', 'Two', 'B', 'bob')]);
  setRoomMusic({ queue: [], playing: null });

  setCurrentUser(me);
  const asMember = mount(() => <MusicPanel onClose={() => {}} />);
  await flush();
  expect(allLabelled(asMember, 'Delete track')).toHaveLength(1);
  dispose?.();
  host?.remove();

  setCurrentUser(dave);
  const asStaff = mount(() => <MusicPanel onClose={() => {}} />);
  await flush();
  expect(allLabelled(asStaff, 'Delete track')).toHaveLength(2);
});

it('Stop for everyone is for staff only and sends the clear command', async () => {
  setRoomMusic({ queue: [], playing: { entry: entry('e1', track('t1', 'One', 'A'), 'bob'), position_ms: 0, at_server_ms: 0, paused: false } });

  setCurrentUser(me);
  const asMember = mount(() => <MusicPanel onClose={() => {}} />);
  await flush();
  expect(buttonNamed(asMember, 'Stop for everyone')).toBeUndefined();
  dispose?.();
  host?.remove();

  setCurrentUser(dave);
  const asStaff = mount(() => <MusicPanel onClose={() => {}} />);
  await flush();
  buttonNamed(asStaff, 'Stop for everyone')!.click();
  await flush();
  expect(api.musicCommand).toHaveBeenCalledWith({ kind: 'clear' });
});

it('the library search matches the title and the artist', async () => {
  vi.mocked(api.musicLibrary).mockResolvedValue([track('t1', 'Yellow Submarine', 'The Beatles'), track('t2', 'Kind of Blue', 'Miles Davis')]);
  setRoomMusic({ queue: [], playing: null });
  const view = mount(() => <MusicPanel onClose={() => {}} />);
  await flush();
  const search = view.querySelector<HTMLInputElement>('.music-add .field input')!;

  search.value = 'beatles';
  search.dispatchEvent(new Event('input', { bubbles: true }));
  await flush();
  expect(view.textContent).toContain('Yellow Submarine');
  expect(view.textContent).not.toContain('Kind of Blue');

  search.value = 'blue';
  search.dispatchEvent(new Event('input', { bubbles: true }));
  await flush();
  expect(view.textContent).toContain('Kind of Blue');
  expect(view.textContent).not.toContain('Yellow Submarine');
});

it('a refused upload shows the server reason in its translated text', async () => {
  vi.spyOn(api, 'musicUpload').mockRejectedValue(new Error('Music file is too large (limit 50 MB)'));
  setRoomMusic({ queue: [], playing: null });
  const view = mount(() => <MusicPanel onClose={() => {}} />);
  await flush();
  buttonNamed(view, 'From my computer')!.click();
  await flush();

  const input = view.querySelector<HTMLInputElement>('input[type="file"]')!;
  const file = new File(['x'], 'big.mp3', { type: 'audio/mpeg' });
  Object.defineProperty(input, 'files', { value: [file] });
  input.dispatchEvent(new Event('change'));
  await flush();
  await flush();

  expect(view.textContent).toContain('The music file is too large');
});

it('a finished upload from my computer goes straight into the queue', async () => {
  vi.spyOn(api, 'musicUpload').mockResolvedValue(track('t9', 'Fresh', 'New'));
  setRoomMusic({ queue: [], playing: null });
  const view = mount(() => <MusicPanel onClose={() => {}} />);
  await flush();
  buttonNamed(view, 'From my computer')!.click();
  await flush();

  const input = view.querySelector<HTMLInputElement>('input[type="file"]')!;
  Object.defineProperty(input, 'files', { value: [new File(['x'], 'fresh.mp3', { type: 'audio/mpeg' })] });
  input.dispatchEvent(new Event('change'));
  await flush();
  await flush();

  expect(api.musicUpload).toHaveBeenCalledWith(expect.any(File), false);
  expect(api.musicCommand).toHaveBeenCalledWith({ kind: 'enqueue', track_id: 't9' });
});

it('the computer tab says the file is removed after it plays', async () => {
  setRoomMusic({ queue: [], playing: null });
  const view = mount(() => <MusicPanel onClose={() => {}} />);
  await flush();
  buttonNamed(view, 'From my computer')!.click();
  await flush();
  expect(view.textContent).toContain('The file is sent to the server for this session and removed after it plays.');
});

it('the panel renders in the Finch layout too', async () => {
  setNestView(true);
  setRoomMusic({ queue: [], playing: null });
  const view = mount(() => <MusicPanel onClose={() => {}} />);
  await flush();
  expect(view.textContent).toContain('Nothing is playing. Add a track to start.');
});

it('seeks once when the slider is released, not on every move', async () => {
  const command = vi.spyOn(api, 'musicCommand').mockResolvedValue();
  const song = track('t1', 'Song', null);
  setRoomMusic({ queue: [], playing: { entry: entry('e1', song, 'bob'), position_ms: 0, at_server_ms: 0, paused: true } });
  mount(() => <MusicPanel onClose={() => {}} />);
  await flush();
  const slider = document.querySelector<HTMLInputElement>('[aria-label="Track progress"]')!;
  for (const value of ['10', '20', '30']) {
    slider.value = value;
    slider.dispatchEvent(new Event('input', { bubbles: true }));
  }
  expect(command).not.toHaveBeenCalledWith(expect.objectContaining({ kind: 'seek' }));
  slider.dispatchEvent(new Event('change', { bubbles: true }));
  expect(command).toHaveBeenCalledTimes(1);
  expect(command).toHaveBeenCalledWith({ kind: 'seek', position_ms: 30_000 });
});
