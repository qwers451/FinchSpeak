import { createEffect, createSignal, For, on, onCleanup, Show } from 'solid-js';
import { api } from '../../services/api';
import { loadScreenSources, screenSources, startSharing } from '../../stores/screenStore';
import type { MonitorSource, WindowSource } from '../../types';
import { Button, Icon, Modal, Toggle } from '../../ui';
import { t } from '../../i18n';

/** Windows names a monitor with a device path in front of DISPLAY1; people call it DISPLAY1. */
export const monitorName = (name: string) => name.replace(/^\\\\\.\\/, '');

type Picked = { kind: 'monitor' | 'window'; id: number; label: string };

/** How often a visible tile gets a fresh picture. */
const PREVIEW_REFRESH_MS = 1000;

/** A live picture of a screen or window, refreshed about once a second while it is on screen. */
function SourceThumb(props: { kind: 'monitor' | 'window'; id: number; index: number }) {
  const [url, setUrl] = createSignal<string | null>(null);
  const [failed, setFailed] = createSignal(false);
  let alive = true;
  let timer: ReturnType<typeof setTimeout> | undefined;

  const refresh = async () => {
    try {
      const bytes = await api.captureThumbnail(props.kind, props.id, 320);
      if (!alive) return;
      const next = URL.createObjectURL(new Blob([bytes], { type: 'image/jpeg' }));
      const previous = url();
      setUrl(next);
      setFailed(false);
      if (previous) URL.revokeObjectURL(previous);
    } catch {
      if (alive) setFailed(true);
    }
    if (alive) timer = setTimeout(() => void refresh(), PREVIEW_REFRESH_MS);
  };
  // Tiles start one after the other so the first second does not capture everything at once.
  timer = setTimeout(() => void refresh(), props.index * 150);
  onCleanup(() => {
    alive = false;
    clearTimeout(timer);
    const current = url();
    if (current) URL.revokeObjectURL(current);
  });

  return (
    <div class="thumb source-thumb">
      <Show when={url()} fallback={<span class={failed() ? 'thumb-fail' : 'skel pulse'} style={{ position: 'absolute', inset: '0', 'border-radius': '0' }}>{failed() ? 'No preview' : ''}</span>}>
        <img src={url()!} alt="" draggable={false} />
      </Show>
    </div>
  );
}

/** E1/E2: choose a screen or window with live previews, optionally with the system sound. */
export function SharePickerModal(props: { open: boolean; onClose: () => void }) {
  const [tab, setTab] = createSignal<'monitor' | 'window'>('monitor');
  const [picked, setPicked] = createSignal<Picked | null>(null);
  const [withAudio, setWithAudio] = createSignal(false);
  const [starting, setStarting] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [loading, setLoading] = createSignal(false);

  const load = async () => {
    setLoading(true);
    await loadScreenSources();
    setLoading(false);
    // The main screen is the likeliest choice.
    if (!picked()) {
      const main = screenSources().monitors.find((monitor) => monitor.is_primary) ?? screenSources().monitors[0];
      if (main) setPicked({ kind: 'monitor', id: main.id, label: monitorName(main.name) });
    }
  };
  createEffect(on(() => props.open, (open) => {
    if (open) {
      setError(null);
      setPicked(null);
      void load();
    }
  }));

  const start = async () => {
    const choice = picked();
    if (!choice || starting()) return;
    setStarting(true);
    setError(null);
    const failure = await startSharing(choice.kind, choice.id, withAudio(), choice.label);
    setStarting(false);
    if (failure) setError(failure);
    else props.onClose();
  };

  const monitors = () => screenSources().monitors;
  const windows = () => screenSources().windows;
  const empty = () => !loading() && (tab() === 'monitor' ? monitors().length === 0 : windows().length === 0);
  const isPicked = (kind: 'monitor' | 'window', id: number) => picked()?.kind === kind && picked()?.id === id;

  const pickMonitor = (monitor: MonitorSource) => setPicked({ kind: 'monitor', id: monitor.id, label: monitorName(monitor.name) });
  const pickWindow = (window: WindowSource) => setPicked({ kind: 'window', id: window.id, label: window.title });

  return (
    <Modal open={props.open} onClose={props.onClose} title={t('Share your screen')} width="860px">
      <div class="col g16">
        <div class="tabs" role="tablist">
          <button type="button" role="tab" aria-selected={tab() === 'monitor'} class={`tab${tab() === 'monitor' ? ' on' : ''}`} onClick={() => setTab('monitor')}>
            <Icon name="monitor" size="sm" />{t('Screens')}
          </button>
          <button type="button" role="tab" aria-selected={tab() === 'window'} class={`tab${tab() === 'window' ? ' on' : ''}`} onClick={() => setTab('window')}>
            <Icon name="box" size="sm" />{t('Windows')}
          </button>
          <span class="grow" />
          <Button size="sm" variant="ghost" icon="refresh" onClick={() => void load()}>{t('Refresh')}</Button>
        </div>

        <Show when={error()}>
          <div class="auth-banner red" role="alert">
            <Icon name="warn" class="tr" />
            <div class="col grow"><strong class="fs13">{t('Could not start sharing')}</strong><span class="t2 fs12">{error()}</span></div>
          </div>
        </Show>

        <div class="picker-grid" role="listbox" aria-label={tab() === 'monitor' ? t('Screens') : t('Windows')}>
          <Show when={tab() === 'monitor'}>
            <For each={monitors()}>
              {(monitor, index) => (
                <div role="option" tabindex="0" aria-selected={isPicked('monitor', monitor.id)} class={`source${isPicked('monitor', monitor.id) ? ' sel' : ''}`}
                  onClick={() => pickMonitor(monitor)} onKeyDown={(event) => (event.key === 'Enter' || event.key === ' ') && (event.preventDefault(), pickMonitor(monitor))}>
                  <SourceThumb kind="monitor" id={monitor.id} index={index()} />
                  <div class="tmeta">
                    <Icon name="monitor" size="sm" class="t3" />
                    <div class="col grow" style={{ 'min-width': 0 }}><strong class="fs13 trunc">{monitorName(monitor.name)}</strong><span class="t3 fs12">{monitor.width} × {monitor.height}</span></div>
                    <Show when={monitor.is_primary}><span class="tag admin">{t('Main')}</span></Show>
                  </div>
                </div>
              )}
            </For>
          </Show>
          <Show when={tab() === 'window'}>
            <For each={windows()}>
              {(window, index) => (
                <div role="option" tabindex="0" aria-selected={isPicked('window', window.id)} class={`source${isPicked('window', window.id) ? ' sel' : ''}`}
                  onClick={() => pickWindow(window)} onKeyDown={(event) => (event.key === 'Enter' || event.key === ' ') && (event.preventDefault(), pickWindow(window))}>
                  <SourceThumb kind="window" id={window.id} index={index()} />
                  <div class="tmeta">
                    <Icon name="box" size="sm" class="t3" />
                    <div class="col grow" style={{ 'min-width': 0 }}><strong class="fs13 trunc" title={window.title}>{window.title}</strong><span class="t3 fs12 trunc">{window.app_name}</span></div>
                  </div>
                </div>
              )}
            </For>
          </Show>
          <Show when={empty()}>
            <div class="col center g8 picker-empty">
              <Icon name="monitor" size="xl" class="t3" />
              <strong>{tab() === 'monitor' ? t('No screens found') : t('No windows to share')}</strong>
              <span class="t2 fs13">{tab() === 'monitor' ? t('Check that a display is connected, then refresh.') : t('Minimised windows are not listed. Open the window you want and refresh.')}</span>
            </div>
          </Show>
          <Show when={loading() && monitors().length + windows().length === 0}><span class="skel pulse" style={{ height: '160px' }} /></Show>
        </div>

        <div class="setrow">
          <div class="col grow">
            <strong class="fs13">{t('Share system sound too')}</strong>
            <span class="t3 fs12">{t('Everything your speakers play goes out with the picture.')}</span>
          </div>
          <Toggle label={t('Share system sound too')} checked={withAudio()} onChange={setWithAudio} />
        </div>
        <Show when={withAudio()}>
          <div class="auth-banner amber" role="note">
            <Icon name="warn" />
            <span class="fs13">{t("Other people's voices are part of that sound: they will hear themselves with a small delay. Use headphones.")}</span>
          </div>
        </Show>
        <span class="t3 fs12">{t('30 frames per second; pictures larger than 4K are scaled down before they are sent.')}</span>

        <div class="row g8" style={{ 'justify-content': 'flex-end' }}>
          <Button variant="secondary" onClick={props.onClose}>{t('Cancel')}</Button>
          <Button variant="primary" icon="share" loading={starting()} disabled={!picked()} onClick={() => void start()}>{t('Start sharing')}</Button>
        </div>
      </div>
    </Modal>
  );
}
