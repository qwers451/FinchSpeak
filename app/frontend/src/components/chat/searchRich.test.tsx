import { render } from 'solid-js/web';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { api } from '../../services/api';
import { setCurrentUser } from '../../stores/authStore';
import { activeChannel, revealTarget, setActiveChannel, setChannels, setMessages } from '../../stores/chatStore';
import { clearToasts } from '../../ui';
import { highlight, SearchBox, searchTerms } from './SearchBox';
import { RichText } from './RichText';

const channel = { id: 'c1', short_id: 1, name: 'general', channel_type: 'text' as const, position: 0, bitrate_kbps: 64 };
const hit = (id: string, content: string, channel_id = 'c1') => ({ message_id: id, channel_id, channel_name: 'general', author_id: 'a', author_name: 'Alice', content, timestamp: 1_760_000_000 });

let dispose: (() => void) | undefined;
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));
const mount = (view: () => unknown) => {
  const host = document.createElement('div');
  document.body.appendChild(host);
  dispose = render(view as () => never, host);
  return host;
};

beforeEach(() => {
  vi.restoreAllMocks();
  clearToasts();
  setCurrentUser({ id: 'me', username: 'alice', display_name: 'Alice', role: 'member' });
  setChannels([channel]);
  setActiveChannel(channel);
  setMessages([]);
});
afterEach(() => {
  dispose?.();
  dispose = undefined;
  document.body.innerHTML = '';
  vi.useRealTimers();
});

it('splits what was typed into words and marks them in a result', () => {
  expect(searchTerms('Rocket, lea')).toEqual(['rocket', 'lea']);
  expect(highlight('A Rocket League match', ['rocket', 'lea']).filter((p) => p.hit).map((p) => p.text)).toEqual(['Rocket', 'League']);
  expect(highlight('nothing', [])).toEqual([{ text: 'nothing', hit: false }]);
});

it('searches after a short pause, shows the hits with the words marked, and clears with Escape', async () => {
  vi.useFakeTimers();
  const search = vi.spyOn(api, 'searchMessages').mockResolvedValue([hit('m1', 'Anyone up for a Rocket League match?')]);
  const host = mount(() => <SearchBox />);
  const input = host.querySelector('input')!;
  input.value = 'rocket';
  input.dispatchEvent(new Event('input', { bubbles: true }));
  expect(search).not.toHaveBeenCalled();
  await vi.advanceTimersByTimeAsync(300);
  expect(search).toHaveBeenCalledWith('rocket', undefined);
  expect(host.querySelector('.search-hit')?.textContent).toContain('Rocket League');
  expect(host.querySelector('.search-hit mark')?.textContent).toBe('Rocket');
  input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
  await vi.advanceTimersByTimeAsync(10);
  expect(host.querySelector('.search-results')).toBeNull();
  expect(input.value).toBe('');
});

it('says when nothing was found, and can narrow to this channel', async () => {
  vi.useFakeTimers();
  const search = vi.spyOn(api, 'searchMessages').mockResolvedValue([]);
  const host = mount(() => <SearchBox />);
  const input = host.querySelector('input')!;
  input.value = 'zzz';
  input.dispatchEvent(new Event('input', { bubbles: true }));
  await vi.advanceTimersByTimeAsync(300);
  expect(host.textContent).toContain('Nothing found.');
  Array.from(host.querySelectorAll('button')).find((b) => b.textContent === 'Only this channel')!.click();
  await vi.advanceTimersByTimeAsync(10);
  expect(search).toHaveBeenLastCalledWith('zzz', 'c1');
});

it('opening a result loads history until the message is there and marks it to scroll to', async () => {
  vi.useFakeTimers();
  vi.spyOn(api, 'searchMessages').mockResolvedValue([hit('old', 'long ago')]);
  const pages = [
    { channel_id: 'c1', has_more: true, messages: [{ id: 'new', author_id: 'a', author_name: 'A', content: 'x', attachments: [], timestamp: 5 }] },
    { channel_id: 'c1', has_more: false, messages: [{ id: 'old', author_id: 'a', author_name: 'A', content: 'long ago', attachments: [], timestamp: 1 }] },
  ];
  vi.spyOn(api, 'fetchChannelMessages').mockImplementation(async () => pages.shift()!);
  const host = mount(() => <SearchBox />);
  const input = host.querySelector('input')!;
  input.value = 'long';
  input.dispatchEvent(new Event('input', { bubbles: true }));
  await vi.advanceTimersByTimeAsync(300);
  host.querySelector<HTMLElement>('.search-hit')!.click();
  await vi.advanceTimersByTimeAsync(50);
  expect(revealTarget()).toBe('old');
  expect(activeChannel()?.id).toBe('c1');
});

// ---- rich text

const lookup = (login: string) => (login === 'bob' ? 'u2' : undefined);

it('renders the Markdown subset as elements', () => {
  const host = mount(() => <RichText text={'**bold** and *soft* and `code`\n> quote\n```js\nlet x;\n```'} lookup={lookup} />);
  expect(host.querySelector('strong')?.textContent).toBe('bold');
  expect(host.querySelector('em')?.textContent).toBe('soft');
  expect(host.querySelector('.md-code')?.textContent).toBe('code');
  expect(host.querySelector('blockquote')?.textContent).toBe('quote');
  expect(host.querySelector('pre code')?.textContent).toBe('let x;');
});

it('never turns text into markup, and only web links are clickable', () => {
  const host = mount(() => <RichText text={'<img src=x onerror=alert(1)> <script>alert(2)</script> [bad](javascript:alert(3)) [ok](https://example.com)'} lookup={lookup} />);
  expect(host.querySelector('img')).toBeNull();
  expect(host.querySelector('script')).toBeNull();
  const links = Array.from(host.querySelectorAll('a'));
  expect(links.map((a) => a.getAttribute('href'))).toEqual(['https://example.com/']);
  expect(host.textContent).toContain('<img src=x onerror=alert(1)>');
  expect(host.textContent).toContain('[bad](javascript:alert(3))');
});

it('known mentions become chips inside formatted text', () => {
  const host = mount(() => <RichText text={'hi **@bob**, and @nobody'} lookup={lookup} />);
  expect(Array.from(host.querySelectorAll('.mention-chip')).map((c) => c.textContent)).toEqual(['@bob']);
});

it('a link opens in the browser of the person, not in the app window', async () => {
  const open = vi.spyOn(window, 'open').mockReturnValue(null);
  const host = mount(() => <RichText text="see https://example.com/page now" lookup={lookup} />);
  host.querySelector('a')!.click();
  await flush();
  expect(open).toHaveBeenCalledWith('https://example.com/page', '_blank', 'noopener,noreferrer');
});
