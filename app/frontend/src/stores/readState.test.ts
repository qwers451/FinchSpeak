import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { api } from '../services/api';
import { setCurrentUser } from './authStore';
import { setActiveChannel } from './chatStore';
import { isMentioned, markRead, noteIncoming, resetActivity, setUnreadFromServer, unread } from './activityStore';

const me = { id: 'me', username: 'alice', display_name: 'Alice Cooper', role: 'member' as const };

beforeEach(() => {
  vi.useFakeTimers();
  resetActivity();
  setActiveChannel(null);
  setCurrentUser(me);
  vi.spyOn(document, 'hasFocus').mockReturnValue(true);
});
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

it('takes the unread counts the server reports, with the first unread message', () => {
  setUnreadFromServer([
    { channel_id: 'c1', count: 3, mentions: 1, first_unread_id: 'm10' },
    { channel_id: 'c2', count: 0, mentions: 0, first_unread_id: null },
  ]);
  expect(unread()).toEqual({ c1: { count: 3, mention: true, mentions: 1, firstUnreadId: 'm10' } });
});

it('the open channel is not shown as unread when the window has focus', () => {
  setActiveChannel({ id: 'c1', short_id: 1, name: 'general', channel_type: 'text', position: 0, bitrate_kbps: 64 });
  setUnreadFromServer([{ channel_id: 'c1', count: 2, mentions: 0, first_unread_id: 'm1' }]);
  expect(unread()).toEqual({});
});

it('keeps the first unread message of a channel while more arrive', () => {
  vi.spyOn(document, 'hasFocus').mockReturnValue(false);
  noteIncoming({ id: 'm1', channel_id: 'c2', author_id: 'bob', content: 'one', mentions: [] });
  noteIncoming({ id: 'm2', channel_id: 'c2', author_id: 'bob', content: 'two', mentions: ['me'] });
  expect(unread()['c2']).toMatchObject({ count: 2, mention: true, firstUnreadId: 'm1' });
});

it('trusts the server list of mentions over the text', () => {
  expect(isMentioned({ content: 'hello @alice', mentions: [] })).toBe(false);
  expect(isMentioned({ content: 'hello', mentions: ['me'] })).toBe(true);
  expect(isMentioned({ content: 'hello @alice' })).toBe(true);
});

it('reading a channel tells the server once, with the newest message, after a short pause', () => {
  const report = vi.spyOn(api, 'markChannelRead').mockResolvedValue();
  markRead('c1', 'm1');
  markRead('c1', 'm2');
  expect(report).not.toHaveBeenCalled();
  vi.advanceTimersByTime(400);
  expect(report).toHaveBeenCalledTimes(1);
  expect(report).toHaveBeenCalledWith('c1', 'm2');
});

it('a message that is not stored yet is never reported as read', () => {
  const report = vi.spyOn(api, 'markChannelRead').mockResolvedValue();
  markRead('c1', 'local-abc');
  vi.advanceTimersByTime(400);
  expect(report).not.toHaveBeenCalled();
});
