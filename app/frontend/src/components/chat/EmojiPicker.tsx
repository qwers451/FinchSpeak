import { createSignal, For, onCleanup, onMount, Show } from 'solid-js';
import { Icon } from '../../ui';
import { QUICK_REACTIONS, searchEmoji } from './emoji';
import { t } from '../../i18n';

/** Quick row of six, a search box and the full grid. Closes on Esc and on a click outside. */
export function EmojiPicker(props: { onPick: (emoji: string) => void; onClose: () => void; showQuick?: boolean }) {
  const [query, setQuery] = createSignal('');
  let root: HTMLDivElement | undefined;

  const onKey = (event: KeyboardEvent) => {
    if (event.key === 'Escape') {
      event.stopPropagation();
      props.onClose();
    }
  };
  const onOutside = (event: MouseEvent) => {
    if (root && !root.contains(event.target as Node)) props.onClose();
  };
  onMount(() => {
    document.addEventListener('keydown', onKey, true);
    // After the click that opened it has finished.
    setTimeout(() => document.addEventListener('mousedown', onOutside), 0);
    root?.querySelector<HTMLInputElement>('input')?.focus();
  });
  onCleanup(() => {
    document.removeEventListener('keydown', onKey, true);
    document.removeEventListener('mousedown', onOutside);
  });

  const results = () => searchEmoji(query());

  return (
    <div ref={root} class="card emoji-picker" role="dialog" aria-label={t('Emoji')}>
      <Show when={props.showQuick !== false && !query()}>
        <div class="row g4">
          <For each={QUICK_REACTIONS}>
            {(emoji) => <button type="button" class="react quick" aria-label={emoji} onClick={() => props.onPick(emoji)}>{emoji}</button>}
          </For>
        </div>
      </Show>
      <div class="input" style={{ height: '34px' }}>
        <Icon name="search" size="sm" class="t3" />
        <input value={query()} placeholder={t('Search emoji')} aria-label={t('Search emoji')} onInput={(event) => setQuery(event.currentTarget.value)} />
      </div>
      <div class="emoji-grid">
        <For each={results()} fallback={<span class="t3 fs12" style={{ 'grid-column': '1 / -1', padding: '8px' }}>{t('Nothing found')}</span>}>
          {(entry) => <button type="button" aria-label={entry.words.split(' ')[0]} title={entry.words} onClick={() => props.onPick(entry.char)}>{entry.char}</button>}
        </For>
      </div>
    </div>
  );
}
