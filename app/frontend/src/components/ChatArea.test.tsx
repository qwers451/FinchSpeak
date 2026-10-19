import { render } from 'solid-js/web';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { api } from '../services/api';
import { setCurrentUser } from '../stores/authStore';
import { setUnread, resetActivity } from '../stores/activityStore';
import { replyingTo, setActiveChannel, setChatLoading, setMessages, setReplyingTo } from '../stores/chatStore';
import { setPendingFiles } from '../stores/attachmentStore';
import { DEFAULT_PREFS, setPrefs } from '../stores/settingsStore';
import { setConnectionStatus, setKnownUsers } from '../stores/sessionStore';
import type { Channel, Message, User } from '../types';
import { clearToasts } from '../ui';
import { ChatArea } from './ChatArea';
import { mentionAt } from './chat/Composer';

const channel: Channel = { id: 'c1', short_id: 1, name: 'general', channel_type: 'text', position: 0, bitrate_kbps: 64, topic: 'Game nights' };
const me: User = { id: 'me', username: 'alice', display_name: 'Alice Cooper', role: 'member' };
const bob: User = { id: 'bob', username: 'bob', display_name: 'Bob Marley', role: 'member' };
const dave: User = { id: 'dave', username: 'dave', display_name: 'Dave Okoro', role: 'admin' };

const T0 = Math.floor(new Date('2026-10-16T10:00:00').getTime() / 1000);
const msg = (id: string, author: User, offset: number, extra: Partial<Message> = {}): Message => ({
  id, channel_id: 'c1', author_id: author.id, author_name: author.display_name, content: `text ${id}`, attachments: [], timestamp: T0 + offset, ...extra,
});

let dispose: (() => void) | undefined;
const mount = () => {
  const host = document.createElement('div');
  document.body.appendChild(host);
  dispose = render(() => <ChatArea />, host);
  return host;
};
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));
const type = (el: HTMLTextAreaElement | HTMLInputElement, value: string) => {
  el.value = value;
  el.setSelectionRange?.(value.length, value.length);
  el.dispatchEvent(new InputEvent('input', { bubbles: true }));
};
const press = (el: Element, key: string, init: KeyboardEventInit = {}) =>
  el.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init }));
const button = (host: ParentNode, label: string) => host.querySelector<HTMLButtonElement>(`[aria-label="${label}"]`)!;

beforeEach(() => {
  localStorage.clear();
  Object.defineProperty(Element.prototype, 'scrollIntoView', { value: vi.fn(), configurable: true });
  vi.spyOn(document, 'hasFocus').mockReturnValue(true);
  resetActivity();
  clearToasts();
  setCurrentUser(me);
  setKnownUsers([me, bob, dave]);
  setConnectionStatus('connected');
  setActiveChannel(channel);
  setMessages([]);
  setReplyingTo(null);
  setPendingFiles([]);
});
afterEach(() => {
  dispose?.();
  dispose = undefined;
  document.body.innerHTML = '';
  vi.restoreAllMocks();
});

it('shows the channel, its topic, a day separator and one header for close messages of one author', () => {
  setMessages([msg('a', bob, 0), msg('b', bob, 60), msg('c', dave, 120)]);
  const host = mount();
  expect(host.querySelector('.mhead')?.textContent).toContain('general');
  expect(host.querySelector('.mhead')?.textContent).toContain('Game nights');
  expect(host.querySelectorAll('.daysep')).toHaveLength(1);
  expect(host.querySelectorAll('.msg')).toHaveLength(3);
  expect(host.querySelectorAll('.msg:not(.compact) .name')).toHaveLength(2);
  expect(host.querySelector('.msg.compact')?.textContent).toContain('text b');
  expect(host.textContent).toContain('Admin'); // Dave
});

it('draws the "New messages" divider where the server says the unread part begins', () => {
  setUnread({ c1: { count: 2, mention: false, firstUnreadId: 'b' } });
  setMessages([msg('a', bob, 0), msg('b', dave, 60), msg('c', dave, 90)]);
  const host = mount();
  const rows = Array.from(host.querySelector('.message-list')!.children).map((el) => el.className.split(' ')[0]);
  expect(rows).toEqual(['daysep', 'msg', 'daysep', 'msg', 'msg']);
  expect(host.querySelector('.daysep.new')?.textContent).toBe('New messages');
});

it('highlights a message that mentions me and renders known mentions as chips', () => {
  setMessages([msg('a', bob, 0, { content: 'hey @alice and @nobody', mentions: ['me'] })]);
  const host = mount();
  expect(host.querySelector('.msg.mention')).not.toBeNull();
  expect(Array.from(host.querySelectorAll('.mention-chip')).map((chip) => chip.textContent)).toEqual(['@alice']);
});

it('replying shows a bar, Esc cancels it, and the quote renders with a deleted original handled', async () => {
  setMessages([
    msg('a', bob, 0, { content: 'Anyone up for a game?' }),
    msg('b', dave, 60, { reply_to: { message_id: 'a', author_name: 'Bob Marley', snippet: 'Anyone up for a game?' } }),
    msg('c', dave, 120, { reply_to: { message_id: 'gone', author_name: '', snippet: '', deleted: true } }),
  ]);
  const host = mount();
  const quotes = host.querySelectorAll('.reply-quote');
  expect(quotes[0].textContent).toContain('Bob Marley');
  expect(quotes[0].textContent).toContain('Anyone up for a game?');
  expect(quotes[1].textContent).toContain('Original message deleted');

  button(host.querySelector('[data-message-id="a"]')!, 'Reply').click();
  await flush();
  expect(replyingTo()?.id).toBe('a');
  expect(host.querySelector('.reply-bar')?.textContent).toContain('Replying to Bob Marley');
  press(host.querySelector('textarea[aria-label="Message"]')!, 'Escape');
  await flush();
  expect(replyingTo()).toBeNull();
  expect(host.querySelector('.reply-bar')).toBeNull();
});

it('sends on Enter, keeps Shift+Enter for a new line, and clears the field', async () => {
  const send = vi.spyOn(api, 'sendTextMessage').mockResolvedValue();
  const host = mount();
  const box = host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message"]')!;
  type(box, 'line one');
  press(box, 'Enter', { shiftKey: true });
  expect(send).not.toHaveBeenCalled();
  press(box, 'Enter');
  await flush();
  expect(send).toHaveBeenCalledWith('c1', 'line one', [], expect.any(String), undefined);
  expect(box.value).toBe('');
});

it('refuses to send more than 4000 characters and says by how much', async () => {
  const send = vi.spyOn(api, 'sendTextMessage').mockResolvedValue();
  const host = mount();
  const box = host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message"]')!;
  type(box, 'x'.repeat(4212));
  await flush();
  expect(host.querySelector('.composer')?.textContent).toContain('212 over');
  expect(button(host, 'Send').disabled).toBe(true);
  press(box, 'Enter');
  expect(send).not.toHaveBeenCalled();
});

it('is disabled with an explanation while offline', () => {
  setConnectionStatus('reconnecting');
  const host = mount();
  const box = host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message"]')!;
  expect(box.disabled).toBe(true);
  expect(box.placeholder).toContain('offline');
  expect(button(host, 'Send').disabled).toBe(true);
});

it('suggests members after @ and inserts the chosen login', async () => {
  const host = mount();
  const box = host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message"]')!;
  type(box, 'ping @da');
  await flush();
  const options = host.querySelectorAll('[role="option"]');
  expect(options).toHaveLength(1);
  expect(options[0].textContent).toContain('Dave Okoro');
  press(box, 'Enter');
  await flush();
  expect(box.value).toBe('ping @dave ');
  expect(host.querySelector('[role="listbox"]')).toBeNull();
});

it('finds the @word under the caret and ignores addresses', () => {
  expect(mentionAt('hello @bo', 9)).toEqual({ start: 6, query: 'bo' });
  expect(mentionAt('hello @bo and more', 9)).toEqual({ start: 6, query: 'bo' });
  expect(mentionAt('mail a@bo', 9)).toBeNull();
  expect(mentionAt('no mention', 10)).toBeNull();
});

it('editing keeps the message until saved, Esc cancels, Enter saves through the server', async () => {
  const edit = vi.spyOn(api, 'editMessage').mockResolvedValue();
  setMessages([msg('m', me, 0, { content: 'Count me in' })]);
  const host = mount();
  button(host, 'Edit message').click();
  await flush();
  const box = host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Edit message"]')!;
  type(box, 'Count me out');
  press(box, 'Escape');
  await flush();
  expect(host.querySelector('textarea[aria-label="Edit message"]')).toBeNull();
  expect(host.textContent).toContain('Count me in');
  expect(edit).not.toHaveBeenCalled();

  button(host, 'Edit message').click();
  await flush();
  const again = host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Edit message"]')!;
  type(again, 'Count me out');
  press(again, 'Enter');
  await flush();
  expect(edit).toHaveBeenCalledWith('m', 'Count me out');
});

it('deleting asks first, and only the author (or an admin) is offered it', async () => {
  const remove = vi.spyOn(api, 'deleteMessage').mockResolvedValue();
  setMessages([msg('mine', me, 0), msg('theirs', bob, 60)]);
  const host = mount();
  expect(host.querySelector('[data-message-id="theirs"] [aria-label="Delete message"]')).toBeNull();
  button(host.querySelector('[data-message-id="mine"]')!, 'Delete message').click();
  await flush();
  expect(document.querySelector('[role="dialog"]')?.textContent).toContain('Delete this message?');
  expect(remove).not.toHaveBeenCalled();
  Array.from(document.querySelectorAll('[role="dialog"] button')).find((b) => b.textContent === 'Delete')!.dispatchEvent(new MouseEvent('click', { bubbles: true }));
  await flush();
  expect(remove).toHaveBeenCalledWith('mine');
});

it('an unsent message shows Not sent with Retry and Delete', async () => {
  const send = vi.spyOn(api, 'sendTextMessage').mockResolvedValue();
  setMessages([msg('local-x', me, 0, { state: 'failed', failure: 'offline', client_msg_id: 'x', content: 'Did anyone see the patch notes?' })]);
  const host = mount();
  expect(host.querySelector('.msg.failed')?.textContent).toContain('Not sent');
  Array.from(host.querySelectorAll('.msg.failed button')).find((b) => b.textContent === 'Retry')!.dispatchEvent(new MouseEvent('click', { bubbles: true }));
  await flush();
  expect(send).toHaveBeenCalledWith('c1', 'Did anyone see the patch notes?', [], 'x', undefined);
});

it('reaction chips show who reacted, mark mine, and toggle on click', async () => {
  const react = vi.spyOn(api, 'setReaction').mockResolvedValue();
  setMessages([msg('a', bob, 0, { reactions: [{ emoji: '👍', user_ids: ['me', 'bob'] }, { emoji: '🎉', user_ids: Array.from({ length: 120 }, (_, i) => `u${i}`) }] })]);
  const host = mount();
  const chips = host.querySelectorAll<HTMLButtonElement>('button.react');
  expect(chips[0].classList.contains('mine')).toBe(true);
  expect(chips[0].querySelector('.react-tip')?.textContent).toContain('You, Bob Marley');
  expect(chips[1].textContent).toContain('99+');
  chips[0].click();
  await flush();
  expect(react).toHaveBeenCalledWith('a', '👍', false);
});

it('ru: the reaction tooltip calls me "Вы" and an unknown reactor "Кто-то"', () => {
  setPrefs({ ...DEFAULT_PREFS, appearance: { ...DEFAULT_PREFS.appearance, language: 'ru' } });
  setMessages([msg('a', bob, 0, { reactions: [{ emoji: '👍', user_ids: ['me', 'bob', 'ghost'] }] })]);
  const host = mount();
  const tip = host.querySelector('button.react .react-tip')?.textContent ?? '';
  expect(tip).toContain('Вы, Bob Marley, Кто-то');
  expect(tip).not.toMatch(/\b(You|Someone)\b/);
});

it('the jump button counts unseen messages with the right plural form', async () => {
  const jumpLabel = () => document.querySelector('.jump-wrap button')?.textContent?.replace(/\s+/g, ' ').trim();
  const arriveWhileScrolledUp = async (language: 'ru' | 'en', total: number) => {
    setPrefs({ ...DEFAULT_PREFS, appearance: { ...DEFAULT_PREFS.appearance, language } });
    setMessages([msg('m0', bob, 0)]);
    const host = mount();
    const list = host.querySelector('.message-list') as HTMLElement;
    Object.defineProperty(list, 'scrollHeight', { configurable: true, value: 5000 });
    Object.defineProperty(list, 'clientHeight', { configurable: true, value: 400 });
    await flush();
    list.scrollTop = 0; // the reader has scrolled up, so new messages are only counted
    const seen: string[] = [];
    for (let i = 1; i <= total; i++) {
      setMessages([...Array.from({ length: i + 1 }, (_, n) => msg(`m${n}`, bob, n))]);
      await flush();
      seen.push(jumpLabel() ?? '');
    }
    return seen;
  };
  const ru = await arriveWhileScrolledUp('ru', 5);
  expect(ru).toEqual(['1 новое сообщение', '2 новых сообщения', '3 новых сообщения', '4 новых сообщения', '5 новых сообщений']);
  dispose?.();
  dispose = undefined;
  document.body.innerHTML = '';
  const en = await arriveWhileScrolledUp('en', 2);
  expect(en).toEqual(['1 new message', '2 new messages']);
});

it('dropping files on the chat attaches them, with an overlay while dragging', async () => {
  const upload = vi.spyOn(api, 'uploadFile').mockImplementation(async (name) => ({ id: name, filename: name, size_bytes: 1, content_type: 'text/plain', url: '' }));
  const host = mount();
  const area = host.querySelector('main')!;
  const dataTransfer = { types: ['Files'], files: [new File(['a'], 'a.txt'), new File(['b'], 'b.txt')] };
  const event = (type: string) => Object.assign(new Event(type, { bubbles: true, cancelable: true }), { dataTransfer });
  area.dispatchEvent(event('dragenter'));
  await flush();
  expect(host.querySelector('.drop-overlay')).not.toBeNull();
  area.dispatchEvent(event('drop'));
  await flush();
  await flush();
  expect(host.querySelector('.drop-overlay')).toBeNull();
  expect(upload).toHaveBeenCalledTimes(2);
  expect(host.querySelectorAll('.file-pill')).toHaveLength(2);
});

it('an empty channel welcomes people', () => {
  const host = mount();
  expect(host.textContent).toContain('Welcome to #general!');
});

it('shows skeletons while the first page loads', () => {
  setChatLoading(true);
  const host = mount();
  expect(host.querySelector('[aria-label="Loading messages"]')).not.toBeNull();
  expect(host.textContent).not.toContain('Welcome to #general!');
  setChatLoading(false);
});

it('an open emoji picker and an unsaved edit survive the clock ticking and new messages arriving', async () => {
  vi.useFakeTimers();
  try {
    setMessages([msg('mine', me, 0, { content: 'Count me in' }), msg('theirs', bob, 60)]);
    const host = mount();
    button(host.querySelector('[data-message-id="theirs"]')!, 'Add reaction').click();
    button(host.querySelector('[data-message-id="mine"]')!, 'Edit message').click();
    await vi.advanceTimersByTimeAsync(10);
    type(host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Edit message"]')!, 'Count me out');
    expect(host.querySelector('.emoji-picker')).not.toBeNull();

    await vi.advanceTimersByTimeAsync(2000); // the typing clock ticks four times
    setMessages([msg('mine', me, 0, { content: 'Count me in' }), msg('theirs', bob, 60), msg('later', dave, 120)]);
    await vi.advanceTimersByTimeAsync(10);

    expect(host.querySelector('.emoji-picker')).not.toBeNull();
    expect(host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Edit message"]')?.value).toBe('Count me out');
    expect(host.querySelectorAll('.msg')).toHaveLength(3);
  } finally {
    vi.useRealTimers();
  }
});
