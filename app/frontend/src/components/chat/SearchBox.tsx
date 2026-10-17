import { createSignal, For, onCleanup, Show } from 'solid-js';
import { api } from '../../services/api';
import { activeChannel, revealMessage } from '../../stores/chatStore';
import type { SearchHit } from '../../types';
import { Icon, pushToast } from '../../ui';
import { clock, dayLabel } from './chatFormat';
import { t } from '../../i18n';

const DEBOUNCE_MS = 250;

/** The words to highlight in a hit: what was typed, without punctuation. */
export function searchTerms(text: string): string[] {
  return text.split(/[^\p{L}\p{N}]+/u).filter(Boolean).map((word) => word.toLowerCase());
}

/** Cut `content` into parts, marking the ones that start with a search term. */
export function highlight(content: string, terms: string[]): { text: string; hit: boolean }[] {
  if (terms.length === 0) return [{ text: content, hit: false }];
  const parts: { text: string; hit: boolean }[] = [];
  for (const piece of content.split(/([\p{L}\p{N}]+)/u)) {
    if (!piece) continue;
    const lower = piece.toLowerCase();
    parts.push({ text: piece, hit: /[\p{L}\p{N}]/u.test(piece) && terms.some((term) => lower.startsWith(term)) });
  }
  return parts;
}

/** Search in the channel header: results in a list below; picking one opens the message. */
export function SearchBox() {
  const [text, setText] = createSignal('');
  const [hits, setHits] = createSignal<SearchHit[] | null>(null);
  const [everywhere, setEverywhere] = createSignal(true);
  const [busy, setBusy] = createSignal(false);
  let timer: ReturnType<typeof setTimeout> | undefined;
  let serial = 0;
  onCleanup(() => clearTimeout(timer));

  const run = async () => {
    const query = text().trim();
    const mine = ++serial;
    if (!query) {
      setHits(null);
      return;
    }
    setBusy(true);
    try {
      const found = await api.searchMessages(query, everywhere() ? undefined : activeChannel()?.id);
      if (mine === serial) setHits(found);
    } catch (err) {
      if (mine === serial) pushToast({ kind: 'error', title: t('Search failed'), text: String(err) });
    } finally {
      if (mine === serial) setBusy(false);
    }
  };
  const schedule = () => {
    clearTimeout(timer);
    timer = setTimeout(() => void run(), DEBOUNCE_MS);
  };
  const close = () => {
    serial++;
    setText('');
    setHits(null);
  };
  const open = async (hit: SearchHit) => {
    close();
    if (!(await revealMessage(hit.channel_id, hit.message_id))) {
      pushToast({ kind: 'info', title: t('That message is too far back'), text: t('It is in #{channel}; scroll up to find it.', { channel: hit.channel_name }) });
    }
  };

  return (
    <div class="search-box" role="search">
      <div class="input" style={{ width: '220px', height: '34px', 'border-radius': '10px' }}>
        <Icon name="search" size="sm" class="t3" />
        <input
          value={text()}
          placeholder={t('Search')}
          aria-label={t('Search messages')}
          onInput={(event) => { setText(event.currentTarget.value); schedule(); }}
          onKeyDown={(event) => { if (event.key === 'Escape') close(); if (event.key === 'Enter') { clearTimeout(timer); void run(); } }}
        />
        <Show when={text()}><button type="button" class="icon-close" aria-label={t('Clear search')} onClick={close}><Icon name="x" size="sm" /></button></Show>
      </div>
      <Show when={hits()}>
        {(list) => (
          <div class="card search-results" role="listbox" aria-label={t('Search results')}>
            <div class="row g8 between" style={{ padding: '6px 8px' }}>
              <span class="sec-label">{busy() ? t('Searching…') : t('{count}{plus} found', { count: list().length, plus: list().length === 30 ? '+' : '' })}</span>
              <button type="button" class="link-btn" style={{ margin: 0 }} onClick={() => { setEverywhere(!everywhere()); void run(); }}>{everywhere() ? t('Only this channel') : t('All channels')}</button>
            </div>
            <For each={list()} fallback={<div class="t3 fs13" style={{ padding: '12px' }}>{t('Nothing found.')}</div>}>
              {(hit) => (
                <button type="button" role="option" class="search-hit" onClick={() => void open(hit)}>
                  <span class="t3 fs12">#{hit.channel_name} · <strong class="t2">{hit.author_name}</strong> · {dayLabel(hit.timestamp)} {clock(hit.timestamp)}</span>
                  <span class="search-text"><For each={highlight(hit.content.slice(0, 240), searchTerms(text()))}>{(part) => (part.hit ? <mark>{part.text}</mark> : <>{part.text}</>)}</For></span>
                </button>
              )}
            </For>
          </div>
        )}
      </Show>
    </div>
  );
}
