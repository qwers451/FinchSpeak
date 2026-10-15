import { beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../services/api';
import { setCurrentUser } from './authStore';
import { handleIncomingMessage, handleMessageRejected, messages, selectChannel, sendMessage, setActiveChannel, setMessages } from './chatStore';
import { appError, setAppError } from './notificationStore';
import type { Channel } from '../types';

const channel: Channel = { id: 'chat', short_id: 1, name: 'general', channel_type: 'text', position: 0, bitrate_kbps: 64 };
const live = (id: string, content = id) => ({ message_id: id, channel_id: 'chat', author_id: 'other', author_name: 'Other', content, attachments: [], timestamp: 1 });

beforeEach(() => {
  vi.restoreAllMocks();
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

  it('AUD-036 removes only the failed optimistic message', async () => {
    vi.spyOn(api, 'sendTextMessage').mockRejectedValue(new Error('offline'));
    vi.spyOn(console, 'error').mockImplementation(() => {});
    await sendMessage('failed');
    expect(messages()).toEqual([]);
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

  it('AUD-036 a rejection rolls back exactly the message it names, even with identical text', async () => {
    const ids = await sendTwice();
    setAppError(null);
    handleMessageRejected({ client_msg_id: ids[1], code: 400, message: 'Message must be 1..=4000 characters' });
    expect(messages().map((message) => message.id)).toEqual([`local-${ids[0]}`]);
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
});
