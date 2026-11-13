import { createSignal, For, onCleanup, onMount, Show } from 'solid-js';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { t } from '../../i18n';
import { Avatar, Icon } from '../../ui';
import type { OverlayState } from '../../stores/overlayStore';
import { parsePrefs, prefs, setPrefs } from '../../stores/settingsStore';
import './overlay.css';

/** The mini window's content: the room and the people in it, who is speaking, who is muted. */
export function OverlayView(props: { state: OverlayState | null; opacity: number }) {
  const room = () => props.state?.room ?? null;
  const people = () => props.state?.people ?? [];
  return (
    <div class="overlay-card" data-tauri-drag-region style={{ opacity: props.opacity }}>
      <div class="overlay-head" data-tauri-drag-region>
        <Icon name="pip" size="sm" />
        <span class="overlay-room" data-tauri-drag-region>{room() ?? 'FinchSpeak'}</span>
      </div>
      <Show
        when={room()}
        fallback={<p class="overlay-empty">{t('Not in a room')}</p>}
      >
        <ul class="overlay-people">
          <For each={people()}>
            {(person) => (
              <li class={`overlay-person${person.speaking ? ' speaking' : ''}`}>
                <Avatar name={person.name} id={person.name} size={32} src={person.avatar_url} speaking={person.speaking} />
                <span class="overlay-name">{person.name}</span>
                <Show when={person.muted}>
                  <Icon name="mic-off" size="sm" title={t('Microphone is off')} />
                </Show>
              </li>
            )}
          </For>
        </ul>
      </Show>
    </div>
  );
}

/** The mini window's own page: takes the room state from the main window and the looks from the shared settings. */
export function OverlayWindow() {
  const [state, setState] = createSignal<OverlayState | null>(null);
  let unlisten: UnlistenFn | undefined;
  const onStorage = (event: StorageEvent) => {
    if (event.key === 'finchspeak.prefs') setPrefs(parsePrefs(event.newValue));
  };
  onMount(() => {
    window.addEventListener('storage', onStorage);
    void listen<OverlayState>('overlay-state', (event) => setState(event.payload))
      .then((off) => { unlisten = off; })
      .catch(() => undefined);
  });
  onCleanup(() => {
    unlisten?.();
    window.removeEventListener('storage', onStorage);
  });
  return <OverlayView state={state()} opacity={prefs().overlay.opacity} />;
}
