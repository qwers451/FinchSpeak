import { For, Show, createSignal } from 'solid-js';
import { isSharing, loadScreenSources, screenSources, startSharing, stopSharing } from '../stores/screenStore';

/** "Share screen" button: pick a monitor or a window, or stop the current share. */
export function ScreenSharePicker() {
  const [open, setOpen] = createSignal(false);
  const [withAudio, setWithAudio] = createSignal(false);

  const toggle = async () => {
    if (isSharing()) {
      await stopSharing();
      return;
    }
    if (!open()) await loadScreenSources();
    setOpen(!open());
  };

  const share = async (kind: 'monitor' | 'window', id: number) => {
    setOpen(false);
    await startSharing(kind, id, withAudio());
  };

  return (
    <div class="screen-share-picker">
      <button type="button" class={isSharing() ? 'danger' : ''} onClick={() => void toggle()}>
        {isSharing() ? 'Stop sharing' : 'Share screen'}
      </button>
      <Show when={open() && !isSharing()}>
        <div class="screen-source-list" role="listbox" aria-label="Screen to share">
          <label class="screen-audio-option">
            <input type="checkbox" checked={withAudio()} onChange={(event) => setWithAudio(event.currentTarget.checked)} />
            Share system sound too
          </label>
          <Show when={withAudio()}>
            <small class="screen-audio-note">
              Everything your speakers play is sent, including other people's voices: they will hear
              themselves with a delay.
            </small>
          </Show>
          <For each={screenSources().monitors}>
            {(monitor) => (
              <button type="button" role="option" onClick={() => void share('monitor', monitor.id)}>
                🖥 {monitor.name} ({monitor.width}×{monitor.height}){monitor.is_primary ? ' – main' : ''}
              </button>
            )}
          </For>
          <For each={screenSources().windows}>
            {(window) => (
              <button type="button" role="option" onClick={() => void share('window', window.id)}>
                🗔 {window.title} <small>({window.app_name})</small>
              </button>
            )}
          </For>
          <Show when={screenSources().monitors.length + screenSources().windows.length === 0}>
            <div class="screen-source-empty">No screens or windows found.</div>
          </Show>
        </div>
      </Show>
    </div>
  );
}
