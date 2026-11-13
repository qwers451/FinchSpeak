import { render } from 'solid-js/web';
import { t } from '../../i18n';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { api } from '../../services/api';
import type { SoundInfo, User } from '../../types';
import { setCurrentUser } from '../../stores/authStore';
import { resetSoundboardForTests, setSoundboardOpen } from '../../stores/soundboardStore';
import { SoundboardPanel } from './SoundboardPanel';

const mine: SoundInfo = { id: 'mine', name: 'Гудок', emoji: '📯', uploader_id: 'alice', duration_ms: 900 };
const theirs: SoundInfo = { id: 'theirs', name: 'Гонг', emoji: null, uploader_id: 'bob', duration_ms: 2000 };
let dispose: (() => void) | undefined;

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));
const tiles = () => [...document.querySelectorAll<HTMLElement>('.soundboard-tile')];

beforeEach(async () => {
  resetSoundboardForTests();
  setCurrentUser({ id: 'alice', username: 'alice', display_name: 'Alice', role: 'member' } as unknown as User);
  vi.spyOn(api, 'soundboard').mockResolvedValue([mine, theirs]);
  vi.spyOn(api, 'playBoardSound').mockResolvedValue();
  vi.spyOn(api, 'deleteBoardSound').mockResolvedValue();
  const root = document.createElement('div');
  document.body.append(root);
  dispose = render(() => <SoundboardPanel />, root);
  setSoundboardOpen(true);
  await flush();
});

afterEach(() => {
  setSoundboardOpen(false);
  dispose?.();
  document.body.innerHTML = '';
  vi.restoreAllMocks();
});

it('lists the board when it opens and shows each sound with its emoji', () => {
  expect(api.soundboard).toHaveBeenCalled();
  expect(tiles().map((tile) => tile.textContent?.trim())).toEqual(['📯Гудок', '♪Гонг']);
});

it('searches by name', async () => {
  const search = document.querySelector<HTMLInputElement>('input[type="search"]');
  if (!search) throw new Error('no search box');
  search.value = 'гон';
  search.dispatchEvent(new Event('input', { bubbles: true }));
  await flush();
  expect(tiles().map((tile) => tile.textContent?.trim())).toEqual(['♪Гонг']);
});

it('plays a sound when its tile is clicked', async () => {
  tiles()[1].querySelector<HTMLButtonElement>('.soundboard-play')?.click();
  await flush();
  expect(api.playBoardSound).toHaveBeenCalledWith('theirs');
});

it('offers removal only for my own sounds, unless I am a moderator', async () => {
  const removeButtons = () => document.querySelectorAll(`button[aria-label="${t('Remove sound')}"]`);
  expect(removeButtons()).toHaveLength(1);

  setCurrentUser({ id: 'alice', username: 'alice', display_name: 'Alice', role: 'moderator' } as unknown as User);
  await flush();
  expect(removeButtons()).toHaveLength(2);
  removeButtons()[1].dispatchEvent(new MouseEvent('click', { bubbles: true }));
  await flush();
  expect(api.deleteBoardSound).toHaveBeenCalledWith('theirs');
});

it('says the board is empty when there are no sounds', async () => {
  vi.mocked(api.soundboard).mockResolvedValue([]);
  setSoundboardOpen(false);
  setSoundboardOpen(true);
  await flush();
  expect(document.body.textContent).toContain(t('No sounds yet'));
});
