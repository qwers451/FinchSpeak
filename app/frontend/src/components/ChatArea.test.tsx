import { render } from 'solid-js/web';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { api } from '../services/api';
import { setCurrentUser } from '../stores/authStore';
import { setUnread, resetActivity } from '../stores/activityStore';
import { handleIncomingMessage, messages, replyingTo, sendMessage, setActiveChannel, setChatLoading, setMessages, setReplyingTo } from '../stores/chatStore';
import { setPendingFiles } from '../stores/attachmentStore';
import { resetDraftCache } from '../stores/drafts';
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
  resetDraftCache();
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
  vi.useRealTimers();
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

it('opens the mention list and the emoji picker inside the input anchor, above the field', async () => {
  const host = mount();
  const box = host.querySelector<HTMLTextAreaElement>('textarea')!;
  type(box, 'ping @da');
  await flush();
  const anchor = host.querySelector('.composer-anchor')!;
  expect(anchor.querySelector('.mention-menu')).not.toBeNull();
  expect(anchor.querySelector('.composer')).not.toBeNull();
  type(box, '');
  await flush();
  button(host, 'Emoji').click();
  await flush();
  expect(anchor.querySelector('.composer-picker')).not.toBeNull();
  expect(host.querySelectorAll('.composer-anchor')).toHaveLength(1);
});

it('puts the input right after the reply bar, so the bar and the input can share a border', async () => {
  const host = mount();
  expect(host.querySelector('.reply-bar')).toBeNull();
  setReplyingTo(msg('a', bob, 0));
  await flush();
  expect(host.querySelector('.reply-bar + .composer-anchor > .composer')).not.toBeNull();
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

const jumpButton = (host: ParentNode) => host.querySelector<HTMLButtonElement>('.jump-wrap button');
const jumpLabel = () => document.querySelector('.jump-wrap button')?.textContent?.replace(/\s+/g, ' ').trim();
const sizeList = (list: HTMLElement) => {
  Object.defineProperty(list, 'scrollHeight', { configurable: true, value: 5000 });
  Object.defineProperty(list, 'clientHeight', { configurable: true, value: 400 });
};
const scrollList = (list: HTMLElement, top: number) => {
  list.scrollTop = top;
  list.dispatchEvent(new Event('scroll'));
};

it('the jump button appears only after scrolling more than a screen up, and hides near the bottom', async () => {
  setMessages([msg('m0', bob, 0)]);
  const host = mount();
  const list = host.querySelector<HTMLElement>('.message-list')!;
  sizeList(list);
  await flush();
  scrollList(list, 4600); // at the bottom
  await flush();
  expect(jumpButton(host)).toBeNull();
  scrollList(list, 4000); // 600px above the bottom: more than the 400px screen
  await flush();
  expect(jumpLabel()).toBe('Jump to latest');
  scrollList(list, 4300); // 300px above: within a screen again
  await flush();
  expect(jumpButton(host)).toBeNull();
});

it('the jump button counts messages that arrive while scrolled away, and the count clears once the list reaches the bottom', async () => {
  setMessages([msg('m0', bob, 0)]);
  const host = mount();
  const list = host.querySelector<HTMLElement>('.message-list')!;
  sizeList(list);
  await flush();
  const scrollTo = vi.fn();
  Object.defineProperty(list, 'scrollTo', { configurable: true, value: scrollTo });
  scrollList(list, 0);
  await flush();
  expect(jumpLabel()).toBe('Jump to latest');

  setMessages([msg('m0', bob, 0), msg('m1', bob, 1)]);
  await flush();
  expect(jumpLabel()).toBe('1 new');
  setMessages([msg('m0', bob, 0), msg('m1', bob, 1), msg('m2', dave, 2)]);
  await flush();
  expect(jumpLabel()).toBe('2 new');

  jumpButton(host)!.click();
  await flush();
  expect(scrollTo).toHaveBeenCalledWith({ top: 5000, behavior: 'smooth' });
  expect(jumpLabel()).toBe('2 new'); // the count stays until the list actually reaches the bottom
  scrollList(list, 4600);
  await flush();
  expect(jumpButton(host)).toBeNull();
});

it('the count also clears when the reader scrolls back to the bottom by hand', async () => {
  setMessages([msg('m0', bob, 0)]);
  const host = mount();
  const list = host.querySelector<HTMLElement>('.message-list')!;
  sizeList(list);
  await flush();
  scrollList(list, 0);
  await flush();
  setMessages([msg('m0', bob, 0), msg('m1', bob, 1)]);
  await flush();
  expect(jumpLabel()).toBe('1 new');
  scrollList(list, 4600);
  await flush();
  expect(jumpButton(host)).toBeNull();
  scrollList(list, 0);
  await flush();
  expect(jumpLabel()).toBe('Jump to latest');
});

it('the jump scrolls instantly when the reader asked for less motion', async () => {
  setMessages([msg('m0', bob, 0)]);
  const host = mount();
  const list = host.querySelector<HTMLElement>('.message-list')!;
  sizeList(list);
  await flush();
  const scrollTo = vi.fn();
  Object.defineProperty(list, 'scrollTo', { configurable: true, value: scrollTo });
  document.documentElement.dataset.reduceMotion = 'true';
  try {
    scrollList(list, 0);
    await flush();
    jumpButton(host)!.click();
    expect(scrollTo).toHaveBeenCalledWith({ top: 5000, behavior: 'auto' });
  } finally {
    delete document.documentElement.dataset.reduceMotion;
  }
});

it('the jump button is labelled in Russian, with the plural form of the count', async () => {
  setPrefs({ ...DEFAULT_PREFS, appearance: { ...DEFAULT_PREFS.appearance, language: 'ru' } });
  setMessages([msg('m0', bob, 0)]);
  const host = mount();
  const list = host.querySelector<HTMLElement>('.message-list')!;
  sizeList(list);
  await flush();
  scrollList(list, 0);
  await flush();
  expect(jumpLabel()).toBe('К последним');
  const seen: string[] = [];
  for (let i = 1; i <= 5; i++) {
    setMessages(Array.from({ length: i + 1 }, (_, n) => msg(`m${n}`, bob, n)));
    await flush();
    seen.push(jumpLabel() ?? '');
  }
  expect(seen).toEqual(['1 новое', '2 новых', '3 новых', '4 новых', '5 новых']);
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

it('a sent message is one row: the confirmed copy replaces the pending one in place and does not animate again', async () => {
  vi.spyOn(api, 'sendTextMessage').mockResolvedValue();
  setMessages([msg('a', bob, 0)]);
  const host = mount();
  await sendMessage('hello there');
  await flush();
  const pending = host.querySelector<HTMLElement>('.msg.sending')!;
  expect(pending).not.toBeNull();
  expect(pending.classList.contains('msg-enter')).toBe(true);
  const clientMsgId = messages()[1].client_msg_id!;

  handleIncomingMessage({ message_id: 'srv-1', client_msg_id: clientMsgId, channel_id: 'c1', author_id: 'me', author_name: 'Alice Cooper', content: 'hello there', attachments: [], timestamp: Math.floor(Date.now() / 1000) });
  await flush();

  const rows = host.querySelectorAll<HTMLElement>('.msg');
  expect(rows).toHaveLength(2);
  expect(rows[1]).toBe(pending); // same DOM node: nothing was unmounted, so nothing animates a second time
  expect(rows[1].dataset.messageId).toBe('srv-1');
  expect(rows[1].classList.contains('sending')).toBe(false);
  expect(rows[1].classList.contains('msg-enter')).toBe(true);
  expect(rows[1].style.getPropertyValue('--i')).toBe('0');
});

it('scrolls to our own message once it is rendered', async () => {
  vi.spyOn(api, 'sendTextMessage').mockResolvedValue();
  setMessages([msg('a', bob, 0)]);
  const host = mount();
  const list = host.querySelector<HTMLElement>('.message-list')!;
  Object.defineProperty(list, 'scrollHeight', { configurable: true, get: () => 1000 });
  Object.defineProperty(list, 'clientHeight', { configurable: true, get: () => 200 });
  await sendMessage('mine');
  await flush();
  expect(list.scrollTop).toBe(1000);
});

it('Up in an empty composer opens my last message in this channel for editing', async () => {
  setMessages([msg('a', me, 0), msg('b', bob, 60), msg('c', me, 120)]);
  const host = mount();
  press(host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message"]')!, 'ArrowUp');
  await flush();
  const edit = host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Edit message"]');
  expect(edit?.value).toBe('text c');
  expect(host.querySelector('[data-message-id="c"] .edit-box')).not.toBeNull();
});

it('Up does nothing when the composer has text, when I have no message here, or with a modifier or the emoji picker open', async () => {
  setMessages([msg('a', me, 0)]);
  const host = mount();
  const box = host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message"]')!;
  type(box, 'not empty');
  press(box, 'ArrowUp');
  await flush();
  expect(host.querySelector('.edit-box')).toBeNull();

  type(box, '');
  press(box, 'ArrowUp', { shiftKey: true });
  await flush();
  expect(host.querySelector('.edit-box')).toBeNull();

  button(host, 'Emoji').click();
  await flush();
  press(box, 'ArrowUp');
  await flush();
  expect(host.querySelector('.edit-box')).toBeNull();
  expect(host.querySelector('.emoji-picker')).not.toBeNull();

  setMessages([msg('b', bob, 0)]);
  await flush();
  button(host, 'Emoji').click(); // close the picker
  await flush();
  press(box, 'ArrowUp');
  await flush();
  expect(host.querySelector('.edit-box')).toBeNull();
});

it('each channel keeps its own draft: switching away and back restores it', async () => {
  const other: Channel = { id: 'c2', short_id: 2, name: 'random', channel_type: 'text', position: 1, bitrate_kbps: 64 };
  const host = mount();
  const box = host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message"]')!;
  type(box, 'half a thought');
  setActiveChannel(other);
  await flush();
  expect(box.value).toBe('');
  type(box, 'other draft');
  setActiveChannel(channel);
  await flush();
  expect(box.value).toBe('half a thought');
});

it('a draft survives a reload and is stored under the channel id', async () => {
  vi.useFakeTimers();
  const host = mount();
  type(host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message"]')!, 'keep me');
  await vi.advanceTimersByTimeAsync(320);
  expect(localStorage.getItem('finchspeak.draft.c1')).toBe('keep me');
  dispose?.();
  dispose = undefined;
  document.body.innerHTML = '';
  resetDraftCache(); // as after a reload: nothing is held in memory, only localStorage
  const reloaded = mount();
  expect(reloaded.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message"]')!.value).toBe('keep me');
});

it('sending clears the draft of the channel it was sent from and no other', async () => {
  vi.useFakeTimers();
  vi.spyOn(api, 'sendTextMessage').mockResolvedValue();
  localStorage.setItem('finchspeak.draft.c2', 'later');
  const host = mount();
  const box = host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message"]')!;
  type(box, 'now');
  press(box, 'Enter');
  await vi.advanceTimersByTimeAsync(320);
  expect(localStorage.getItem('finchspeak.draft.c1')).toBeNull();
  expect(localStorage.getItem('finchspeak.draft.c2')).toBe('later');
});

it('an emptied draft is removed from storage rather than kept', async () => {
  vi.useFakeTimers();
  const host = mount();
  const box = host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message"]')!;
  type(box, 'abc');
  await vi.advanceTimersByTimeAsync(320);
  expect(localStorage.getItem('finchspeak.draft.c1')).toBe('abc');
  type(box, '');
  await vi.advanceTimersByTimeAsync(320);
  expect(localStorage.getItem('finchspeak.draft.c1')).toBeNull();
});

it('a storage that refuses writes does not stop typing', async () => {
  vi.useFakeTimers();
  vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => { throw new Error('quota'); });
  const host = mount();
  const box = host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message"]')!;
  type(box, 'still typing');
  await vi.advanceTimersByTimeAsync(320);
  expect(box.value).toBe('still typing');
});

it('the jump button position follows the composer through a CSS variable with its height', async () => {
  const host = mount();
  await flush();
  const main = host.querySelector<HTMLElement>('main')!;
  expect(main.style.getPropertyValue('--composer-h')).toMatch(/^\d+(\.\d+)?px$/);
});

it('Up with the caret away from the start of the field does nothing', async () => {
  setMessages([msg('a', me, 0)]);
  const host = mount();
  const box = host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message"]')!;
  type(box, 'abc');
  box.setSelectionRange(1, 1);
  press(box, 'ArrowUp');
  await flush();
  expect(host.querySelector('.edit-box')).toBeNull();
  expect(box.value).toBe('abc');
});

it('Up in an open mention list moves through the list and never opens an edit', async () => {
  setMessages([msg('a', me, 0)]);
  const host = mount();
  const box = host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message"]')!;
  type(box, 'ping @da');
  await flush();
  expect(host.querySelector('[role="listbox"]')).not.toBeNull();
  press(box, 'ArrowUp');
  await flush();
  expect(host.querySelector('.edit-box')).toBeNull();
  expect(host.querySelector('[role="listbox"]')).not.toBeNull();
});

it('Up during an IME composition does nothing, and Up with the key code 229 does nothing', async () => {
  setMessages([msg('a', me, 0)]);
  const host = mount();
  const box = host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message"]')!;
  press(box, 'ArrowUp', { isComposing: true });
  await flush();
  expect(host.querySelector('.edit-box')).toBeNull();
  const composing = new KeyboardEvent('keydown', { key: 'ArrowUp', bubbles: true, cancelable: true });
  Object.defineProperty(composing, 'keyCode', { value: 229 });
  box.dispatchEvent(composing);
  await flush();
  expect(host.querySelector('.edit-box')).toBeNull();
  press(box, 'ArrowUp'); // the control: a plain Up does open the edit
  await flush();
  expect(host.querySelector('.edit-box')).not.toBeNull();
});

it('after Up opens the editor, the edited message is scrolled into view', async () => {
  const scroll = vi.fn();
  Object.defineProperty(Element.prototype, 'scrollIntoView', { value: scroll, configurable: true });
  setMessages([msg('a', me, 0), msg('b', bob, 60), msg('c', me, 120)]);
  const host = mount();
  press(host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message"]')!, 'ArrowUp');
  await flush();
  expect(scroll).toHaveBeenCalledWith({ block: 'nearest' });
  expect(scroll.mock.contexts).toContain(host.querySelector('[data-message-id="c"]'));
});

it('switching channels writes the draft being typed to the old channel key at once', async () => {
  vi.useFakeTimers();
  const other: Channel = { id: 'c2', short_id: 2, name: 'random', channel_type: 'text', position: 1, bitrate_kbps: 64 };
  const host = mount();
  type(host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message"]')!, 'half');
  setActiveChannel(other);
  await vi.advanceTimersByTimeAsync(0); // no debounce wait: the write happens on the switch itself
  expect(localStorage.getItem('finchspeak.draft.c1')).toBe('half');
  expect(localStorage.getItem('finchspeak.draft.c2')).toBeNull();
});

it('two people who happen to use the same client id keep separate rows', async () => {
  setMessages([
    msg('s1', bob, 0, { client_msg_id: 'same' }),
    msg('s2', me, 1, { client_msg_id: 'same' }),
  ]);
  const host = mount();
  await flush();
  const rows = host.querySelectorAll<HTMLElement>('.msg');
  expect(rows).toHaveLength(2);
  expect(rows[0].dataset.messageId).toBe('s1');
  expect(rows[1].dataset.messageId).toBe('s2');
});
