import { createSignal, onCleanup, onMount, Show } from 'solid-js';
import { Icon } from './Icon';
import { t } from '../i18n';

const inTauri = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

async function windowCall(action: 'minimize' | 'toggleMaximize' | 'close') {
  if (!inTauri()) return;
  const { getCurrentWindow } = await import('@tauri-apps/api/window');
  await getCurrentWindow()[action]();
}

/** The window's own title bar (the app has no system frame). */
export function Titlebar(props: { title?: string }) {
  // The middle button says what a click will do, so it follows the real state of the window.
  const [maximized, setMaximized] = createSignal(false);
  let gone = false;
  let stopListening: (() => void) | undefined;
  onCleanup(() => {
    gone = true;
    stopListening?.();
  });
  onMount(() => {
    if (!inTauri()) return;
    void (async () => {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      const current = getCurrentWindow();
      const sync = async () => {
        const value = await current.isMaximized();
        if (!gone) setMaximized(value);
      };
      await sync();
      const unlisten = await current.onResized(() => void sync());
      if (gone) unlisten();
      else stopListening = unlisten;
    })().catch(() => {});
  });

  return (
    <div class="titlebar" data-tauri-drag-region onDblClick={() => void windowCall('toggleMaximize')}>
      <img src="/custom/assets/brand/finch-mark.svg" alt="" width="18" height="18" data-tauri-drag-region />
      <span data-tauri-drag-region>{props.title ?? 'FinchSpeak'}</span>
      <Show when={inTauri()}>
        <div class="wc">
          <button type="button" aria-label={t('Minimize')} title={t('Minimize')} onClick={() => void windowCall('minimize')}>
            <Icon name="min" size="sm" />
          </button>
          <button type="button" aria-label={t(maximized() ? 'Restore' : 'Maximize')} title={t(maximized() ? 'Restore' : 'Maximize')} onClick={() => void windowCall('toggleMaximize')}>
            <Icon name="box" size="sm" />
          </button>
          <button type="button" aria-label={t('Close')} title={t('Close')} class="close" onClick={() => void windowCall('close')}>
            <Icon name="x" size="sm" />
          </button>
        </div>
      </Show>
    </div>
  );
}
