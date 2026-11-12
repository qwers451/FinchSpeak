import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { bindLevel, updateVoiceLevels } from './levelStore';

beforeEach(() => vi.useFakeTimers());
afterEach(() => {
  updateVoiceLevels({ levels: [] });
  vi.useRealTimers();
});

it('writes the latest level to each bound avatar and expires it after 200 ms', () => {
  const tile = document.createElement('span');
  const sidebar = document.createElement('span');
  const unbindTile = bindLevel(tile, 7);
  updateVoiceLevels({ levels: [[7, 62]] });
  const unbindSidebar = bindLevel(sidebar, 7);
  expect(tile.style.getPropertyValue('--lvl')).toBe('0.62');
  expect(sidebar.style.getPropertyValue('--lvl')).toBe('0.62');

  vi.advanceTimersByTime(150);
  updateVoiceLevels({ levels: [[7, 45]] });
  vi.advanceTimersByTime(199);
  expect(tile.style.getPropertyValue('--lvl')).toBe('0.45');
  vi.advanceTimersByTime(1);
  expect(tile.style.getPropertyValue('--lvl')).toBe('0');
  expect(sidebar.style.getPropertyValue('--lvl')).toBe('0');
  unbindTile();
  unbindSidebar();
});

it('clears every active avatar on the final empty report', () => {
  const avatar = document.createElement('span');
  const unbind = bindLevel(avatar, 7);
  updateVoiceLevels({ levels: [[7, 88]] });
  updateVoiceLevels({ levels: [] });
  expect(avatar.style.getPropertyValue('--lvl')).toBe('0');
  unbind();
});

it('keeps a detached avatar out of later updates', () => {
  const avatar = document.createElement('span');
  bindLevel(avatar, 7)();
  updateVoiceLevels({ levels: [[7, 88]] });
  expect(avatar.style.getPropertyValue('--lvl')).toBe('');
});
