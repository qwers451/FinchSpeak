import { beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../services/api';
import { setCurrentUser } from './authStore';
import {
  discardFailed, handleIncomingMessage, handleMessageDeleted, handleMessageRejected, hasMoreHistory, loadOlder, messages,
  replyingTo, retryMessage, selectChannel, sendMessage, setActiveChannel, setMessages, setReplyingTo,
} from './chatStore';
import { readOutbox } from './outbox';
import { appError, setAppError } from './notificationStore';
import type { Channel } from '../types';

const channel: Channel = { id: 'chat', short_id: 1, name: 'general', channel_type: 'text', position: 0, bitrate_kbps: 64 };
const live = (id: string, content = id) => ({ message_id: id, channel_id: 'chat', author_id: 'other', author_name: 'Other', content, attachments: [], timestamp: 1 });

beforeEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
  setMessages([]);
  setActiveChannel(channel);
  setCurrentUser({ id: 'me', username: 'me', display_name: 'Me', role: 'member' });
});

describe('chat regressions', () => {
  it('AUD-053 sends a message with the signed-in author', async () => {
    vi.spyOn(api, 'sendTextMessage').mockResolvedValue();
    await sendMessage('hello');
    expect(messages()).toMatchObject([{ author_id: 'me', content: 'hello' }]);
  });

  it('AUD-055 accepts two live messages and normalizes their ids', () => {
    handleIncomingMessage(live('one'));
    handleIncomingMessage(live('two'));
    expect(messages().map((message) => message.id)).toEqual(['one', 'two']);
  });

  it('AUD-010 keeps chronological history and supplies its channel id', async () => {
    vi.spyOn(api, 'fetchChannelMessages').mockResolvedValue({ channel_id: 'chat', has_more: false, messages: [
      { id: 'older', author_id: 'other', author_name: 'Other', content: 'first', attachments: [], timestamp: 1 },
      { id: 'newer', author_id: 'other', author_name: 'Other', content: 'second', attachments: [], timestamp: 2 },
    ] });
    await selectChannel(channel);
    expect(messages()).toMatchObject([{ id: 'older', channel_id: 'chat' }, { id: 'newer', channel_id: 'chat' }]);
  });

  it('AUD-036 a send that fails stays visible as not sent, with its reason', async () => {
    vi.spyOn(api, 'sendTextMessage').mockRejectedValue(new Error('offline'));
    vi.spyOn(console, 'error').mockImplementation(() => {});
    await sendMessage('failed');
    expect(messages()).toMatchObject([{ content: 'failed', state: 'failed', failure: expect.stringContaining('offline') }]);
  });

  it('does not consume our pending message when another author sends identical text', async () => {
    vi.spyOn(api, 'sendTextMessage').mockResolvedValue();
    await sendMessage('same');
    handleIncomingMessage(live('remote', 'same'));
    expect(messages()).toHaveLength(2);
  });

  it('does not overwrite a newer channel selection with slow history', async () => {
    let resolveFirst!: (value: { channel_id: string; messages: []; has_more: boolean }) => void;
    vi.spyOn(api, 'fetchChannelMessages').mockImplementationOnce(() => new Promise((resolve) => { resolveFirst = resolve; }))
      .mockResolvedValueOnce({ channel_id: 'second', has_more: false, messages: [{ id: 'second-message', author_id: 'other', author_name: 'Other', content: 'second', attachments: [], timestamp: 2 }] });
    const firstRequest = selectChannel(channel);
    await selectChannel({ ...channel, id: 'second' });
    resolveFirst({ channel_id: 'chat', messages: [], has_more: false });
    await firstRequest;
    expect(messages().map((message) => message.id)).toEqual(['second-message']);
  });

  it('keeps a live message received while history is loading', async () => {
    let finish!: (value: { channel_id: string; messages: []; has_more: boolean }) => void;
    vi.spyOn(api, 'fetchChannelMessages').mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
    const loading = selectChannel(channel);
    handleIncomingMessage(live('arrived-during-load'));
    finish({ channel_id: 'chat', messages: [], has_more: false });
    await loading;
    expect(messages().map((message) => message.id)).toEqual(['arrived-during-load']);
  });

  it('replaces only one pending copy for each echoed message', async () => {
    vi.spyOn(api, 'sendTextMessage').mockResolvedValue();
    await sendMessage('same');
    await sendMessage('same');
    handleIncomingMessage({ ...live('echo', 'same'), author_id: 'me' });
    expect(messages().filter((message) => message.id.startsWith('local-'))).toHaveLength(1);
    handleIncomingMessage({ ...live('echo', 'same'), author_id: 'me' });
    expect(messages()).toHaveLength(2);
  });

  it('keeps chronological order when refreshing the same channel', async () => {
    setMessages([{ id: 'older', channel_id: 'chat', author_id: 'other', author_name: 'Other', content: 'older', attachments: [], timestamp: 1 }]);
    vi.spyOn(api, 'fetchChannelMessages').mockResolvedValue({ channel_id: 'chat', has_more: false, messages: [
      { id: 'newer', author_id: 'other', author_name: 'Other', content: 'newer', attachments: [], timestamp: 2 },
    ] });
    await selectChannel(channel);
    expect(messages().map((message) => message.id)).toEqual(['older', 'newer']);
  });

  it('reconciles pending messages included in history before their live echo', async () => {
    let finish!: (value: Awaited<ReturnType<typeof api.fetchChannelMessages>>) => void;
    vi.spyOn(api, 'fetchChannelMessages').mockImplementation(() => new Promise((resolve) => { finish = resolve; }));
    vi.spyOn(api, 'sendTextMessage').mockResolvedValue();
    const loading = selectChannel(channel);
    await sendMessage('same');
    finish({ channel_id: 'chat', has_more: false, messages: [
      { id: 'persisted', author_id: 'me', author_name: 'Me', content: 'same', attachments: [], timestamp: 1 },
    ] });
    await loading;
    handleIncomingMessage({ ...live('persisted', 'same'), author_id: 'me' });
    expect(messages().map((message) => message.id)).toEqual(['persisted']);
  });

  // --- AUD-036: messages are matched by the id the client chose, never by their text ---

  async function sendTwice() {
    const ids: string[] = [];
    vi.spyOn(api, 'sendTextMessage').mockImplementation(async (_channel, _content, _attachments, clientMsgId) => {
      ids.push(clientMsgId as string);
    });
    await sendMessage('same');
    await sendMessage('same');
    return ids;
  }

  it('AUD-036 sends a distinct client id with every message', async () => {
    const ids = await sendTwice();
    expect(ids).toHaveLength(2);
    expect(ids[0]).not.toEqual(ids[1]);
    expect(messages().map((message) => message.id)).toEqual([`local-${ids[0]}`, `local-${ids[1]}`]);
  });

  it('AUD-036 a rejection marks exactly the message it names, even with identical text', async () => {
    const ids = await sendTwice();
    setAppError(null);
    handleMessageRejected({ client_msg_id: ids[1], code: 400, message: 'Message must be 1..=4000 characters' });
    expect(messages().map((message) => [message.id, message.state])).toEqual([[`local-${ids[0]}`, 'sending'], [`local-${ids[1]}`, 'failed']]);
    expect(messages()[1].failure).toContain('Message must be 1..=4000 characters');
    expect(appError()).toContain('Message must be 1..=4000 characters');
  });

  it('AUD-036 a rejection of an unknown id changes nothing', async () => {
    const ids = await sendTwice();
    handleMessageRejected({ client_msg_id: 'someone-elses', code: 400, message: 'no' });
    expect(messages()).toHaveLength(ids.length);
  });

  it('AUD-036 the echo replaces the pending message with the same id, not merely the same text', async () => {
    const ids = await sendTwice();
    handleIncomingMessage({ ...live('stored', 'same'), author_id: 'me', client_msg_id: ids[1] });
    expect(messages().map((message) => message.id).sort()).toEqual([`local-${ids[0]}`, 'stored'].sort());
  });

  it('AUD-036 history that already holds a pending message (a resend after reconnect) replaces it', async () => {
    const ids = await sendTwice();
    vi.spyOn(api, 'fetchChannelMessages').mockResolvedValue({ channel_id: 'chat', has_more: false, messages: [
      { id: 'persisted', author_id: 'me', author_name: 'Me', content: 'same', attachments: [], timestamp: 1, client_msg_id: ids[1] },
    ] });
    await selectChannel(channel);
    expect(messages().map((message) => message.id).sort()).toEqual([`local-${ids[0]}`, 'persisted'].sort());
  });

  // ---- stage 7: replies, retry, paging

  const stored = (id: string, timestamp: number, extra: object = {}) =>
    ({ id, author_id: 'other', author_name: 'Other', content: id, attachments: [], timestamp, ...extra });

  it('a reply carries a quote of the original and clears the reply target', async () => {
    const send = vi.spyOn(api, 'sendTextMessage').mockResolvedValue();
    handleIncomingMessage(live('original', 'Anyone up for a game tonight?'));
    setReplyingTo(messages()[0]);
    await sendMessage('count me in');
    expect(replyingTo()).toBeNull();
    expect(send).toHaveBeenCalledWith('chat', 'count me in', [], expect.any(String), 'original');
    expect(messages()[1].reply_to).toMatchObject({ message_id: 'original', author_name: 'Other', snippet: 'Anyone up for a game tonight?' });
  });

  it('deleting the original turns the quote in its replies into "deleted"', () => {
    handleIncomingMessage(live('original'));
    handleIncomingMessage({ ...live('answer'), reply_to: { message_id: 'original', author_name: 'Other', snippet: 'original' } });
    handleMessageDeleted({ message_id: 'original' });
    expect(messages().map((message) => message.id)).toEqual(['answer']);
    expect(messages()[0].reply_to).toEqual({ message_id: 'original', author_name: '', snippet: '', deleted: true });
  });

  it('retry sends a failed message again with the same client id, and it survives a restart', async () => {
    const send = vi.spyOn(api, 'sendTextMessage').mockRejectedValueOnce(new Error('offline')).mockResolvedValue();
    vi.spyOn(console, 'error').mockImplementation(() => {});
    await sendMessage('try again');
    const firstId = send.mock.calls[0][3];
    expect(readOutbox().map((entry) => entry.client_msg_id)).toEqual([firstId]);

    // "Restart": nothing in memory, the failed message comes back from storage.
    setMessages([]);
    vi.spyOn(api, 'fetchChannelMessages').mockResolvedValue({ channel_id: 'chat', has_more: false, messages: [] });
    await selectChannel(channel);
    expect(messages()).toMatchObject([{ content: 'try again', state: 'failed' }]);

    await retryMessage(messages()[0].id);
    expect(send.mock.calls[1][3]).toBe(firstId);
    expect(messages()[0].state).toBe('sending');
    handleIncomingMessage({ ...live('stored-after-retry', 'try again'), author_id: 'me', client_msg_id: firstId });
    expect(messages().map((message) => message.id)).toEqual(['stored-after-retry']);
    expect(readOutbox()).toEqual([]);
  });

  it('discarding a failed message removes it for good', async () => {
    vi.spyOn(api, 'sendTextMessage').mockRejectedValue(new Error('offline'));
    vi.spyOn(console, 'error').mockImplementation(() => {});
    await sendMessage('never mind');
    discardFailed(messages()[0].id);
    expect(messages()).toEqual([]);
    expect(readOutbox()).toEqual([]);
  });

  it('a message stored by the server just before the connection dropped is not shown as failed', async () => {
    vi.spyOn(api, 'sendTextMessage').mockRejectedValue(new Error('timeout'));
    vi.spyOn(console, 'error').mockImplementation(() => {});
    await sendMessage('did it arrive?');
    const clientMsgId = messages()[0].client_msg_id!;
    setMessages([]);
    vi.spyOn(api, 'fetchChannelMessages').mockResolvedValue({ channel_id: 'chat', has_more: false, messages: [
      stored('persisted', 5, { author_id: 'me', content: 'did it arrive?', client_msg_id: clientMsgId }),
    ] });
    await selectChannel(channel);
    expect(messages().map((message) => message.id)).toEqual(['persisted']);
    expect(readOutbox()).toEqual([]);
  });

  it('scrolling up loads the next older page by cursor, without duplicates', async () => {
    const fetch = vi.spyOn(api, 'fetchChannelMessages')
      .mockResolvedValueOnce({ channel_id: 'chat', has_more: true, messages: [stored('m3', 3), stored('m4', 4)] })
      .mockResolvedValueOnce({ channel_id: 'chat', has_more: false, messages: [stored('m1', 1), stored('m2', 2), stored('m3', 3)] });
    await selectChannel(channel);
    expect(hasMoreHistory()).toBe(true);
    await loadOlder();
    expect(fetch).toHaveBeenLastCalledWith('chat', undefined, 50, 'm3');
    expect(messages().map((message) => message.id)).toEqual(['m1', 'm2', 'm3', 'm4']);
    expect(hasMoreHistory()).toBe(false);
    await loadOlder();
    expect(fetch).toHaveBeenCalledTimes(2);
  });
});
