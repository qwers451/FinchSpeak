import { createEffect, createSignal, For, on, onCleanup, Show } from 'solid-js';
import { api } from '../../services/api';
import { loadScreenSources, screenAudioSource, screenSources, startSharing } from '../../stores/screenStore';
import type { MonitorSource, WindowSource } from '../../types';
import { Button, Icon, Modal, Toggle } from '../../ui';
import { t } from '../../i18n';

/** Windows names a monitor with a device path in front of DISPLAY1; people call it DISPLAY1. */
export const monitorName = (name: string) => name.replace(/^\\\\\.\\/, '');

type Picked = { kind: 'monitor' | 'window'; id: number; label: string };

const FPS_KEY = 'finchspeak.shareFps';
const SHARE_AUDIO_KEY = 'finchspeak.shareAudio';

/** The frame rate the sharer chose last time; 30 unless 60 was saved. */
function savedFps(): 30 | 60 {
  try {
    return localStorage.getItem(FPS_KEY) === '60' ? 60 : 30;
  } catch {
    return 30;
  }
}

/** How often a visible tile gets a fresh picture. */
const PREVIEW_REFRESH_MS = 1000;

/** A live picture of a screen or window, refreshed about once a second while it is on screen. */
function SourceThumb(props: { kind: 'monitor' | 'window'; id: number; index: number; minimized?: boolean }) {
  // A minimized window has no picture to capture: it shows a placeholder and is not polled.
  if (props.minimized) {
    return (
      <div class="thumb source-thumb">
        <span class="source-placeholder minimized-placeholder">
          <Icon name="box" size="xl" class="t3" />
          <span class="t2 fs12">{t('Minimized')}</span>
        </span>
      </div>
    );
  }
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
      <Show when={url()} fallback={<span class={`source-placeholder ${failed() ? 'thumb-fail' : 'skel pulse'}`}>{failed() ? t('No preview') : ''}</span>}>
        <img src={url()!} alt="" draggable={false} />
      </Show>
    </div>
  );
}

/** E1/E2: choose a screen or window with live previews, optionally with the system sound. */
export function SharePickerModal(props: { open: boolean; onClose: () => void }) {
  const [tab, setTab] = createSignal<'monitor' | 'window'>('monitor');
  const [picked, setPicked] = createSignal<Picked | null>(null);
  const [audioChoice, setAudioChoice] = createSignal<boolean>();
  const [loopbackSupported, setLoopbackSupported] = createSignal<boolean>();
  const withAudio = () => audioChoice() ?? loopbackSupported();
  const excludesSelf = () => loopbackSupported() === true && screenAudioSource() !== 'device';
  const [audioScope, setAudioScope] = createSignal<'system' | 'window'>('system');
  const [starting, setStarting] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [loading, setLoading] = createSignal(false);
  const [fps, setFps] = createSignal<30 | 60>(savedFps());
  const chooseFps = (value: 30 | 60) => {
    setFps(value);
    try {
      localStorage.setItem(FPS_KEY, String(value));
    } catch {
      // Remembering the choice is a convenience; sharing works without it.
    }
  };

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
    if (!open) return;
    let current = true;
    onCleanup(() => { current = false; });
    setError(null);
    setPicked(null);
    setAudioScope('system');
    let savedAudio: string | null = null;
    try { savedAudio = localStorage.getItem(SHARE_AUDIO_KEY); } catch { /* storage unavailable */ }
    setAudioChoice(savedAudio === '1' || savedAudio === '0' ? savedAudio === '1' : undefined);
    setLoopbackSupported(undefined);
    void api.processLoopbackSupported().catch(() => false).then((supported) => {
      if (current) setLoopbackSupported(supported);
    });
    void load();
  }));

  const changeAudio = (enabled: boolean) => {
    setAudioChoice(enabled);
    try { localStorage.setItem(SHARE_AUDIO_KEY, enabled ? '1' : '0'); } catch { /* storage unavailable */ }
  };

  const start = async () => {
    const choice = picked();
    const audio = withAudio();
    if (!choice || audio === undefined || starting()) return;
    setStarting(true);
    setError(null);
    const failure = await startSharing(choice.kind, choice.id, audio, choice.label, choice.kind === 'window' && excludesSelf() ? audioScope() : 'system', fps());
    setStarting(false);
    if (failure) setError(failure);
    else props.onClose();
  };

  const monitors = () => screenSources().monitors;
  const windows = () => screenSources().windows;
  const empty = () => !loading() && (tab() === 'monitor' ? monitors().length === 0 : windows().length === 0);
  const isPicked = (kind: 'monitor' | 'window', id: number) => picked()?.kind === kind && picked()?.id === id;

  const pickMonitor = (monitor: MonitorSource) => { setAudioScope('system'); setPicked({ kind: 'monitor', id: monitor.id, label: monitorName(monitor.name) }); };
  const pickWindow = (window: WindowSource) => { setAudioScope('system'); setPicked({ kind: 'window', id: window.id, label: window.title }); };

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
                    <div class="col grow"><strong class="fs13 trunc">{monitorName(monitor.name)}</strong><span class="t3 fs12">{monitor.width} × {monitor.height}</span></div>
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
                  <SourceThumb kind="window" id={window.id} index={index()} minimized={window.is_minimized} />
                  <div class="tmeta">
                    <Icon name="box" size="sm" class="t3" />
                    <div class="col grow"><strong class="fs13 trunc" title={window.title}>{window.title}</strong><span class="t3 fs12 trunc">{window.app_name}</span></div>
                  </div>
                </div>
              )}
            </For>
          </Show>
          <Show when={empty()}>
            <div class="col center g8 picker-empty">
              <Icon name="monitor" size="xl" class="t3" />
              <strong>{tab() === 'monitor' ? t('No screens found') : t('No windows to share')}</strong>
              <span class="t2 fs13">{tab() === 'monitor' ? t('Check that a display is connected, then refresh.') : t('Open the window you want and refresh.')}</span>
            </div>
          </Show>
          <Show when={loading() && monitors().length + windows().length === 0}><span class="skel pulse picker-loading" /></Show>
        </div>

        <Show when={tab() === 'window' && windows().some((window) => window.is_minimized)}>
          <span class="t3 fs12">{t('A minimized window shows up for viewers once you open it.')}</span>
        </Show>
        <div class="setrow">
          <strong class="fs13">{t('Frame rate')}</strong>
          <div class="tabs" role="group" aria-label={t('Frame rate')}>
            <button type="button" class={`tab${fps() === 30 ? ' on' : ''}`} aria-pressed={fps() === 30} onClick={() => chooseFps(30)}>30</button>
            <button type="button" class={`tab${fps() === 60 ? ' on' : ''}`} aria-pressed={fps() === 60} onClick={() => chooseFps(60)}>60</button>
          </div>
        </div>
        <Show when={fps() === 60}>
          <span class="t3 fs12">{t('Smoother, but needs a faster computer and connection. The picture is limited to 1080p.')}</span>
        </Show>
        <div class="setrow">
          <div class="col grow">
            <strong class="fs13">{t('Share system sound too')}</strong>
            <span class="t3 fs12">{withAudio() && picked()?.kind === 'window' && excludesSelf() && audioScope() === 'window' ? t('Only this window') : excludesSelf() ? t('Everything your computer plays goes out with the picture, except voices from FinchSpeak.') : t('Everything your speakers play goes out with the picture.')}</span>
          </div>
          <Toggle label={t('Share system sound too')} checked={withAudio() ?? false} onChange={changeAudio} />
        </div>
        <Show when={withAudio() && picked()?.kind === 'window' && excludesSelf()}>
          <div class="setrow">
            <strong class="fs13">{t('Sound')}</strong>
            <div class="tabs" role="group" aria-label={t('Sound')}>
              <button type="button" class={`tab${audioScope() === 'system' ? ' on' : ''}`} aria-pressed={audioScope() === 'system'} onClick={() => setAudioScope('system')}>{t('Whole computer')}</button>
              <button type="button" class={`tab${audioScope() === 'window' ? ' on' : ''}`} aria-pressed={audioScope() === 'window'} onClick={() => setAudioScope('window')}>{t('Only this window')}</button>
            </div>
          </div>
        </Show>
        <Show when={withAudio() && !excludesSelf()}>
          <div class="auth-banner amber" role="note">
            <Icon name="warn" />
            <span class="fs13">{t("This version of Windows cannot leave FinchSpeak out of the sound: people in the room will hear themselves with a delay. Update to Windows 10 2004 or newer.")}</span>
          </div>
        </Show>
        <span class="t3 fs12">{t('Pictures larger than 4K are scaled down before they are sent.')}</span>

        <div class="row g8 dialog-actions-primary">
          <Button variant="secondary" onClick={props.onClose}>{t('Cancel')}</Button>
          <Button variant="primary" icon="share" loading={starting()} disabled={!picked() || withAudio() === undefined} onClick={() => void start()}>{t('Start sharing')}</Button>
        </div>
      </div>
    </Modal>
  );
}
