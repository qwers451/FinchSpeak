import { beforeEach, expect, it, vi } from 'vitest';
import { api } from '../services/api';
import { appError, setAppError } from './notificationStore';
import { activeChannel, deleteMessage, editMessage, messages, setActiveChannel, setMessages, toggleReaction } from './chatStore';
import { clearTyping, markRead, mentions, noteIncoming, noteTyping, notifyTyping, resetActivity, typingNames, unread } from './activityStore';
import { setCurrentUser } from './authStore';
import { sessionHandlers, knownUsers, setKnownUsers } from './sessionStore';
import { attachFile, downloadAttachment, MAX_ATTACHMENT_BYTES, pendingAttachments, setPendingAttachments, takePending } from './attachmentStore';
import { saveDisplayName, savePassword } from './profileStore';
import { setVoiceParticipants, allVoiceParticipants } from './voiceStore';
import type { Message, User } from '../types';

const me: User = { id: 'me', username: 'alice', display_name: 'Alice', role: 'member', status: 'online' } as User;
const message = (id: string, extra: Partial<Message> = {}): Message =>
  ({ id, channel_id: 'c1', author_id: 'bob', author_name: 'Bob', content: 'hello', attachments: [], timestamp: 1, ...extra });
const channel = { id: 'c1', short_id: 1, name: 'general', channel_type: 'text', position: 1, topic: null, bitrate_kbps: 64 } as never;

beforeEach(() => {
  vi.restoreAllMocks();
  setAppError(null);
  resetActivity();
  setCurrentUser(me);
  setActiveChannel(channel);
  setMessages([message('m1'), message('m2', { author_id: 'me', author_name: 'Alice' })]);
  setPendingAttachments([]);
});

// ---- editing, deleting, reactions

it('AUD-044 an edit changes the text and marks the message edited', () => {
  sessionHandlers['message-edited']({ message_id: 'm1', channel_id: 'c1', content: 'fixed', edited_at: 99 });
  expect(messages()[0]).toMatchObject({ content: 'fixed', edited_at: 99 });
  expect(messages()[1].content).toBe('hello');
});

it('AUD-044 a deleted message disappears', () => {
  sessionHandlers['message-deleted']({ message_id: 'm1', channel_id: 'c1' });
  expect(messages().map((m) => m.id)).toEqual(['m2']);
});

it('AUD-044 reactions are added per user and vanish when the last one is taken back', () => {
  const react = (user_id: string, added: boolean) =>
    sessionHandlers['reaction-changed']({ message_id: 'm1', channel_id: 'c1', user_id, emoji: '👍', added });
  react('bob', true);
  react('me', true);
  react('me', true); // repeated: still one
  expect(messages()[0].reactions).toEqual([{ emoji: '👍', user_ids: ['bob', 'me'] }]);
  react('bob', false);
  react('me', false);
  expect(messages()[0].reactions).toEqual([]);
});

it('AUD-044 clicking a reaction adds mine, clicking again removes it', async () => {
  const set = vi.spyOn(api, 'setReaction').mockResolvedValue();
  await toggleReaction(messages()[0], '👍');
  expect(set).toHaveBeenLastCalledWith('m1', '👍', true);
  const reacted = message('m1', { reactions: [{ emoji: '👍', user_ids: ['me'] }] });
  await toggleReaction(reacted, '👍');
  expect(set).toHaveBeenLastCalledWith('m1', '👍', false);
});

it('AUD-044 edit and delete failures are shown, empty edits are not sent', async () => {
  const edit = vi.spyOn(api, 'editMessage').mockRejectedValue('HTTP 403: Permission denied');
  await editMessage('m1', '   ');
  expect(edit).not.toHaveBeenCalled();
  await editMessage('m1', 'new');
  expect(appError()).toContain('403');
  vi.spyOn(api, 'deleteMessage').mockRejectedValue('HTTP 404: No such message');
  await deleteMessage('m1');
  expect(appError()).toContain('404');
});

// ---- unread and mentions

it('AUD-044 messages in other channels are counted as unread, mentions flagged', () => {
  noteIncoming({ channel_id: 'c2', author_id: 'bob', content: 'hi' });
  noteIncoming({ channel_id: 'c2', author_id: 'bob', content: 'hey @alice look' });
  expect(unread()['c2']).toEqual({ count: 2, mention: true });
  markRead('c2');
  expect(unread()['c2']).toBeUndefined();
});

it('AUD-044 own messages never count, and neither does the open channel when the window has focus', () => {
  vi.spyOn(document, 'hasFocus').mockReturnValue(true);
  noteIncoming({ channel_id: 'c2', author_id: 'me', content: 'mine' });
  noteIncoming({ channel_id: 'c1', author_id: 'bob', content: 'open channel' });
  expect(unread()).toEqual({});
  vi.spyOn(document, 'hasFocus').mockReturnValue(false);
  noteIncoming({ channel_id: 'c1', author_id: 'bob', content: 'window in background' });
  expect(unread()['c1']?.count).toBe(1);
});

it('AUD-044 a mention is @display name or @login, any case, and not a bare name', () => {
  expect(mentions('ping @ALICE', me)).toBe(true);
  expect(mentions('hi @alice!', me)).toBe(true);
  expect(mentions('alice is here', me)).toBe(false);
  expect(mentions('@bob', me)).toBe(false);
});

// ---- typing

it('AUD-044 typing hints expire and end with the next message of that person', () => {
  noteTyping({ channel_id: 'c1', user_id: 'bob', username: 'Bob' }, 1000);
  expect(typingNames('c1', 2000)).toEqual(['Bob']);
  expect(typingNames('c1', 6000)).toEqual([]);
  noteTyping({ channel_id: 'c1', user_id: 'bob', username: 'Bob' }, Date.now());
  sessionHandlers['new-message']({ message_id: 'm9', channel_id: 'c1', author_id: 'bob', author_name: 'Bob', content: 'done', attachments: [], timestamp: 5 });
  expect(typingNames('c1')).toEqual([]);
  clearTyping('c1', 'nobody');
});

it('AUD-044 typing is reported at most every few seconds', () => {
  const send = vi.spyOn(api, 'sendTyping').mockResolvedValue();
  notifyTyping('c1', 100_000);
  notifyTyping('c1', 101_000);
  notifyTyping('c1', 102_900);
  expect(send).toHaveBeenCalledTimes(1);
  notifyTyping('c1', 103_100);
  expect(send).toHaveBeenCalledTimes(2);
});

// ---- profile

it('AUD-043 a renamed user is renamed everywhere on screen', () => {
  setKnownUsers([{ ...me, id: 'bob', username: 'bob', display_name: 'Bob' } as User]);
  setVoiceParticipants([{ user_id: 'bob', peer_id: 5, username: 'Bob', channel_id: 'v' }]);
  sessionHandlers['user-profile-changed']({ user_id: 'bob', display_name: 'Robert' });
  expect(knownUsers()[0].display_name).toBe('Robert');
  expect(allVoiceParticipants()[0].username).toBe('Robert');
  expect(messages()[0].author_name).toBe('Robert');
  expect(activeChannel()?.id).toBe('c1');
});

it('AUD-043 changing my own name updates the signed-in user', async () => {
  vi.spyOn(api, 'updateProfile').mockResolvedValue({ ...me, display_name: 'Alice C' });
  expect(await saveDisplayName('Alice C')).toBe(true);
  sessionHandlers['user-profile-changed']({ user_id: 'me', display_name: 'Alice D' });
  expect(messages()[1].author_name).toBe('Alice D');
});

it('AUD-043 a password change needs both new passwords to match', async () => {
  const change = vi.spyOn(api, 'changePassword').mockResolvedValue();
  expect(await savePassword('old', 'new-pass-1', 'different')).toBe(false);
  expect(change).not.toHaveBeenCalled();
  expect(appError()).toContain('do not match');
  expect(await savePassword('old', 'new-pass-1', 'new-pass-1')).toBe(true);
  expect(change).toHaveBeenCalledWith('old', 'new-pass-1');
});

// ---- attachments

it('AUD-041 an oversized file is refused before it is read', async () => {
  const upload = vi.spyOn(api, 'uploadFile');
  const big = new File([new Uint8Array(1)], 'big.bin');
  Object.defineProperty(big, 'size', { value: MAX_ATTACHMENT_BYTES + 1 });
  await attachFile(big);
  expect(upload).not.toHaveBeenCalled();
  expect(appError()).toContain('big.bin');
});

it('AUD-041 an uploaded file waits for the next message and is sent once', async () => {
  vi.spyOn(api, 'uploadFile').mockResolvedValue({ id: 'f1', filename: 'a.txt', size_bytes: 3, content_type: 'text/plain', url: '/api/files/f1' });
  await attachFile(new File(['abc'], 'a.txt', { type: 'text/plain' }));
  expect(pendingAttachments().map((a) => a.id)).toEqual(['f1']);
  expect(takePending()).toEqual(['f1']);
  expect(pendingAttachments()).toEqual([]);
});

it('AUD-041 downloading fetches the bytes with our login and offers them as a file', async () => {
  vi.spyOn(api, 'downloadFile').mockResolvedValue(new TextEncoder().encode('abc').buffer as ArrayBuffer);
  const created: string[] = [];
  vi.stubGlobal('URL', { createObjectURL: () => { created.push('blob:1'); return 'blob:1'; }, revokeObjectURL: vi.fn() });
  const click = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => {});
  await downloadAttachment({ id: 'f1', filename: 'a.txt', size_bytes: 3, content_type: 'text/plain', url: '' });
  expect(created).toEqual(['blob:1']);
  expect(click).toHaveBeenCalled();
  vi.unstubAllGlobals();
});
