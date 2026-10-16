import { Show } from 'solid-js';
import { Icon } from './Icon';

const inTauri = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

async function windowCall(action: 'minimize' | 'toggleMaximize' | 'close') {
  if (!inTauri()) return;
  const { getCurrentWindow } = await import('@tauri-apps/api/window');
  await getCurrentWindow()[action]();
}

/** The window's own title bar (the app has no system frame). */
export function Titlebar(props: { title?: string }) {
  return (
    <div class="titlebar" data-tauri-drag-region onDblClick={() => void windowCall('toggleMaximize')}>
      <img src="./mark.svg" alt="" width="18" height="18" data-tauri-drag-region />
      <span data-tauri-drag-region>{props.title ?? 'FinchSpeak'}</span>
      <Show when={inTauri()}>
        <div class="wc">
          <button type="button" aria-label="Minimize" onClick={() => void windowCall('minimize')}>
            <Icon name="min" size="sm" />
          </button>
          <button type="button" aria-label="Maximize" onClick={() => void windowCall('toggleMaximize')}>
            <Icon name="box" size="sm" />
          </button>
          <button type="button" aria-label="Close" class="close" onClick={() => void windowCall('close')}>
            <Icon name="x" size="sm" />
          </button>
        </div>
      </Show>
    </div>
  );
}
