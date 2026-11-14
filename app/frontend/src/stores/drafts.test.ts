import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { discardDraft, flushDraft, readDraft, resetDraftCache, saveDraftSoon } from './drafts';

beforeEach(() => {
  localStorage.clear();
  resetDraftCache();
});
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

it('a draft is written to localStorage under the channel id, after the debounce', () => {
  vi.useFakeTimers();
  saveDraftSoon('c1', 'hello');
  expect(localStorage.getItem('finchspeak.draft.c1')).toBeNull();
  vi.advanceTimersByTime(300);
  expect(localStorage.getItem('finchspeak.draft.c1')).toBe('hello');
});

it('flushing writes a pending draft at once, and an empty draft removes the key', () => {
  saveDraftSoon('c1', 'hello');
  flushDraft();
  expect(localStorage.getItem('finchspeak.draft.c1')).toBe('hello');
  saveDraftSoon('c1', '');
  flushDraft();
  expect(localStorage.getItem('finchspeak.draft.c1')).toBeNull();
});

it('each channel reads back its own draft', () => {
  saveDraftSoon('a', 'for a');
  saveDraftSoon('b', 'for b');
  flushDraft();
  expect(readDraft('a')).toBe('for a');
  expect(readDraft('b')).toBe('for b');
  expect(readDraft('none')).toBe('');
});

it('a draft is read back from localStorage after the cache is gone', () => {
  localStorage.setItem('finchspeak.draft.c7', 'stored');
  resetDraftCache();
  expect(readDraft('c7')).toBe('stored');
});

it('discarding a channel draft cancels its pending write and leaves other channels alone', () => {
  vi.useFakeTimers();
  saveDraftSoon('c1', 'sent text');
  saveDraftSoon('c2', 'kept');
  flushDraft();
  saveDraftSoon('c1', 'sent text, changed');
  discardDraft('c1');
  vi.advanceTimersByTime(500);
  expect(localStorage.getItem('finchspeak.draft.c1')).toBeNull();
  expect(readDraft('c1')).toBe('');
  expect(readDraft('c2')).toBe('kept');
});

it('a storage that throws is ignored and the draft stays in memory', () => {
  vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => { throw new Error('quota'); });
  vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => { throw new Error('blocked'); });
  expect(() => { saveDraftSoon('c1', 'in memory'); flushDraft(); }).not.toThrow();
  expect(readDraft('c1')).toBe('in memory');
});
