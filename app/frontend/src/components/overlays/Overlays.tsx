import { createMemo, createSignal, For, onCleanup, onMount, Show } from 'solid-js';
import { logoutUser } from '../../stores/authStore';
import { activeChannel, channels, selectChannel } from '../../stores/chatStore';
import { markRead } from '../../stores/activityStore';
import { END_TEXT, sessionEnded, setSessionEnded } from '../../stores/sessionEnd';
import { activeVoiceChannel, joinVoice } from '../../stores/voiceStore';
import type { Channel } from '../../types';
import { Button, Icon, Modal } from '../../ui';
import { t } from '../../i18n';

/** Is the keyboard focus somewhere text is typed? Then single-key shortcuts must keep out. */
export function typingNow(target: EventTarget | null = document.activeElement): boolean {
  const el = target as HTMLElement | null;
  if (!el) return false;
  return el.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(el.tagName);
}

const SHORTCUTS: [string[], string][] = [
  [['Ctrl', 'K'], 'Jump to a channel'],
  [['?'], 'Show this list'],
  [['Enter'], 'Send the message'],
  [['Shift', 'Enter'], 'New line in a message'],
  [['@'], 'Mention someone'],
  [['Esc'], 'Cancel an edit or reply, close a window'],
  [['Push-to-talk key'], 'Speak while it is held (set in Settings)'],
];

/** `?` shows the shortcuts, Ctrl+K a quick channel switcher. */
export function Overlays() {
  const [help, setHelp] = createSignal(false);
  const [switcher, setSwitcher] = createSignal(false);
  const [query, setQuery] = createSignal('');
  const [selected, setSelected] = createSignal(0);

  const matches = createMemo(() => {
    const needle = query().trim().toLowerCase();
    return channels().filter((channel) => !needle || channel.name.toLowerCase().includes(needle)).slice(0, 8);
  });

  const onKey = (event: KeyboardEvent) => {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k') {
      event.preventDefault();
      setQuery('');
      setSelected(0);
      setSwitcher(true);
      return;
    }
    if (event.key === '?' && !typingNow(event.target) && !event.ctrlKey && !event.altKey) {
      event.preventDefault();
      setHelp(!help());
    }
  };
  onMount(() => window.addEventListener('keydown', onKey));
  onCleanup(() => window.removeEventListener('keydown', onKey));

  const go = (channel: Channel) => {
    setSwitcher(false);
    void selectChannel(channel);
    if (channel.channel_type === 'text') markRead(channel.id);
    else if (activeVoiceChannel()?.id !== channel.id) void joinVoice(channel);
  };

  return (
    <>
    <Modal open={help()} onClose={() => setHelp(false)} title={t('Keyboard shortcuts')} width="480px">
        <div class="col g12">
          <For each={SHORTCUTS}>
            {([keys, what]) => (
              <div class="row g12">
                <span class="row g4" style={{ width: '170px', flex: 'none' }}><For each={keys}>{(key) => <span class="kbd">{key}</span>}</For></span>
            <span class="t2 fs13">{t(what)}</span>
              </div>
            )}
          </For>
          <div class="row" style={{ 'justify-content': 'flex-end' }}><Button variant="primary" onClick={() => setHelp(false)}>{t('Close')}</Button></div>
        </div>
      </Modal>

      <Modal open={switcher()} onClose={() => setSwitcher(false)} title={t('Jump to a channel')} width="440px">
        <div class="col g8">
          <div class="input">
            <Icon name="search" size="sm" class="t3" />
            <input
              data-autofocus
              aria-label={t('Channel name')}
              placeholder={t('general')}
              value={query()}
              onInput={(event) => { setQuery(event.currentTarget.value); setSelected(0); }}
              onKeyDown={(event) => {
                const list = matches();
                if (event.key === 'ArrowDown') { event.preventDefault(); setSelected((selected() + 1) % Math.max(1, list.length)); }
                else if (event.key === 'ArrowUp') { event.preventDefault(); setSelected((selected() - 1 + list.length) % Math.max(1, list.length)); }
                else if (event.key === 'Enter' && list[selected()]) { event.preventDefault(); go(list[selected()]); }
              }}
            />
          </div>
          <div class="col" role="listbox" aria-label={t('Channels')}>
            <For each={matches()} fallback={<span class="t3 fs13" style={{ padding: '8px' }}>{t('No channel like that.')}</span>}>
              {(channel, index) => (
                <button type="button" role="option" aria-selected={index() === selected()} class={`ch${index() === selected() ? ' on' : ''}`} style={{ 'text-align': 'left' }} onClick={() => go(channel)}>
                  <Icon name={channel.channel_type === 'voice' ? 'vol' : 'hash'} size="sm" />
                  <span class="trunc">{channel.name}</span>
                  <Show when={activeChannel()?.id === channel.id}><span class="t3 fs12" style={{ 'margin-left': 'auto' }}>{t('open')}</span></Show>
                </button>
              )}
            </For>
          </div>
        </div>
      </Modal>
    </>
  );
}

/** I2: the server ended the session; says why and offers to sign in again. */
export function SessionEndedScreen() {
  return (
    <Show when={sessionEnded()}>
      {(kind) => (
        <div class="splash session-ended" role="alertdialog" aria-labelledby="session-ended-title">
          <img src="/custom/assets/illustrations/mascot.svg" alt="" width="220" />
          <h2 id="session-ended-title">{t(END_TEXT[kind()].headline)}</h2>
          <p class="t2" style={{ 'max-width': '420px', 'text-align': 'center' }}>{t(END_TEXT[kind()].body)}</p>
          <Button variant="primary" size="lg" onClick={() => { setSessionEnded(null); void logoutUser(); }}>{t('Back to sign in')}</Button>
        </div>
      )}
    </Show>
  );
}
