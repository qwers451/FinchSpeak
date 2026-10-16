import { createEffect, createMemo, createSignal, For, on, onCleanup, onMount, Show } from 'solid-js';
import { markRead, typingNames, unread } from '../stores/activityStore';
import { attachFiles } from '../stores/attachmentStore';
import { currentUser } from '../stores/authStore';
import { activeChannel, chatLoading, hasMoreHistory, loadingOlder, loadOlder, messages, revealTarget, setRevealTarget, setReplyingTo } from '../stores/chatStore';
import type { Message } from '../types';
import { Button, Icon, pushToast } from '../ui';
import { rowInfo } from './chat/chatFormat';
import { Composer } from './chat/Composer';
import { SearchBox } from './chat/SearchBox';
import { MessageItem } from './chat/MessageItem';

/** Closer than this to the bottom counts as "reading the latest messages". */
const STICK_TO_BOTTOM_PX = 96;
/** Loading older messages starts when the list is scrolled this close to the top. */
const LOAD_OLDER_PX = 160;

function Skeleton() {
  return (
    <div class="col g16" style={{ padding: '16px 20px' }} aria-label="Loading messages" role="status">
      <For each={[140, 100, 120]}>
        {(width, index) => (
          <div class="row g12" style={{ 'align-items': 'flex-start' }}>
            <span class="skel pulse" style={{ width: '40px', height: '40px', 'border-radius': '50%', flex: 'none' }} />
            <div class="col g8 grow">
              <span class="skel pulse" style={{ height: '12px', width: `${width}px` }} />
              <span class="skel pulse" style={{ height: '12px', width: `${90 - index() * 20}%` }} />
              <Show when={index() === 1}><span class="skel pulse" style={{ height: '12px', width: '40%' }} /></Show>
            </div>
          </div>
        )}
      </For>
    </div>
  );
}

export function ChatArea() {
  const [now, setNow] = createSignal(Date.now());
  const [divider, setDivider] = createSignal<string | null>(null);
  const [dragging, setDragging] = createSignal(false);
  const [unseen, setUnseen] = createSignal(0);
  let list: HTMLDivElement | undefined;
  let dragDepth = 0;

  // Typing hints expire on their own: re-evaluate twice a second.
  const timer = setInterval(() => setNow(Date.now()), 500);
  onCleanup(() => clearInterval(timer));

  // Items are keyed by message id so a change elsewhere (a new message, a reaction, the clock) never
  // rebuilds a message that is being edited or has its emoji picker open.
  const [today, setToday] = createSignal(Date.now());
  const dayTimer = setInterval(() => setToday(Date.now()), 60_000);
  onCleanup(() => clearInterval(dayTimer));
  const byId = createMemo(() => new Map(messages().map((message) => [message.id, message])));
  const ids = createMemo(() => messages().map((message) => message.id));
  const lastStored = () => [...messages()].reverse().find((message) => !message.state)?.id ?? null;
  const nearBottom = () => !list || list.scrollHeight - list.scrollTop - list.clientHeight < STICK_TO_BOTTOM_PX;
  const toBottom = () => { if (list) list.scrollTop = list.scrollHeight; setUnseen(0); };

  // Opening a channel: remember where the unread part starts, then count the channel as read.
  createEffect(on(() => activeChannel()?.id, (channelId) => {
    setDivider(channelId ? unread()[channelId]?.firstUnreadId ?? null : null);
    setUnseen(0);
    if (channelId) markRead(channelId, lastStored());
    queueMicrotask(() => {
      const target = list?.querySelector('.daysep.new');
      if (target) target.scrollIntoView({ block: 'center' });
      else toBottom();
    });
  }));

  // New messages: follow them when at the bottom (or when they are ours), otherwise say how many.
  let knownLast: string | null = null;
  createEffect(() => {
    const all = messages();
    const newest = all[all.length - 1];
    const channel = activeChannel();
    if (!newest || !channel) { knownLast = null; return; }
    const isNew = knownLast !== null && newest.id !== knownLast && !loadingOlder();
    knownLast = newest.id;
    if (!isNew) return;
    const ours = newest.author_id === currentUser()?.id;
    if (ours || nearBottom()) {
      queueMicrotask(toBottom);
      if (document.hasFocus()) markRead(channel.id, lastStored());
    } else {
      setUnseen((count) => count + 1);
    }
  });

  // Older messages arrive above: keep what the user is reading where it is.
  let heightBefore = 0;
  createEffect(on(loadingOlder, (loading) => {
    if (loading && list) heightBefore = list.scrollHeight;
    else if (!loading && list && heightBefore) {
      const previous = heightBefore;
      heightBefore = 0;
      queueMicrotask(() => { if (list) list.scrollTop += list.scrollHeight - previous; });
    }
  }));

  const onScroll = () => {
    if (!list) return;
    if (list.scrollTop < LOAD_OLDER_PX && hasMoreHistory()) void loadOlder();
    if (nearBottom() && unseen() > 0) {
      setUnseen(0);
      const channel = activeChannel();
      if (channel && document.hasFocus()) markRead(channel.id, lastStored());
    }
  };

  const jumpTo = (messageId: string) => {
    const target = list?.querySelector<HTMLElement>(`[data-message-id="${CSS.escape(messageId)}"]`);
    if (!target) {
      pushToast({ kind: 'info', title: 'That message is further up', text: 'Scroll up to load older messages.' });
      return;
    }
    target.scrollIntoView({ block: 'center', behavior: 'smooth' });
    target.classList.add('flash');
    setTimeout(() => target.classList.remove('flash'), 1600);
  };

  const reply = (message: Message) => setReplyingTo(message);

  // A search result was opened: scroll to the message once it is there.
  createEffect(() => {
    const target = revealTarget();
    if (!target) return;
    queueMicrotask(() => { jumpTo(target); setRevealTarget(null); });
  });

  const typingLine = createMemo(() => {
    const channel = activeChannel();
    if (!channel) return '';
    const names = typingNames(channel.id, now());
    if (names.length === 0) return '';
    if (names.length === 1) return `${names[0]} is typing…`;
    if (names.length === 2) return `${names[0]} and ${names[1]} are typing…`;
    return 'Several people are typing…';
  });

  // Dropping files anywhere on the chat attaches them.
  const hasFiles = (event: DragEvent) => Array.from(event.dataTransfer?.types ?? []).includes('Files');
  const onDragEnter = (event: DragEvent) => { if (hasFiles(event)) { dragDepth++; setDragging(true); } };
  const onDragLeave = () => { dragDepth = Math.max(0, dragDepth - 1); if (dragDepth === 0) setDragging(false); };
  const onDragOver = (event: DragEvent) => { if (hasFiles(event)) event.preventDefault(); };
  const onDrop = (event: DragEvent) => {
    event.preventDefault();
    dragDepth = 0;
    setDragging(false);
    const files = Array.from(event.dataTransfer?.files ?? []);
    if (files.length > 0) void attachFiles(files);
  };
  // A drop outside the chat must not navigate the window to the file.
  onMount(() => {
    const block = (event: DragEvent) => { if (hasFiles(event)) event.preventDefault(); };
    window.addEventListener('dragover', block);
    window.addEventListener('drop', block);
    onCleanup(() => { window.removeEventListener('dragover', block); window.removeEventListener('drop', block); });
  });

  return (
    <main class="main chat" onDragEnter={onDragEnter} onDragLeave={onDragLeave} onDragOver={onDragOver} onDrop={onDrop}>
      <div class="mhead">
        <Icon name="hash" size="lg" class="t3" />
        <h3 style={{ 'font-size': '18px' }}>{activeChannel()?.name ?? 'select-channel'}</h3>
        <Show when={activeChannel()?.topic}>
          <span class="vsep" style={{ height: '20px', 'align-self': 'center' }} />
          <span class="t2 fs13 trunc grow">{activeChannel()?.topic}</span>
        </Show>
        <Show when={!activeChannel()?.topic}><span class="grow" /></Show>
        <SearchBox />
      </div>

      <div class="message-list" ref={list} onScroll={onScroll} role="log" aria-live="polite" aria-label="Messages">
        <Show when={loadingOlder()}><div class="row center g8 t3 fs12" style={{ padding: '8px' }}><span class="spin mini" />Loading older messages…</div></Show>
        <Show when={chatLoading() && messages().length === 0}><Skeleton /></Show>
        <Show when={!chatLoading() && messages().length === 0}>
          <div class="col center g12 empty-chat">
            <img src="./mascot.svg" alt="" width="150" />
            <h3 style={{ 'font-size': '20px' }}>Welcome to #{activeChannel()?.name}!</h3>
            <span class="t2 fs13">This is the start of the conversation. Say hello — someone is always listening.</span>
          </div>
        </Show>
        <For each={ids()}>
          {(id, index) => {
            const message = () => byId().get(id);
            const info = createMemo(() => {
              const current = message();
              return current ? rowInfo(messages()[index() - 1], current, divider(), today()) : null;
            });
            return (
              <Show when={message()}>
                {(current) => (
                  <>
                    <Show when={info()?.day}>{(label) => <div class="daysep">{label()}</div>}</Show>
                    <Show when={info()?.divider}><div class="daysep new">New messages</div></Show>
                    <MessageItem message={current()} compact={info()?.compact ?? false} onReply={reply} onJump={jumpTo} />
                  </>
                )}
              </Show>
            );
          }}
        </For>
      </div>

      <Show when={unseen() > 0}>
        <div class="jump-wrap">
          <Button size="sm" variant="primary" icon="down" onClick={toBottom}>{unseen() === 1 ? '1 new message' : `${unseen()} new messages`}</Button>
        </div>
      </Show>

      <div class="typing" aria-live="polite">
        <Show when={typingLine()}><i /><i /><i />{typingLine()}</Show>
      </div>
      <Composer />

      <Show when={dragging()}>
        <div class="drop-overlay" aria-hidden="true">
          <Icon name="clip" size="xl" />
          <strong>Drop files to attach</strong>
          <span class="t2 fs13">Up to 10 files, 10 MB each</span>
        </div>
      </Show>
    </main>
  );
}
